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
        let end = body.floor_char_boundary(max);
        format!("{}…", &body[..end])
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    // ─── truncate_body ────────────────────────────────────

    #[test]
    fn truncate_under_limit_unchanged() {
        assert_eq!(truncate_body("hello", 10), "hello");
    }

    #[test]
    fn truncate_at_exact_limit() {
        assert_eq!(truncate_body("hello", 5), "hello");
    }

    #[test]
    fn truncate_over_limit_adds_ellipsis() {
        assert_eq!(truncate_body("hello world", 5), "hello…");
    }

    #[test]
    fn truncate_empty_string() {
        assert_eq!(truncate_body("", 100), "");
    }

    // ─── LlmError Display ─────────────────────────────────

    #[test]
    fn error_display_formats() {
        assert_eq!(LlmError::Timeout.to_string(), "Request timed out");
        assert_eq!(
            LlmError::Auth("bad key".into()).to_string(),
            "Authentication failed: bad key"
        );
        assert_eq!(
            LlmError::Stream("parse failed".into()).to_string(),
            "Stream error: parse failed"
        );
        assert_eq!(
            LlmError::Status {
                status: 429,
                body: "slow down".into()
            }
            .to_string(),
            "API returned status 429: slow down"
        );
    }

    // ─── From<LlmError> for LuwuError ─────────────────────

    #[test]
    fn converts_to_luwu_error() {
        let err: luwu_core::LuwuError = LlmError::Timeout.into();
        assert!(err.to_string().contains("Request timed out"));
    }
}
