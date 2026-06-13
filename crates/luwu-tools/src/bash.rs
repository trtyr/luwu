//! Bash tool — execute shell commands.
//!
//! Runs a command via the system shell with working directory, timeout,
//! and token-efficient output formatting.

use async_trait::async_trait;
use luwu_core::{Result, Tool, ToolContext, ToolOutput};
use serde_json::Value;
use std::sync::OnceLock;
use tracing::debug;

/// Maximum output size in bytes (~25KB, roughly 8K tokens).
const MAX_OUTPUT: usize = 25 * 1024;
/// Default command timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Tool for executing shell commands via the system shell.
pub struct BashTool;

impl Default for BashTool {
    fn default() -> Self {
        Self
    }
}

impl BashTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Executes a shell command in a persistent bash session and returns the output. \
         The command runs in the project's working directory. \
         Use this tool for any task that requires running shell commands: \
         building projects, running tests, executing scripts, installing packages, \
         managing git, searching files, or any other system operation. \
         \
         Guidelines for using this tool: \
         - Use commands that produce concise output. For long outputs, pipe through \
         `head -50`, `tail -20`, or `wc -l` to limit what is returned. \
         - For file searches, prefer `grep -r` or `find` over listing full directories. \
         - Commands time out after 30 seconds by default. For longer-running commands, \
         set the `timeout` parameter. \
         - Output is truncated at ~25KB. If output is truncated, refine your command \
         to be more targeted (e.g. `grep` instead of `cat`, `head` instead of full output). \
         - Each invocation runs in the working directory. Use `cd` within the command \
         if you need to operate in a different directory."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The bash command to execute. Runs via `/bin/sh -c`, so pipes (`|`), \
                     redirections (`>`), and `&&` chains all work. The command runs in the project's \
                     working directory."
                },
                "timeout": {
                    "type": "integer",
                    "description": "Maximum time in seconds to wait for the command to complete. \
                     Defaults to 30. Set higher for long-running builds or tests (e.g. 120 for `cargo build`)."
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, input: Value, context: ToolContext) -> Result<ToolOutput> {
        let command = input["command"].as_str().ok_or_else(|| {
            luwu_core::LuwuError::Tool(
                "The 'command' parameter is required. \
                 Provide the shell command to execute, e.g. \"ls -la\" or \"cargo test\"."
                    .into(),
            )
        })?;

        if command.trim().is_empty() {
            return Ok(ToolOutput::error(
                "The 'command' parameter must not be empty. \
                 Provide a valid shell command to execute.",
            ));
        }

        let timeout_secs = input["timeout"].as_u64().unwrap_or(DEFAULT_TIMEOUT_SECS);

        debug!(command = %command, timeout = timeout_secs, "Executing bash command");

        // If rtk is available, prefix the command for token-efficient output.
        let final_command = if is_rtk_available() {
            format!("rtk {}", command)
        } else {
            command.to_string()
        };

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            tokio::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(&final_command)
                .current_dir(&context.working_dir)
                .output(),
        )
        .await
        .map_err(|_| {
            luwu_core::LuwuError::Tool(format!(
                "Command timed out after {timeout_secs}s: `{}`\n\
                 Tips: increase the `timeout` parameter, or break the command into smaller steps.",
                truncate_str(command, 200)
            ))
        })?
        .map_err(|e| {
            luwu_core::LuwuError::Tool(format!(
                "Failed to execute command `{}`: {e}\n\
                 The shell may not be available or the command is malformed.",
                truncate_str(command, 100)
            ))
        })?;

        let exit_code = output.status.code().unwrap_or(-1);
        let is_error = !output.status.success();

        // Build output with clear sections.
        let mut sections: Vec<String> = Vec::new();

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Only include non-empty sections to save tokens.
        if !stdout.is_empty() {
            sections.push(stdout.trim_end().to_string());
        }
        if !stderr.is_empty() {
            sections.push(format!("stderr:\n{}", stderr.trim_end()));
        }

        let mut result = sections.join("\n\n");

        // Truncate with helpful guidance.
        if result.len() > MAX_OUTPUT {
            let original_lines = result.lines().count();
            result.truncate(MAX_OUTPUT);
            // Don't cut in the middle of a line.
            if let Some(pos) = result.rfind('\n') {
                result.truncate(pos);
            }
            result.push_str(&format!(
                "\n\n[Output truncated — {} lines total, showing first {} lines. \
                 Refine your command to produce less output, e.g. pipe through `head`, \
                 `tail`, `grep`, or use more specific arguments.]",
                original_lines,
                result.lines().count(),
            ));
        }

        // Prepend exit code only on failure.
        if is_error {
            result = format!("Exit code: {exit_code}\n{result}");
        }

        Ok(ToolOutput {
            content: result,
            is_error,
        })
    }
}

/// Truncate a string for display in error messages.
fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { &s[..max] }
}

/// Check whether `rtk` is available on PATH. Result is cached.
fn is_rtk_available() -> bool {
    static RTK_AVAILABLE: OnceLock<bool> = OnceLock::new();
    *RTK_AVAILABLE.get_or_init(|| {
        std::process::Command::new("rtk")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}
