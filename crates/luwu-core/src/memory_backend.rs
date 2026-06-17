//! Memory backend abstraction — the boundary between `luwu-tools` and `luwu-memory`.
//!
//! ## Why this exists
//!
//! The `memory` tool in `luwu-tools` needs to read and write persistent memory
//! entries. Before this trait, it depended directly on `luwu_memory::MemoryStore`,
//! which meant `luwu-tools` → `luwu-memory` was a hard dependency. That
//! violated the layered architecture (tools should depend on the microkernel,
//! not on infrastructure crates).
//!
//! With this trait:
//! - `luwu-tools` depends on the `MemoryBackend` trait (defined here).
//! - `luwu-memory` provides the concrete `MemoryStore` implementation.
//! - Tests and alternative implementations (in-memory, encrypted, network)
//!   can be plugged in without changing the tool code.
//!
//! ## Design
//!
//! - **Stateless factory pattern**: `MemoryBackend` is per-instance and the
//!   tool creates a fresh one on each `execute()` call (via a factory closure).
//!   This avoids the previous "share one `MemoryStore` across all calls"
//!   risk where concurrent invocations could clobber each other.
//! - **Data types live here**: `Priority`, `Observation`, `Reflection` are
//!   domain types, not storage types, so they belong in the microkernel.
//!   `luwu-memory` re-exports them for backward compatibility.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Result type for memory backend operations.
pub type MemoryResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

// ---------------------------------------------------------------------------
// Domain types — moved from luwu-memory::workers so the trait can be defined
// in the microkernel without luwu-core depending on luwu-memory.
// ---------------------------------------------------------------------------

/// Priority level for observations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    High,
    Medium,
    Low,
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::High => write!(f, "high"),
            Self::Medium => write!(f, "medium"),
            Self::Low => write!(f, "low"),
        }
    }
}

/// Generate a 12-character hex ID for traceability (used by Observation/Reflection
/// `new()` constructors). Uses `DefaultHasher` seeded from system time + a static
/// counter for low collision probability within a single process.
fn gen_hex_id() -> String {
    use std::hash::{Hash, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
        .hash(&mut hasher);
    n.hash(&mut hasher);
    // 12-char hex ID matching the original format (truncate from 16).
    let full = format!("{:016x}", hasher.finish());
    full[..12].to_string()
}

/// A timestamped observation extracted by an Observer-style worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    /// 12-char hex ID for traceability.
    pub id: String,
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// Priority (high/medium/low).
    pub priority: Priority,
    /// The observation content.
    pub content: String,
    /// Category: event, decision, preference, error, pattern.
    pub category: String,
}

impl Observation {
    /// Create a new observation with auto-generated ID and current timestamp.
    pub fn new(
        priority: Priority,
        category: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: gen_hex_id(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            priority,
            category: category.into(),
            content: content.into(),
        }
    }
}

/// A durable reflection synthesized by a Reflector-style worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reflection {
    /// 12-char hex ID.
    pub id: String,
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// The reflection content — a stable fact, pattern, or constraint.
    pub content: String,
    /// Source observation IDs that led to this reflection.
    pub source_ids: Vec<String>,
}

impl Reflection {
    /// Create a new reflection with auto-generated ID and current timestamp.
    pub fn new(content: impl Into<String>, source_ids: Vec<String>) -> Self {
        Self {
            id: gen_hex_id(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            content: content.into(),
            source_ids,
        }
    }
}

// ---------------------------------------------------------------------------
// Trait — the abstraction over any persistent memory implementation.
// ---------------------------------------------------------------------------

/// Abstraction over a persistent memory store. The `memory` tool in
/// `luwu-tools` uses this trait so it can be wired with any backend
/// (filesystem-backed `MemoryStore` from `luwu-memory`, or a test mock).
///
/// All methods take `&self` (immutable) so the trait is trivially `Sync`-safe.
/// The concrete implementation is expected to handle its own internal locking
/// for concurrent access.
pub trait MemoryBackend: Send + Sync {
    /// Return all observations (chronological, oldest first).
    fn read_observations(&self) -> Vec<Observation>;

    /// Return all reflections (chronological, oldest first).
    fn read_reflections(&self) -> Vec<Reflection>;

    /// Full-text search across all memory layers. Returns a human-readable
    /// result string ready for display in the tool output.
    fn search_all(&self, query: &str) -> String;

    /// Append a durable entry to the global memory layer (`~/.luwu/memory/global.md`).
    fn append_global_entry(&self, entry: &str) -> MemoryResult<()>;

    /// Append a durable entry to the project memory layer
    /// (`~/.luwu/memory/<project-hash>/project.md`).
    fn append_project_entry(&self, entry: &str) -> MemoryResult<()>;

    /// Path to the global memory file (for read/direct-edit operations).
    fn global_path(&self) -> &Path;

    /// Path to the project memory file (for read/direct-edit operations).
    fn project_path(&self) -> PathBuf;
}

/// Factory closure type for creating fresh `MemoryBackend` instances.
///
/// The `memory` tool holds a `MemoryBackendFactory` and invokes it on every
/// `execute()` call to get a fresh backend bound to the current
/// `(home, working_dir, session_id)`. This avoids cross-session state bleed
/// and lets concurrent tool invocations use independent backends.
pub type MemoryBackendFactory =
    Arc<dyn Fn(&Path, &Path, &str) -> Box<dyn MemoryBackend> + Send + Sync>;

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock backend for testing the tool layer without filesystem access.
    pub struct MockBackend {
        pub observations: Vec<Observation>,
        pub reflections: Vec<Reflection>,
        pub global_entries: std::sync::Mutex<Vec<String>>,
        pub project_entries: std::sync::Mutex<Vec<String>>,
    }

    impl MockBackend {
        pub fn new() -> Self {
            Self {
                observations: vec![Observation {
                    id: "obs001".into(),
                    timestamp: "2026-01-01T00:00:00Z".into(),
                    priority: Priority::High,
                    content: "Test observation".into(),
                    category: "event".into(),
                }],
                reflections: vec![],
                global_entries: std::sync::Mutex::new(vec![]),
                project_entries: std::sync::Mutex::new(vec![]),
            }
        }
    }

    impl MemoryBackend for MockBackend {
        fn read_observations(&self) -> Vec<Observation> {
            self.observations.clone()
        }
        fn read_reflections(&self) -> Vec<Reflection> {
            self.reflections.clone()
        }
        fn search_all(&self, _query: &str) -> String {
            "Mock search result".to_string()
        }
        fn append_global_entry(&self, entry: &str) -> MemoryResult<()> {
            self.global_entries.lock().unwrap().push(entry.to_string());
            Ok(())
        }
        fn append_project_entry(&self, entry: &str) -> MemoryResult<()> {
            self.project_entries.lock().unwrap().push(entry.to_string());
            Ok(())
        }
        fn global_path(&self) -> &Path {
            Path::new("/tmp/mock/global.md")
        }
        fn project_path(&self) -> PathBuf {
            PathBuf::from("/tmp/mock/project.md")
        }
    }

    #[test]
    fn mock_backend_roundtrip() {
        let m = MockBackend::new();
        assert_eq!(m.read_observations().len(), 1);
        assert_eq!(m.read_observations()[0].id, "obs001");
        assert!(m.append_project_entry("[insight] hello").is_ok());
        assert_eq!(m.project_entries.lock().unwrap().len(), 1);
    }
}
