//! Memory checkpoint and history search endpoints.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;

use luwu_memory::MemoryStore;

use crate::app::AppState;
use crate::error::ApiError;

/// GET /v1/sessions/{id}/checkpoint — get latest checkpoint.
pub async fn get_checkpoint(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<axum::response::Response, ApiError> {
    let luwu_home = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".luwu");
    let memory = MemoryStore::new(&luwu_home, &state.working_dir, &id);

    let cp = memory
        .read_checkpoint()
        .ok_or_else(|| ApiError::NotFound(format!("No checkpoint found for session '{id}'")))?;
    let json = serde_json::json!({
        "session_id": id,
        "checkpoint": cp,
        "raw": memory.read_checkpoint_raw(),
    });
    Ok(Json(json).into_response())
}

/// GET /v1/sessions/{id}/history?q=keyword — search session history.
pub async fn search_history(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<axum::response::Response, ApiError> {
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
        let log = memory
            .history_log()
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        let entries = log
            .read_all()
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        let json = serde_json::json!({
            "session_id": id,
            "entries": entries.iter().rev().take(limit).collect::<Vec<_>>(),
            "total": entries.len(),
        });
        return Ok(Json(json).into_response());
    }

    let entries = memory
        .search_history(query, limit)
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let json = serde_json::json!({
        "session_id": id,
        "query": query,
        "entries": entries,
    });
    Ok(Json(json).into_response())
}
