//! HTTP API handlers — OpenAI-compatible endpoints + agent event stream.
//!
//! Endpoints:
//! - `GET  /health`                         — Health check
//! - `GET  /v1/models`                      — List available models
//! - `POST /v1/chat/completions`            — Chat (OpenAI-compatible, real SSE streaming)
//! - `GET  /v1/sessions`                    — List sessions
//! - `POST /v1/sessions`                    — Create session
//! - `GET  /v1/sessions/{id}`               — Get session info
//! - `DELETE /v1/sessions/{id}`             — Delete session
//! - `POST /v1/sessions/{id}/chat`          — Agent chat (full event stream + cycle management)
//! - `POST /v1/sessions/{id}/cancel`        — Cancel running turn
//! - `GET  /v1/sessions/{id}/checkpoint`    — Get latest checkpoint
//! - `GET  /v1/sessions/{id}/history`       — Search session history
//! - `POST /v1/sessions/{id}/cancel`        — Cancel running turn
//! - `GET  /v1/sessions/{id}/checkpoint`    — Get latest checkpoint
//! - `GET  /v1/sessions/{id}/history`       — Search session history

use std::convert::Infallible;
use std::sync::Arc;
use crate::types::*;

use axum::extract::{Path, State};
use axum::response::sse::{Event as SseEvent, Sse};
use axum::response::IntoResponse;
use axum::Json;
use axum::Router;
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use tokio::task::JoinSet;

use luwu_core::{
    message::Role,
    LlmProvider,
    EventBus, Message, RunningGuard, SessionManager, SessionSummary, TrySetRunningError, ToolRegistry, TurnEngine,
    TurnEvent, CycleState, CycleAction,
};
use luwu_llm::openai::OpenAiProvider;
use luwu_tools;
use luwu_memory::{
    MemoryFileType, CorrectionDetector, CorrectionPattern, MemoryStore,
    Observation, Priority, Reflection,
    apply_consolidation, consolidation_prompt,
    compile_summary, observer_prompt, reflector_prompt,
};

use crate::app::{AppState, builtin_tool_registry};

// ---------------------------------------------------------------------------
// Handlers — Health & Models
// ---------------------------------------------------------------------------

pub async fn health() -> &'static str {
    "ok"
}

pub async fn list_models(State(state): State<Arc<AppState>>) -> Json<ModelsResponse> {
    let mut models = Vec::new();

    if let Some(default_model) = &state.config.default.model {
        models.push(ModelInfo {
            id: default_model.clone(),
            object: "model".to_string(),
            created: 0,
            owned_by: state
                .config
                .default
                .provider
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
        });
    }

    for (name, provider) in &state.config.providers {
        if let Some(model) = &provider.model {
            models.push(ModelInfo {
                id: model.clone(),
                object: "model".to_string(),
                created: 0,
                owned_by: name.clone(),
            });
        }
    }

    Json(ModelsResponse {
        object: "list".to_string(),
        data: models,
    })
}

// ---------------------------------------------------------------------------
// Handlers — OpenAI-compatible Chat Completions (real streaming)
// ---------------------------------------------------------------------------

