//! memory tool — persistent memory management (read + write + delete).
//!
//! Inspired by pi-hermes-memory's add/replace/remove design:
//! - `search`: query all memory layers (global, project, session)
//! - `write`: append durable facts to global or project memory
//! - `delete`: remove memory entries by substring match
//! - `#N` / `#N:path` / `mode:touched`: observation drill-down (legacy)
//!
//! Categories: failure, correction, insight, preference, convention, tool-quirk
//! (categories are advisory tags in the entry text, not enforced structurally)
//!
//! ## Architecture (P2 #22 decoupling)
//!
//! This module does **not** depend on `luwu-memory`. It uses the
//! `MemoryBackend` trait from `luwu-core` instead, so any backend
//! implementation (filesystem-backed `MemoryStore` from `luwu-memory`, or
//! a test mock) can be plugged in. The `MemoryBackendFactory` closure
//! creates a fresh backend per `execute()` call to avoid state bleed
//! across concurrent invocations.

use async_trait::async_trait;
use luwu_core::memory_backend::{MemoryBackend, MemoryBackendFactory, MemoryResult};
use luwu_core::{Tool, ToolContext, ToolOutput};
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;
use tracing::debug;

/// Memory tool. Holds a `MemoryBackendFactory` and uses it to create a
/// fresh `Box<dyn MemoryBackend>` on every `execute()` call.
pub struct MemoryTool {
    factory: MemoryBackendFactory,
}

impl MemoryTool {
    /// Create a new `MemoryTool` that builds backends via the given factory.
    pub fn new(factory: MemoryBackendFactory) -> Self {
        Self { factory }
    }
}

impl Default for MemoryTool {
    fn default() -> Self {
        // No-op factory that errors — callers must provide a real factory.
        // This exists so the type can be `Default` for convenience wrappers
        // (the actual `all_builtin_tools` always passes a real factory).
        Self::new(Arc::new(|_home, _working_dir, _session_id| -> Box<dyn MemoryBackend> {
            Box::new(StubBackend)
        }))
    }
}

// `Arc` import shim so `Default::default` compiles.
use std::sync::Arc;

/// Stub backend used by `Default` — returns errors for everything. Real
/// instantiation always goes through `MemoryTool::new(factory)` with a
/// proper `MemoryBackendFactory` (see `all_builtin_tools` in lib.rs).
struct StubBackend;

impl MemoryBackend for StubBackend {
    fn read_observations(&self) -> Vec<luwu_core::memory_backend::Observation> {
        vec![]
    }
    fn read_reflections(&self) -> Vec<luwu_core::memory_backend::Reflection> {
        vec![]
    }
    fn search_all(&self, _query: &str) -> String {
        "memory backend not configured (use MemoryTool::new with a real factory)".to_string()
    }
    fn append_global_entry(&self, _entry: &str) -> MemoryResult<()> {
        Err("memory backend not configured".into())
    }
    fn append_project_entry(&self, _entry: &str) -> MemoryResult<()> {
        Err("memory backend not configured".into())
    }
    fn global_path(&self) -> &std::path::Path {
        std::path::Path::new("/dev/null")
    }
    fn project_path(&self) -> std::path::PathBuf {
        std::path::PathBuf::from("/dev/null")
    }
}

// ---------------------------------------------------------------------------
// Helpers — all take `&dyn MemoryBackend` so they don't care about the
// concrete type. Tests can call these with a mock.
// ---------------------------------------------------------------------------

fn path_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
        r#"(?:crates|src|tests|docs)/[^\s:'\")]+|[a-zA-Z_][a-zA-Z0-9_/]*\.(rs|toml|json|md|py)"#
    ).expect("static path regex")
    })
}

fn expand_observation(store: &dyn MemoryBackend, index: usize) -> String {
    let obs_list = store.read_observations();
    if index >= obs_list.len() {
        return format!("No observation at index {index}. Total: {}", obs_list.len());
    }
    let o = &obs_list[index];
    format!(
        "Observation #{index}\n  ID: {}\n  Time: {}\n  Priority: {}\n  Category: {}\n  Content: {}",
        o.id, o.timestamp, o.priority, o.category, o.content
    )
}

