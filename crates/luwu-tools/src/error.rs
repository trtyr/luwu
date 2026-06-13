//! Error types for luwu-tools.
//!
//! Internal error classification for built-in tool implementations.
//! All `ToolError` values can be converted to [`LuwuError`] via `From`,
//! preserving backward compatibility with the `Tool` trait signature.

use thiserror::Error;

/// Errors that can occur during tool execution.
#[derive(Error, Debug)]
pub enum ToolError {
    /// Filesystem or OS I/O failure.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to parse input or output (regex, JSON, path, etc.).
    #[error("Parse error: {0}")]
    Parse(String),

    /// Invalid user-supplied input (missing field, bad path, etc.).
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// External service or dependency failure.
    #[error("External error: {0}")]
    External(String),
}

impl From<ToolError> for luwu_core::LuwuError {
    fn from(e: ToolError) -> Self {
        luwu_core::LuwuError::Tool(e.to_string())
    }
}
