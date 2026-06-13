//! Luwu memory — four-layer memory system for long-running agent sessions.
//!
//! Layers:
//! - **Global**: user preferences, cross-project
//! - **Project**: project knowledge, architecture decisions
//! - **Session**: working state checkpoint (11 structured fields)
//! - **History**: full JSONL conversation log
//!
//! Key concepts:
//! - **Checkpoint**: structured state snapshot written by independent Writer
//! - **Cycle**: a window-bounded segment of the session; rebuild starts new cycle
//! - **Writer**: separate LLM call that extracts checkpoint, concurrent with main loop
//! - **Rebuild**: context reconstruction from persisted memory when window fills up

pub mod checkpoint;
pub mod history;
pub mod store;

pub use checkpoint::Checkpoint;
pub use history::{HistoryEntry, HistoryLog, TokenEstimator};
pub use store::MemoryStore;
