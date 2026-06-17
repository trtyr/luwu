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
        info!(model = %request.model, messages = request.messages.len(), "LLM stream request");

        let (tx, rx) = tokio::sync::mpsc::channel(128);

        let client = self.client.clone();
        let api_key = self.api_key.clone();
        let base_url = self.base_url.clone();

        tokio::spawn(async move {
            const MAX_STREAM_ATTEMPTS: u32 = 3;

            for attempt in 1..=MAX_STREAM_ATTEMPTS {
                let request = client
                    .post(format!("{base_url}/chat/completions"))
                    .bearer_auth(&api_key)
                    .header("Content-Type", "application/json")
                    .json(&body);

                let resp = match crate::retry::send_with_retry(&request).await {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = tx.send(Err(e.into())).await;
                        return;
                    }
                };

                let event_stream = Box::pin(sse::parse_sse_stream(resp));
                let outcome = consume_stream(event_stream, &tx).await;

                match outcome {
                    StreamOutcome::Completed => return,
                    StreamOutcome::StalledNoData if attempt < MAX_STREAM_ATTEMPTS => {
                        let delay_secs = 1u64 << (attempt - 1); // 1s, 2s
                        tracing::warn!(
                            attempt,
                            max_attempts = MAX_STREAM_ATTEMPTS,
                            delay_secs,
                            "SSE stream stalled before any data — retrying"
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
                        continue;
                    }
                    StreamOutcome::StalledNoData => {
                        tracing::warn!("SSE stream stalled after all retries exhausted");
                        let _ = tx.send(Err(LlmError::Timeout.into())).await;
                        return;
                    }
                    StreamOutcome::StalledPartial | StreamOutcome::Errored => return,
                }
            }
        });

        Ok(rx)
    }
}

// ---------------------------------------------------------------------------
// Stream consumer — turns SSE events into LlmEvents
// ---------------------------------------------------------------------------

/// Outcome of consuming an SSE stream — used by the retry loop in `stream()`.
enum StreamOutcome {
    /// Stream completed normally (received Done or stream ended).
    Completed,
    /// Stream stalled (timeout) before any data was received.
    /// Safe to retry — no events were sent through the channel.
    StalledNoData,
    /// Stream stalled after partial data was received.
    /// NOT safe to retry — caller already received some events.
    StalledPartial,
    /// Stream errored out (error event already sent through channel).
    Errored,
}

/// Stall timeout — how long to wait for the next SSE event before giving up.
///
/// 60 seconds is generous enough for reasoning models (GLM-4.7, o3) that
/// may spend significant time "thinking" before emitting the first token,
/// while still catching genuinely stalled connections.
const STALL_TIMEOUT_SECS: u64 = 60;

