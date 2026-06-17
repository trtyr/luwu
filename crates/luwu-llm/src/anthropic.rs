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

/// Anthropic API provider.
pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    base_url: String,
}

impl AnthropicProvider {
    /// Create a new provider with an API key and the default base URL.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_base_url(api_key, "https://api.anthropic.com")
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
        info!(model = %request.model, messages = request.messages.len(), "LLM stream request");

        let (tx, rx) = tokio::sync::mpsc::channel(128);

        let client = self.client.clone();
        let api_key = self.api_key.clone();
        let base_url = self.base_url.clone();

        tokio::spawn(async move {
            const MAX_STREAM_ATTEMPTS: u32 = 3;

            for attempt in 1..=MAX_STREAM_ATTEMPTS {
                let request = client
                    .post(format!("{base_url}/messages"))
                    .header("x-api-key", &api_key)
                    .header("anthropic-version", "2023-06-01")
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
// Stream consumer — turns Anthropic SSE events into LlmEvents
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
/// 60 seconds is generous enough for reasoning models that may spend
/// significant time "thinking" before emitting the first token.
const STALL_TIMEOUT_SECS: u64 = 60;

async fn consume_stream(
    mut event_stream: std::pin::Pin<
        Box<dyn futures::Stream<Item = std::result::Result<sse::SseEvent, reqwest::Error>> + Send>,
    >,
    tx: &tokio::sync::mpsc::Sender<Result<LlmEvent>>,
) -> StreamOutcome {
    // Accumulate tool call arguments across content_block_delta events.
    let mut active_tool_calls: HashMap<String, PartialToolCall> = HashMap::new();
    let mut received_data = false;
    // Anthropic splits usage across two SSE events: input_tokens arrives
    // in message_start, output_tokens + cache fields in message_delta.
    // We track input_tokens here and combine them at message_delta time.
    let mut input_tokens: u64 = 0;

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
                if let Err(send_err) = tx
                    .send(Err(
                        LlmError::Stream(format!("SSE stream error: {e}")).into()
                    ))
                    .await
                {
                    tracing::warn!(%send_err, "Failed to send SSE error event");
                }
                return StreamOutcome::Errored;
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
                            // Strip trailing U+FFFD from the SSE chunk — the
                            // `sse` crate uses `from_utf8_lossy` which replaces
                            // invalid UTF-8 bytes (from mid-char chunk splits
                            // or lossy model output) with U+FFFD. Frontend
                            // then shows garbled "���" suffixes. See
                            // `strip_trailing_replacement`.
                            let safe = strip_trailing_replacement(&text);
                            if !safe.is_empty() {
                                received_data = true;
                                let _ = tx
                                    .send(Ok(LlmEvent::TextDelta(safe.to_string())))
                                    .await;
                            }
                        }
                    }
                    DeltaType::InputJsonDelta { partial_json } => {
                        if let Some(tc) = active_tool_calls.get_mut(&delta.index.to_string()) {
                            tc.arguments.push_str(&partial_json);
                            received_data = true;
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
                    received_data = true;
                    let _ = tx.send(Ok(LlmEvent::ToolCallBegin { id, name })).await;
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
                    received_data = true;
                    // Real Anthropic usage — not estimates. The cache fields
                    // are what the LLM provider actually reported, not 0.
                    // Enable RUST_LOG=luwu_llm=debug to see the raw values
                    // from the API response per request.
                    let cache_read = usage.cache_read_input_tokens.unwrap_or(0);
                    let cache_creation = usage.cache_creation_input_tokens.unwrap_or(0);
                    tracing::debug!(
                        "Provider usage (raw from API response) — \
                         input={} output={} | cache: read={:?} creation={:?}",
                        input_tokens,
                        usage.output_tokens,
                        usage.cache_read_input_tokens,
                        usage.cache_creation_input_tokens,
                    );
                    // Full prompt = non-cached input + cache reads + cache
                    // creations. Anthropic's `input_tokens` is the NON-cached
                    // portion only; cache_read and cache_creation are
                    // separate fields that together make up the cached part.
                    // Use input_tokens from message_start when available,
                    // falling back to delta's input_tokens if message_start
                    // was missing or 0 (defensive — Anthropic normally sends
                    // both, but some proxy implementations may skip start).
                    let final_input_tokens = input_tokens.max(usage.input_tokens.unwrap_or(0));
                    let prompt_total = final_input_tokens + cache_read + cache_creation;
                    let _ = tx
                        .send(Ok(LlmEvent::Done(LlmUsage {
                            prompt_tokens: prompt_total,
                            completion_tokens: usage.output_tokens,
                            total_tokens: prompt_total + usage.output_tokens,
                            // Tokens served from cache — the "cache hit"
                            // count that matters for billing (Anthropic:
                            // ~1/10 of full price; GLM Coding Plan similar).
                            prompt_cache_hit_tokens: cache_read,
                            // Non-cached portion = final_input_tokens
                            // (Anthropic already excludes cache_read and
                            // cache_creation from this field). This is the
                            // full-price portion of the prompt.
                            prompt_cache_miss_tokens: final_input_tokens,
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
                // Store input_tokens for later merge with message_delta's
                // output_tokens + cache fields. Anthropic splits the usage
                // object across two SSE events: input_tokens arrives here
                // in message_start, and the rest (output_tokens,
                // cache_creation_input_tokens, cache_read_input_tokens)
                // arrives in the final message_delta event.
                input_tokens = msg_start.message.usage.input_tokens;
                debug!(
                    input_tokens,
                    "Anthropic message started — input_tokens stored for later merge"
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
    #[allow(dead_code)]
    name: String,
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
            req.tools
                .iter()
                .map(|t| serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                }))
                .collect::<Vec<_>>()
        );
    }

    if let Some(temp) = req.temperature {
        body["temperature"] = serde_json::json!(temp);
    }
    if !req.stop_sequences.is_empty() {
        body["stop_sequences"] = serde_json::json!(req.stop_sequences);
    }

    // MiniMax via Anthropic protocol: thinking defaults to OFF in MiniMax's
    // Anthropic-compatible endpoint (different from native Anthropic API
    // which defaults to ON). Without this, the model produces no
    // reasoning_content — TUI's ReasoningBlock will be empty and the model
    // appears to "skip thinking" entirely. We must explicitly enable it.
    //
    // Detection is permissive (case-insensitive, matches `MiniMax-M3`,
    // `MiniMax-M2.7`, `abab-6`, etc.) to cover all MiniMax naming
    // conventions. We do NOT enable thinking for real Anthropic API
    // models (claude-*), where the user opts in explicitly via
    // `extra_body` if desired.
    //
    // budget_tokens: 8192 is a reasonable default — enough for most
    // agentic reasoning without blowing up the context window.
    if req.model.to_lowercase().contains("minimax-") || req.model.to_lowercase().contains("abab") {
        // `entry().or_insert()` lets the user's `extra_body.thinking` win
        // later in the merge step, so they can override the default
        // budget_tokens (e.g. bump to 16384 for longer reasoning) or even
        // disable thinking by setting `{"type": "disabled"}`.
        // `body` is a `Value` (not a `Map`), so we go through
        // `as_object_mut()` to access the underlying `Map::entry` API.
        if let Some(map) = body.as_object_mut() {
            map.entry("thinking".to_string()).or_insert(serde_json::json!({
                "type": "enabled",
                "budget_tokens": 8192
            }));
        }
    }

    // Merge user-supplied extra_body last so explicit user values always
    // override the provider defaults above. Mirrors the OpenAI provider's
    // behaviour in `openai.rs::build_request_body`. This unlocks:
    //   - Anthropic-native users passing `metadata`, `tool_choice`, etc.
    //   - MiniMax users tuning `thinking.budget_tokens` or disabling it
    //   - Any future Anthropic-specific fields the user wants to set
    if let Some(Value::Object(extras)) = &req.extra_body
        && let Some(map) = body.as_object_mut()
    {
        for (k, v) in extras {
            map.insert(k.clone(), v.clone());
        }
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
            // Anthropic's equivalent of reasoning_content is the
            // `thinking` content block. Only valid in assistant messages.
            // We emit it here for completeness; Anthropic will reject it
            // if the role is wrong, and the API server normalizes the
            // ordering.
            ContentPart::Reasoning { text: reasoning } => {
                content.push(serde_json::json!({
                    "type": "thinking",
                    "thinking": reasoning,
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
    Text {
        #[allow(dead_code)]
        text: String,
    },
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

    // Anthropic reports these in the final message_delta event. All three
    // are optional because only providers that have prefix caching enabled
    // (GLM Coding Plan, Anthropic direct, etc.) send them — providers
    // without caching simply omit the fields and we treat as 0.
    #[serde(default)]
    input_tokens: Option<u64>,
    /// Tokens just written to cache (5-min or 1-hour TTL). Informational;
    /// we don't track this separately in LlmUsage.
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
    /// Tokens served from cache — this is the "cache hit" count that
    /// matters for billing (~1/10 of full price on Anthropic, similar on
    /// GLM Coding Plan). Maps to LlmUsage::prompt_cache_hit_tokens.
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
}

/// Strip trailing U+FFFD (UTF-8 replacement character) from a string.
///
/// The `sse` crate uses `from_utf8_lossy` internally, which replaces
/// invalid UTF-8 bytes with U+FFFD. If an SSE chunk is split
/// mid-character (rare but possible with small socket buffers or
/// model output that ends on a partial multi-byte sequence), the
/// trailing byte(s) become U+FFFD. Stripping them prevents the
/// frontend from seeing garbled text like "有什么具体想做的吗？���".
///
/// Safe default: even if the model intentionally emits U+FFFD (legal
/// but unusual), trailing instances are not meaningful output and
/// stripping them is the right call.
fn strip_trailing_replacement(s: &str) -> &str {
    const REPLACEMENT: &[u8] = &[0xEF, 0xBF, 0xBD]; // U+FFFD in UTF-8
    let bytes = s.as_bytes();
    let mut end = bytes.len();
    while end >= REPLACEMENT.len()
        && &bytes[end - REPLACEMENT.len()..end] == REPLACEMENT
    {
        end -= REPLACEMENT.len();
    }
    // end is at a char boundary because REPLACEMENT is a valid
    // 3-byte UTF-8 sequence and we only strip whole instances.
    std::str::from_utf8(&bytes[..end]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use luwu_core::message::{ContentPart, Message, Role};

    fn make_request(model: &str, extra_body: Option<serde_json::Value>) -> LlmRequest {
        LlmRequest {
            model: model.to_string(),
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentPart::Text { text: "hi".to_string() }],
                name: None,
                tool_call_id: None,
            }],
            tools: vec![],
            system_prompt: None,
            temperature: None,
            max_tokens: None,
            stop_sequences: vec![],
            extra_body,
        }
    }

    #[test]
    fn minimax_model_injects_default_thinking() {
        let req = make_request("MiniMax-M3", None);
        let body = build_request_body(&req).unwrap();
        let thinking = &body["thinking"];
        assert_eq!(thinking["type"], "enabled");
        assert_eq!(thinking["budget_tokens"], 8192);
    }

    #[test]
    fn abab_model_also_gets_default_thinking() {
        let req = make_request("abab-6-chat", None);
        let body = build_request_body(&req).unwrap();
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 8192);
    }

    #[test]
    fn minimax_user_can_override_budget_tokens_via_extra_body() {
        // User wants 16384 budget instead of default 8192
        let req = make_request(
            "MiniMax-M3",
            Some(serde_json::json!({
                "thinking": { "type": "enabled", "budget_tokens": 16384 }
            })),
        );
        let body = build_request_body(&req).unwrap();
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 16384);
    }

    #[test]
    fn minimax_user_can_disable_thinking_via_extra_body() {
        let req = make_request(
            "MiniMax-M3",
            Some(serde_json::json!({
                "thinking": { "type": "disabled" }
            })),
        );
        let body = build_request_body(&req).unwrap();
        assert_eq!(body["thinking"]["type"], "disabled");
    }

    #[test]
    fn real_anthropic_does_not_inject_thinking_by_default() {
        // claude-* must NOT get thinking auto-injected — users opt in
        let req = make_request("claude-3-5-sonnet-20241022", None);
        let body = build_request_body(&req).unwrap();
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn real_anthropic_can_opt_in_via_extra_body() {
        // claude user wants thinking enabled with custom budget
        let req = make_request(
            "claude-3-5-sonnet-20241022",
            Some(serde_json::json!({
                "thinking": { "type": "enabled", "budget_tokens": 10000 }
            })),
        );
        let body = build_request_body(&req).unwrap();
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 10000);
    }

    #[test]
    fn extra_body_merges_arbitrary_anthropic_fields() {
        // metadata, tool_choice, top_k — all flow through to Anthropic
        let req = make_request(
            "claude-3-5-sonnet-20241022",
            Some(serde_json::json!({
                "metadata": { "user_id": "u-123" },
                "tool_choice": { "type": "any" },
                "top_k": 5
            })),
        );
        let body = build_request_body(&req).unwrap();
        assert_eq!(body["metadata"]["user_id"], "u-123");
        assert_eq!(body["tool_choice"]["type"], "any");
        assert_eq!(body["top_k"], 5);
    }

    // ── strip_trailing_replacement ──────────────────────────────────

    #[test]
    fn strip_no_replacement_returns_unchanged() {
        // Plain ASCII, no U+FFFD — should be a no-op.
        assert_eq!(strip_trailing_replacement("hello world"), "hello world");
    }

    #[test]
    fn strip_chinese_no_replacement_returns_unchanged() {
        // Valid CJK, no U+FFFD — should be a no-op.
        assert_eq!(
            strip_trailing_replacement("有什么具体想做的吗？"),
            "有什么具体想做的吗？"
        );
    }

    #[test]
    fn strip_single_trailing_replacement() {
        // Single trailing U+FFFD from a chunk split — strip it.
        assert_eq!(
            strip_trailing_replacement("有什么具体想做的吗？\u{FFFD}"),
            "有什么具体想做的吗？"
        );
    }

    #[test]
    fn strip_multiple_trailing_replacements() {
        // Multiple trailing U+FFFDs — strip all of them.
        assert_eq!(
            strip_trailing_replacement("hello\u{FFFD}\u{FFFD}\u{FFFD}"),
            "hello"
        );
    }

    #[test]
    fn strip_keeps_internal_replacements() {
        // U+FFFD in the middle of the string should be preserved.
        assert_eq!(
            strip_trailing_replacement("hello\u{FFFD}world\u{FFFD}"),
            "hello\u{FFFD}world"
        );
    }

    #[test]
    fn strip_empty_string() {
        assert_eq!(strip_trailing_replacement(""), "");
    }

    #[test]
    fn strip_all_replacements() {
        // Entire string is just U+FFFDs — result is empty.
        assert_eq!(
            strip_trailing_replacement("\u{FFFD}\u{FFFD}\u{FFFD}"),
            ""
        );
    }

    #[test]
    fn strip_chinese_with_trailing_replacement() {
        // Real-world case: model response ends with CJK + lossy U+FFFD.
        assert_eq!(
            strip_trailing_replacement("我可以帮你完成各种软件开发任务\u{FFFD}"),
            "我可以帮你完成各种软件开发任务"
        );
    }
}