#[allow(clippy::collapsible_if)]
#[tracing::instrument(skip(state))]
pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> axum::response::Response {
    let resolved = match state.config.resolve(None) {
        Ok(r) => r,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                e.to_string(),
            )
                .into_response();
        }
    };

    // Always use the config model — ignore client-provided model name.
    let model = resolved.model.clone();

    // Extract the last user message.
    let user_msg = req
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .and_then(|m| {
            m.content.as_ref().and_then(|c| match c {
                serde_json::Value::String(s) => Some(s.clone()),
                _ => None,
            })
        })
        .unwrap_or_default();

    let should_stream = req.stream.unwrap_or(false);

    // Build engine.
    let provider = OpenAiProvider::with_client(&resolved.api_key, &resolved.base_url, state.http_client.clone());
    let tools = builtin_tool_registry();
    let events = EventBus::new(256);
    let working_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let engine = TurnEngine::new(std::sync::Arc::new(provider), tools, state.skills.clone(), events, working_dir);

    // Convert request messages to core Messages.
    let mut messages: Vec<Message> = Vec::new();
    for msg in &req.messages {
        if msg.role == "user" {
            if let Some(serde_json::Value::String(text)) = &msg.content {
                messages.push(Message::user(text));
            }
        }
    }

    if should_stream {
        // === Real SSE streaming ===
        let chunk_id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
        let model_str = model.clone();

        let event_rx = engine
            .run_stream(
                luwu_core::SessionId::new(),
                model_str.clone(),
                messages,
                user_msg,
                None,
            )
            .await;

        let stream = async_stream::stream! {
            // Role chunk.
            let role_chunk = ChatChunk {
                id: chunk_id.clone(),
                object: "chat.completion.chunk".to_string(),
                created: chrono::Utc::now().timestamp(),
                model: model_str.clone(),
                choices: vec![ChatChunkChoice {
                    index: 0,
                    delta: ChatChunkDelta {
                        role: Some("assistant".to_string()),
                        content: None,
                    },
                    finish_reason: None,
                }],
            };
            yield Ok::<_, Infallible>(
                SseEvent::default().data(serde_json::to_string(&role_chunk).unwrap())
            );

            // Forward TurnEvents as SSE chunks.
            let mut rx = event_rx;
            let mut last_reasoning: Option<String> = None; // Track reasoning to avoid duplicate content
            while let Some(event) = rx.recv().await {
                match event {
                    TurnEvent::TextDelta { delta } => {
                        // Skip if this is a reasoning fallback duplicate.
                        if let Some(ref reasoning) = last_reasoning {
                            if delta == *reasoning {
                                continue; // Engine fallback emitted reasoning as TextDelta — skip to avoid duplicate
                            }
                        }
                        let chunk = ChatChunk {
                            id: chunk_id.clone(),
                            object: "chat.completion.chunk".to_string(),
                            created: chrono::Utc::now().timestamp(),
                            model: model_str.clone(),
                            choices: vec![ChatChunkChoice {
                                index: 0,
                                delta: ChatChunkDelta {
                                    role: None,
                                    content: Some(delta),
                                },
                                finish_reason: None,
                            }],
                        };
                        yield Ok(
                            SseEvent::default().data(serde_json::to_string(&chunk).unwrap())
                        );
                    }
                    TurnEvent::ReasoningDelta { delta } => {
                        last_reasoning = Some(delta.clone());
                        // Forward reasoning as content for OpenAI compatibility.
                        let chunk = ChatChunk {
                            id: chunk_id.clone(),
                            object: "chat.completion.chunk".to_string(),
                            created: chrono::Utc::now().timestamp(),
                            model: model_str.clone(),
                            choices: vec![ChatChunkChoice {
                                index: 0,
                                delta: ChatChunkDelta {
                                    role: None,
                                    content: Some(delta),
                                },
                                finish_reason: None,
                            }],
                        };
                        yield Ok(
                            SseEvent::default().data(serde_json::to_string(&chunk).unwrap())
                        );
                    }
                    TurnEvent::ReasoningDelta { delta } => {
                        // Forward reasoning as content for OpenAI compatibility.
                        let chunk = ChatChunk {
                            id: chunk_id.clone(),
                            object: "chat.completion.chunk".to_string(),
                            created: chrono::Utc::now().timestamp(),
                            model: model_str.clone(),
                            choices: vec![ChatChunkChoice {
                                index: 0,
                                delta: ChatChunkDelta {
                                    role: None,
                                    content: Some(delta),
                                },
                                finish_reason: None,
                            }],
                        };
                        yield Ok(
                            SseEvent::default().data(serde_json::to_string(&chunk).unwrap())
                        );
                    }
                    TurnEvent::Done { .. } => {
                        // Final chunk with finish_reason: "stop".
                        let done_chunk = ChatChunk {
                            id: chunk_id.clone(),
                            object: "chat.completion.chunk".to_string(),
                            created: chrono::Utc::now().timestamp(),
                            model: model_str.clone(),
                            choices: vec![ChatChunkChoice {
                                index: 0,
                                delta: ChatChunkDelta {
                                    role: None,
                                    content: None,
                                },
                                finish_reason: Some("stop".to_string()),
                            }],
                        };
                        yield Ok(
                            SseEvent::default().data(serde_json::to_string(&done_chunk).unwrap())
                        );
                        yield Ok(SseEvent::default().data("[DONE]"));
                        break;
                    }
                    TurnEvent::Cancelled => {
                        let done_chunk = ChatChunk {
                            id: chunk_id.clone(),
                            object: "chat.completion.chunk".to_string(),
                            created: chrono::Utc::now().timestamp(),
                            model: model_str.clone(),
                            choices: vec![ChatChunkChoice {
                                index: 0,
                                delta: ChatChunkDelta {
                                    role: None,
                                    content: None,
                                },
                                finish_reason: Some("cancel".to_string()),
                            }],
                        };
                        yield Ok(
                            SseEvent::default().data(serde_json::to_string(&done_chunk).unwrap())
                        );
                        yield Ok(SseEvent::default().data("[DONE]"));
                        break;
                    }
                    TurnEvent::Error { message } => {
                        let error_chunk = serde_json::json!({
                            "error": { "message": message, "type": "server_error" }
                        });
                        yield Ok(
                            SseEvent::default().data(serde_json::to_string(&error_chunk).unwrap())
                        );
                        break;
                    }
                    // For OpenAI compat, we skip tool events in this endpoint.
                    // They are available via the /v1/sessions/{id}/chat endpoint.
                    _ => {}
                }
            }
        };

        let sse = Sse::new(stream).keep_alive(
            axum::response::sse::KeepAlive::new().interval(std::time::Duration::from_secs(15)),
        );

        sse.into_response()
    } else {
        // === Non-streaming ===
        let mut session = luwu_core::SessionData::new(model.clone());

        for msg in &req.messages {
            if msg.role == "user" {
                if let Some(serde_json::Value::String(text)) = &msg.content {
                    session.push_message(Message::user(text));
                }
            }
        }

        match engine.run(&mut session, user_msg).await {
            Ok(result) => {
                let response = ChatResponse {
                    id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                    object: "chat.completion".to_string(),
                    created: chrono::Utc::now().timestamp(),
                    model: model.clone(),
                    choices: vec![ChatChoice {
                        index: 0,
                        message: ChatResponseMessage {
                            role: "assistant".to_string(),
                            content: Some(result.assistant_text),
                            tool_calls: None,
                        },
                        finish_reason: Some("stop".to_string()),
                    }],
                    usage: ChatUsage::default(),
                };
                Json(response).into_response()
            }
            Err(e) => {
                (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers — Sessions
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Handlers — Agent Chat (full event stream with tool visibility)
// ---------------------------------------------------------------------------

#[tracing::instrument(skip(state))]
pub async fn agent_chat(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<AgentChatRequest>,
) -> axum::response::Response {
    // Atomically check-and-set running state (eliminates TOCTOU race).
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

    // Safety net: auto-reset is_running on drop (panic, disconnect, cancel).
    let _running_guard = RunningGuard::new(state.sessions.clone(), id.clone());

    // Get session for provider resolution.
    let session = match state.sessions.get(&id).await {
        Some(s) => s,
        None => {
            let _ = state.sessions.set_running(&id, false).await;
            return (axum::http::StatusCode::NOT_FOUND, "Session not found").into_response();
        }
    };

    // Resolve provider — use session's provider if set, otherwise default.
    let resolved = match state.config.resolve(session.data.provider.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            let _ = state.sessions.set_running(&id, false).await;
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                e.to_string(),
            )
                .into_response();
        }
    };

    // Build engine.
    let provider = OpenAiProvider::with_client(&resolved.api_key, &resolved.base_url, state.http_client.clone());
    let provider_arc: std::sync::Arc<dyn LlmProvider> = std::sync::Arc::new(provider);
    let tools = builtin_tool_registry();
    let events = EventBus::new(256);
    let working_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let engine = TurnEngine::new(provider_arc.clone(), tools, state.skills.clone(), events, working_dir.clone());

    // Memory store and cycle state.
    let luwu_home = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".luwu");
    let memory = std::sync::Arc::new(
        MemoryStore::new(&luwu_home, &state.working_dir, &id)
    );
    let mut cycle = CycleState::default();

    let model = session.data.model.clone();
    let messages = session.data.messages.clone();
    let session_id = session.data.id.clone();
    let sessions = state.sessions.clone();
    let id_clone = id.clone();

    // Track writer task for rebuild synchronization.
    let mut writer_handle: Option<tokio::task::JoinHandle<()>> = None;

    // Start streaming.
    let user_msg_for_session = req.message.clone();

    // Correction detection — if user is correcting us, save immediately.
    {
        let mut detector = CorrectionDetector::new();
        detector.advance_turn();
        if let Some(correction) = detector.detect(&user_msg_for_session) {
            let label = match correction.pattern_type {
                CorrectionPattern::Strong => "纠错",
                CorrectionPattern::Weak => "疑似纠错",
            };
            let entry = format!("[{}] {}", label, correction.full_message);
            let mem_c = memory.clone();
            state.spawn_worker(async move {
                if let Err(e) = mem_c.append_correction(&entry) { tracing::warn!(%e, "Failed to save correction"); }
            });
            tracing::info!("Correction detected and saved");
        }
    }
    // Clone messages for deterministic compaction + observer worker.
    let messages_for_workers = messages.clone();
    let event_rx = engine
        .run_stream(session_id, model, messages, req.message, Some(cancel_token))
        .await;

    if req.stream {
        // SSE — forward ALL TurnEvents as JSON, with cycle management.
        let stream = async_stream::stream! {
            let mut rx = event_rx;
            let mut _assistant_text = String::new();

            while let Some(event) = rx.recv().await {
                // Serialize the event.
                let json = serde_json::to_string(&event).unwrap();
                yield Ok::<_, Infallible>(SseEvent::default().data(json));

                // Track tokens and cycle state.
                match &event {
                    TurnEvent::Done { assistant_text, usage, .. } => {
                        _assistant_text = assistant_text.clone();
                        // Feed precise usage from LLM API into cycle.
                        cycle.add_tokens(usage.total_tokens as usize);

                        // Persist messages to session for multi-turn.
                        let sessions_c = sessions.clone();
                        let id_c = id_clone.clone();
                        let user_msg = user_msg_for_session.clone();
                        let asst_text = assistant_text.clone();
                        state.spawn_worker(async move {
                            let mut msgs = vec![luwu_core::Message::user(&user_msg)];
                            if !asst_text.is_empty() {
                                msgs.push(luwu_core::Message::assistant(&asst_text));
                            }
                            sessions_c.append_messages(&id_c, msgs).await;
                        });

                        // Check if any memory files need consolidation.
                        let needed = memory.check_consolidation();
                        if !needed.is_empty() {
                            for n in &needed {
                                let content = std::fs::read_to_string(&n.path).unwrap_or_default();
                                let rk2 = resolved.api_key.clone();
                                let rb2 = resolved.base_url.clone();
                                let np = n.path.clone();
                                let ft = n.file_type;
                                state.spawn_worker(async move {
                                    if let Err(e) = run_consolidation_writer(&rk2, &rb2, &content, &np, ft).await { tracing::warn!(%e, "Consolidation writer failed"); }
                                });
                            }
                            let con_evt = serde_json::json!({
                                "type": "consolidation",
                                "files": needed.iter().map(|n| n.file_type.label()).collect::<Vec<_>>(),
                            });
                            yield Ok(SseEvent::default().data(con_evt.to_string()));
                        }
                        break;
                    }
                    TurnEvent::ToolCompleted { .. } => {
                        if let CycleAction::Checkpoint = cycle.add_tool_call() {
                            cycle.mark_tool_call_checkpoint();

                            // Deterministic compaction — zero LLM cost.
                            let det_summary = compile_summary(&messages_for_workers, &working_dir);
                            if let Err(e) = memory.write_checkpoint_raw(&det_summary.to_markdown()) { tracing::warn!(%e, "Failed to write checkpoint"); }

                            // Spawn Observer worker.
                            let _wm = memory.clone();
                            let rb = resolved.base_url.clone();
                            let rk = resolved.api_key.clone();
                            let obs_msgs = messages_for_workers.clone();

                            state.spawn_worker(async move {
                                if let Err(e) = run_observer_worker(&rk, &rb, &obs_msgs, &_wm).await { tracing::warn!(%e, "Observer worker failed"); }
                            });

                            let tc_event = serde_json::json!({
                                "type": "checkpoint",
                                "trigger": "tool_calls",
                                "count": cycle.tool_usage(),
                            });
                            yield Ok(SseEvent::default().data(tc_event.to_string()));
                        }
                    }
                    TurnEvent::Cancelled | TurnEvent::Error { .. } => break,
                    _ => {}
                }

                // Cycle checkpoint/rebuild check is done per LLM call (not per event).
                // Cycle checkpoint/rebuild after each LLM call.
                match cycle.check() {
                    CycleAction::Checkpoint => {
                        let pct = cycle.usage_pct();
                        cycle.mark_checkpoint(pct);

                        let _writer_memory = memory.clone();
                        let resolved_base = resolved.base_url.clone();
                        let resolved_key = resolved.api_key.clone();
                        let obs_msgs = messages_for_workers.clone();

                        // Deterministic compaction — zero LLM cost.
                        let det_summary = compile_summary(&messages_for_workers, &working_dir);
                        if let Err(e) = memory.write_checkpoint_raw(&det_summary.to_markdown()) { tracing::warn!(%e, "Failed to write checkpoint"); }

                        // Spawn Observer worker.
                        state.spawn_worker(async move {
                            if let Err(e) = run_observer_worker(
                                &resolved_key,
                                &resolved_base,
                                &obs_msgs,
                                &_writer_memory,
                            ).await { tracing::warn!(%e, "Observer worker failed"); }
                        });

                        let cp_event = serde_json::json!({
                            "type": "checkpoint",
                            "cycle": cycle.cycle_index,
                            "usage_pct": pct,
                        });
                        yield Ok(SseEvent::default().data(cp_event.to_string()));
                    }
                    CycleAction::Rebuild => {
                        // Observer runs as tracked worker — no handle to await.

                        // Spawn Reflector to synthesize observations into reflections.
                        let _refl_memory = memory.clone();
                        let _refl_base = resolved.base_url.clone();
                        let _refl_key = resolved.api_key.clone();
                        state.spawn_worker(async move {
                            if let Err(e) = run_reflector_worker(
                                &_refl_key,
                                &_refl_base,
                                &_refl_memory,
                            ).await { tracing::warn!(%e, "Reflector worker failed"); }
                        });

                        let rb_event = serde_json::json!({
                            "type": "rebuild",
                            "cycle": cycle.cycle_index,
                        });
                        yield Ok(SseEvent::default().data(rb_event.to_string()));

                        cycle.reset_cycle();
                    }
                    CycleAction::Continue => {}
                }
            }

            // Update session.
            let _ = sessions.set_running(&id_clone, false).await;
        };

        let sse = Sse::new(stream).keep_alive(
            axum::response::sse::KeepAlive::new().interval(std::time::Duration::from_secs(15)),
        );

        sse.into_response()
    } else {
        // Non-streaming — collect all events and return the final result.
        let stream = async_stream::stream! {
            let mut rx = event_rx;
            let mut collected_text = String::new();

            while let Some(event) = rx.recv().await {
                match event {
                    TurnEvent::TextDelta { delta } => {
                        collected_text.push_str(&delta);
                    }
                    TurnEvent::Done { .. } => {
                        let resp = ChatResponse {
                            id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                            object: "chat.completion".to_string(),
                            created: chrono::Utc::now().timestamp(),
                            model: session.data.model.clone(),
                            choices: vec![ChatChoice {
                                index: 0,
                                message: ChatResponseMessage {
                                    role: "assistant".to_string(),
                                    content: Some(collected_text),
                                    tool_calls: None,
                                },
                                finish_reason: Some("stop".to_string()),
                            }],
                            usage: ChatUsage::default(),
                        };
                        yield Ok::<_, Infallible>(
                            SseEvent::default().data(serde_json::to_string(&resp).unwrap())
                        );
                        break;
                    }
                    TurnEvent::Cancelled => {
                        let err = serde_json::json!({"error": "cancelled"});
                        yield Ok(
                            SseEvent::default().data(serde_json::to_string(&err).unwrap())
                        );
                        break;
                    }
                    TurnEvent::Error { message } => {
                        let err = serde_json::json!({"error": message});
                        yield Ok(
                            SseEvent::default().data(serde_json::to_string(&err).unwrap())
                        );
                        break;
                    }
                    _ => {} // Skip tool events for non-streaming.
                }
            }

            let _ = sessions.set_running(&id_clone, false).await;
        };

        let sse = Sse::new(stream).keep_alive(
            axum::response::sse::KeepAlive::new().interval(std::time::Duration::from_secs(15)),
        );

        sse.into_response()
    }
}

/// Run the consolidation Writer — an LLM call that merges memory entries.
async fn run_consolidation_writer(
    api_key: &str,
    base_url: &str,
    content: &str,
    file_path: &std::path::Path,
    file_type: MemoryFileType,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use serde_json::json;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("failed to build worker HTTP client");
    let body = json!({
        "model": "MiniMax-M3",
        "temperature": 0.1,
        "max_tokens": 4096,
        "messages": [
            {"role": "system", "content": consolidation_prompt()},
            {"role": "user", "content": content}
        ]
    });

    let url = format!("{}/chat/completions", base_url);
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    let data: serde_json::Value = resp.json().await?;
    let consolidated = data
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or(content);

    let needed = luwu_memory::ConsolidationNeeded {
        file_type,
        current_size: content.chars().count(),
        threshold: 8000,
        path: file_path.to_path_buf(),
    };
    apply_consolidation(&needed, consolidated);
    tracing::info!("Consolidated {} memory file", file_type.label());
    Ok(())
}
/// Run the checkpoint Writer — an independent LLM call that extracts
/// structured state from conversation history.
async fn run_checkpoint_writer(
    api_key: &str,
    base_url: &str,
    _messages: &[Message],
    memory: &MemoryStore,
) -> Result<(), String> {
    use luwu_core::writer_system_prompt;

    let client: reqwest::Client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("failed to build worker HTTP client");
    let system_prompt = writer_system_prompt();

    // For now, use the latest checkpoint as input to the writer.
    // In a full implementation, this would serialize the full conversation history.
    let current_checkpoint = memory.read_checkpoint_raw();
    let user_content = if current_checkpoint.is_empty() {
        "（新会话，尚无历史记录）".to_string()
    } else {
        format!("以下是当前 checkpoint，请增量更新：\n\n{}", current_checkpoint)
    };

    let body = serde_json::json!({
        "model": "MiniMax-M3",
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_content}
        ],
        "temperature": 0.1,
        "max_tokens": 4096
    });

    let resp = client
        .post(format!("{}/chat/completions", base_url))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Writer request failed: {e}"))?;

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Writer parse failed: {e}"))?;

    let output = data["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("");

    if !output.is_empty() {
        memory
            .write_checkpoint_raw(output)
            .map_err(|e| format!("Writer write failed: {e}"))?;
    }

    Ok(())
}

