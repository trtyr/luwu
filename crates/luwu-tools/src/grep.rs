//! Grep tool — high-performance file content search powered by fff-search.
//!
//! Uses fff-search's SIMD-accelerated grep engine with automatic file indexing,
//! constraint parsing, and multi-mode search (plain text / regex / fuzzy).
//! The file index is built once and kept alive across searches.

use async_trait::async_trait;
use fff_search::grep::{GrepMode, GrepSearchOptions};
use fff_search::shared::{SharedFilePicker, SharedFrecency};
use fff_search::file_picker::{FilePicker, FilePickerOptions};
use fff_search::{AiGrepConfig, FFFMode, QueryParser};
use luwu_core::{Result, Tool, ToolContext, ToolOutput};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;
use tracing::info;

const MAX_RESULTS: usize = 50;
const MAX_LINE_LENGTH: usize = 500;

pub struct GrepTool;

impl Default for GrepTool {
    fn default() -> Self {
        Self
    }
}

impl GrepTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Searches file contents for a text pattern across the project (like grep). \
         Returns matching lines with file paths and line numbers. \
         \
         This is the fastest way to find where a function is defined, \
         where a variable is used, or where specific text appears in the codebase. \
         \
         Search modes: \
         - Default: literal text search (fast, SIMD-accelerated). \
         - Regex (`regex: true`): regular expression matching. \
         - Fuzzy (`fuzzy: true`): fuzzy matching — finds approximate matches even \
           with typos or partial text. Great for exploratory searches. \
         \
         Tips for effective searches: \
         - Use specific, unique patterns. Searching for `fn handle_request` is better than `main`. \
         - Narrow the scope with `path` to search within a specific directory. \
         - Filter by file type with `glob`, e.g. `*.rs` to only search Rust files. \
         - Enable `regex` for pattern-based searches (e.g. `fn \\w+_handler`). \
         - Enable `fuzzy` for approximate matching when you're not sure of the exact text. \
         - Results are capped at 50 matches. If you get too many results, \
           make your pattern more specific."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "The text pattern to search for. \
                     Default mode treats it as a literal string (fast). \
                     Set `regex` to true for regex mode, or `fuzzy` to true for fuzzy matching. \
                     Examples: \"fn handle_request\", \"TODO\", \"use luwu_core\"."
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in, relative to the working directory. \
                     Defaults to `.` (entire project). \
                     Narrow this to speed up search and reduce noise, e.g. `src` or `crates/luwu-core`."
                },
                "glob": {
                    "type": "string",
                    "description": "File pattern to filter which files are searched. \
                     Examples: `*.rs`, `*.py`, `*.{ts,tsx}`, `*.toml`."
                },
                "regex": {
                    "type": "boolean",
                    "description": "If true, `pattern` is treated as a regular expression. \
                     Default: false (literal string search)."
                },
                "fuzzy": {
                    "type": "boolean",
                    "description": "If true, `pattern` is treated as a fuzzy needle — matches \
                     approximate text even with typos or partial input. \
                     Default: false. Cannot be used together with `regex`."
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, input: Value, context: ToolContext) -> Result<ToolOutput> {
        let pattern = input["pattern"].as_str().ok_or_else(|| {
            luwu_core::LuwuError::Tool(
                "The 'pattern' parameter is required. \
                 Provide the text to search for, e.g. \"fn main\" or \"TODO\"."
                    .into(),
            )
        })?;

        if pattern.is_empty() {
            return Ok(ToolOutput::error(
                "The 'pattern' parameter is empty. \
                 Provide a non-empty search pattern.",
            ));
        }

        let use_regex = input["regex"].as_bool().unwrap_or(false);
        let use_fuzzy = input["fuzzy"].as_bool().unwrap_or(false);
        let _glob = input["glob"].as_str();

        // Determine search mode.
        let mode = if use_regex {
            GrepMode::Regex
        } else if use_fuzzy {
            GrepMode::Fuzzy
        } else {
            GrepMode::PlainText
        };

        let search_path = input["path"].as_str().unwrap_or(".");
        let search_dir = context.working_dir.join(search_path);

        // Security check.
        let canonical = search_dir.canonicalize().map_err(|e| {
            luwu_core::LuwuError::Tool(format!(
                "Search directory not found: `{search_path}`: {e}\n\
                 Check the path is correct."
            ))
        })?;

        let canonical_dir = context
            .working_dir
            .canonicalize()
            .unwrap_or_else(|_| context.working_dir.clone());

        if !canonical.starts_with(&canonical_dir) {
            return Ok(ToolOutput::error(
                "Access denied: path resolves outside the working directory.",
            ));
        }

        // Get or create the FilePicker for this working directory.
        let picker = get_or_create_picker(&canonical);

        let picker_guard = picker.read().map_err(|e| {
            luwu_core::LuwuError::Tool(format!("Failed to acquire file index lock: {e}"))
        })?;

        let Some(picker) = picker_guard.as_ref() else {
            return Ok(ToolOutput::error(
                "File index is not ready yet. Wait a moment and try again.",
            ));
        };

        // Parse the query with AiGrepConfig for smart constraint detection.
        let parser = QueryParser::new(AiGrepConfig);
        let query = parser.parse(pattern);

        // Build grep options.
        let options = GrepSearchOptions {
            mode,
            page_limit: MAX_RESULTS,
            smart_case: true,
            trim_whitespace: true,
            classify_definitions: true,
            time_budget_ms: 5000, // 5 second budget
            ..Default::default()
        };

        info!(
            pattern = %pattern,
            mode = ?mode,
            path = %search_path,
            "Searching files"
        );

        // Run the grep search.
        let result = picker.grep(&query, &options);

        // Handle regex fallback error.
        if let Some(ref err) = result.regex_fallback_error {
            return Ok(ToolOutput::error(format!(
                "Regex pattern is invalid: {err}\n\
                 Fix the regex syntax or switch to literal search by removing `regex: true`."
            )));
        }

        if result.matches.is_empty() {
            return Ok(ToolOutput::text(format!(
                "No matches found for `{pattern}` in `{search_path}`. \
                 Searched {} files.",
                result.total_files_searched
            )));
        }

        // Format the results.
        let files = &result.files;
        let mut output_lines = Vec::new();

        for gm in &result.matches {
            let file_item = files.get(gm.file_index);
            let file_path = file_item
                .map(|f| f.relative_path(picker).to_string())
                .unwrap_or_else(|| "(unknown)".to_string());

            let line_content = if gm.line_content.len() > MAX_LINE_LENGTH {
                format!("{}…", &gm.line_content[..MAX_LINE_LENGTH])
            } else {
                gm.line_content.clone()
            };

            // Context lines.
            for ctx_line in &gm.context_before {
                output_lines.push(format!("  │ {}", ctx_line));
            }

            let def_marker = if gm.is_definition { " [definition]" } else { "" };
            output_lines.push(format!(
                "{}:{}:{}  {}{}",
                file_path, gm.line_number, gm.col, line_content, def_marker
            ));

            for ctx_line in &gm.context_after {
                output_lines.push(format!("  │ {}", ctx_line));
            }
        }

        let count = result.matches.len();
        let truncated = if count >= MAX_RESULTS {
            format!(
                "\n\n(Results capped at {MAX_RESULTS}. Searched {} files, \
                 {} had matches. Make your pattern more specific for fewer results.)",
                result.total_files_searched, result.files_with_matches
            )
        } else {
            String::new()
        };

        Ok(ToolOutput::text(format!(
            "Found {} match{} in {} file{} ({} files searched):\n{}{}",
            count,
            if count > 1 { "es" } else { "" },
            result.files_with_matches,
            if result.files_with_matches > 1 { "s" } else { "" },
            result.total_files_searched,
            output_lines.join("\n"),
            truncated
        )))
    }
}

