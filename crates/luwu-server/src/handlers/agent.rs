//! Agent chat endpoint — full event stream with tool visibility + cycle management.

use std::convert::Infallible;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::response::sse::{Event as SseEvent, Sse};

use luwu_core::{
    CycleAction, CycleState, EventBus, RunningGuard, TrySetRunningError, TurnEngine,
    TurnEvent,
};
use luwu_memory::{CorrectionDetector, CorrectionPattern, MemoryStore, compile_summary};

use crate::app::{AppState, builtin_tool_registry, create_provider};
use crate::handlers::workers::{
    run_consolidation_writer, run_observer_worker, run_reflector_worker,
};
use crate::types::*;

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
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    // Build engine — provider selected by config (factory pattern).
    let provider_arc = create_provider(&resolved, state.http_client.clone());
    let tools = builtin_tool_registry();
    let events = EventBus::new(256);
    let working_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let engine = TurnEngine::new(
        provider_arc.clone(),
        tools,
        state.skills.clone(),
        events,
        working_dir.clone(),
    );

    // Memory store and cycle state.
    let luwu_home = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".luwu");
    let memory = std::sync::Arc::new(MemoryStore::new(&luwu_home, &state.working_dir, &id));
    let mut cycle = CycleState::default();

    let model = session.data.model.clone();
    let messages = session.data.messages.clone();
    let session_id = session.data.id.clone();
    let sessions = state.sessions.clone();
    let id_clone = id.clone();

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
                if let Err(e) = mem_c.append_correction(&entry) {
                    tracing::warn!(%e, "Failed to save correction");
                }
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
                                let np = n.path.clone();
                                let ft = n.file_type;
                                let prov = provider_arc.clone();
                                let mdl = resolved.model.clone();
                                state.spawn_worker(async move {
                                    if let Err(e) = run_consolidation_writer(prov, mdl, content, np, ft).await { tracing::warn!(%e, "Consolidation writer failed"); }
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
                            let prov = provider_arc.clone();
                            let mdl = resolved.model.clone();
                            let obs_msgs = messages_for_workers.clone();

                            state.spawn_worker(async move {
                                if let Err(e) = run_observer_worker(prov, mdl, obs_msgs, _wm).await { tracing::warn!(%e, "Observer worker failed"); }
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

                // Cycle checkpoint/rebuild after each LLM call.
                match cycle.check() {
                    CycleAction::Checkpoint => {
                        let pct = cycle.usage_pct();
                        cycle.mark_checkpoint(pct);

                        let _writer_memory = memory.clone();
                        let prov = provider_arc.clone();
                        let mdl = resolved.model.clone();
                        let obs_msgs = messages_for_workers.clone();

                        // Deterministic compaction — zero LLM cost.
                        let det_summary = compile_summary(&messages_for_workers, &working_dir);
                        if let Err(e) = memory.write_checkpoint_raw(&det_summary.to_markdown()) { tracing::warn!(%e, "Failed to write checkpoint"); }

                        // Spawn Observer worker.
                        state.spawn_worker(async move {
                            if let Err(e) = run_observer_worker(
                                prov,
                                mdl,
                                obs_msgs,
                                _writer_memory,
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
                        // Spawn Reflector to synthesize observations into reflections.
                        let _refl_memory = memory.clone();
                        let prov = provider_arc.clone();
                        let mdl = resolved.model.clone();
                        state.spawn_worker(async move {
                            if let Err(e) = run_reflector_worker(
                                prov,
                                mdl,
                                _refl_memory,
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