/// Convert messages into a readable transcript for LLM workers.
fn messages_to_transcript(messages: &[Message], max_chars: usize) -> String {
    use luwu_core::message::ContentPart;
    let mut out = String::new();
    for msg in messages {
        let role = match msg.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
            _ => "System",
        };
        for part in &msg.content {
            match part {
                ContentPart::Text { text } => {
                    out.push_str(&format!("{role}: {text}\n\n"));
                }
                ContentPart::ToolCall { name, .. } => {
                    out.push_str(&format!("{role}: [called tool: {name}]\n\n"));
                }
                ContentPart::ToolResult { content, is_error, .. } => {
                    let prefix = if *is_error { "Tool Error" } else { "Tool Result" };
                    out.push_str(&format!("{prefix}: {content}\n\n"));
                }
            }
            if out.len() > max_chars {
                out.truncate(max_chars);
                out.push_str("...[truncated]");
                return out;
            }
        }
    }
    out
}

/// Run the Observer worker — extracts timestamped observations from conversation.
async fn run_observer_worker(
    api_key: &str,
    base_url: &str,
    messages: &[Message],
    memory: &MemoryStore,
) -> Result<usize, String> {
    let transcript = messages_to_transcript(messages, 30_000);
    if transcript.is_empty() {
        return Ok(0);
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("failed to build worker HTTP client");
    let body = serde_json::json!({
        "model": "MiniMax-M3",
        "messages": [
            {"role": "system", "content": observer_prompt()},
            {"role": "user", "content": transcript}
        ],
        "temperature": 0.1,
        "max_tokens": 2048
    });

    let resp = client
        .post(format!("{base_url}/chat/completions"))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Observer request failed: {e}"))?;

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Observer parse failed: {e}"))?;

    let output = data["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("");

    let mut count = 0;
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            let priority = match v.get("priority").and_then(|p| p.as_str()) {
                Some("high") => Priority::High,
                Some("low") => Priority::Low,
                _ => Priority::Medium,
            };
            let category = v.get("category").and_then(|c| c.as_str()).unwrap_or("event").to_string();
            let content = v.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string();
            if !content.is_empty() {
                let obs = Observation::new(priority, category, content);
                if let Err(e) = memory.append_observation(&obs) { tracing::warn!(%e, "Failed to append observation"); }
                count += 1;
            }
        }
    }

    tracing::info!("Observer extracted {} observations", count);
    Ok(count)
}

