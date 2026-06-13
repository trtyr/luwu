//! History — JSONL conversation log for full session replay.
//!
//! Every message, tool call, and tool result is appended to a JSONL file.
//! This is the lowest layer of the four-layer memory system — the most
//! complete but also the slowest to search.

use chrono::Utc;
use luwu_core::Message;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use tracing::debug;

/// A single entry in the history log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// Entry type: "user", "assistant", "tool_call", "tool_result".
    pub role: String,
    /// The content (text, JSON, etc.).
    pub content: String,
    /// Estimated token count for this entry.
    pub tokens: usize,
}

/// Writer for JSONL history files. Appends entries, supports search.
pub struct HistoryLog {
    path: PathBuf,
}

impl HistoryLog {
    /// Create or open a history log at the given path.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Ensure file exists.
        File::create(path)?;
        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    /// Append a history entry.
    pub fn append(&self, entry: &HistoryEntry) -> std::io::Result<()> {
        let mut file = OpenOptions::new().append(true).open(&self.path)?;
        let json = serde_json::to_string(entry)?;
        writeln!(file, "{json}")?;
        debug!(role = %entry.role, tokens = entry.tokens, "Appended history entry");
        Ok(())
    }

    /// Append a message from the conversation.
    pub fn append_message(&self, msg: &Message, estimator: &TokenEstimator) -> std::io::Result<()> {
        let entry = HistoryEntry {
            timestamp: Utc::now().to_rfc3339(),
            role: format!("{:?}", msg.role),
            content: serde_json::to_string(&msg.content).unwrap_or_default(),
            tokens: estimator.estimate_message(msg),
        };
        self.append(&entry)
    }

    /// Read all entries from the history log.
    pub fn read_all(&self) -> std::io::Result<Vec<HistoryEntry>> {
        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if let Ok(entry) = serde_json::from_str::<HistoryEntry>(&line) {
                entries.push(entry);
            }
        }
        Ok(entries)
    }

    /// Search entries by keyword (case-insensitive).
    pub fn search(&self, query: &str, max_results: usize) -> std::io::Result<Vec<HistoryEntry>> {
        let query_lower = query.to_lowercase();
        let entries = self.read_all()?;
        let mut results = Vec::new();
        for entry in entries.into_iter().rev() {
            if results.len() >= max_results {
                break;
            }
            if entry.content.to_lowercase().contains(&query_lower) {
                results.push(entry);
            }
        }
        results.reverse();
        Ok(results)
    }

    /// Get the file path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Token usage estimator.
pub struct TokenEstimator {
    /// Characters per token ratio. Default 4 (good for English).
    /// Chinese text is closer to 2 chars/token.
    pub chars_per_token: usize,
}

impl Default for TokenEstimator {
    fn default() -> Self {
        Self { chars_per_token: 4 }
    }
}

impl TokenEstimator {
    /// Estimate tokens for a string.
    pub fn estimate(&self, text: &str) -> usize {
        if text.is_empty() {
            return 0;
        }
        let char_count = text.chars().count();
        let cjk_count = text
            .chars()
            .filter(|c| {
                ('\u{4E00}'..='\u{9FFF}').contains(c) || ('\u{3040}'..='\u{30FF}').contains(c)
            })
            .count();
        let ratio = if cjk_count * 2 > char_count {
            2 // CJK-heavy: each CJK char ≈ 1-2 tokens
        } else {
            self.chars_per_token
        };
        char_count / ratio
    }

    /// Estimate tokens for a Message (sum of all content parts).
    pub fn estimate_message(&self, msg: &Message) -> usize {
        let mut total = 0;
        for part in &msg.content {
            use luwu_core::ContentPart;
            match part {
                ContentPart::Text { text } => total += self.estimate(text),
                ContentPart::ToolCall {
                    id: _,
                    name,
                    arguments,
                } => {
                    total += self.estimate(name);
                    total += self.estimate(&arguments.to_string());
                }
                ContentPart::ToolResult {
                    id: _,
                    content,
                    is_error: _,
                } => {
                    total += self.estimate(content);
                }
            }
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_estimate_english() {
        let est = TokenEstimator::default();
        // "Hello world" = 11 chars / 4 ≈ 2 tokens
        assert_eq!(est.estimate("Hello world"), 2);
    }

    #[test]
    fn token_estimate_cjk() {
        let est = TokenEstimator::default();
        // CJK-heavy text uses ratio 2
        let text = "你好世界这是一个测试";
        assert!(est.estimate(text) > 0);
        // 10 chars / 2 = 5 tokens
        assert_eq!(est.estimate(text), 5);
    }

    #[test]
    fn history_roundtrip() {
        let dir = std::env::temp_dir().join("luwu_test_history");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("test.jsonl");
        let log = HistoryLog::open(&path).unwrap();

        log.append(&HistoryEntry {
            timestamp: "2026-01-01T00:00:00Z".into(),
            role: "user".into(),
            content: "hello".into(),
            tokens: 1,
        })
        .unwrap();

        log.append(&HistoryEntry {
            timestamp: "2026-01-01T00:00:01Z".into(),
            role: "assistant".into(),
            content: "hi there".into(),
            tokens: 2,
        })
        .unwrap();

        let entries = log.read_all().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].role, "user");
        assert_eq!(entries[1].role, "assistant");
    }
}
