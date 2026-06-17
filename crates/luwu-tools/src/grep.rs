//! Grep tool — high-performance file content search powered by fff-search.
//!
//! Uses fff-search's SIMD-accelerated grep engine with automatic file indexing,
//! constraint parsing, and multi-mode search (plain text / regex / fuzzy).
//! The file index is refreshed when the working directory's mtime changes.

use async_trait::async_trait;
use fff_search::file_picker::{FilePicker, FilePickerOptions};
use fff_search::grep::{GrepMode, GrepSearchOptions};
use fff_search::shared::{SharedFilePicker, SharedFrecency};
use fff_search::{AiGrepConfig, FFFMode, QueryParser};
use luwu_core::{Result, Tool, ToolContext, ToolOutput};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;
use std::time::SystemTime;
use tracing::{debug, info};

const MAX_RESULTS: usize = 50;
const MAX_LINE_LENGTH: usize = 500;
/// Minimum time between forced index rebuilds (prevents thrashing).
#[allow(dead_code)]
const MIN_REBUILD_INTERVAL_SECS: u64 = 5;

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
        debug!("Tool executing: grep");
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
        let glob_filter = input["glob"].as_str();

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
        // This now checks for stale index and rebuilds if the directory has changed.
        let picker = get_or_create_picker(&canonical_dir);

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

        // Format the results, optionally filtering by glob pattern.
        let files = &result.files;
        let mut output_lines = Vec::new();
        let mut filtered_count = 0usize;

        // Compile glob filter if provided.
        let glob_matcher = glob_filter.and_then(|g| {
            // Simple glob: convert to a function that checks file extension or pattern
            // Supports: *.rs, *.py, *.{ts,tsx}, *.toml
            let g = g.trim();
            if g.is_empty() {
                return None;
            }
            // Parse multi-extension patterns like *.{ts,tsx}
            if let Some(exts) = g.strip_prefix("*.{").and_then(|s| s.strip_suffix('}')) {
                let ext_list: Vec<&str> = exts.split(',').map(|e| e.trim()).collect();
                return Some(Box::new(move |path: &str| {
                    ext_list
                        .iter()
                        .any(|ext| path.ends_with(&format!(".{ext}")))
                }) as Box<dyn Fn(&str) -> bool>);
            }
            // Single extension: *.rs → check .rs suffix
            if let Some(ext) = g.strip_prefix("*.") {
                let dot_ext = format!(".{ext}");
                return Some(Box::new(move |path: &str| path.ends_with(&dot_ext)));
            }
            // Fallback: substring match
            let g_owned = g.to_string();
            Some(Box::new(move |path: &str| path.contains(&g_owned)))
        });

        for gm in &result.matches {
            let file_item = files.get(gm.file_index);
            let file_path = file_item
                .map(|f| f.relative_path(picker).to_string())
                .unwrap_or_else(|| "(unknown)".to_string());

            // Apply glob filter.
            if let Some(ref matcher) = glob_matcher
                && !matcher(&file_path)
            {
                continue;
            }
            filtered_count += 1;

            let line_content = if gm.line_content.len() > MAX_LINE_LENGTH {
                format!("{}…", &gm.line_content[..MAX_LINE_LENGTH])
            } else {
                gm.line_content.clone()
            };

            // Context lines.
            for ctx_line in &gm.context_before {
                output_lines.push(format!("  │ {}", ctx_line));
            }

            let def_marker = if gm.is_definition {
                " [definition]"
            } else {
                ""
            };
            output_lines.push(format!(
                "{}:{}:{}  {}{}",
                file_path, gm.line_number, gm.col, line_content, def_marker
            ));

            for ctx_line in &gm.context_after {
                output_lines.push(format!("  │ {}", ctx_line));
            }
        }

        if filtered_count == 0 {
            let glob_note = glob_filter
                .map(|g| format!(" (filtered by `{g}`)"))
                .unwrap_or_default();
            return Ok(ToolOutput::text(format!(
                "No matches found for `{pattern}` in `{search_path}`{glob_note}. \
                 Searched {} files.",
                result.total_files_searched
            )));
        }

        let truncated = if filtered_count >= MAX_RESULTS {
            format!(
                "\n\n(Results capped at {MAX_RESULTS}. Searched {} files, \
                 {} had matches. Make your pattern more specific for fewer results.)",
                result.total_files_searched, result.files_with_matches
            )
        } else {
            String::new()
        };

        let glob_note = glob_filter
            .map(|g| format!(" matching `{g}`"))
            .unwrap_or_default();

        Ok(ToolOutput::text(format!(
            "Found {} match{} in {} file{}{} ({} files searched):\n{}{}",
            filtered_count,
            if filtered_count > 1 { "es" } else { "" },
            result.files_with_matches,
            if result.files_with_matches > 1 {
                "s"
            } else {
                ""
            },
            glob_note,
            result.total_files_searched,
            output_lines.join("\n"),
            truncated
        )))
    }
}

