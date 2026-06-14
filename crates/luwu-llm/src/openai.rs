//! OpenAI LLM provider.
//!
//! Implements [`LlmProvider`] for the OpenAI Chat Completions API
//! with streaming support and function calling.
//!
//! # Wire format
//!
//! The provider translates [`LlmRequest`] → OpenAI chat completion request,
//! and OpenAI SSE chunks → [`LlmEvent`].
//!
//! # Supported models
//!
//! Any model available via the OpenAI API: gpt-4o, gpt-4.1, o3, o4-mini, etc.
//! Also works with OpenAI-compatible endpoints (Ollama, vLLM) via `base_url` override.

use crate::error::LlmError;
use async_trait::async_trait;
use futures::StreamExt;
use luwu_core::{ContentPart, LlmEvent, LlmProvider, LlmRequest, LlmUsage, Message, Result, Role};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use tracing::{debug, info};

use crate::sse;

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

/// OpenAI API provider.
pub struct OpenAiProvider {
    client: Client,
    api_key: String,
    base_url: String,
}

impl OpenAiProvider {
    /// Create a new provider with an API key and default base URL.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_base_url(api_key, "https://api.openai.com/v1")
    }

    /// Create a provider with a custom base URL (for OpenAI-compatible endpoints).
    pub fn with_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build OpenAI HTTP client"),
            api_key: api_key.into(),
            base_url: base_url.into(),
        }
    }

    /// Create a provider with an existing reqwest client (shared connection pool).
    pub fn with_client(
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        client: Client,
    ) -> Self {
        Self {
            client,
            api_key: api_key.into(),
            base_url: base_url.into(),
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    fn name(&self) -> &str {
        "openai"
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let resp = self
            .client
            .get(format!("{}/models", self.base_url))
            .bearer_auth(&self.api_key)
            .send()
            .await
            .map_err(|e| luwu_core::LuwuError::Llm(e.to_string()))?;

        let body: ModelsResponse = resp
            .json()
            .await
            .map_err(|e| luwu_core::LuwuError::Llm(e.to_string()))?;

        Ok(body.data.into_iter().map(|m| m.id).collect())
    }

    async fn stream(
        &self,
        request: LlmRequest,
    ) -> Result<tokio::sync::mpsc::Receiver<Result<LlmEvent>>> {
        let body = build_request_body(&request)?;
        let _llm_start = std::time::Instant::now();
        info!(model = %request.model, messages = request.messages.len(), "LLM stream request");

        let request = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .header("Content-Type", "application/json")
            .json(&body);

        let resp = crate::retry::send_with_retry(&request).await?;

        let (tx, rx) = tokio::sync::mpsc::channel(128);
        let event_stream = Box::pin(sse::parse_sse_stream(resp));

        tokio::spawn(async move {
            consume_stream(event_stream, tx).await;
        });

        Ok(rx)
    }
}

// ---------------------------------------------------------------------------
// Stream consumer — turns SSE events into LlmEvents
// ---------------------------------------------------------------------------

async fn consume_stream(
    mut event_stream: std::pin::Pin<
        Box<dyn futures::Stream<Item = std::result::Result<sse::SseEvent, reqwest::Error>> + Send>,
    >,
    tx: tokio::sync::mpsc::Sender<Result<LlmEvent>>,
) {
    // Accumulate partial tool calls across chunks.
    let mut pending_tool_calls: HashMap<String, PartialToolCall> = HashMap::new();

    loop {
        let result =
            match tokio::time::timeout(std::time::Duration::from_secs(30), event_stream.next())
                .await
            {
                Ok(Some(r)) => r,
                Ok(None) => break, // stream ended normally
                Err(_) => {
                    // LLM stalled mid-stream — 30s with no data
                    let _ = tx.send(Err(LlmError::Timeout.into())).await;
                    break;
                }
            };

        let sse_event = match result {
            Ok(e) => e,
            Err(e) => {
                let _ = tx
                    .send(Err(
                        LlmError::Stream(format!("SSE stream error: {e}")).into()
                    ))
                    .await;
                break;
            }
        };

        let chunk: Chunk = match serde_json::from_str(&sse_event.data) {
            Ok(c) => c,
            Err(e) => {
                debug!(data = %sse_event.data, "Skipping non-JSON SSE event: {e}");
                continue;
            }
        };

        // Process each choice (normally there's only one).
        for choice in chunk.choices {
            let delta = choice.delta;

            // Text content.
            if let Some(content) = &delta.content
                && !content.is_empty()
            {
                let _ = tx.send(Ok(LlmEvent::TextDelta(content.clone()))).await;
            }

            // Reasoning/thinking content (GLM-4.7, DeepSeek, MiniMax).
            if let Some(reasoning) = &delta.reasoning_content
                && !reasoning.is_empty()
            {
                let _ = tx
                    .send(Ok(LlmEvent::ReasoningDelta(reasoning.clone())))
                    .await;
            }

            // Tool call deltas.
            if let Some(tool_calls) = delta.tool_calls {
                for tc in tool_calls {
                    let entry = pending_tool_calls
                        .entry(tc.index.to_string())
                        .or_insert_with(|| PartialToolCall {
                            id: tc.id.clone().unwrap_or_default(),
                            name: String::new(),
                            arguments: String::new(),
                        });

                    if let Some(id) = tc.id {
                        entry.id = id;
                    }
                    if let Some(name) = tc.function.name {
                        entry.name = name;
                        // Emit ToolCallBegin when we first learn the name.
                        let _ = tx
                            .send(Ok(LlmEvent::ToolCallBegin {
                                id: entry.id.clone(),
                                name: entry.name.clone(),
                            }))
                            .await;
                    }
                    if let Some(args_delta) = tc.function.arguments {
                        entry.arguments.push_str(&args_delta);
                        let _ = tx
                            .send(Ok(LlmEvent::ToolCallDelta {
                                id: entry.id.clone(),
                                delta: args_delta,
                            }))
                            .await;
                    }
                }
            }

            // If the model is done for this choice, flush pending tool calls.
            if choice.finish_reason.as_deref() == Some("stop")
                || choice.finish_reason.as_deref() == Some("tool_calls")
            {
                for (_, _tc) in pending_tool_calls.drain() {
                    // Consumers that tracked ToolCallDelta already have the data.
                    // The channel closing signals completion.
                }
            }
        }

        // Usage info is sometimes in the final chunk.
        if let Some(usage) = chunk.usage {
            let event = LlmEvent::Done(LlmUsage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total_tokens,
            });
            let _ = tx.send(Ok(event)).await;
        }
    }
}

