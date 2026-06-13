//! Session manager — server-side session storage with file persistence.
//!
//! Manages conversation sessions for the HTTP API layer.
//! Each session holds a [`SessionData`] plus runtime state like the cancel token.
//!
//! # Persistence
//!
//! Sessions are persisted as JSON files in `~/.luwu/sessions/{id}.json`.
//! Every mutation (create, update, append, delete) is written through to disk
//! synchronously while holding the write lock — ensuring consistency between
//! the in-memory map and the filesystem.
//!
//! On startup, [`SessionManager::load_from_disk`] restores all sessions.
//! Runtime state (`is_running`, `cancel_token`) is never persisted; sessions
//! always resume in a clean, not-running state.
//!
//! # Concurrency
//!
//! Different sessions write to different files — no file-level contention.
//! Same-session writes are serialized by the `RwLock` on the internal map.
//! [`append_messages`] performs the append + disk-write atomically inside a
//! single write lock, eliminating the read-modify-write race.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::engine::CancelToken;
use crate::message::Message;
use crate::session::SessionData;

/// A managed session with runtime state.
#[derive(Debug)]
pub struct ManagedSession {
    /// The core session data (messages, model, etc.) — this is what gets persisted.
    pub data: SessionData,
    /// Cancellation token for the currently running turn (if any) — NOT persisted.
    pub cancel_token: CancelToken,
    /// Whether a turn is currently running — NOT persisted (always false on load).
    pub is_running: bool,
}

/// Error returned by [`SessionManager::try_set_running`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrySetRunningError {
    NotFound,
    AlreadyRunning,
}
/// Server-side session manager with file-based persistence.
#[derive(Debug, Clone)]
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, ManagedSession>>>,
    /// Directory where session JSON files are stored.
    sessions_dir: PathBuf,
}

