//! Tool system abstraction.
//!
//! The [`Tool`] trait is the interface for anything the agent can *do* —
//! run a shell command, read a file, search code, etc. Tools are registered
//! with the turn engine and exposed to the LLM as callable functions.

use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;

use crate::error::Result;
use crate::event::SessionId;

/// Output from a tool execution.
#[derive(Debug, Clone)]
pub struct ToolOutput {
    /// The text content of the tool's result.
    pub content: String,
    /// Whether the tool execution failed.
    pub is_error: bool,
}

impl ToolOutput {
    /// Create a successful text output.
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }

    /// Create an error output.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: message.into(),
            is_error: true,
        }
    }
}

/// Context provided to a tool during execution.
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// Working directory for the tool execution.
    pub working_dir: PathBuf,
    /// The current session.
    pub session_id: SessionId,
}

/// A tool that the agent can invoke.
///
/// Each tool (bash, file read/write, grep, etc.) implements this trait.
/// Tools are registered in a registry and their definitions are passed
/// to the LLM as callable functions.
#[async_trait]
pub trait Tool: Send + Sync {
    /// The unique name of this tool (e.g. "bash", "read_file").
    fn name(&self) -> &str;

    /// A human-readable description of what the tool does.
    /// This is sent to the LLM to help it decide when to call the tool.
    fn description(&self) -> &str;

    /// JSON Schema describing the tool's input parameters.
    fn parameters_schema(&self) -> Value;

    /// Execute the tool with the given input.
    async fn execute(&self, input: Value, context: ToolContext) -> Result<ToolOutput>;
}
