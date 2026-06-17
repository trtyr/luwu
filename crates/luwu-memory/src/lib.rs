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
//! - **Deterministic compaction**: zero-LLM structured summary extraction
//! - **Observations/Reflections**: three-layer memory workers (Observer/Reflector/Dropper)

pub mod checkpoint;
pub mod consolidation;
pub mod correction;
pub mod deterministic;
pub mod history;
pub mod search_index;
pub mod store;
pub mod workers;

pub use checkpoint::Checkpoint;
pub use consolidation::{
    ConsolidationChecker, ConsolidationConfig, ConsolidationNeeded, ConsolidationResult,
    MemoryFileType, apply_consolidation, consolidation_prompt,
};
pub use correction::{CorrectionDetector, CorrectionPattern, CorrectionResult};
pub use deterministic::{DeterministicSummary, FileChange, compile as compile_summary};
pub use history::{HistoryEntry, HistoryLog, TokenEstimator};
pub use search_index::{SearchIndex, SearchResult};
pub use store::MemoryStore;
pub use workers::{WorkerThresholds, dropper_prompt, observer_prompt, reflector_prompt};
// Re-export the domain types from luwu-core for backward compatibility.
pub use luwu_core::{Observation, Priority, Reflection};
