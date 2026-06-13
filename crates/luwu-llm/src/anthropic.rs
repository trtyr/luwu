//! Anthropic LLM provider.
//!
//! Implements [`LlmProvider`] for the Anthropic Messages API
//! with streaming support and tool calling.
//!
//! # Wire format
//!
//! The provider translates [`LlmRequest`] → Anthropic messages request,
//! and Anthropic SSE events → [`LlmEvent`].
//!
//! # Supported models
//!
//! Claude Sonnet 4, Claude Opus 4, Claude Haiku 3.5, etc.

use async_trait::async_trait;
use futures::StreamExt;
use luwu_core::{
    ContentPart, LlmEvent, LlmProvider, LlmRequest, LlmUsage, Message, Result, Role,
};
use reqwest::Client;
use crate::error::{LlmError, truncate_body};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use tracing::debug;

use crate::sse;

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

/// Anthropic API provider.
pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    base_url: String,
}

impl AnthropicProvider {
    /// Create a new provider with an API key and the default base URL.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_base_url(api_key, "https://api.anthropic.com/v1")
    }

    /// Create a provider with a custom base URL.
    pub fn with_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build Anthropic HTTP client"),
            api_key: api_key.into(),
            base_url: base_url.into(),
        }
    }

    /// Create a provider with an existing reqwest client (shared connection pool).
    pub fn with_client(api_key: impl Into<String>, base_url: impl Into<String>, client: Client) -> Self {
        Self {
            client,
            api_key: api_key.into(),
            base_url: base_url.into(),
        }
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        // Anthropic doesn't have a models listing endpoint in the same way.
        // Return the known model IDs.
        Ok(vec![
            "claude-sonnet-4-20250514".into(),
            "claude-opus-4-20250514".into(),
            "claude-haiku-3-5-20241022".into(),
        ])
    }

    async fn stream(
        &self,
        request: LlmRequest,
    ) -> Result<tokio::sync::mpsc::Receiver<Result<LlmEvent>>> {
        let body = build_request_body(&request)?;
        debug!(model = %request.model, "Sending Anthropic streaming request");

        let request = self
            .client
            .post(format!("{}/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
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
// Stream consumer — turns Anthropic SSE events into LlmEvents
// ---------------------------------------------------------------------------

async fn consume_stream(
    mut event_stream: std::pin::Pin<Box<dyn futures::Stream<Item = std::result::Result<sse::SseEvent, reqwest::Error>> + Send>>,
    tx: tokio::sync::mpsc::Sender<Result<LlmEvent>>,
) {
    // Accumulate tool call arguments across content_block_delta events.
    let mut active_tool_calls: HashMap<String, PartialToolCall> = HashMap::new();

    loop {
        let result = match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            event_stream.next(),
        ).await {
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
                if let Err(send_err) = tx
                    .send(Err(LlmError::Stream(format!("SSE stream error: {e}")).into()))
                    .await
                {
                    tracing::warn!(%send_err, "Failed to send SSE error event");
                }
                break;
            }
        };

        let event_type = sse_event.event_type.as_deref().unwrap_or("");

        match event_type {
            "content_block_delta" => {
                let delta: ContentBlockDelta = match serde_json::from_str(&sse_event.data) {
                    Ok(d) => d,
                    Err(e) => {
                        debug!(data = %sse_event.data, "Skipping malformed content_block_delta: {e}");
                        continue;
                    }
                };

                match delta.delta {
                    DeltaType::TextDelta { text } => {
                        if !text.is_empty() {
                            let _ = tx.send(Ok(LlmEvent::TextDelta(text))).await;
                        }
                    }
                    DeltaType::InputJsonDelta { partial_json } => {
                        if let Some(tc) = active_tool_calls.get_mut(&delta.index.to_string()) {
                            tc.arguments.push_str(&partial_json);
                            let _ = tx
                                .send(Ok(LlmEvent::ToolCallDelta {
                                    id: tc.id.clone(),
                                    delta: partial_json,
                                }))
                                .await;
                        }
                    }
                }
            }

            "content_block_start" => {
                let block: ContentBlockStart = match serde_json::from_str(&sse_event.data) {
                    Ok(b) => b,
                    Err(e) => {
                        debug!(data = %sse_event.data, "Skipping malformed content_block_start: {e}");
                        continue;
                    }
                };

                if let ContentBlockContentType::ToolUse { id, name } = block.content_block {
                    active_tool_calls.insert(
                        block.index.to_string(),
                        PartialToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            arguments: String::new(),
                        },
                    );
                    let _ = tx
                        .send(Ok(LlmEvent::ToolCallBegin { id, name }))
                        .await;
                }
            }

            "content_block_stop" => {
                let block: ContentBlockStop = match serde_json::from_str(&sse_event.data) {
                    Ok(b) => b,
                    Err(e) => {
                        debug!(data = %sse_event.data, "Skipping malformed content_block_stop: {e}");
                        continue;
                    }
                };

                // Finalize the tool call if one was active for this block.
                if let Some(tc) = active_tool_calls.remove(&block.index.to_string()) {
                    // The full arguments have been accumulated in tc.arguments.
                    // Consumers that tracked deltas already have the data;
                    // this is just cleanup.
                    let _ = tx
                        .send(Ok(LlmEvent::ToolCallDelta {
                            id: tc.id,
                            delta: String::new(), // signal: done accumulating
                        }))
                        .await;
                }
            }

            "message_delta" => {
                let msg_delta: MessageDelta = match serde_json::from_str(&sse_event.data) {
                    Ok(d) => d,
                    Err(e) => {
                        debug!(data = %sse_event.data, "Skipping malformed message_delta: {e}");
                        continue;
                    }
                };

                if let Some(usage) = msg_delta.usage {
                    let _ = tx
                        .send(Ok(LlmEvent::Done(LlmUsage {
                            prompt_tokens: 0, // Anthropic sends output_tokens here
                            completion_tokens: usage.output_tokens,
                            total_tokens: usage.output_tokens, // Will be corrected by message_start usage
                        })))
                        .await;
                }
            }

            "message_start" => {
                // Contains the initial message with input usage.
                let msg_start: MessageStart = match serde_json::from_str(&sse_event.data) {
                    Ok(m) => m,
                    Err(e) => {
                        debug!(data = %sse_event.data, "Skipping malformed message_start: {e}");
                        continue;
                    }
                };
                // We don't emit a Done event here since the message isn't done yet.
                // Store usage info if needed for later.
                debug!(
                    input_tokens = msg_start.message.usage.input_tokens,
                    "Anthropic message started"
                );
            }

            // message_stop, ping, etc. — ignore.
            _ => {
                debug!(event_type, "Ignoring Anthropic SSE event type");
            }
        }
    }
}

