//! Error types for luwu-server.
//!
//! [`ApiError`] is the unified error type for all HTTP handlers.
//! It maps internal errors to appropriate HTTP status codes and
//! JSON error responses.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

/// Errors that can occur during API request handling.
#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
            ApiError::Internal(msg) => {
                tracing::error!(error = %self, "Internal server error");
                (StatusCode::INTERNAL_SERVER_ERROR, msg.clone())
            }
        };

        Json(json!({ "error": message })).into_response_with_status(status)
    }
}

/// Helper trait to attach a status code to a Json response.
trait IntoResponseWithStatus {
    fn into_response_with_status(self, status: StatusCode) -> Response;
}

impl IntoResponseWithStatus for Json<serde_json::Value> {
    fn into_response_with_status(self, status: StatusCode) -> Response {
        let mut response = self.into_response();
        *response.status_mut() = status;
        response
    }
}

impl From<luwu_core::LuwuError> for ApiError {
    fn from(e: luwu_core::LuwuError) -> Self {
        match e {
            luwu_core::LuwuError::Session(msg) => ApiError::NotFound(msg),
            luwu_core::LuwuError::Config(msg) => ApiError::BadRequest(msg),
            other => ApiError::Internal(other.to_string()),
        }
    }
}