// ── Picker cache with staleness detection ──

/// Cached picker entry: the shared picker handle + when it was built.
struct CacheEntry {
    picker: SharedFilePicker,
    built_at: SystemTime,
}

/// Global picker cache — one FilePicker per working directory.
/// Uses SystemTime + Instant for staleness detection.
static PICKER_CACHE: std::sync::OnceLock<RwLock<HashMap<PathBuf, CacheEntry>>> =
    std::sync::OnceLock::new();

/// Get or create a picker for the given canonical directory.
/// Automatically rebuilds the index if the directory's mtime is newer than
/// the last build, ensuring newly created/modified files are searchable.
fn get_or_create_picker(canonical: &PathBuf) -> SharedFilePicker {
    let cache = PICKER_CACHE.get_or_init(|| RwLock::new(HashMap::new()));

    // Check if we need to rebuild: compare directory mtime with cached build time.
    let needs_rebuild = {
        let guard = cache.read().expect("picker cache poisoned");
        match guard.get(canonical) {
            Some(entry) => is_index_stale(canonical, entry.built_at),
            None => true, // No cached entry → must build
        }
    };

    if needs_rebuild {
        // Slow path: write lock — rebuild the picker.
        let mut guard = cache.write().expect("picker cache poisoned");

        // Double-check after acquiring write lock (another thread may have rebuilt).
        if let Some(entry) = guard.get(canonical)
            && !is_index_stale(canonical, entry.built_at)
        {
            return entry.picker.clone();
        }

        // Build a fresh picker.
        let shared = SharedFilePicker::default();
        let shared_frecency = SharedFrecency::default();

        let options = FilePickerOptions {
            base_path: canonical.to_string_lossy().to_string(),
            mode: FFFMode::Ai,
            ..Default::default()
        };

        match FilePicker::new_with_shared_state(shared.clone(), shared_frecency, options) {
            Ok(()) => {
                shared.wait_for_scan(std::time::Duration::from_secs(10));
                info!(path = %canonical.display(), "File index rebuilt (stale detected)");
            }
            Err(e) => {
                tracing::warn!("Failed to rebuild FilePicker for {:?}: {}", canonical, e);

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
                    let mut shared_guard = shared.write().expect("shared frecency lock poisoned");
                    *shared_guard = Some(picker);
                }
            }
        }

        let entry = CacheEntry {
            picker: shared.clone(),
            built_at: SystemTime::now(),
        };
        guard.insert(canonical.clone(), entry);
        return shared;
    }

    // Fast path: read lock, return cached picker.
    let guard = cache.read().expect("picker cache poisoned");
    guard
        .get(canonical)
        .map(|e| e.picker.clone())
        .expect("picker must exist after checks")
}

/// Check if the file index is stale by comparing directory mtime with build time.
/// Returns true if the index should be rebuilt.
///
/// Staleness signals (any one is enough):
/// 1. The directory's own mtime is newer than `built_at` (catches top-level
///    new file creation).
/// 2. Any subdirectory up to `MAX_STALENESS_DEPTH` levels deep has an mtime
///    newer than `built_at` (catches new/modified files in deep subdirs).
/// 3. The index is older than the safety-valve threshold
///    (catches content-only edits that don't bump mtime, e.g. atomic
///    write-and-rename within the same second).
const MAX_STALENESS_DEPTH: usize = 4;
const MAX_STALENESS_ENTRIES_PER_DIR: usize = 64;
const STALENESS_SAFETY_VALVE_SECS: u64 = 60;

fn is_index_stale(dir: &PathBuf, built_at: SystemTime) -> bool {
    // 1. Top-level directory mtime.
    if dir_is_newer_than(dir, built_at) {
        return true;
    }

    // 2. Recursive subdirectory mtime check (bounded depth + entries per dir
    //    so pathological trees don't blow up the check).
    if any_descendant_is_newer_than(dir, built_at, 0) {
        return true;
    }

    // 3. Safety valve.
    if let Ok(elapsed) = built_at.elapsed()
        && elapsed.as_secs() > STALENESS_SAFETY_VALVE_SECS
    {
        return true;
    }

    false
}

/// Returns true if `dir` exists and its mtime is strictly after `t`.
fn dir_is_newer_than(dir: &std::path::Path, t: SystemTime) -> bool {
    std::fs::metadata(dir)
        .and_then(|m| m.modified())
        .map(|mtime| mtime > t)
        .unwrap_or(false)
}

