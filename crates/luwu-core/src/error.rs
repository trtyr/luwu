//! Error types for luwu-core.
//!
//! All errors flow through [`LuwuError`]. Plugins can downstream their own
//! error types via `LuwuError::Tool` or `LuwuError::Llm`.

use thiserror::Error;

/// The unified error type for all luwu operations.
#[derive(Error, Debug)]
pub enum LuwuError {
    #[error("LLM provider error: {0}")]
    Llm(String),

    /// LLM authentication failed (401/403-equivalent). Distinguished
    /// from generic `Llm(String)` so handlers can map to HTTP 401.
    #[error("LLM authentication failed: {0}")]
    LlmAuth(String),

    /// LLM request timed out. Distinguished from generic `Llm(String)`
    /// so handlers can map to HTTP 504 (Gateway Timeout) instead of 500.
    /// Unit variant — the cause is implied by the variant name; the
    /// caller can log the upstream `LlmError` for full context.
    #[error("LLM request timed out")]
    LlmTimeout,

    #[error("Tool execution error: {0}")]
    Tool(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Session error: {0}")]
    Session(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Alias for `Result<T, LuwuError>`.
pub type Result<T> = std::result::Result<T, LuwuError>;
