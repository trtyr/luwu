//! LLM provider abstraction.
//!
//! The [`LlmProvider`] trait is the single interface through which luwu talks
//! to any large language model. Each provider (OpenAI, Anthropic, Google, etc.)
//! implements this trait in its own plugin crate.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Result;
use crate::message::Message;

/// A tool definition sent to the LLM so it knows what it can call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// A JSON Schema object describing the tool's input parameters.
    pub parameters: Value,
}

/// A completion request to send to the LLM.
#[derive(Debug, Clone)]
pub struct LlmRequest {
    /// Model identifier (provider-specific, e.g. "gpt-4o", "claude-sonnet-4").
    pub model: String,
    /// The conversation history so far.
    pub messages: Vec<Message>,
    /// Tools the model is allowed to call during this request.
    pub tools: Vec<ToolDefinition>,
    /// An optional system prompt prepended to the messages.
    pub system_prompt: Option<String>,
    /// Sampling temperature (0.0–2.0). `None` uses the provider's default.
    pub temperature: Option<f64>,
    /// Maximum tokens to generate. `None` uses the provider's default.
    pub max_tokens: Option<u32>,
    /// Stop sequences that end generation early.
    pub stop_sequences: Vec<String>,
    /// Extra body fields to merge into the request JSON.
    ///
    /// Used for provider-specific knobs that don't map to the standard
    /// OpenAI/Anthropic fields — e.g. DeepSeek's thinking toggle:
    /// `{"thinking": {"type": "enabled"}}` or `{"thinking": {"type": "disabled"}}`.
    /// `None` means "don't add anything extra".
    pub extra_body: Option<Value>,
}

/// Streaming events emitted by the LLM during a completion.
#[derive(Debug, Clone)]
pub enum LlmEvent {
    /// A chunk of text content from the model.
    TextDelta(String),

    /// A chunk of reasoning/thinking content from the model.
    /// Models like GLM-4.7, DeepSeek, MiniMax emit this during thinking.
    ReasoningDelta(String),

    /// The model is starting a tool call.
    ToolCallBegin { id: String, name: String },
    /// A chunk of tool-call arguments (streamed incrementally).
    ToolCallDelta { id: String, delta: String },

    /// The model has finished generating.
    Done(LlmUsage),

    /// An error occurred during streaming.
    Error(String),
}

/// Token usage statistics for a single completion.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LlmUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    /// Tokens served from provider-side cache (DeepSeek prefix caching,
    /// Anthropic prompt caching). Default `0` for providers that don't
    /// report this. Cache-hit tokens are billed at a steep discount
    /// (~1/50 of cache-miss on DeepSeek V4).
    #[serde(default)]
    pub prompt_cache_hit_tokens: u64,
    /// Tokens NOT served from cache (full price). Default `0` for
    /// providers that don't report this.
    #[serde(default)]
    pub prompt_cache_miss_tokens: u64,
}

/// Trait for LLM providers.
///
/// Each provider (OpenAI, Anthropic, Google, etc.) implements this trait.
/// The provider streams [`LlmEvent`]s back through an `mpsc` channel,
/// which the turn engine consumes.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Human-readable name of this provider (e.g. "openai", "anthropic").
    fn name(&self) -> &str;

    /// List the models available under this provider.
    async fn list_models(&self) -> Result<Vec<String>>;

    /// Stream a completion request.
    ///
    /// The provider spawns an internal task that sends events into the
    /// returned channel. The caller reads from the channel until it closes.
    async fn stream(
        &self,
        request: LlmRequest,
    ) -> Result<tokio::sync::mpsc::Receiver<Result<LlmEvent>>>;

    /// Non-streaming completion — returns the full text response.
    ///
    /// Default implementation collects `TextDelta` events from `stream()`.
    /// Providers may override for a direct API call (no SSE overhead).
    async fn complete(&self, request: LlmRequest) -> Result<String> {
        let mut rx = self.stream(request).await?;
        let mut result = String::new();
        while let Some(event) = rx.recv().await {
            match event {
                Ok(LlmEvent::TextDelta(delta)) => result.push_str(&delta),
                Ok(LlmEvent::Done(_)) => break,
                Ok(LlmEvent::Error(e)) => return Err(crate::error::LuwuError::Llm(e)),
                Err(e) => return Err(e),
                _ => {}
            }
        }
        Ok(result)
    }
}