fn drill_down(store: &dyn MemoryBackend, working_dir: &std::path::Path, index: usize) -> String {
    let obs_list = store.read_observations();
    if index >= obs_list.len() {
        return format!("No observation at index {index}. Total: {}", obs_list.len());
    }
    let o = &obs_list[index];
    let paths: Vec<&str> = path_regex()
        .find_iter(&o.content)
        .map(|m| m.as_str())
        .collect();
    if paths.is_empty() {
        return format!(
            "Observation #{index} has no file paths.\nContent: {}",
            o.content
        );
    }
    let mut results = Vec::new();
    for p in &paths {
        match std::fs::read_to_string(working_dir.join(p)) {
            Ok(content) => {
                // Safe truncation at UTF-8 char boundary (defensive — content
                // from external files can contain CJK).
                let slice_end = content
                    .char_indices()
                    .take_while(|(i, _)| *i < 5000)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(content.len());
                let t = if slice_end < content.len() {
                    format!("{}...[truncated]", &content[..slice_end])
                } else {
                    content
                };
                results.push(format!("--- {p} ---\n{t}"));
            }
            Err(e) => results.push(format!("--- {p} ---\n[error: {e}]")),
        }
    }
    results.join("\n\n")
}

fn touched_files(store: &dyn MemoryBackend) -> String {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for o in store.read_observations() {
        for m in path_regex().find_iter(&o.content) {
            *counts.entry(m.as_str().to_string()).or_default() += 1;
        }
    }
    for r in store.read_reflections() {
        for m in path_regex().find_iter(&r.content) {
            *counts.entry(m.as_str().to_string()).or_default() += 1;
        }
    }
    if counts.is_empty() {
        return "No files referenced.".to_string();
    }
    let mut sorted: Vec<_> = counts.into_iter().collect();
    sorted.sort_by_key(|k| std::cmp::Reverse(k.1));
    let mut out = String::from("Files touched:\n");
    for (p, c) in sorted {
        out.push_str(&format!("  {p} ({c}x)\n"));
    }
    out
}

