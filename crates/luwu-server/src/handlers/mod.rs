//! HTTP API handlers — modular per-feature layout.
//!
//! Module organization:
//! - `health` — health check + model listing
//! - `chat` — OpenAI-compatible chat completions
//! - `sessions` — session CRUD + cancel
//! - `agent` — agent event stream with tool visibility + cycle management
//! - `skills` — skill listing + detail
//! - `memory_ops` — checkpoint + history search
//! - `workers` — memory worker functions (consolidation, observer, reflector, checkpoint)

pub mod health;
pub mod chat;
pub mod sessions;
pub mod agent;
pub mod skills;
pub mod memory_ops;
pub mod workers;

// Flat re-exports so `handlers::xxx` keeps working in app.rs router.
pub use health::{health, list_models};
pub use chat::chat_completions;
pub use sessions::{list_sessions, create_session, get_session, delete_session, cancel_turn};
pub use agent::agent_chat;
pub use skills::{list_skills, get_skill_detail};
pub use memory_ops::{get_checkpoint, search_history};
