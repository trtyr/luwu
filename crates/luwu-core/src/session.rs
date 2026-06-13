//! Session types.
//!
//! A session represents a single conversation between the user and the agent.
//! It tracks the full message history, metadata, and can be persisted to
//! storage.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::event::SessionId;
use crate::message::Message;

/// Lightweight metadata about a session (for listing, not full content).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: SessionId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: usize,
    /// Optional title (e.g., derived from the first user message).
    pub title: Option<String>,
}

/// Full session data including all messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub id: SessionId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub messages: Vec<Message>,
    pub title: Option<String>,
    /// The model used for this session.
    pub model: String,
}

impl SessionData {
    /// Create a new empty session with the given model.
    pub fn new(model: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: SessionId::new(),
            created_at: now,
            updated_at: now,
            messages: Vec::new(),
            title: None,
            model: model.into(),
        }
    }

    /// Add a message to the session.
    pub fn push_message(&mut self, message: Message) {
        self.updated_at = Utc::now();
        self.messages.push(message);
    }
}