static RE_N: OnceLock<Regex> = OnceLock::new();
static RE_NPATH: OnceLock<Regex> = OnceLock::new();

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "Persistent memory system — search, write, and delete durable knowledge. \
         \
         Actions: \
         - search (default): Search all memory layers (global + project + session). \
           Special syntax: #N to expand observation N, #N:path for file drill-down, \
           mode:touched to list all referenced files. \
         - write: Append a durable memory entry. Requires 'content'. \
           Optional: target (global|project, default: project), \
           category (failure|correction|insight|preference|convention|tool-quirk). \
         - delete: Remove an entry by substring match. Requires 'pattern'. \
           Optional: target (global|project). \
         \
         Memory survives across sessions. Use it for: \
         - User preferences and project conventions \
         - Lessons learned from failures \
         - Architecture decisions and their rationale \
         - Tool quirks and environment facts \
         \
         Only store stable facts — not temporary task progress."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["search", "write", "delete"],
                    "description": "Action to perform. Default: search."
                },
                "query": {
                    "type": "string",
                    "description": "Search query (for action=search). \
                     Also supports #N, #N:path, mode:touched syntax."
                },
                "content": {
                    "type": "string",
                    "description": "Memory entry text (for action=write)."
                },
                "category": {
                    "type": "string",
                    "enum": ["failure", "correction", "insight", "preference", "convention", "tool-quirk"],
                    "description": "Category tag for the entry (for action=write). Default: insight."
                },
                "target": {
                    "type": "string",
                    "enum": ["global", "project"],
                    "description": "Memory layer (for action=write/delete). Default: project."
                },
                "pattern": {
                    "type": "string",
                    "description": "Substring to match for deletion (for action=delete)."
                }
            }
        })
    }

    async fn execute(&self, input: Value, context: ToolContext) -> luwu_core::Result<ToolOutput> {
        debug!("Tool executing: memory");
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("search");

        let home = dirs::home_dir().map(|h| h.join(".luwu")).ok_or_else(|| {
            luwu_core::LuwuError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "home dir not found",
            ))
        })?;

        // Build a fresh backend via the factory (was: `MemoryStore::new`).
        // This is the decoupling point — luwu-tools no longer knows about
        // `MemoryStore`, only the `MemoryBackend` trait.
        let store = (self.factory)(&home, &context.working_dir, "");

        match action {
            "search" => {
                let query = input.get("query").and_then(|v| v.as_str()).unwrap_or("");
                if query.trim().is_empty() {
                    return Ok(ToolOutput::error("query is required for search."));
                }
                let q = query.trim();

                if q == "mode:touched" {
                    return Ok(ToolOutput::text(touched_files(&*store)));
                }

                let npath_re = RE_NPATH
                    .get_or_init(|| Regex::new(r#"^#(\d+):path$"#).expect("static npath regex"));
                if let Some(c) = npath_re.captures(q) {
                    let idx: usize = c[1].parse().unwrap_or(0);
                    return Ok(ToolOutput::text(drill_down(
                        &*store,
                        &context.working_dir,
                        idx,
                    )));
                }

                let n_re = RE_N.get_or_init(|| Regex::new(r#"^#(\d+)$"#).expect("static n regex"));
                if let Some(c) = n_re.captures(q) {
                    let idx: usize = c[1].parse().unwrap_or(0);
                    return Ok(ToolOutput::text(expand_observation(&*store, idx)));
                }

                Ok(ToolOutput::text(store.search_all(query)))
            }

            "write" => {
                let content = input.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if content.trim().is_empty() {
                    return Ok(ToolOutput::error("content is required for write."));
                }

                let target = input
                    .get("target")
                    .and_then(|v| v.as_str())
                    .unwrap_or("project");
                let category = input
                    .get("category")
                    .and_then(|v| v.as_str())
                    .unwrap_or("insight");

                // Format entry with category tag
                let entry = format!("[{category}] {content}");

                let result = match target {
                    "global" => store.append_global_entry(&entry),
                    _ => store.append_project_entry(&entry),
                };

                match result {
                    Ok(_) => Ok(ToolOutput::text(format!(
                        "Memory saved ({target}/{category}): {content}"
                    ))),
                    Err(e) => Ok(ToolOutput::error(format!("Failed to write memory: {e}"))),
                }
            }

            "delete" => {
                let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
                if pattern.trim().is_empty() {
                    return Ok(ToolOutput::error("pattern is required for delete."));
                }

                let target = input
                    .get("target")
                    .and_then(|v| v.as_str())
                    .unwrap_or("project");

                // Read the memory file, filter out matching entries, rewrite
                let (file_path, file_content) = match target {
                    "global" => {
                        let p = store.global_path().to_path_buf();
                        let content = std::fs::read_to_string(&p).unwrap_or_default();
                        (p, content)
                    }
                    _ => {
                        let p = store.project_path();
                        let content = std::fs::read_to_string(&p).unwrap_or_default();
                        (p, content)
                    }
                };

                // Entries are §-delimited
                let entries: Vec<&str> = file_content.split("§").collect();
                let before = entries.len();
                let filtered: Vec<&str> = entries
                    .into_iter()
                    .filter(|e| !e.contains(pattern))
                    .collect();
                let removed = before - filtered.len();

                if removed == 0 {
                    return Ok(ToolOutput::text(format!(
                        "No entries matching '{pattern}' in {target} memory."
                    )));
                }

                let new_content = filtered.join("§");
                match std::fs::write(&file_path, new_content) {
                    Ok(_) => Ok(ToolOutput::text(format!(
                        "Deleted {removed} entr{plural} matching '{pattern}' from {target} memory.",
                        plural = if removed > 1 { "ies" } else { "y" }
                    ))),
                    Err(e) => Ok(ToolOutput::error(format!(
                        "Failed to update memory file: {e}"
                    ))),
                }
            }

            _ => Ok(ToolOutput::error(format!(
                "Unknown action: '{action}'. Valid: search, write, delete."
            ))),
        }
    }
}