/// Run the Reflector worker — synthesizes durable reflections from observations.
async fn run_reflector_worker(
    api_key: &str,
    base_url: &str,
    memory: &MemoryStore,
) -> Result<usize, String> {
    let observations = memory.read_observations();
    if observations.is_empty() {
        return Ok(0);
    }

    let obs_text: String = observations
        .iter()
        .map(|o| format!("[{}] {} ({}): {}", o.id, o.timestamp, o.priority, o.content))
        .collect::<Vec<_>>()
        .join("\n");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("failed to build worker HTTP client");
    let body = serde_json::json!({
        "model": "MiniMax-M3",
        "messages": [
            {"role": "system", "content": reflector_prompt()},
            {"role": "user", "content": obs_text}
        ],
        "temperature": 0.1,
        "max_tokens": 2048
    });

    let resp = client
        .post(format!("{base_url}/chat/completions"))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Reflector request failed: {e}"))?;

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Reflector parse failed: {e}"))?;

    let output = data["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("");

    let mut count = 0;
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            let content = v.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string();
            let source_ids: Vec<String> = v
                .get("source_ids")
                .and_then(|s| s.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            if !content.is_empty() {
                let refl = Reflection::new(content, source_ids);
                if let Err(e) = memory.append_reflection(&refl) { tracing::warn!(%e, "Failed to append reflection"); }
                count += 1;
            }
        }
    }

    tracing::info!("Reflector synthesized {} reflections", count);
    Ok(count)
}

