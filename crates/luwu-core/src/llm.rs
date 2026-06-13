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
}