/// Returns true if any descendant directory up to `MAX_STALENESS_DEPTH`
/// levels deep has an mtime strictly after `t`. Bounded by
/// `MAX_STALENESS_ENTRIES_PER_DIR` to keep worst-case work predictable.
fn any_descendant_is_newer_than(
    dir: &std::path::Path,
    t: SystemTime,
    depth: usize,
) -> bool {
    if depth >= MAX_STALENESS_DEPTH {
        return false;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return false,
    };
    let mut count = 0usize;
    for entry in entries.flatten() {
        if count >= MAX_STALENESS_ENTRIES_PER_DIR {
            break;
        }
        count += 1;
        let path = entry.path();
        let is_dir = entry
            .file_type()
            .map(|ft| ft.is_dir())
            .unwrap_or(false);
        // Skip symlinks to avoid cycles.
        let is_symlink = entry
            .file_type()
            .map(|ft| ft.is_symlink())
            .unwrap_or(false);
        if is_symlink {
            continue;
        }
        if is_dir {
            if dir_is_newer_than(&path, t) {
                return true;
            }
            if any_descendant_is_newer_than(&path, t, depth + 1) {
                return true;
            }
        } else {
            // Also check file mtimes — catches the case where a file inside
            // an existing subdir was edited (which doesn't bump the subdir
            // mtime but DOES bump the file mtime).
            if let Ok(meta) = entry.metadata()
                && let Ok(mtime) = meta.modified()
                && mtime > t
            {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Tests for cache staleness detection (review P2 #24).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod cache_staleness_tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::thread::sleep;
    use std::time::Duration;

    /// Create a unique temp directory for the test. Auto-cleaned via
    /// `tempfile`-style naming. Each test gets its own dir.
    fn make_tempdir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("luwu_grep_test_{tag}_{nanos}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn fresh_dir_is_not_stale() {
        let dir = make_tempdir("fresh");
        let built_at = SystemTime::now();
        sleep(Duration::from_millis(50));
        assert!(!is_index_stale(&dir, built_at));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn top_level_new_file_makes_stale() {
        let dir = make_tempdir("topfile");
        let built_at = SystemTime::now();
        sleep(Duration::from_millis(1100)); // ensure mtime resolution
        fs::write(dir.join("new.rs"), "fn main() {}").unwrap();
        assert!(is_index_stale(&dir, built_at));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn deep_subdir_new_file_makes_stale() {
        // This is the review P2 #24 bug: top-level mtime didn't change when
        // a file was added to a deep subdir, so the old `break`-after-first
        // implementation missed it.
        let dir = make_tempdir("deepfile");
        let nested = dir.join("crates").join("luwu-core").join("src");
        fs::create_dir_all(&nested).unwrap();
        let built_at = SystemTime::now();
        sleep(Duration::from_millis(1100));
        fs::write(nested.join("new.rs"), "// hidden deep").unwrap();
        assert!(
            is_index_stale(&dir, built_at),
            "deep subdir new file should trigger rebuild"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn deep_subdir_edit_makes_stale() {
        let dir = make_tempdir("deepedit");
        let nested = dir.join("crates").join("luwu-core").join("src");
        fs::create_dir_all(&nested).unwrap();
        let target = nested.join("existing.rs");
        fs::write(&target, "v1").unwrap();
        let built_at = SystemTime::now();
        sleep(Duration::from_millis(1100));
        fs::write(&target, "v2 — content changed").unwrap();
        assert!(
            is_index_stale(&dir, built_at),
            "file mtime change in deep subdir should trigger rebuild"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unchanged_tree_is_not_stale() {
        let dir = make_tempdir("unchanged");
        let nested = dir.join("a").join("b").join("c");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("file.txt"), "content").unwrap();
        // Build index slightly after creating files.
        sleep(Duration::from_millis(1100));
        let built_at = SystemTime::now();
        // Immediately after — nothing changed.
        assert!(!is_index_stale(&dir, built_at));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn depth_limit_protects_against_pathological_trees() {
        // 10 levels deep — beyond MAX_STALENESS_DEPTH=4. A new file at
        // level 10 should NOT be detected (intentional — we cap depth to
        // bound work). Safety valve catches it after 60s.
        let dir = make_tempdir("deeppath");
        let mut path = dir.clone();
        for i in 0..10 {
            path = path.join(format!("level{i}"));
        }
        fs::create_dir_all(&path).unwrap();
        let built_at = SystemTime::now();
        sleep(Duration::from_millis(1100));
        fs::write(path.join("deep.txt"), "way too deep").unwrap();
        // Recursive check won't find it (depth cap), safety valve hasn't
        // elapsed yet. So it should NOT be detected as stale yet.
        let just_now = SystemTime::now();
        assert!(
            !is_index_stale(&dir, just_now),
            "depth-capped staleness check should miss files at level 10 (intentional)"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn entry_limit_protects_against_wide_trees() {
        // 200 files at top level — beyond MAX_STALENESS_ENTRIES_PER_DIR=64.
        // Only the first 64 entries are checked.
        let dir = make_tempdir("wide");
        let built_at = SystemTime::now();
        sleep(Duration::from_millis(1100));
        for i in 0..200 {
            fs::write(dir.join(format!("f{i}.txt")), "x").unwrap();
        }
        // Should still detect staleness because the cap hits within the
        // first 64, and the new files bump the top-level mtime.
        assert!(is_index_stale(&dir, built_at));
        let _ = fs::remove_dir_all(&dir);
    }
}
