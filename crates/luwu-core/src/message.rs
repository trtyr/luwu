//! Message types for agent conversations.
//!
//! These are provider-agnostic — each LLM plugin translates between
//! [`Message`] and its own wire format (OpenAI chat completions, Anthropic
//! messages, etc.).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Who sent a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A single message in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentPart>,
    /// Optional sender name (e.g., the tool name for tool-result messages).
    pub name: Option<String>,
    /// Tool call ID — used to match tool results back to their calls.
    pub tool_call_id: Option<String>,
}

/// One part of a message's content.
///
/// A message can contain mixed text, tool calls, and tool results —
/// mirroring the multi-part content model used by modern LLM APIs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    /// Plain text content.
    #[serde(rename = "text")]
    Text { text: String },

    /// The LLM wants to call a tool.
    #[serde(rename = "tool_call")]
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
    },

    /// The result of a tool invocation.
    #[serde(rename = "tool_result")]
    ToolResult {
        id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
}

// ---- Convenience constructors ----

impl Message {
    /// Create a simple user text message.
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentPart::Text { text: text.into() }],
            name: None,
            tool_call_id: None,
        }
    }

    /// Create a system prompt message.
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: vec![ContentPart::Text { text: text.into() }],
            name: None,
            tool_call_id: None,
        }
    }

    /// Create a simple assistant text message.
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![ContentPart::Text { text: text.into() }],
            name: None,
            tool_call_id: None,
        }
    }

    /// Create a tool result message.
    pub fn tool_result(
        call_id: impl Into<String>,
        content: impl Into<String>,
        is_error: bool,
    ) -> Self {
        Self {
            role: Role::Tool,
            content: vec![ContentPart::ToolResult {
                id: call_id.into(),
                content: content.into(),
                is_error,
            }],
            name: None,
            tool_call_id: None,
        }
    }
}
