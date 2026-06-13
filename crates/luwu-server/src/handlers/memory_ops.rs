//! Memory checkpoint and history search endpoints.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;

use luwu_memory::MemoryStore;

use crate::app::AppState;

/// GET /v1/sessions/{id}/checkpoint — get latest checkpoint.
pub async fn get_checkpoint(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let luwu_home = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".luwu");
    let memory = MemoryStore::new(&luwu_home, &state.working_dir, &id);

    match memory.read_checkpoint() {
        Some(cp) => {
            let json = serde_json::json!({
                "session_id": id,
                "checkpoint": cp,
                "raw": memory.read_checkpoint_raw(),
            });
            Json(json).into_response()
        }
        None => (
            axum::http::StatusCode::NOT_FOUND,
            "No checkpoint found for this session",
        )
            .into_response(),
    }
}

/// GET /v1/sessions/{id}/history?q=keyword — search session history.
pub async fn search_history(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> axum::response::Response {
    let luwu_home = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".luwu");
    let memory = MemoryStore::new(&luwu_home, &state.working_dir, &id);

    let query = params.get("q").map(|s| s.as_str()).unwrap_or("");
    let limit: usize = params
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);

    if query.is_empty() {
        // Return recent history.
        match memory.history_log() {
            Ok(log) => match log.read_all() {
                Ok(entries) => {
                    let json = serde_json::json!({
                        "session_id": id,
                        "entries": entries.iter().rev().take(limit).collect::<Vec<_>>(),
                        "total": entries.len(),
                    });
                    return Json(json).into_response();
                }
                Err(e) => {
                    return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
                        .into_response();
                }
            },
            Err(e) => {
                return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
                    .into_response();
            }
        }
    }

    match memory.search_history(query, limit) {
        Ok(entries) => {
            let json = serde_json::json!({
                "session_id": id,
                "query": query,
                "entries": entries,
            });
            Json(json).into_response()
        }
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
