//! Task list endpoint — returns the current todo list for a session.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde::Serialize;

use crate::app::AppState;

#[derive(Debug, Serialize)]
pub struct TasksResponse {
    pub tasks: Vec<serde_json::Value>,
}

/// GET /v1/sessions/{id}/tasks — returns the current task list.
pub async fn list_tasks(
    Path(id): Path<String>,
    State(_state): State<Arc<AppState>>,
) -> axum::response::Response {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "no home dir").into_response();
        }
    };
    let path = home
        .join(".luwu")
        .join("sessions")
        .join(&id)
        .join("tasks.json");

    match std::fs::read_to_string(&path) {
        Ok(content) => {
            // tasks.json is { "tasks": [...], "next_id": N }
            // Return just the tasks array, filtering out deleted tombstones.
            let store: serde_json::Value = serde_json::from_str(&content).unwrap_or_default();
            let tasks = store
                .get("tasks")
                .and_then(|t| t.as_array())
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|t| {
                    t.get("status")
                        .and_then(|s| s.as_str())
                        .map(|s| s != "deleted")
                        .unwrap_or(true)
                })
                .collect::<Vec<_>>();
            Json(TasksResponse { tasks }).into_response()
        }
        Err(_) => Json(TasksResponse { tasks: vec![] }).into_response(),
    }
}
