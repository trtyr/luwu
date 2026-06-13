//! Session manager — server-side session storage.
//!
//! Manages conversation sessions for the HTTP API layer.
//! Each session holds a [`SessionData`] plus runtime state like the cancel token.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::engine::CancelToken;
use crate::message::Message;
use crate::session::SessionData;

/// A managed session with runtime state.
#[derive(Debug)]
pub struct ManagedSession {
    /// The core session data (messages, model, etc.).
    pub data: SessionData,
    /// Cancellation token for the currently running turn (if any).
    pub cancel_token: CancelToken,
    /// Whether a turn is currently running.
    pub is_running: bool,
}

/// Server-side session manager.
#[derive(Debug, Clone)]
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, ManagedSession>>>,
}

impl SessionManager {
    /// Create an empty session manager.
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new session and store it.
    pub async fn create(&self, model: impl Into<String>) -> ManagedSessionRef {
        let data = SessionData::new(model);
        let id = data.id.to_string();

        let session = ManagedSession {
            data,
            cancel_token: CancelToken::new(),
            is_running: false,
        };

        self.sessions.write().await.insert(id.clone(), session);

        ManagedSessionRef {
            id,
            _manager: self.clone(),
        }
    }

    /// Get a session by ID.
    pub async fn get(&self, id: &str) -> Option<ManagedSession> {
        self.sessions.read().await.get(id).cloned()
    }

    /// List all session summaries.
    pub async fn list(&self) -> Vec<SessionSummary> {
        let sessions = self.sessions.read().await;
        sessions
            .values()
            .map(|s| SessionSummary {
                id: s.data.id.to_string(),
                model: s.data.model.clone(),
                message_count: s.data.messages.len(),
                title: s.data.title.clone(),
                created_at: s.data.created_at,
                updated_at: s.data.updated_at,
                is_running: s.is_running,
            })
            .collect()
    }

    /// Update a session's data (e.g., after adding messages).
    pub async fn update_messages(&self, id: &str, messages: Vec<Message>) -> bool {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(id) {
            session.data.messages = messages;
            session.data.updated_at = Utc::now();
            true
        } else {
            false
        }
    }

    /// Set the running state and get a reference to the cancel token.
    pub async fn set_running(&self, id: &str, running: bool) -> Option<CancelToken> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(id) {
            session.is_running = running;
            if running {
                // Reset the cancel token for a new run.
                session.cancel_token = CancelToken::new();
            }
            Some(session.cancel_token.clone())
        } else {
            None
        }
    }

    /// Cancel the currently running turn for a session.
    pub async fn cancel(&self, id: &str) -> bool {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(id) {
            if session.is_running {
                session.cancel_token.cancel();
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Delete a session.
    pub async fn delete(&self, id: &str) -> bool {
        self.sessions.write().await.remove(id).is_some()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// A lightweight reference to a session (just the ID + manager pointer).
#[derive(Debug, Clone)]
pub struct ManagedSessionRef {
    pub id: String,
    pub _manager: SessionManager,
}

/// Summary of a session (for listing, without full message history).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub model: String,
    pub message_count: usize,
    pub title: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub is_running: bool,
}

// Clone impl for ManagedSession.
impl Clone for ManagedSession {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            cancel_token: self.cancel_token.clone(),
            is_running: self.is_running,
        }
    }
}
