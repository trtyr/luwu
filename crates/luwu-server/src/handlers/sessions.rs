//! Session CRUD and cancel endpoints.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::Json;

use luwu_core::SessionSummary;

use crate::app::AppState;
use crate::types::*;

pub async fn list_sessions(State(state): State<Arc<AppState>>) -> Json<SessionListResponse> {
    let sessions = state.sessions.list().await;
    Json(SessionListResponse { sessions })
}

pub async fn create_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSessionRequest>,
) -> axum::response::Response {
    let resolved = match state.config.resolve(req.provider.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                e.to_string(),
            )
                .into_response();
        }
    };

    let model = req.model.unwrap_or(resolved.model);
    let session_ref = if let Some(provider) = &req.provider {
        state.sessions.create_with_provider(&model, provider).await
    } else {
        state.sessions.create(&model).await
    };

    Json(CreateSessionResponse {
        id: session_ref.id,
        model,
    })
    .into_response()
}

pub async fn get_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> axum::response::Response {
    match state.sessions.get(&id).await {
        Some(session) => {
            let summary = SessionSummary {
                id: session.data.id.to_string(),
                model: session.data.model.clone(),
                message_count: session.data.messages.len(),
                title: session.data.title.clone(),
                created_at: session.data.created_at,
                updated_at: session.data.updated_at,
                is_running: session.is_running,
            };
            Json(summary).into_response()
        }
        None => (axum::http::StatusCode::NOT_FOUND, "Session not found").into_response(),
    }
}

pub async fn delete_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> axum::response::Response {
    if state.sessions.delete(&id).await {
        (axum::http::StatusCode::OK, "Deleted").into_response()
    } else {
        (axum::http::StatusCode::NOT_FOUND, "Session not found").into_response()
    }
}

pub async fn cancel_turn(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> axum::response::Response {
    if state.sessions.cancel(&id).await {
        Json(serde_json::json!({"status": "cancelled"})).into_response()
    } else {
        (
            axum::http::StatusCode::NOT_FOUND,
            "Session not found or not running",
        )
            .into_response()
    }
}
