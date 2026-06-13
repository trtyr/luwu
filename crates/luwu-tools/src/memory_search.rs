//! memory_search tool — persistent memory search + observation drill-down.

use async_trait::async_trait;
use luwu_core::{Tool, ToolContext, ToolOutput};
use luwu_memory::MemoryStore;
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

pub struct MemorySearchTool;

impl MemorySearchTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MemorySearchTool {
    fn default() -> Self {
        Self
    }
}

fn path_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
        r#"(?:crates|src|tests|docs)/[^\s:'\")]+|[a-zA-Z_][a-zA-Z0-9_/]*\.(rs|toml|json|md|py)"#
    ).expect("static path regex")
    })
}

fn expand_observation(store: &MemoryStore, index: usize) -> String {
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

fn drill_down(store: &MemoryStore, working_dir: &std::path::Path, index: usize) -> String {
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
                let t = if content.len() > 5000 {
                    format!("{}...[truncated]", &content[..5000])
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

fn touched_files(store: &MemoryStore) -> String {
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
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    let mut out = String::from("Files touched:\n");
    for (p, c) in sorted {
        out.push_str(&format!("  {p} ({c}x)\n"));
    }
    out
}

static RE_N: OnceLock<Regex> = OnceLock::new();
static RE_NPATH: OnceLock<Regex> = OnceLock::new();

#[async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &str {
        "memory_search"
    }

    fn description(&self) -> &str {
        "Searches persistent memory and session observations. Four modes: keyword search across all memory layers, #N to expand observation N, #N:path for file drill-down from observation N, mode:touched to list all referenced files."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search keyword or special syntax" }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: Value, context: ToolContext) -> luwu_core::Result<ToolOutput> {
        let query = input.get("query").and_then(|v| v.as_str()).unwrap_or("");
        if query.trim().is_empty() {
            return Ok(ToolOutput::error("query is required"));
        }
        let home = dirs::home_dir().map(|h| h.join(".luwu")).ok_or_else(|| {
            luwu_core::LuwuError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "home dir not found",
            ))
        })?;
        let store = MemoryStore::new(&home, &context.working_dir, "");
        let q = query.trim();

        if q == "mode:touched" {
            return Ok(ToolOutput::text(touched_files(&store)));
        }

        let npath_re =
            RE_NPATH.get_or_init(|| Regex::new(r#"^#(\d+):path$"#).expect("static npath regex"));
        if let Some(c) = npath_re.captures(q) {
            let idx: usize = c[1].parse().unwrap_or(0);
            return Ok(ToolOutput::text(drill_down(
                &store,
                &context.working_dir,
                idx,
            )));
        }

        let n_re = RE_N.get_or_init(|| Regex::new(r#"^#(\d+)$"#).expect("static n regex"));
        if let Some(c) = n_re.captures(q) {
            let idx: usize = c[1].parse().unwrap_or(0);
            return Ok(ToolOutput::text(expand_observation(&store, idx)));
        }

        Ok(ToolOutput::text(store.search_all(query)))
    }
}
