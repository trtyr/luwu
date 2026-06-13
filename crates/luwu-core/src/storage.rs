//! Storage abstraction for session persistence.
//!
//! The [`Storage`] trait is the interface for saving and loading sessions.
//! Implementations can use files, SQLite, or any other backend.

use async_trait::async_trait;

use crate::error::Result;
use crate::event::SessionId;
use crate::session::{SessionData, SessionMeta};

/// Trait for session persistence backends.
#[async_trait]
pub trait Storage: Send + Sync {
    /// Save a session (creates or updates).
    async fn save_session(&self, session: &SessionData) -> Result<()>;

    /// Load a full session by its ID.
    async fn load_session(&self, id: &SessionId) -> Result<SessionData>;

    /// List all sessions (metadata only).
    async fn list_sessions(&self) -> Result<Vec<SessionMeta>>;

    /// Delete a session by its ID.
    async fn delete_session(&self, id: &SessionId) -> Result<()>;
}