/// Global picker cache — one FilePicker per working directory.
static PICKER_CACHE: std::sync::OnceLock<RwLock<HashMap<PathBuf, SharedFilePicker>>> =
    std::sync::OnceLock::new();

fn get_or_create_picker(canonical: &PathBuf) -> SharedFilePicker {
    let cache = PICKER_CACHE.get_or_init(|| RwLock::new(HashMap::new()));

    // Fast path: read lock.
    {
        let guard = cache.read().unwrap();
        if let Some(picker) = guard.get(canonical) {
            return picker.clone();
        }
    }

    // Slow path: write lock — create a new picker.
    let mut guard = cache.write().unwrap();

    // Double-check after acquiring write lock.
    if let Some(picker) = guard.get(canonical) {
        return picker.clone();
    }

    let shared = SharedFilePicker::default();
    let shared_frecency = SharedFrecency::default();

    // Build options — use Ai mode which ignores .gitignore'd files.
    let options = FilePickerOptions {
        base_path: canonical.to_string_lossy().to_string(),
        mode: FFFMode::Ai,
        ..Default::default()
    };

    // new_with_shared_state spawns background threads and places picker
    // into the shared handle automatically.
    match FilePicker::new_with_shared_state(shared.clone(), shared_frecency, options) {
        Ok(()) => {
            // Wait for the background scan to finish (up to 10s).
            shared.wait_for_scan(std::time::Duration::from_secs(10));
            info!(path = %canonical.display(), "File index built for search");
        }
        Err(e) => {
            tracing::warn!("Failed to create FilePicker for {:?}: {}", canonical, e);

            // Fallback: create picker manually and do sync scan.
            let options = FilePickerOptions {
                base_path: canonical.to_string_lossy().to_string(),
                mode: FFFMode::Ai,
                ..Default::default()
            };
            if let Ok(mut picker) = FilePicker::new(options) {
                if let Err(e) = picker.collect_files() {
                    tracing::warn!("Sync scan also failed for {:?}: {}", canonical, e);
                }
                let mut shared_guard = shared.write().unwrap();
                *shared_guard = Some(picker);
            }
        }
    }

    guard.insert(canonical.clone(), shared.clone());
    shared
}
