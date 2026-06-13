//! OpenAI-compatible chat completions endpoint (real SSE streaming).

use std::convert::Infallible;
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::response::sse::{Event as SseEvent, Sse};

use luwu_core::{EventBus, Message, TurnEngine, TurnEvent};


use crate::app::{create_provider, AppState, builtin_tool_registry};
use crate::types::*;

#[allow(clippy::collapsible_if)]
#[tracing::instrument(skip(state))]
pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> axum::response::Response {
    let resolved = match state.config.resolve(None) {
        Ok(r) => r,
        Err(e) => {
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
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
    // Build engine — provider selected by config (factory pattern).
    let provider = create_provider(&resolved, state.http_client.clone());
    let tools = builtin_tool_registry();
    let events = EventBus::new(256);
    let working_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let engine = TurnEngine::new(
        provider,
        tools,
        state.skills.clone(),
        events,
        working_dir,
    );

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
            let mut last_reasoning: Option<String> = None;
            while let Some(event) = rx.recv().await {
                match event {
                    TurnEvent::TextDelta { delta } => {
                        if let Some(ref reasoning) = last_reasoning {
                            if delta == *reasoning {
                                continue;
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
