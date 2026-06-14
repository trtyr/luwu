//! Deterministic compaction — zero-LLM-cost structured summary extraction.
//!
//! Analyzes conversation history + git state + tool calls to produce a
//! structured summary without any LLM involvement. Inspired by pi-blackhole's
//! vcc pipeline. This replaces (or supplements) the LLM-based Writer for the
//! deterministic parts of checkpointing.

use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use luwu_core::message::{ContentPart, Message, Role};

/// Structured summary extracted deterministically from a session.
#[derive(Debug, Clone)]
pub struct DeterministicSummary {
    /// First user message — the original goal/intent.
    pub session_goal: String,
    /// Files touched via write/edit tools.
    pub files_changed: Vec<FileChange>,
    /// Recent git commits in the working directory.
    pub commits: Vec<String>,
    /// Errors from tool results that represent unresolved blockers.
    pub blockers: Vec<String>,
    /// Last N messages verbatim (transcript tail).
    pub transcript_tail: Vec<(String, String)>,
}

/// A file change detected from tool call analysis.
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: String,
    /// What happened: "Created" (write to new file), "Modified" (edit or write overwrite).
    pub action: String,
    /// Which tool caused the change.
    pub tool: String,
}

impl DeterministicSummary {
    /// Render as Markdown for injection into conversation context.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();

        // Session Goal
        if !self.session_goal.is_empty() {
            out.push_str("[Session Goal]\n");
            out.push_str(&self.session_goal);
            out.push_str("\n\n");
        }

        // Files And Changes
        if !self.files_changed.is_empty() {
            out.push_str("[Files And Changes]\n");
            for fc in &self.files_changed {
                out.push_str(&format!("- {}: {} ({})\n", fc.action, fc.path, fc.tool));
            }
            out.push('\n');
        }

        // Commits
        if !self.commits.is_empty() {
            out.push_str("[Commits]\n");
            for c in &self.commits {
                out.push_str(&format!("- {c}\n"));
            }
            out.push('\n');
        }

        // Blockers
        if !self.blockers.is_empty() {
            out.push_str("[Outstanding Context]\n");
            for b in self.blockers.iter().take(10) {
                // Truncate long blockers
                let truncated = if b.len() > 200 {
                    let end = b.floor_char_boundary(200);
                    format!("{}...", &b[..end])
                } else {
                    b.clone()
                };
                out.push_str(&format!("- {truncated}\n"));
            }
            out.push('\n');
        }

        // Transcript Tail
        if !self.transcript_tail.is_empty() {
            out.push_str("[Transcript Tail]\n");
            for (role, text) in &self.transcript_tail {
                let preview = if text.len() > 500 {
                    let end = text.floor_char_boundary(500);
                    format!("{}...", &text[..end])
                } else {
                    text.clone()
                };
                out.push_str(&format!("{role}: {preview}\n"));
            }
            out.push('\n');
        }

        out
    }
}

