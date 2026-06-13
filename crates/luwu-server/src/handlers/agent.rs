//! Agent chat endpoint — thin HTTP transport over [`AgentService`].
//!
//! The handler extracts the HTTP request, resolves config (mapping errors to
//! status codes), constructs the service, and maps [`AgentEvent`]s to SSE.

use std::convert::Infallible;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::response::sse::{Event as SseEvent, Sse};

use luwu_core::{RunningGuard, TrySetRunningError};

use crate::app::{AppState, create_provider};
use crate::services::agent_service::{AgentEvent, AgentService};
use crate::types::*;

#[tracing::instrument(skip(state))]
pub async fn agent_chat(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<AgentChatRequest>,
) -> axum::response::Response {
    let should_stream = req.stream;

    // ── HTTP gate: atomic check-and-set running state ──
    let cancel_token = match state.sessions.try_set_running(&id).await {
        Ok(t) => t,
        Err(TrySetRunningError::AlreadyRunning) => {
            return (
                axum::http::StatusCode::CONFLICT,
                "Session already has a running turn",
            )
                .into_response();
        }
        Err(TrySetRunningError::NotFound) => {
            return (axum::http::StatusCode::NOT_FOUND, "Session not found").into_response();
        }
    };

    // Safety net: auto-reset is_running on drop.
    let _running_guard = RunningGuard::new(state.sessions.clone(), id.clone());

    // ── HTTP gate: session lookup ──
    let session = match state.sessions.get(&id).await {
        Some(s) => s,
        None => {
            let _ = state.sessions.set_running(&id, false).await;
            return (axum::http::StatusCode::NOT_FOUND, "Session not found").into_response();
        }
    };

    // ── HTTP gate: config resolution ──
    let resolved = match state.config.resolve(session.data.provider.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            let _ = state.sessions.set_running(&id, false).await;
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    // ── Build service ──
    let provider = create_provider(&resolved, state.http_client.clone());
    let working_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

    let service = AgentService::new(
        state.clone(),
        provider,
        resolved,
        session.data.id.to_string(),
        working_dir,
    );

    // ── Run ──
    let model = session.data.model.clone();
    let messages = session.data.messages.clone();

    let mut event_rx = service
        .run(req.message, messages, model, Some(cancel_token))
        .await;

    // ── Transport: map AgentEvent → SSE ──
    let stream = async_stream::stream! {
        // For non-streaming: collect text, emit single ChatResponse at the end.
        if !should_stream {
            let mut collected = String::new();
            while let Some(agent_event) = event_rx.recv().await {
                match agent_event {
                    AgentEvent::Turn(luwu_core::TurnEvent::TextDelta { delta }) => {
                        collected.push_str(&delta);
                    }
                    AgentEvent::Turn(luwu_core::TurnEvent::Done { .. }) => {
                        let response = serde_json::json!({
                            "type": "done",
                            "assistant_text": collected,
                        });
                        yield Ok::<_, Infallible>(SseEvent::default().data(response.to_string()));
                        break;
                    }
                    AgentEvent::Turn(luwu_core::TurnEvent::Cancelled) => {
                        let evt = serde_json::json!({ "type": "cancelled" });
                        yield Ok(SseEvent::default().data(evt.to_string()));
                        break;
                    }
                    AgentEvent::Turn(luwu_core::TurnEvent::Error { message }) => {
                        let evt = serde_json::json!({ "type": "error", "message": message });
                        yield Ok(SseEvent::default().data(evt.to_string()));
                        break;
                    }
                    _ => {}
                }
            }
            return;
        }

        // For streaming: forward all events as they arrive.
        while let Some(agent_event) = event_rx.recv().await {
            match agent_event {
                AgentEvent::Turn(turn_event) => {
                    let json = serde_json::to_string(&turn_event).unwrap();
                    yield Ok::<_, Infallible>(SseEvent::default().data(json));
                }
                AgentEvent::ToolCheckpoint { count } => {
                    let evt = serde_json::json!({
                        "type": "checkpoint",
                        "trigger": "tool_calls",
                        "count": count,
                    });
                    yield Ok(SseEvent::default().data(evt.to_string()));
                }
                AgentEvent::CycleCheckpoint { cycle, usage_pct } => {
                    let evt = serde_json::json!({
                        "type": "checkpoint",
                        "cycle": cycle,
                        "usage_pct": usage_pct,
                    });
                    yield Ok(SseEvent::default().data(evt.to_string()));
                }
                AgentEvent::Consolidation { files } => {
                    let evt = serde_json::json!({
                        "type": "consolidation",
                        "files": files,
                    });
                    yield Ok(SseEvent::default().data(evt.to_string()));
                }
                AgentEvent::Rebuild { cycle } => {
                    let evt = serde_json::json!({
                        "type": "rebuild",
                        "cycle": cycle,
                    });
                    yield Ok(SseEvent::default().data(evt.to_string()));
                }
            }
        }
    };

    let sse = Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new().interval(std::time::Duration::from_secs(15)),
    );

    sse.into_response()
}
