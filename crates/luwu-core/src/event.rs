//! Event system for agent lifecycle.
//!
//! The [`EventBus`] is the nervous system of luwu. Every meaningful thing
//! that happens — a turn starts, the LLM produces text, a tool is invoked —
//! is broadcast as an [`Event`]. Any component (TUI, logger, debugger) can
//! subscribe and react.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::llm::LlmUsage;
use crate::tool::ToolOutput;

// ---- ID types ----

/// Unique identifier for a session.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for a turn within a session.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TurnId(pub String);

impl TurnId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl Default for TurnId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TurnId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---- Event enum ----

/// Events emitted by the agent during execution.
///
/// These are broadcast to all subscribers via the [`EventBus`],
/// allowing UI layers, loggers, and debuggers to react to state changes.
#[derive(Debug, Clone)]
pub enum Event {
    // -- Session lifecycle --
    SessionCreated {
        session_id: SessionId,
    },
    SessionClosed {
        session_id: SessionId,
    },

    // -- Turn lifecycle --
    TurnStarted {
        session_id: SessionId,
        turn_id: TurnId,
    },
    TurnCompleted {
        session_id: SessionId,
        turn_id: TurnId,
        usage: LlmUsage,
    },

    // -- LLM streaming --
    LlmTextDelta {
        session_id: SessionId,
        turn_id: TurnId,
        delta: String,
    },
    LlmToolCall {
        session_id: SessionId,
        turn_id: TurnId,
        call_id: String,
        tool_name: String,
        arguments: Value,
    },

    // -- Tool execution --
    ToolStarted {
        session_id: SessionId,
        turn_id: TurnId,
        call_id: String,
        tool_name: String,
    },
    ToolCompleted {
        session_id: SessionId,
        turn_id: TurnId,
        call_id: String,
        output: ToolOutput,
    },

    // -- Errors --
    Error {
        session_id: SessionId,
        turn_id: Option<TurnId>,
        message: String,
    },
}

// ---- Turn event (streaming output for API consumers) ----

/// Lightweight events emitted during a streaming turn.
/// These are designed for API consumers (SSE, WebSocket) —
/// they carry just enough info without exposing internal types.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum TurnEvent {
    /// A text delta from the LLM.
    #[serde(rename = "text_delta")]
    TextDelta { delta: String },

    /// Reasoning/thinking content from the model (GLM, DeepSeek, MiniMax).
    #[serde(rename = "reasoning_delta")]
    ReasoningDelta { delta: String },

    /// The LLM is requesting a tool call.
    #[serde(rename = "tool_call")]
    ToolCall {
        call_id: String,
        tool_name: String,
        arguments: Value,
    },

    /// A tool execution has started.
    #[serde(rename = "tool_started")]
    ToolStarted { call_id: String, tool_name: String },

    /// A tool execution has finished.
    #[serde(rename = "tool_completed")]
    ToolCompleted {
        call_id: String,
        tool_name: String,
        output: String,
        is_error: bool,
    },

    /// A single agentic iteration has completed (LLM call + optional tool calls).
    #[serde(rename = "iteration_end")]
    IterationEnd {
        iteration: u32,
        tool_calls: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<crate::llm::LlmUsage>,
    },

    /// The entire turn is done.
    #[serde(rename = "done")]
    Done {
        assistant_text: String,
        llm_calls: u32,
        tool_calls: u32,
        usage: crate::llm::LlmUsage,
    },

    /// The turn was cancelled by the user.
    #[serde(rename = "cancelled")]
    Cancelled,

    /// An error occurred.
    #[serde(rename = "error")]
    Error { message: String },

    /// The agent loop is stuck — same tool called repeatedly with
    /// identical arguments, or a 2-call cycle has been detected. The
    /// engine breaks the loop and emits this so the UI can show why.
    #[serde(rename = "stuck")]
    Stuck {
        tool: String,
        reason: String,
    },

    /// Token budget soft cap reached — a system message was injected
    /// asking the LLM to wrap up. Not a hard stop; the LLM gets one
    /// more iteration to gracefully conclude.
    #[serde(rename = "budget_warning")]
    BudgetWarning {
        used_tokens: u64,
        soft_cap: u64,
    },
}

// ---- Event bus ----

/// A simple pub/sub event bus backed by a `tokio::sync::broadcast` channel.
///
/// Clone it freely — each clone can publish and subscribe independently.
/// Subscribers that fall behind will receive a lag notice and miss events.
#[derive(Clone)]
pub struct EventBus {
    sender: tokio::sync::broadcast::Sender<Event>,
}

impl EventBus {
    /// Create a new event bus with the given buffer capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = tokio::sync::broadcast::channel(capacity);
        Self { sender }
    }

    /// Broadcast an event to all subscribers.
    pub fn publish(&self, event: Event) {
        // Ignore errors — it just means nobody is listening.
        let _ = self.sender.send(event);
    }

    /// Subscribe to events. Returns a new receiver.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Event> {
        self.sender.subscribe()
    }
}

impl std::fmt::Debug for EventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventBus").finish()
    }
}
