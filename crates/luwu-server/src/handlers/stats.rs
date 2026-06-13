//! Runtime stats endpoint — lightweight observability for the agent server.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;

use crate::app::AppState;

/// GET /v1/stats — runtime statistics.
///
/// Returns session counts, worker task counts, and uptime info.
/// This is a lightweight polling endpoint — no locks held for long.
#[tracing::instrument(skip(state))]
pub async fn stats(State(state): State<Arc<AppState>>) -> axum::response::Response {
    let sessions = state.sessions.list().await;
    let total = sessions.len();
    let running = sessions.iter().filter(|s| s.is_running).count();

    let worker_count = state
        .worker_tasks
        .try_lock()
        .map(|tasks| tasks.len())
        .unwrap_or(0);

    Json(serde_json::json!({
        "sessions": {
            "total": total,
            "running": running,
        },
        "workers": {
            "active": worker_count,
        },
    }))
    .into_response()
}