async fn consume_stream(
    mut event_stream: std::pin::Pin<
        Box<dyn futures::Stream<Item = std::result::Result<sse::SseEvent, reqwest::Error>> + Send>,
    >,
    tx: &tokio::sync::mpsc::Sender<Result<LlmEvent>>,
) -> StreamOutcome {
    // Accumulate partial tool calls across chunks.
    let mut pending_tool_calls: HashMap<String, PartialToolCall> = HashMap::new();
    let mut received_data = false;

    loop {
        let result = match tokio::time::timeout(
            std::time::Duration::from_secs(STALL_TIMEOUT_SECS),
            event_stream.next(),
        )
        .await
        {
            Ok(Some(r)) => r,
            Ok(None) => return StreamOutcome::Completed, // stream ended normally
            Err(_) => {
                // LLM stalled mid-stream
                return if received_data {
                    let _ = tx.send(Err(LlmError::Timeout.into())).await;
                    StreamOutcome::StalledPartial
                } else {
                    StreamOutcome::StalledNoData
                };
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
                return StreamOutcome::Errored;
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
                received_data = true;
                let _ = tx.send(Ok(LlmEvent::TextDelta(content.clone()))).await;
            }

            // Reasoning/thinking content (GLM-4.7, DeepSeek, MiniMax).
            if let Some(reasoning) = &delta.reasoning_content
                && !reasoning.is_empty()
            {
                received_data = true;
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
                        received_data = true;
                        let _ = tx
                            .send(Ok(LlmEvent::ToolCallBegin {
                                id: entry.id.clone(),
                                name: entry.name.clone(),
                            }))
                            .await;
                    }
                    if let Some(args_delta) = tc.function.arguments {
                        entry.arguments.push_str(&args_delta);
                        received_data = true;
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
            received_data = true;
            // Cache hit/miss resolution order (precedence: flat > nested):
            // 1. DeepSeek V4 flat fields (prompt_cache_hit_tokens) — most accurate
            //    because DeepSeek distinguishes hit vs miss.
            // 2. OpenAI/GLM nested prompt_tokens_details.cached_tokens — only hit
            //    count available; miss stays 0 (the non-cached portion =
            //    prompt_tokens - cached_tokens is the uncached prompt, not a
            //    "miss from cache hit price" — there's no cache-miss tier on GLM).
            // 3. Plain OpenAI: both default to 0.
            let prompt_cache_hit_tokens = usage
                .prompt_cache_hit_tokens
                .or_else(|| usage.prompt_tokens_details.as_ref().map(|d| d.cached_tokens))
                .unwrap_or(0);
            let prompt_cache_miss_tokens = usage.prompt_cache_miss_tokens.unwrap_or(0);
            let event = LlmEvent::Done(LlmUsage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total_tokens,
                prompt_cache_hit_tokens,
                prompt_cache_miss_tokens,
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

    // Merge provider-specific extras (DeepSeek's thinking toggle, etc.).
    // Keys in `req.extra_body` override anything we set above, which is
    // the right behavior for deliberate overrides like forcing thinking off.
    if let Some(Value::Object(extras)) = &req.extra_body {
        for (k, v) in extras {
            body[k] = v.clone();
        }
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

        // DeepSeek-V4 (thinking mode) requires the assistant's
        // reasoning_content to be echoed back whenever the same assistant
        // turn also contains tool calls. The API rejects the request with
        // a 400 otherwise. For non-tool-call messages DeepSeek ignores
        // reasoning_content, so we only emit it on this branch.
        let reasoning: String = msg
            .content
            .iter()
            .filter_map(|p| match p {
                ContentPart::Reasoning { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");

        let mut json = serde_json::json!({
            "role": role,
            "content": if text.is_empty() { Value::Null } else { Value::String(text) },
            "tool_calls": tool_calls,
        });
        if !reasoning.is_empty() {
            json["reasoning_content"] = Value::String(reasoning);
        }
        return Ok(json);
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

/// OpenAI `usage` object. DeepSeek V4 (and other providers with prefix
/// caching) add `prompt_cache_hit_tokens` and `prompt_cache_miss_tokens`;
/// OpenAI itself does not. Both fields are optional so the same struct
/// works for both.
/// OpenAI `usage` object. Three cache-format families are supported:
///
/// 1. **OpenAI/GLM nested**: `prompt_tokens_details.cached_tokens`
///    (the standard OpenAI shape, used by GLM for prefix cache hits).
/// 2. **DeepSeek V4 flat**: `prompt_cache_hit_tokens` + `prompt_cache_miss_tokens`
///    (DeepSeek's split-hit/miss form, with cache-miss at full price and
///    cache-hit at ~1/50 price on V4-Flash).
/// 3. **Plain OpenAI**: no cache fields at all (everything defaults to 0).
///
/// All three deserialize into the same `LlmUsage` and the consume_stream
/// mapping below reads from whichever field is present.
#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    /// OpenAI/GLM nested cache hit count (under `prompt_tokens_details`).
    #[serde(default)]
    prompt_tokens_details: Option<PromptTokensDetails>,
    /// DeepSeek V4 flat cache hit count.
    #[serde(default)]
    prompt_cache_hit_tokens: Option<u64>,
    /// DeepSeek V4 flat cache miss count.
    #[serde(default)]
    prompt_cache_miss_tokens: Option<u64>,
}

/// OpenAI-standard `prompt_tokens_details` sub-object. GLM puts the
/// prefix-cache hit count here as `cached_tokens`; the field is optional
/// so providers that don't report it simply deserialize to None.
#[derive(Debug, Deserialize)]
struct PromptTokensDetails {
    #[serde(default)]
    cached_tokens: u64,
}