// ---------------------------------------------------------------------------
// Handlers — Cancel
// ---------------------------------------------------------------------------

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

// ── Skill API Handlers ────────────────────────────────────────

/// GET /v1/skills — list all loaded skills.
pub async fn list_skills(
    State(state): State<Arc<AppState>>,
) -> axum::response::Response {
    let skills = state.skills.list();
    let summary: Vec<serde_json::Value> = skills
        .iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name,
                "description": s.description,
            })
        })
        .collect();
    Json(serde_json::json!({ "skills": summary })).into_response()
}

/// GET /v1/skills/{name} — get skill details.
pub async fn get_skill_detail(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> axum::response::Response {
    let Some(skill) = state.skills.get(&name) else {
        return (axum::http::StatusCode::NOT_FOUND, "Skill not found").into_response();
    };
    let files = state.skills.skill_files(&name);
    Json(serde_json::json!({
        "name": skill.name,
        "description": skill.description,
        "instructions": skill.instructions,
        "base_path": skill.base_path.to_string_lossy(),
        "files": files,
    }))
    .into_response()
}
// ── Memory API Handlers ────────────────────────────────────────

/// GET /v1/sessions/{id}/checkpoint — get latest checkpoint.
pub async fn get_checkpoint(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
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
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
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
                    return (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        e.to_string(),
                    )
                        .into_response();
                }
            },
            Err(e) => {
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    e.to_string(),
                )
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
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            e.to_string(),
        )
            .into_response(),
    }
}