impl SessionManager {
    /// Create an empty session manager with no persistence directory.
    /// Use [`with_persistence`](Self::with_persistence) to enable disk storage.
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            sessions_dir: PathBuf::new(),
        }
    }

    /// Create a session manager backed by a filesystem directory.
    ///
    /// All sessions will be persisted as `{sessions_dir}/{id}.json`.
    /// The directory is created if it does not exist.
    pub fn with_persistence(sessions_dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let dir = sessions_dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            sessions_dir: dir,
        })
    }

    /// Load all persisted sessions from disk into memory.
    ///
    /// Should be called once on server startup before serving requests.
    /// Sessions that fail to parse are skipped with a warning.
    /// All loaded sessions resume with `is_running: false`.
    pub async fn load_from_disk(&self) -> usize {
        if self.sessions_dir.as_os_str().is_empty() {
            return 0;
        }

        let entries = match std::fs::read_dir(&self.sessions_dir) {
            Ok(e) => e,
            Err(err) => {
                warn!(
                    "Cannot read sessions directory {}: {err}",
                    self.sessions_dir.display()
                );
                return 0;
            }
        };

        let mut count = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }

            let raw = match std::fs::read_to_string(&path) {
                Ok(r) => r,
                Err(err) => {
                    warn!("Cannot read session file {}: {err}", path.display());
                    continue;
                }
            };

            let data: SessionData = match serde_json::from_str(&raw) {
                Ok(d) => d,
                Err(err) => {
                    warn!("Cannot parse session file {}: {err}", path.display());
                    continue;
                }
            };

            let id = data.id.to_string();
            debug!(
                "Loaded session {id} from disk ({} messages)",
                data.messages.len()
            );

            let session = ManagedSession {
                data,
                cancel_token: CancelToken::new(),
                is_running: false,
            };

            self.sessions.write().await.insert(id, session);
            count += 1;
        }

        if count > 0 {
            info!("Recovered {count} sessions from disk");
        }

        count
    }

    // ─── Session CRUD ──────────────────────────────────────────────

    /// Create a new session and store it.
    pub async fn create(&self, model: impl Into<String>) -> ManagedSessionRef {
        let data = SessionData::new(model);
        self.insert_session(data).await
    }

    /// Create a session with a specific provider.
    pub async fn create_with_provider(
        &self,
        model: impl Into<String>,
        provider: impl Into<String>,
    ) -> ManagedSessionRef {
        let data = SessionData::with_provider(model, provider);
        self.insert_session(data).await
    }

    async fn insert_session(&self, data: SessionData) -> ManagedSessionRef {
        let id = data.id.to_string();

        // Persist to disk before inserting into memory.
        self.persist_session(&id, &data).await;

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

    /// Update a session's entire message list (write-through).
    pub async fn update_messages(&self, id: &str, messages: Vec<Message>) -> bool {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(id) {
            session.data.messages = messages;
            session.data.updated_at = Utc::now();
            self.persist_session(id, &session.data).await;
            true
        } else {
            false
        }
    }

    /// Atomically append messages to a session (write-lock, no read-modify-write race).
    pub async fn append_messages(&self, id: &str, new_messages: Vec<Message>) -> bool {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(id) {
            session.data.messages.extend(new_messages);
            session.data.updated_at = Utc::now();
            self.persist_session(id, &session.data).await;
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
                session.cancel_token = CancelToken::new();
            }
            Some(session.cancel_token.clone())
        } else {
            None
        }
    }

    /// Atomically check if running and set to running if not.
    ///
    /// Combines the check-and-set into one lock acquisition,
    /// eliminating the TOCTOU race between `get()` and `set_running()`.
    pub async fn try_set_running(&self, id: &str) -> Result<CancelToken, TrySetRunningError> {
        let mut sessions = self.sessions.write().await;
        let session = sessions.get_mut(id).ok_or(TrySetRunningError::NotFound)?;
        if session.is_running {
            return Err(TrySetRunningError::AlreadyRunning);
        }
        session.is_running = true;
        session.cancel_token = CancelToken::new();
        Ok(session.cancel_token.clone())
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

    /// Delete a session (removes from memory and disk).
    pub async fn delete(&self, id: &str) -> bool {
        let existed = self.sessions.write().await.remove(id).is_some();
        if existed {
            self.remove_session_file(id).await;
        }
        existed
    }

    // ─── Persistence helpers ───────────────────────────────────────

    /// Write a session's data to disk as JSON.
    /// Called while holding the write lock — synchronous to ensure consistency.
    async fn persist_session(&self, id: &str, data: &SessionData) {
        if self.sessions_dir.as_os_str().is_empty() {
            return;
        }

        let path = self.sessions_dir.join(format!("{id}.json"));
        match serde_json::to_string_pretty(data) {
            Ok(json) => {
                if let Err(err) = tokio::fs::write(&path, json).await {
                    warn!("Failed to persist session {id}: {err}");
                }
            }
            Err(err) => {
                warn!("Failed to serialize session {id}: {err}");
            }
        }
    }

    /// Remove a session's file from disk.
    async fn remove_session_file(&self, id: &str) {
        if self.sessions_dir.as_os_str().is_empty() {
            return;
        }
        let path = self.sessions_dir.join(format!("{id}.json"));
        let _ = tokio::fs::remove_file(path).await;
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

/// RAII guard that resets `is_running` to false when dropped.
///
/// Create this right after `try_set_running()` and hold it for the
/// duration of the agent turn. When it goes out of scope (normal exit,
/// early return, panic, stream cancellation), it spawns a task to reset
/// the running flag — preventing stuck sessions.
pub struct RunningGuard {
    sessions: SessionManager,
    id: String,
}

impl RunningGuard {
    /// Create a guard for the given session.
    /// The caller must have already called `try_set_running()` successfully.
    pub fn new(sessions: SessionManager, id: impl Into<String>) -> Self {
        Self {
            sessions,
            id: id.into(),
        }
    }
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        let sessions = self.sessions.clone();
        let id = self.id.clone();
        // Drop can't be async — spawn a task to do the reset.
        tokio::spawn(async move {
            sessions.set_running(&id, false).await;
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // ─── CRUD ──────────────────────────────────────────────

    #[tokio::test]
    async fn create_and_get() {
        let mgr = SessionManager::new();
        let session = mgr.create("test-model").await;
        let got = mgr.get(&session.id).await;
        assert!(got.is_some());
        assert_eq!(got.unwrap().data.model, "test-model");
    }

    #[tokio::test]
    async fn list_returns_all_sessions() {
        let mgr = SessionManager::new();
        mgr.create("model-a").await;
        mgr.create("model-b").await;
        assert_eq!(mgr.list().await.len(), 2);
    }

    #[tokio::test]
    async fn delete_removes_session() {
        let mgr = SessionManager::new();
        let session = mgr.create("model").await;
        assert!(mgr.delete(&session.id).await);
        assert!(mgr.get(&session.id).await.is_none());
        assert!(mgr.list().await.is_empty());
    }

    // ─── try_set_running (TOCTOU fix) ─────────────────────

    #[tokio::test]
    async fn try_set_running_marks_session_running() {
        let mgr = SessionManager::new();
        let session = mgr.create("model").await;
        assert!(mgr.try_set_running(&session.id).await.is_ok());
        assert!(mgr.get(&session.id).await.unwrap().is_running);
    }

    #[tokio::test]
    async fn try_set_running_rejects_concurrent() {
        let mgr = SessionManager::new();
        let session = mgr.create("model").await;
        mgr.try_set_running(&session.id).await.unwrap();
        let err = mgr.try_set_running(&session.id).await.unwrap_err();
        assert_eq!(err, TrySetRunningError::AlreadyRunning);
    }

    #[tokio::test]
    async fn try_set_running_missing_session() {
        let mgr = SessionManager::new();
        let err = mgr.try_set_running("nope").await.unwrap_err();
        assert_eq!(err, TrySetRunningError::NotFound);
    }

    // ─── RunningGuard RAII ────────────────────────────────

    #[tokio::test]
    async fn running_guard_resets_on_drop() {
        let mgr = SessionManager::new();
        let session = mgr.create("model").await;
        mgr.try_set_running(&session.id).await.unwrap();
        {
            let _guard = RunningGuard::new(mgr.clone(), &session.id);
        }
        // Drop spawns a background task — give it a tick.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!mgr.get(&session.id).await.unwrap().is_running);
    }

    // ─── Cancel ───────────────────────────────────────────

    #[tokio::test]
    async fn cancel_running_session_succeeds() {
        let mgr = SessionManager::new();
        let session = mgr.create("model").await;
        mgr.try_set_running(&session.id).await.unwrap();
        assert!(mgr.cancel(&session.id).await);
    }

    #[tokio::test]
    async fn cancel_idle_session_fails() {
        let mgr = SessionManager::new();
        let session = mgr.create("model").await;
        assert!(!mgr.cancel(&session.id).await);
    }

    // ─── append_messages ──────────────────────────────────

    #[tokio::test]
    async fn append_messages_grows_history() {
        let mgr = SessionManager::new();
        let session = mgr.create("model").await;
        assert!(
            mgr.append_messages(&session.id, vec![Message::user("hi")])
                .await
        );
        assert_eq!(mgr.get(&session.id).await.unwrap().data.messages.len(), 1);
    }

    #[tokio::test]
    async fn append_messages_missing_session() {
        let mgr = SessionManager::new();
        assert!(
            !mgr.append_messages("ghost", vec![Message::user("hi")])
                .await
        );
    }

    // ─── Persistence round-trip ───────────────────────────

    #[tokio::test]
    async fn persistence_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "luwu-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        let mgr = SessionManager::with_persistence(&dir).unwrap();

        let session = mgr.create("persist-model").await;
        mgr.append_messages(&session.id, vec![Message::user("hello")])
            .await;

        // Fresh manager loads from the same directory.
        let mgr2 = SessionManager::with_persistence(&dir).unwrap();
        assert_eq!(mgr2.load_from_disk().await, 1);

        let loaded = mgr2.get(&session.id).await.unwrap();
        assert_eq!(loaded.data.model, "persist-model");
        assert_eq!(loaded.data.messages.len(), 1);
        assert!(!loaded.is_running); // runtime state not persisted

        let _ = std::fs::remove_dir_all(&dir);
    }
}