/// Cost ratio of cache-hit tokens to cache-miss tokens for the given
/// model. This is used to compute "effective tokens" for budget checks:
///
/// `effective = prompt_cache_miss + (prompt_cache_hit * ratio) + completion`
///
/// Lower = cheaper cache. Returns 0.0 for full cost (i.e. treat as miss).
///
/// Per-provider ratios (as of 2026):
/// - DeepSeek V4: 0.02 (1/50 — V4-Flash hit $0.0028 vs miss $0.14)
/// - GLM/智谱: 0.25 (1/4 — GLM Coding Plan ~4x cheaper on cache hit)
/// - MiniMax: 0.2 (1/5 — MiniMax M-series cache discount)
/// - Default: 0.1 (conservative, native OpenAI/Anthropic prompt caching
///   is ~10x cheaper than full price)
pub fn cache_ratio_for_model(model: &str) -> f64 {
    let m = model.to_lowercase();
    if m.contains("deepseek") {
        0.02 // 1/50
    } else if m.contains("glm") || m.contains("z-") {
        0.25 // 1/4
    } else if m.contains("minimax") || m.contains("abab") {
        0.2 // 1/5
    } else {
        0.1 // default conservative
    }
}

/// Compute "effective tokens" for budget accounting. Cache-hit tokens
/// are weighted by `cache_ratio_for_model` so a long task with high
/// cache hit rate doesn't get prematurely capped.
///
/// Example: a DeepSeek task with 400k cache-hit, 100k cache-miss, and
/// 50k completion tokens (550k raw total). Effective is
/// `100k + 400k*0.02 + 50k = 158k` — well under the 500k soft cap.
pub fn effective_tokens_for(usage: &LlmUsage, model: &str) -> u64 {
    let ratio = cache_ratio_for_model(model);
    let hit_effective = (usage.prompt_cache_hit_tokens as f64 * ratio) as u64;
    usage
        .prompt_cache_miss_tokens
        .saturating_add(hit_effective)
        .saturating_add(usage.completion_tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_ratio_known_providers() {
        assert!((cache_ratio_for_model("deepseek-v4-flash") - 0.02).abs() < 1e-9);
        assert!((cache_ratio_for_model("DeepSeek-V4-Pro") - 0.02).abs() < 1e-9);
        assert!((cache_ratio_for_model("glm-4.7") - 0.25).abs() < 1e-9);
        assert!((cache_ratio_for_model("GLM-5") - 0.25).abs() < 1e-9);
        assert!((cache_ratio_for_model("z-ai-model") - 0.25).abs() < 1e-9);
        assert!((cache_ratio_for_model("MiniMax-M3") - 0.2).abs() < 1e-9);
        assert!((cache_ratio_for_model("abab-6") - 0.2).abs() < 1e-9);
        // Unknown model falls back to conservative 0.1
        assert!((cache_ratio_for_model("gpt-4o") - 0.1).abs() < 1e-9);
        assert!((cache_ratio_for_model("claude-sonnet-4") - 0.1).abs() < 1e-9);
    }

    #[test]
    fn effective_tokens_weights_cache_hits() {
        // DeepSeek: 100k miss + 400k hit + 50k completion
        // = 100k + 400k*0.02 + 50k = 158k
        let usage = LlmUsage {
            prompt_tokens: 500_000,
            completion_tokens: 50_000,
            total_tokens: 550_000,
            prompt_cache_hit_tokens: 400_000,
            prompt_cache_miss_tokens: 100_000,
        };
        let eff = effective_tokens_for(&usage, "deepseek-v4-flash");
        assert_eq!(eff, 158_000);

        // GLM: 100k miss + 400k hit + 50k completion
        // = 100k + 400k*0.25 + 50k = 250k
        let eff = effective_tokens_for(&usage, "glm-4.7");
        assert_eq!(eff, 250_000);

        // No cache hit at all: effective = miss + completion
        let no_cache = LlmUsage {
            prompt_tokens: 200_000,
            completion_tokens: 50_000,
            total_tokens: 250_000,
            prompt_cache_hit_tokens: 0,
            prompt_cache_miss_tokens: 200_000,
        };
        let eff = effective_tokens_for(&no_cache, "deepseek-v4-flash");
        assert_eq!(eff, 250_000);
    }
}
