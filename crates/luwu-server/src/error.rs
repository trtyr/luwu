//! Server-level error types.
//!
//! Centralized HTTP error mapping: handlers return `Result<_, ApiError>`
//! and `ApiError`'s `IntoResponse` impl converts each variant to the
//! appropriate status code + JSON body. New handlers should `use` this
//! type and propagate errors via `?` instead of building inline
//! `StatusCode` tuples.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use luwu_core::LuwuError;

use crate::config::ConfigError;

/// Centralized API error type for consistent HTTP status mapping.
#[derive(Debug)]
pub enum ApiError {
    /// Resource not found (HTTP 404).
    NotFound(String),
    /// Malformed request or invalid config (HTTP 400).
    BadRequest(String),
    /// Conflict with current resource state (HTTP 409) — e.g. agent
    /// already running on the target session.
    Conflict(String),
    /// LLM authentication/authorization failed (HTTP 401).
    Unauthorized(String),
    /// LLM upstream timed out (HTTP 504 Gateway Timeout).
    GatewayTimeout(String),
    /// Catch-all server-side problem (HTTP 500). The variants above
    /// should be preferred for known categories.
    Internal(String),
}

impl From<LuwuError> for ApiError {
    /// Map `LuwuError` variants to HTTP status codes. The goal is to
    /// avoid the `ApiError::Internal` big-bucket: known categories get
    /// specific codes so clients can react appropriately (retry on 504,
    /// refresh credentials on 401, etc.).
    ///
    /// Mapping:
    /// - `LlmAuth`     → 401 Unauthorized
    /// - `LlmTimeout`  → 504 Gateway Timeout
    /// - `Session`     → 404 Not Found
    /// - `Config`      → 400 Bad Request
    /// - other         → 500 Internal Server Error
    fn from(e: LuwuError) -> Self {
        match e {
            LuwuError::LlmAuth(msg) => ApiError::Unauthorized(msg),
            LuwuError::LlmTimeout => ApiError::GatewayTimeout(
                "LLM request timed out".to_string(),
            ),
            LuwuError::Session(_) => ApiError::NotFound(e.to_string()),
            LuwuError::Config(_) => ApiError::BadRequest(e.to_string()),
            other => ApiError::Internal(other.to_string()),
        }
    }
}

/// Map `ConfigError` into the centralized `ApiError` so handlers can use
/// `?` on `Config::load()` / `Config::resolve()` results.
///
/// Mapping rationale:
/// - `Parse` / `InvalidConfig` → 400 BadRequest (the user authored a bad
///   config.toml; they need to fix it)
/// - `ProviderNotFound` → 404 NotFound (the user asked for a named provider
///   that doesn't exist in the file)
/// - `Io` / `NoDefaultProvider` → 500 Internal (server-side problem:
///   filesystem or server misconfiguration)
impl From<ConfigError> for ApiError {
    fn from(e: ConfigError) -> Self {
        match e {
            ConfigError::Parse(_, _) => ApiError::BadRequest(e.to_string()),
            ConfigError::InvalidConfig(_) => ApiError::BadRequest(e.to_string()),
            ConfigError::ProviderNotFound(_) => ApiError::NotFound(e.to_string()),
            ConfigError::Io(_, _) | ConfigError::NoDefaultProvider => {
                ApiError::Internal(e.to_string())
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, msg),
            ApiError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg),
            ApiError::GatewayTimeout(msg) => (StatusCode::GATEWAY_TIMEOUT, msg),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };
        (status, message).into_response()
    }
}
