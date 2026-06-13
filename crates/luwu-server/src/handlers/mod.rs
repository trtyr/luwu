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
//! - `stats` — runtime statistics endpoint

pub mod agent;
pub mod chat;
pub mod health;
pub mod memory_ops;
pub mod sessions;
pub mod skills;
pub mod stats;
pub mod workers;

// Flat re-exports so `handlers::xxx` keeps working in app.rs router.
pub use agent::agent_chat;
pub use chat::chat_completions;
pub use health::{health, list_models};
pub use memory_ops::{get_checkpoint, search_history};
pub use sessions::{cancel_turn, create_session, delete_session, get_session, list_sessions};
pub use skills::{get_skill_detail, list_skills};
pub use stats::stats;