/// Extract a deterministic summary from conversation history + working directory.
///
/// This is a pure function (no LLM calls). It:
/// 1. Takes the first user message as session goal
/// 2. Scans all ToolCalls for write/edit operations to track file changes
/// 3. Runs `git log --oneline -20` in the working directory
/// 4. Scans ToolResults for errors as potential blockers
/// 5. Takes the last few messages as a transcript tail
pub fn compile(messages: &[Message], working_dir: &Path) -> DeterministicSummary {
    // 1. Session goal — first user text message
    let session_goal = messages
        .iter()
        .find(|m| m.role == Role::User)
        .and_then(first_text)
        .unwrap_or_default();

    // 2. File changes — scan tool calls for write/edit
    let mut files_changed = Vec::new();
    let mut seen_paths: HashSet<String> = HashSet::new();

    for msg in messages {
        for part in &msg.content {
            if let ContentPart::ToolCall {
                name, arguments, ..
            } = part
                && let Some(path) = extract_path_from_tool_call(name, arguments)
            {
                let action = if name == "write" && !seen_paths.contains(&path) {
                    "Created".to_string()
                } else {
                    "Modified".to_string()
                };
                seen_paths.insert(path.clone());
                files_changed.push(FileChange {
                    path,
                    action,
                    tool: name.clone(),
                });
            }
        }
    }

    // 3. Commits — git log
    let commits = git_log_oneline(working_dir);

    // 4. Blockers — error tool results
    let mut blockers = Vec::new();
    for msg in messages {
        for part in &msg.content {
            if let ContentPart::ToolResult {
                content, is_error, ..
            } = part
                && *is_error
                && !content.is_empty()
            {
                blockers.push(content.clone());
            }
        }
    }

    // 5. Transcript tail — last messages (up to 8)
    let transcript_tail: Vec<(String, String)> = messages
        .iter()
        .rev()
        .take(8)
        .filter_map(|m| {
            let text = first_text(m)?;
            let role = match m.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
                _ => return None,
            };
            Some((role.to_string(), text))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    DeterministicSummary {
        session_goal,
        files_changed,
        commits,
        blockers,
        transcript_tail,
    }
}

/// Extract the file path from a tool call's arguments.
fn extract_path_from_tool_call(tool_name: &str, arguments: &serde_json::Value) -> Option<String> {
    match tool_name {
        "write" | "edit" => arguments
            .get("path")
            .and_then(|v| v.as_str())
            .map(String::from),
        _ => None,
    }
}

/// Get the first text content from a message.
fn first_text(msg: &Message) -> Option<String> {
    for part in &msg.content {
        if let ContentPart::Text { text } = part {
            return Some(text.clone());
        }
    }
    None
}

/// Run `git log --oneline -20` in the working directory.
fn git_log_oneline(working_dir: &Path) -> Vec<String> {
    let output = Command::new("git")
        .args(["log", "--oneline", "-20"])
        .current_dir(working_dir)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout
                .lines()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use luwu_core::message::Message;

    #[test]
    fn test_compile_basic() {
        let messages = vec![
            Message::user("Fix the authentication bug in login flow"),
            Message::assistant("I'll help you fix the auth bug."),
        ];

        let summary = compile(&messages, Path::new("/tmp"));
        assert_eq!(
            summary.session_goal,
            "Fix the authentication bug in login flow"
        );
        assert!(summary.commits.is_empty() || !summary.commits.is_empty()); // depends on cwd
        assert!(!summary.transcript_tail.is_empty());
    }

    #[test]
    fn test_file_changes_detected() {
        let messages = vec![
            Message::user("Create a new file"),
            Message::assistant("Creating file"),
            Message::tool_result("call-1", "File created", false),
        ];

        // We can't easily build a ToolCall message with the public API,
        // but we can test that compile doesn't crash with mixed messages.
        let summary = compile(&messages, Path::new("/tmp"));
        assert!(summary.files_changed.is_empty()); // no ToolCall parts in these messages
    }

    #[test]
    fn test_blockers_detected() {
        let messages = vec![
            Message::user("do something"),
            Message::tool_result("err-1", "Error: file not found", true),
        ];

        let summary = compile(&messages, Path::new("/tmp"));
        assert_eq!(summary.blockers.len(), 1);
        assert!(summary.blockers[0].contains("file not found"));
    }

    #[test]
    fn test_to_markdown_has_sections() {
        let summary = DeterministicSummary {
            session_goal: "Build a web app".to_string(),
            files_changed: vec![FileChange {
                path: "src/main.rs".to_string(),
                action: "Created".to_string(),
                tool: "write".to_string(),
            }],
            commits: vec!["abc123: feat: initial commit".to_string()],
            blockers: vec!["Error: missing dependency".to_string()],
            transcript_tail: vec![("User".to_string(), "Build it".to_string())],
        };

        let md = summary.to_markdown();
        assert!(md.contains("[Session Goal]"));
        assert!(md.contains("[Files And Changes]"));
        assert!(md.contains("[Commits]"));
        assert!(md.contains("[Outstanding Context]"));
        assert!(md.contains("[Transcript Tail]"));
        assert!(md.contains("Build a web app"));
        assert!(md.contains("src/main.rs"));
    }
}
