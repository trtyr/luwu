//! Write tool — create or completely overwrite a file.
//!
//! This tool is for when you know the *entire* final content of a file.
//! For making small targeted changes to existing files, use `edit` instead.

use async_trait::async_trait;
use luwu_core::{Result, Tool, ToolContext, ToolOutput};
use serde_json::Value;
use tracing::{debug, info};

const MAX_WRITE_SIZE: usize = 500 * 1024; // 500 KB

pub struct WriteTool;

impl Default for WriteTool {
    fn default() -> Self {
        Self
    }
}

impl WriteTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Creates a new file or completely replaces an existing file's contents. \
         Provide the full content — the file will contain exactly what you pass in `content`, \
         nothing more, nothing less. \
         \
         When to use this tool: \
         - Creating a brand new file from scratch \
         - Rewriting a file where you know the complete desired content \
         - Replacing a small file entirely (config files, short scripts, etc.) \
         \
         When NOT to use this tool: \
         - Modifying a few lines in an existing file — use `edit` instead. \
           Using `write` for small edits risks accidentally dropping content from \
           the parts you didn't intend to change. \
         \
         Parent directories are created automatically. \
         Maximum content size is 500KB."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file, relative to the project working directory. \
                     Parent directories are created automatically if they don't exist. \
                     Examples: \"src/main.rs\", \"config/app.toml\"."
                },
                "content": {
                    "type": "string",
                    "description": "The complete file content to write. \
                     The file will contain exactly this text — no merging with existing content. \
                     Make sure to include all necessary lines, imports, closing braces, etc."
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, input: Value, context: ToolContext) -> Result<ToolOutput> {
        debug!("Tool executing: write");
        let path = input["path"].as_str().ok_or_else(|| {
            luwu_core::LuwuError::Tool(
                "The 'path' parameter is required. \
                 Provide the file path relative to the working directory, e.g. \"src/main.rs\"."
                    .into(),
            )
        })?;

        let content = input["content"].as_str().ok_or_else(|| {
            luwu_core::LuwuError::Tool(
                "The 'content' parameter is required. \
                 Provide the complete file content to write."
                    .into(),
            )
        })?;

        if content.is_empty() {
            return Ok(ToolOutput::error(
                "The 'content' parameter is empty. If you want to create an empty file, \
                 pass a single newline `\"\\n\"`. If you want to delete a file, use `bash` \
                 with `rm <path>` instead.",
            ));
        }

        if content.len() > MAX_WRITE_SIZE {
            return Ok(ToolOutput::error(format!(
                "Content is too large: {} bytes (max 500KB). \
                 Consider splitting into multiple smaller files, \
                 or use `edit` to modify only the parts that need changing.",
                content.len()
            )));
        }

        let file_path = context.working_dir.join(path);

        // Security check — stay within working directory.
        let canonical_dir = context
            .working_dir
            .canonicalize()
            .unwrap_or_else(|_| context.working_dir.clone());

        // Create parent dirs if needed.
        if let Some(parent) = file_path.parent() {
            if !parent.exists() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    luwu_core::LuwuError::Tool(format!(
                        "Failed to create parent directories for `{path}`: {e}"
                    ))
                })?;
            }

            if parent.exists() {
                let canonical_parent = parent.canonicalize().map_err(|e| {
                    luwu_core::LuwuError::Tool(format!("Invalid path `{path}`: {e}"))
                })?;
                if !canonical_parent.starts_with(&canonical_dir) {
                    return Ok(ToolOutput::error(
                        "Access denied: path resolves outside the working directory.",
                    ));
                }
            }
        }

        let is_new = !file_path.exists();

        info!(path = %path, size = content.len(), new = is_new, "Writing file");

        tokio::fs::write(&file_path, content).await.map_err(|e| {
            luwu_core::LuwuError::Tool(format!(
                "Failed to write to `{path}`: {e}\n\
                 Check that the path is valid and you have write permissions."
            ))
        })?;

        let line_count = content.lines().count();
        let action = if is_new { "Created" } else { "Overwrote" };

        Ok(ToolOutput::text(format!(
            "{action} `{path}` ({} lines, {} bytes)",
            line_count,
            content.len()
        )))
    }
}
