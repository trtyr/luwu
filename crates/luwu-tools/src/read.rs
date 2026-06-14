//! Read tool — view file contents or list directory entries.
//!
//! When reading files, each line is prefixed with a LINE:HASH anchor:
//! `{line_number}:{hash}|{content}`
//!
//! The hash is a 3-char hex fingerprint of the line content. It can be
//! used as an `anchor` parameter in the `edit` tool for verified edits.

use async_trait::async_trait;
use luwu_core::{Result, Tool, ToolContext, ToolOutput};
use serde_json::Value;
use tracing::debug;

use crate::hashline;

const MAX_READ_SIZE: usize = 100 * 1024; // 100 KB

pub struct ReadTool;

impl Default for ReadTool {
    fn default() -> Self {
        Self
    }
}

impl ReadTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Reads a file's contents or lists a directory's entries. \
         \
         When given a file path: \
         - Returns the file content with LINE:HASH anchors. \
           Each line is formatted as `{line_number}:{hash}|{content}`. \
         - The `{line_number}:{hash}` prefix is called an 'anchor'. \
           Copy the anchor and pass it to `edit` to make verified changes \
           — the edit tool will check that the line still matches the hash \
           before modifying it, preventing accidental edits to the wrong line. \
         - Use `offset` to start from a specific line (1-indexed) and `limit` to \
           restrict how many lines are returned. This is useful for large files. \
         - Binary files return a summary instead of raw content. \
         - Maximum output is ~100KB. For larger files, use `offset` and `limit` \
           to read in chunks, or use `bash` with `head`/`tail`. \
         \
         When given a directory path: \
         - Returns a listing of files and subdirectories sorted alphabetically, \
           with directories marked with a trailing `/` and file sizes shown. \
         - Use `.` to list the project root directory. \
         \
         Always `read` a file before using `edit` to modify it — the LINE:HASH \
         anchors ensure your edits target the correct lines."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to a file or directory, relative to the project working directory. \
                     Use `.` to read the root directory. File examples: \"src/main.rs\". \
                     Directory examples: \"src\", \"crates\"."
                },
                "offset": {
                    "type": "integer",
                    "description": "For files only. The 1-indexed line number to start reading from. \
                     Useful for reading specific sections of large files. \
                     Example: offset=50 starts reading from line 50."
                },
                "limit": {
                    "type": "integer",
                    "description": "For files only. Maximum number of lines to return. \
                     Use with `offset` to read a specific range. \
                     Example: offset=1, limit=50 reads the first 50 lines."
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, input: Value, context: ToolContext) -> Result<ToolOutput> {
        debug!("Tool executing: read");
        let path = input["path"].as_str().ok_or_else(|| {
            luwu_core::LuwuError::Tool(
                "The 'path' parameter is required. \
                     Provide a file or directory path relative to the working directory."
                    .into(),
            )
        })?;

        let file_path = context.working_dir.join(path);

        // Security check — stay within working directory.
        let canonical_dir = context
            .working_dir
            .canonicalize()
            .unwrap_or_else(|_| context.working_dir.clone());

        // Resolve path.
        let canonical = match file_path.canonicalize() {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolOutput::error(format!(
                    "Path not found: `{path}`\n\
                     Error: {e}\n\
                     Check the path is correct. Use `.` to list the root directory, \
                     or `bash` with `ls` to explore."
                )));
            }
        };

        if !canonical.starts_with(&canonical_dir) {
            return Ok(ToolOutput::error(
                "Access denied: path resolves outside the working directory.",
            ));
        }

        // Dispatch: directory or file.
        if canonical.is_dir() {
            read_directory(&canonical, path).await
        } else {
            let offset = input["offset"].as_u64().unwrap_or(0) as usize;
            let limit = input["limit"].as_u64().map(|l| l as usize);
            read_file(&canonical, path, offset, limit).await
        }
    }
}

async fn read_file(
    canonical: &std::path::Path,
    path: &str,
    offset: usize,
    limit: Option<usize>,
) -> Result<ToolOutput> {
    let content = tokio::fs::read(canonical)
        .await
        .map_err(|e| luwu_core::LuwuError::Tool(format!("Failed to read `{path}`: {e}")))?;

    // Binary check.
    if is_binary(&content) {
        let size = content.len();
        return Ok(ToolOutput::text(format!(
            "Binary file: `{path}` ({size} bytes). \
             Use `bash` with `file` or `xxd | head` for binary inspection."
        )));
    }

    let text = String::from_utf8_lossy(&content);
    let total_lines = text.lines().count();
    let mut lines: Vec<&str> = text.lines().collect();

    // Apply offset (1-indexed).
    if offset > 0 && offset <= lines.len() {
        lines = lines.split_off(offset - 1);
    }

    // Apply limit.
    if let Some(limit) = limit {
        lines.truncate(limit);
    }

    let start_line = if offset > 0 { offset } else { 1 };

    // Build output with LINE:HASH anchors.
    let mut result_lines = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let line_num = start_line + i;
        result_lines.push(hashline::format_line(line_num, line));
    }

    let mut result = result_lines.join("\n");

    if result.len() > MAX_READ_SIZE {
        let shown_count = result_lines.len();
        let end = result.floor_char_boundary(MAX_READ_SIZE);
        result.truncate(end);
        if let Some(pos) = result.rfind('\n') {
            result.truncate(pos);
        }
        let shown = result.lines().count();
        result.push_str(&format!(
            "\n\n[Output truncated — file has {} total lines, showing {} lines. \
             Use `offset` and `limit` to read specific sections, \
             e.g. offset={}, limit=50.]",
            total_lines,
            shown.min(shown_count),
            start_line + shown + 1
        ));
    }

    Ok(ToolOutput::text(result))
}

async fn read_directory(canonical: &std::path::Path, path: &str) -> Result<ToolOutput> {
    let mut read_dir = tokio::fs::read_dir(canonical).await.map_err(|e| {
        luwu_core::LuwuError::Tool(format!("Failed to read directory `{path}`: {e}"))
    })?;

    let mut file_entries: Vec<String> = Vec::new();
    let mut dir_entries: Vec<String> = Vec::new();

    while let Some(entry) = read_dir
        .next_entry()
        .await
        .map_err(|e| luwu_core::LuwuError::Tool(format!("Error reading entry: {e}")))?
    {
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry
            .file_type()
            .await
            .map(|ft| ft.is_dir())
            .unwrap_or(false);

        if is_dir {
            dir_entries.push(format!("{name}/"));
        } else {
            let size = entry.metadata().await.map(|m| m.len()).unwrap_or(0);
            file_entries.push(format_entry(&name, size));
        }
    }

    dir_entries.sort();
    file_entries.sort();

    let mut result = dir_entries;
    result.extend(file_entries);

    if result.is_empty() {
        return Ok(ToolOutput::text("(empty directory)"));
    }

    Ok(ToolOutput::text(result.join("\n")))
}

fn format_entry(name: &str, size: u64) -> String {
    if size < 1024 {
        format!("{name:<40} {size} B")
    } else if size < 1024 * 1024 {
        format!("{name:<40} {:.1} KB", size as f64 / 1024.0)
    } else {
        format!("{name:<40} {:.1} MB", size as f64 / (1024.0 * 1024.0))
    }
}

/// Quick binary check — look for null bytes in the first 8KB.
fn is_binary(data: &[u8]) -> bool {
    let check_len = data.len().min(8192);
    data[..check_len].contains(&0)
}
