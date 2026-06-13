//! Error types for luwu-llm.
//!
//! Internal error classification for LLM provider implementations.
//! All `LlmError` values can be converted to [`LuwuError`] via `From`,
//! preserving backward compatibility with the `LlmProvider` trait signature.

use thiserror::Error;

/// Errors that can occur during LLM provider operations.
#[derive(Error, Debug)]
pub enum LlmError {
    /// HTTP transport failure (connection refused, DNS, TLS, etc.).
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// The API returned a non-success status code.
    #[error("API returned status {status}: {body}")]
    Status { status: u16, body: String },

    /// JSON serialization or deserialization failure.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Error during stream parsing or consumption.
    #[error("Stream error: {0}")]
    Stream(String),

    /// Authentication or authorization failure (401, 403).
    #[error("Authentication failed: {0}")]
    Auth(String),

    /// The request timed out.
    #[error("Request timed out")]
    Timeout,
}

impl From<LlmError> for luwu_core::LuwuError {
    fn from(e: LlmError) -> Self {
        luwu_core::LuwuError::Llm(e.to_string())
    }
}

/// Truncate a string to `max` chars, appending "…" if truncated.
/// Used to avoid leaking large API response bodies into error messages.
pub fn truncate_body(body: &str, max: usize) -> String {
    if body.len() <= max {
        body.to_string()
    } else {
        format!("{}…", &body[..max])
    }
}
