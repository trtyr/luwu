//! Server-level error types.
//!
//! Currently handlers use inline `into_response()` for error mapping.
//! This module is reserved for a future centralized error-handling layer.

// ApiError and IntoResponseWithStatus were created in Phase 2 but are not
// yet wired into handlers. They remain here as scaffolding for when we
// centralize HTTP error mapping (instead of inline StatusCode tuples).
#![allow(dead_code)]

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use luwu_core::LuwuError;

/// Centralized API error type for consistent HTTP status mapping.
#[derive(Debug)]
pub enum ApiError {
    NotFound(String),
    BadRequest(String),
    Conflict(String),
    Internal(String),
}

impl From<LuwuError> for ApiError {
    fn from(e: LuwuError) -> Self {
        match e {
            LuwuError::Session(_) => ApiError::NotFound(e.to_string()),
            LuwuError::Config(_) => ApiError::BadRequest(e.to_string()),
            other => ApiError::Internal(other.to_string()),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, msg),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };
        (status, message).into_response()
    }
}