/// Intermediate accumulation state for a tool call that arrives in pieces.
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

// ---------------------------------------------------------------------------
// Request body construction
// ---------------------------------------------------------------------------

fn build_request_body(req: &LlmRequest) -> Result<Value> {
    let mut messages = Vec::new();

    // If there's a system prompt, prepend it as a system message.
    if let Some(sys) = &req.system_prompt {
        messages.push(serde_json::json!({
            "role": "system",
            "content": sys,
        }));
    }

    for msg in &req.messages {
        messages.push(convert_message(msg)?);
    }

    let mut body = serde_json::json!({
        "model": req.model,
        "messages": messages,
        "stream": true,
        "stream_options": { "include_usage": true },
    });

    if !req.tools.is_empty() {
        body["tools"] = serde_json::json!(
            req.tools
                .iter()
                .map(|t| serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                }))
                .collect::<Vec<_>>()
        );
    }

    if let Some(temp) = req.temperature {
        body["temperature"] = serde_json::json!(temp);
    }
    if let Some(max) = req.max_tokens {
        body["max_tokens"] = serde_json::json!(max);
    }
    if !req.stop_sequences.is_empty() {
        body["stop"] = serde_json::json!(req.stop_sequences);
    }

    Ok(body)
}

/// Convert a provider-agnostic [`Message`] into an OpenAI wire-format JSON value.
fn convert_message(msg: &Message) -> Result<Value> {
    let role = match msg.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };

    // Check if this message has tool calls (assistant message).
    let has_tool_calls = msg
        .content
        .iter()
        .any(|p| matches!(p, ContentPart::ToolCall { .. }));

    if has_tool_calls {
        // Assistant message with tool calls.
        let tool_calls: Vec<Value> = msg
            .content
            .iter()
            .filter_map(|p| match p {
                ContentPart::ToolCall {
                    id,
                    name,
                    arguments,
                } => Some(serde_json::json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments.to_string(),
                    }
                })),
                _ => None,
            })
            .collect();

        let text: String = msg
            .content
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");

        return Ok(serde_json::json!({
            "role": role,
            "content": if text.is_empty() { Value::Null } else { Value::String(text) },
            "tool_calls": tool_calls,
        }));
    }

    // Tool result message.
    if role == "tool" {
        let result_part = msg.content.first();
        return match result_part {
            Some(ContentPart::ToolResult { id, content, .. }) => Ok(serde_json::json!({
                "role": "tool",
                "tool_call_id": id,
                "content": content,
            })),
            _ => Err(luwu_core::LuwuError::Llm(
                "Tool message must contain a ToolResult".into(),
            )),
        };
    }

    // Plain text message.
    let text: String = msg
        .content
        .iter()
        .filter_map(|p| match p {
            ContentPart::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");

    let mut json = serde_json::json!({
        "role": role,
        "content": text,
    });

    if let Some(name) = &msg.name {
        json["name"] = Value::String(name.clone());
    }

    Ok(json)
}

// ---------------------------------------------------------------------------
// Wire types (OpenAI API response shapes)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
}

#[derive(Debug, Deserialize)]
struct Chunk {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    delta: Delta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Delta {
    content: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct ToolCallDelta {
    index: u32,
    id: Option<String>,
    #[serde(default)]
    function: FunctionDelta,
}

#[derive(Debug, Default, Deserialize)]
struct FunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
}
