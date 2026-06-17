//! Session CRUD and cancel endpoints.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;

use luwu_core::SessionSummary;

use crate::app::AppState;
use crate::error::ApiError;
use crate::types::*;

pub async fn list_sessions(State(state): State<Arc<AppState>>) -> Json<SessionListResponse> {
    let sessions = state.sessions.list().await;
    Json(SessionListResponse { sessions })
}

pub async fn create_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<axum::response::Response, ApiError> {
    let resolved = state.config.resolve(req.provider.as_deref())?;

    let model = req.model.unwrap_or(resolved.model);
    let session_ref = if let Some(provider) = &req.provider {
        state.sessions.create_with_provider(&model, provider).await
    } else {
        state.sessions.create(&model).await
    };

    Ok(Json(CreateSessionResponse {
        id: session_ref.id,
        model,
    })
    .into_response())
}

pub async fn get_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<axum::response::Response, ApiError> {
    let session = state
        .sessions
        .get(&id)
        .await
        .ok_or_else(|| ApiError::NotFound(format!("Session '{id}' not found")))?;
    let summary = SessionSummary {
        id: session.data.id.to_string(),
        model: session.data.model.clone(),
        message_count: session.data.messages.len(),
        title: session.data.title.clone(),
        created_at: session.data.created_at,
        updated_at: session.data.updated_at,
        is_running: session.is_running,
    };
    Ok(Json(summary).into_response())
}

pub async fn delete_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<axum::response::Response, ApiError> {
    if state.sessions.delete(&id).await {
        Ok((axum::http::StatusCode::OK, "Deleted").into_response())
    } else {
        Err(ApiError::NotFound(format!("Session '{id}' not found")))
    }
}

pub async fn cancel_turn(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<axum::response::Response, ApiError> {
    if state.sessions.cancel(&id).await {
        Ok(Json(serde_json::json!({"status": "cancelled"})).into_response())
    } else {
        Err(ApiError::NotFound(format!(
            "Session '{id}' not found or not running"
        )))
    }
}
