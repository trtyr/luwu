//! Edit tool — make precise, targeted text replacements in existing files.
//!
//! Supports two editing modes:
//! 1. **Anchor mode** — pass an `anchor` (from `read` output) to replace a single
//!    verified line. The edit tool checks the hash before modifying, preventing
//!    accidental edits to the wrong line if the file changed.
//! 2. **Text match mode** — pass `old_text` + `new_text` to find and replace
//!    arbitrary text spans (can span multiple lines).
//!
//! Text match has a three-tier fallback: Strict → Resilient (whitespace-normalized) → Fuzzy (suggestion only).

use async_trait::async_trait;
use luwu_core::{Result, Tool, ToolContext, ToolOutput};
use serde_json::Value;
use tracing::info;

use crate::hashline;

pub struct EditTool;

impl Default for EditTool {
    fn default() -> Self {
        Self
    }
}

impl EditTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Makes precise text replacements in an existing file. \
         Supports two modes: anchor-based and text-match-based. \
         \
         **Anchor mode** (recommended for single-line edits): \
         - Pass `anchor` in the format `{line}:{hash}` (copied from `read` output). \
         - The tool verifies the line still matches the hash before editing. \
         - If the file was modified since you last read it, the edit is rejected \
           with a clear mismatch message — `read` the file again for fresh anchors. \
         - `new_text` replaces the entire anchored line. \
         - Set `new_text` to empty string to delete the line. \
         \
         Example anchor flow: \
         1. `read({path: \"src/main.rs\"})` → output includes `42:4bf|fn hello() {` \
         2. `edit({path: \"src/main.rs\", anchor: \"42:4bf\", new_text: \"fn goodbye() {\"})` \
         \
         **Text match mode** (for multi-line or pattern-based edits): \
         - Pass `old_text` and `new_text`. The tool finds `old_text` in the file \
           and replaces it with `new_text`. \
         - Matching falls back through three tiers: \
           Strict (exact) → Resilient (whitespace-tolerant) → Fuzzy (suggestion only). \
         - Use `read` first to get the exact text, then copy it into `old_text`. \
         - Set `replace_all` to true to replace every occurrence. \
         \
         Important: \
         - The file must already exist. For new files, use `write`. \
         - Always `read` before `edit` — the LINE:HASH anchors make edits safer."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file, relative to the project working directory. \
                     The file must already exist."
                },
                "anchor": {
                    "type": "string",
                    "description": "A LINE:HASH anchor from `read` output, e.g. `\"42:4bf\"`. \
                     When provided, the tool verifies the line at that position still has the same hash, \
                     then replaces it with `new_text`. Takes priority over `old_text` if both are given. \
                     Only replaces a single line. For multi-line edits, use `old_text` instead."
                },
                "old_text": {
                    "type": "string",
                    "description": "The exact text to find and replace. Ignored when `anchor` is provided. \
                     Copy it character-for-character from `read` output, including all whitespace."
                },
                "new_text": {
                    "type": "string",
                    "description": "The replacement text. For anchor mode, this replaces the entire line. \
                     Pass empty string to delete the matched line/text."
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Text match mode only. If true, replace every occurrence of `old_text`. \
                     Default: false (first occurrence only)."
                }
            },
            "required": ["path", "new_text"]
        })
    }

    async fn execute(&self, input: Value, context: ToolContext) -> Result<ToolOutput> {
        let path = input["path"].as_str().ok_or_else(|| {
            luwu_core::LuwuError::Tool(
                "The 'path' parameter is required. \
                 Provide the file path relative to the working directory."
                    .into(),
            )
        })?;

        let new_text = input["new_text"].as_str().unwrap_or("");

        let file_path = context.working_dir.join(path);

        // Security check.
        let canonical = file_path.canonicalize().map_err(|e| {
            luwu_core::LuwuError::Tool(format!(
                "File not found: `{path}`\n\
                 Error: {e}\n\
                 The file must exist before you can edit it. Use `write` to create new files."
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

        // Read current content.
        let content = tokio::fs::read_to_string(&canonical).await.map_err(|e| {
            luwu_core::LuwuError::Tool(format!(
                "Failed to read `{path}`: {e}\n\
                 The file may have incorrect permissions or be a binary file."
            ))
        })?;

        // ── Anchor mode ──
        if let Some(anchor_str) = input["anchor"].as_str() {
            return execute_anchor_edit(&content, anchor_str, new_text, path, &canonical).await;
        }

        // ── Text match mode ──
        let old_text = input["old_text"].as_str().ok_or_else(|| {
            luwu_core::LuwuError::Tool(
                "Either `anchor` or `old_text` is required. \
                 Use `anchor` for single-line edits (recommended), \
                 or `old_text` for multi-line edits."
                    .into(),
            )
        })?;

        if old_text.is_empty() {
            return Ok(ToolOutput::error(
                "The 'old_text' parameter is empty. You must provide the exact text to find \
                 and replace. Use `read` to view the file, then copy the target text.",
            ));
        }

        let replace_all = input["replace_all"].as_bool().unwrap_or(false);
        execute_text_edit(&content, old_text, new_text, replace_all, path, &canonical).await
    }
}

// ─── Anchor mode ───

async fn execute_anchor_edit(
    content: &str,
    anchor_str: &str,
    new_text: &str,
    path: &str,
    canonical: &std::path::Path,
) -> Result<ToolOutput> {
    let (line_num, expected_hash) = hashline::parse_anchor(anchor_str).ok_or_else(|| {
        luwu_core::LuwuError::Tool(format!(
            "Invalid anchor format: `{anchor_str}`. \
             Expected format: `line_number:hash` (e.g. `42:4bf`). \
             Copy the anchor from `read` output."
        ))
    })?;

    // Verify anchor.
    let old_line = hashline::verify_anchor(content, line_num, expected_hash).map_err(|e| {
        luwu_core::LuwuError::Tool(e)
    })?;

    let total_lines = content.lines().count();

    // Replace the line.
    let new_content = replace_line(content, line_num, new_text);

    info!(
        path = %path,
        line = line_num,
        mode = "anchor",
        "Editing file"
    );

    tokio::fs::write(canonical, &new_content)
        .await
        .map_err(|e| luwu_core::LuwuError::Tool(format!("Failed to write changes to `{path}`: {e}")))?;

    if new_text.is_empty() {
        Ok(ToolOutput::text(format!(
            "Deleted line {line_num} in `{path}` (was: `{old_line}`, {} → {} lines)",
            total_lines,
            total_lines - 1
        )))
    } else {
        // Return the new line's anchor so the LLM can verify or chain edits.
        let new_hash = hashline::line_hash(new_text);
        Ok(ToolOutput::text(format!(
            "Replaced line {line_num} in `{path}`.\n  \
             Before: {line_num}:{expected_hash}|{old_line}\n  \
             After:  {line_num}:{new_hash}|{new_text}"
        )))
    }
}

/// Replace a single line (1-indexed) in the content.
fn replace_line(content: &str, line_num: usize, new_text: &str) -> String {
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    if line_num > 0 && line_num <= lines.len() {
        if new_text.is_empty() {
            lines.remove(line_num - 1);
        } else {
            lines[line_num - 1] = new_text.to_string();
        }
    }
    lines.join("\n")
}

// ─── Text match mode ───

async fn execute_text_edit(
    content: &str,
    old_text: &str,
    new_text: &str,
    replace_all: bool,
    path: &str,
    canonical: &std::path::Path,
) -> Result<ToolOutput> {
    // ── Tier 1: Strict (exact) match ──
    let count = content.matches(old_text).count();

    if count > 0 {
        return apply_replacement(
            content,
            old_text,
            new_text,
            replace_all,
            count,
            path,
            canonical,
            MatchTier::Strict,
        )
        .await;
    }

    // ── Tier 2: Resilient (whitespace-normalized) match ──
    let norm_old = normalize(old_text);
    let norm_content = normalize(content);
    let resilient_count = norm_content.matches(&norm_old).count();

    if resilient_count > 0 {
        if let Some((actual_old, actual_count)) =
            find_resilient_match(content, old_text, &norm_old, replace_all)
        {
            return apply_replacement(
                content,
                &actual_old,
                new_text,
                replace_all,
                actual_count,
                path,
                canonical,
                MatchTier::Resilient,
            )
            .await;
        }
    }

    // ── Tier 3: Fuzzy suggestion (no auto-apply) ──
    let total_lines = content.lines().count();
    let suggestion = find_fuzzy_suggestion(content, old_text);

    let first_line_of_old = old_text.lines().next().unwrap_or("");
    let snippet = find_nearby_snippet(content, first_line_of_old);

    Ok(ToolOutput::error(format!(
        "`old_text` was not found in `{path}` ({total_lines} lines).\n\n\
         The text must match exactly — even one extra space or different indentation \
         will cause a mismatch.\n\n\
         Suggestion: `read` the file again and copy the exact text. \
         The file may have changed since you last viewed it.\n\n\
         First line of your old_text:\n  `{first_line_of_old}`\n\
         {snippet}\n\n\
         {suggestion}"
    )))
}

// ─── Apply the replacement and write back ───

#[derive(Debug, Clone, Copy, PartialEq)]
enum MatchTier {
    Strict,
    Resilient,
}

#[allow(clippy::too_many_arguments)]
async fn apply_replacement(
    content: &str,
    old_text: &str,
    new_text: &str,
    replace_all: bool,
    count: usize,
    path: &str,
    canonical: &std::path::Path,
    tier: MatchTier,
) -> Result<ToolOutput> {
    // Multiple matches guard.
    if count > 1 && !replace_all {
        let line_nums = find_match_lines(content, old_text);
        let lines_str = format_line_ranges(&line_nums);

        return Ok(ToolOutput::error(format!(
            "`old_text` was found {} times in `{path}` (lines: {lines_str}), \
             but `replace_all` is false so only the first would be replaced.\n\n\
             Options:\n\
             - Set `replace_all` to true to replace all {} occurrences.\n\
             - Include more surrounding context in `old_text` to match a single occurrence.\n\
             - Use `read` to view the specific lines and craft a more precise match.",
            count, count
        )));
    }

    let new_content = if replace_all {
        content.replace(old_text, new_text)
    } else {
        content.replacen(old_text, new_text, 1)
    };

    let replaced = if replace_all { count } else { 1 };

    info!(
        path = %path,
        occurrences = count,
        replaced,
        tier = ?tier,
        "Editing file"
    );

    // Write back.
    tokio::fs::write(canonical, &new_content)
        .await
        .map_err(|e| luwu_core::LuwuError::Tool(format!("Failed to write changes to `{path}`: {e}")))?;

    let old_lines = old_text.lines().count();
    let new_lines = new_text.lines().count();
    let action = if new_text.is_empty() {
        "Deleted"
    } else {
        "Replaced"
    };

    let tier_note = match tier {
        MatchTier::Strict => String::new(),
        MatchTier::Resilient => {
            "\nNote: Matched via resilient mode (whitespace-normalized). \
             The indentation or spacing in the file differed slightly from your `old_text`."
                .to_string()
        }
    };

    Ok(ToolOutput::text(format!(
        "{action} {replaced} occurrence(s) in `{path}` ({} lines → {} lines){tier_note}",
        old_lines, new_lines
    )))
}

// ─── Tier 2: Resilient matching ───

/// Normalize text for resilient comparison:
/// - Strip leading/trailing whitespace per line
/// - Collapse multiple spaces/tabs into a single space
/// - Normalize line endings
fn normalize(text: &str) -> String {
    text.lines()
        .map(|line| {
            let trimmed = line.trim();
            let mut result = String::with_capacity(trimmed.len());
            let mut prev_was_space = false;
            for ch in trimmed.chars() {
                if ch.is_whitespace() {
                    if !prev_was_space {
                        result.push(' ');
                        prev_was_space = true;
                    }
                } else {
                    result.push(ch);
                    prev_was_space = false;
                }
            }
            result
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn find_resilient_match(
    content: &str,
    _old_text: &str,
    norm_old: &str,
    replace_all: bool,
) -> Option<(String, usize)> {
    let content_lines: Vec<&str> = content.lines().collect();
    let old_line_count = norm_old.lines().count();

    if old_line_count == 0 || old_line_count > content_lines.len() {
        return None;
    }

    let mut matches = Vec::new();

    for start in 0..=(content_lines.len() - old_line_count) {
        let window: Vec<&str> = content_lines[start..start + old_line_count].to_vec();
        let window_text = window.join("\n");
        let norm_window = normalize(&window_text);

        if norm_window == norm_old {
            matches.push(window_text);
            if !replace_all {
                break;
            }
        }
    }

    if matches.is_empty() {
        return None;
    }

    let count = matches.len();
    let actual_old = matches.into_iter().next().expect("matches non-empty checked above");
    Some((actual_old, count))
}

// ─── Tier 3: Fuzzy suggestion ───

fn find_fuzzy_suggestion(content: &str, old_text: &str) -> String {
    let content_lines: Vec<&str> = content.lines().collect();
    let old_lines: Vec<&str> = old_text.lines().collect();

    if old_lines.is_empty() || content_lines.is_empty() {
        return String::new();
    }

    let old_line_count = old_lines.len();
    let window_count = old_line_count.max(3).min(20);

    let old_words: Vec<&str> = old_text.split_whitespace().collect();
    if old_words.is_empty() {
        return String::new();
    }

    let old_word_set: std::collections::HashSet<&str> =
        old_words.iter().copied().collect();

    let mut best_start = 0;
    let mut best_score = 0.0f64;

    let window_size = window_count.min(content_lines.len());
    for start in 0..=(content_lines.len() - window_size) {
        let window: String = content_lines[start..start + window_size].join("\n");
        let window_words: Vec<&str> = window.split_whitespace().collect();

        let window_word_set: std::collections::HashSet<&str> =
            window_words.iter().copied().collect();

        let intersection = old_word_set
            .iter()
            .filter(|w| window_word_set.contains(*w))
            .count();

        let union = old_word_set.len() + window_word_set.len() - intersection;
        let score = if union > 0 {
            intersection as f64 / union as f64
        } else {
            0.0
        };

        if score > best_score {
            best_score = score;
            best_start = start;
        }
    }

    if best_score < 0.15 {
        return "No close match found in the file. The text may have been removed or heavily modified.".into();
    }

    let match_end = (best_start + window_size).min(content_lines.len());
    let match_lines: Vec<String> = content_lines[best_start..match_end]
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let line_num = best_start + i + 1;
            format!("  {:>4}│{}", line_num, line)
        })
        .collect();

    let pct = (best_score * 100.0) as u8;
    let confidence = if best_score >= 0.7 {
        "high"
    } else if best_score >= 0.4 {
        "medium"
    } else {
        "low"
    };

    format!(
        "Fuzzy match ({pct}% similar, {confidence} confidence) near lines {}-{}:\n{}\n\n\
         If this looks right, `read` the file and copy the exact text from those lines.",
        best_start + 1,
        match_end,
        match_lines.join("\n")
    )
}

// ─── Helpers ───

fn find_nearby_snippet(content: &str, fragment: &str) -> String {
    if fragment.is_empty() {
        return String::new();
    }

    let keyword = fragment
        .split_whitespace()
        .next()
        .unwrap_or(fragment)
        .trim();

    if keyword.is_empty() {
        return String::new();
    }

    for (i, line) in content.lines().enumerate() {
        if line.contains(keyword) {
            let line_num = i + 1;
            let ctx_start = i.saturating_sub(2);
            let ctx_end = (i + 3).min(content.lines().count());
            let context_lines: Vec<String> = content
                .lines()
                .enumerate()
                .skip(ctx_start)
                .take(ctx_end - ctx_start)
                .map(|(j, l)| {
                    let prefix = if j == i { ">>>" } else { "   " };
                    format!("  {prefix} {:>4}│{}", j + 1, l)
                })
                .collect();

            return format!(
                "Nearby match on line {line_num}:\n{}",
                context_lines.join("\n")
            );
        }
    }

    "No partial match found — the text may have been removed or the file may have changed.".into()
}

fn find_match_lines(content: &str, old_text: &str) -> Vec<usize> {
    let mut lines = Vec::new();
    let mut search_from = 0;

    while let Some(pos) = content[search_from..].find(old_text) {
        let abs_pos = search_from + pos;
        let line_num = content[..abs_pos].lines().count() + 1;
        if lines.last() != Some(&line_num) {
            lines.push(line_num);
        }
        search_from = abs_pos + 1;
    }

    lines
}

fn format_line_ranges(lines: &[usize]) -> String {
    if lines.is_empty() {
        return "none".into();
    }

    let mut ranges = Vec::new();
    let mut start = lines[0];
    let mut end = lines[0];

    for &line in &lines[1..] {
        if line == end + 1 {
            end = line;
        } else {
            ranges.push(if start == end {
                format!("{start}")
            } else {
                format!("{start}-{end}")
            });
            start = line;
            end = line;
        }
    }
    ranges.push(if start == end {
        format!("{start}")
    } else {
        format!("{start}-{end}")
    });

    ranges.join(", ")
}