/// Intermediate accumulation state for a tool call.
struct PartialToolCall {
    id: String,
    #[allow(dead_code)] name: String,
    arguments: String,
}

// ---------------------------------------------------------------------------
// Request body construction
// ---------------------------------------------------------------------------

fn build_request_body(req: &LlmRequest) -> Result<Value> {
    let mut messages = Vec::new();

    // Anthropic uses a top-level `system` field, not a system message.
    // Extract system prompt if present.
    let system_text = req.system_prompt.clone().unwrap_or_default();

    for msg in &req.messages {
        if msg.role == Role::System {
            // Skip — we already captured it above.
            continue;
        }
        messages.push(convert_message(msg)?);
    }

    let mut body = serde_json::json!({
        "model": req.model,
        "messages": messages,
        "stream": true,
        "max_tokens": req.max_tokens.unwrap_or(16384),
    });

    if !system_text.is_empty() {
        body["system"] = Value::String(system_text);
    }

    if !req.tools.is_empty() {
        body["tools"] = serde_json::json!(
            req.tools.iter().map(|t| serde_json::json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.parameters,
            })).collect::<Vec<_>>()
        );
    }

    if let Some(temp) = req.temperature {
        body["temperature"] = serde_json::json!(temp);
    }
    if !req.stop_sequences.is_empty() {
        body["stop_sequences"] = serde_json::json!(req.stop_sequences);
    }

    Ok(body)
}

/// Convert a provider-agnostic [`Message`] into Anthropic wire format.
fn convert_message(msg: &Message) -> Result<Value> {
    let role = match msg.role {
        Role::User => "user",
        Role::Assistant => "assistant",
        // Anthropic doesn't have system messages in the messages array.
        // Tool results go inside a user message with tool_result content blocks.
        Role::System | Role::Tool => "user",
    };

    let mut content: Vec<Value> = Vec::new();

    for part in &msg.content {
        match part {
            ContentPart::Text { text } => {
                content.push(serde_json::json!({
                    "type": "text",
                    "text": text,
                }));
            }
            ContentPart::ToolCall {
                id,
                name,
                arguments,
            } => {
                content.push(serde_json::json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": arguments,
                }));
            }
            ContentPart::ToolResult {
                id,
                content: result_text,
                is_error,
            } => {
                content.push(serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": id,
                    "content": result_text,
                    "is_error": is_error,
                }));
            }
        }
    }

    // If the content is empty, add a placeholder.
    if content.is_empty() {
        content.push(serde_json::json!({
            "type": "text",
            "text": "",
        }));
    }

    Ok(serde_json::json!({
        "role": role,
        "content": content,
    }))
}

// ---------------------------------------------------------------------------
// Wire types (Anthropic SSE event shapes)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct MessageStart {
    message: MessageStartMessage,
}

#[derive(Debug, Deserialize)]
struct MessageStartMessage {
    usage: MessageStartUsage,
}

#[derive(Debug, Deserialize)]
struct MessageStartUsage {
    input_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct ContentBlockStart {
    index: u32,
    content_block: ContentBlockContentType,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ContentBlockContentType {
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String },
    #[serde(rename = "text")]
    Text { #[allow(dead_code)] text: String },
}

#[derive(Debug, Deserialize)]
struct ContentBlockDelta {
    index: u32,
    delta: DeltaType,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum DeltaType {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
}

#[derive(Debug, Deserialize)]
struct ContentBlockStop {
    index: u32,
}

#[derive(Debug, Deserialize)]
struct MessageDelta {
    usage: Option<MessageDeltaUsage>,
}

#[derive(Debug, Deserialize)]
struct MessageDeltaUsage {
    output_tokens: u64,
}
