//! Memory store — file-system backed four-layer memory.
//!
//! Layers (top to bottom):
//! - Global: user preferences, cross-project (~/.luwu/memory/global.md)
//! - Project: project knowledge (.luwu/memory/project.md)
//! - Session: working state checkpoint
//! - History: full conversation log (JSONL)

use crate::checkpoint::Checkpoint;
use crate::history::{HistoryLog, HistoryEntry, TokenEstimator};
use luwu_core::Message;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use tracing::debug;

/// Four-layer memory store backed by the filesystem.
pub struct MemoryStore {
    /// Root directory for all memory files.
    root: PathBuf,
    /// Global memory path.
    global_path: PathBuf,
    /// Current project memory root.
    project_root: PathBuf,
    /// Current session memory root.
    session_root: PathBuf,
    /// Token estimator.
    estimator: TokenEstimator,
}

impl MemoryStore {
    /// Create a new memory store.
    ///
    /// - `luwu_home`: typically `~/.luwu`
    /// - `project_dir`: the working directory of the project (used for hashing)
    /// - `session_id`: the current session ID
    pub fn new(luwu_home: &Path, project_dir: &Path, session_id: &str) -> Self {
        let root = luwu_home.join("memory");
        let global_path = root.join("global.md");
        let project_hash = hash_path(project_dir);
        let project_root = root.join(&project_hash);
        let session_root = project_root.join("sessions").join(session_id);

        // Ensure directories exist.
        std::fs::create_dir_all(&session_root).ok();

        Self {
            root,
            global_path,
            project_root,
            session_root,
            estimator: TokenEstimator::default(),
        }
    }

    // ── Global Memory ──────────────────────────────────────────

    /// Read global memory (user preferences).
    pub fn read_global(&self) -> String {
        read_file_or_empty(&self.global_path)
    }

    /// Write global memory.
    pub fn write_global(&self, content: &str) -> std::io::Result<()> {
        if let Some(parent) = self.global_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.global_path, content)
    }

    // ── Project Memory ─────────────────────────────────────────

    /// Read project memory.
    pub fn read_project(&self) -> String {
        read_file_or_empty(self.project_root.join("project.md"))
    }

    /// Write project memory (Writer only).
    pub fn write_project(&self, content: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.project_root)?;
        std::fs::write(self.project_root.join("project.md"), content)
    }

    // ── Session Memory (Checkpoint) ────────────────────────────

    /// Read the latest checkpoint.
    pub fn read_checkpoint(&self) -> Option<Checkpoint> {
        let path = self.session_root.join("checkpoint.md");
        let content = std::fs::read_to_string(&path).ok()?;
        if content.trim().is_empty() {
            return None;
        }
        Some(Checkpoint::from_markdown(&content))
    }

    /// Write checkpoint (Writer only).
    pub fn write_checkpoint(&self, checkpoint: &Checkpoint) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.session_root)?;
        let md = checkpoint.to_markdown();
        std::fs::write(self.session_root.join("checkpoint.md"), md)
    }

    /// Write raw checkpoint markdown (from Writer LLM output).
    pub fn write_checkpoint_raw(&self, markdown: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.session_root)?;
        std::fs::write(self.session_root.join("checkpoint.md"), markdown)
    }

    /// Read checkpoint as raw markdown.
    pub fn read_checkpoint_raw(&self) -> String {
        let path = self.session_root.join("checkpoint.md");
        std::fs::read_to_string(&path).unwrap_or_default()
    }

    // ── Notes (main agent scratchpad) ──────────────────────────

    /// Append to notes (main agent's only write channel).
    pub fn append_notes(&self, text: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.session_root)?;
        let path = self.session_root.join("notes.md");
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(file, "{text}")?;
        Ok(())
    }

    /// Read notes.
    pub fn read_notes(&self) -> String {
        read_file_or_empty(self.session_root.join("notes.md"))
    }

    /// Clear notes (Writer does this after routing content to checkpoint fields).
    pub fn clear_notes(&self) -> std::io::Result<()> {
        std::fs::write(self.session_root.join("notes.md"), "")
    }

    // ── History (JSONL conversation log) ───────────────────────

    /// Get or create the history log for this session.
    pub fn history_log(&self) -> std::io::Result<HistoryLog> {
        HistoryLog::open(&self.session_root.join("history.jsonl"))
    }

    /// Append a message to history.
    pub fn append_history(&self, msg: &Message) -> std::io::Result<()> {
        let log = self.history_log()?;
        log.append_message(msg, &self.estimator)
    }

    /// Search history by keyword.
    pub fn search_history(&self, query: &str, max_results: usize) -> std::io::Result<Vec<HistoryEntry>> {
        let log = self.history_log()?;
        log.search(query, max_results)
    }

    // ── Rebuild Context ────────────────────────────────────────

    /// Build the full rebuild context from all memory layers.
    /// This is injected into the new cycle's system prompt.
    pub fn build_rebuild_context(
        &self,
        recent_user_messages: &[String],
    ) -> String {
        let mut ctx = String::new();

        // 1. Checkpoint (working state) — highest priority.
        let checkpoint = self.read_checkpoint_raw();
        if !checkpoint.is_empty() {
            ctx.push_str("## 当前工作状态\n\n");
            ctx.push_str(&checkpoint);
        }

        // 2. Recent user messages (verbatim, prevent writer distortion).
        if !recent_user_messages.is_empty() {
            ctx.push_str("\n\n## 用户原始请求\n\n");
            for msg in recent_user_messages {
                ctx.push_str(msg);
                ctx.push('\n');
            }
        }

        // 3. Project memory.
        let project = self.read_project();
        if !project.is_empty() {
            ctx.push_str("\n\n## 项目知识\n\n");
            ctx.push_str(&project);
        }

        // 4. Global memory.
        let global = self.read_global();
        if !global.is_empty() {
            ctx.push_str("\n\n## 用户偏好\n\n");
            ctx.push_str(&global);
        }

        // 5. Notes.
        let notes = self.read_notes();
        if !notes.is_empty() {
            ctx.push_str("\n\n## 工作笔记\n\n");
            ctx.push_str(&notes);
        }

        // 6. Tail reminder.
        ctx.push_str("\n\n## 下一步\n\n");
        ctx.push_str("请根据以上状态继续工作。从「下一步动作」字段描述的动作开始执行。");

        ctx
    }

    /// Get the estimator reference.
    pub fn estimator(&self) -> &TokenEstimator {
        &self.estimator
    }

    /// Get the session root path.
    pub fn session_root(&self) -> &Path {
        &self.session_root
    }

    /// Get the project root path.
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }
}

/// Hash a path to a short directory name.
fn hash_path(path: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    path.to_string_lossy().hash(&mut hasher);
    let hash = hasher.finish();
    // Use first 12 hex chars for readability.
    format!("{:012x}", hash)
}

/// Read a file, return empty string if not found.
fn read_file_or_empty(path: impl AsRef<Path>) -> String {
    std::fs::read_to_string(&path).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_full_flow() {
        let dir = std::env::temp_dir().join("luwu_test_memory_store");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let store = MemoryStore::new(&dir, Path::new("/tmp/my-project"), "test-session-123");

        // Write global.
        store.write_global("喜欢用 Rust，不喜欢 Go").unwrap();
        assert_eq!(store.read_global(), "喜欢用 Rust，不喜欢 Go");

        // Write project.
        store.write_project("项目使用 tokio 异步运行时").unwrap();
        assert_eq!(store.read_project(), "项目使用 tokio 异步运行时");

        // Write checkpoint.
        let cp = Checkpoint {
            current_intent: "正在修复 bug".into(),
            next_action: "运行测试".into(),
            ..Checkpoint::default()
        };
        store.write_checkpoint(&cp).unwrap();

        let read_cp = store.read_checkpoint().unwrap();
        assert_eq!(read_cp.current_intent, "正在修复 bug");
        assert_eq!(read_cp.next_action, "运行测试");

        // Append notes.
        store.append_notes("发现问题的根因在 engine.rs:42").unwrap();
        store.append_notes("需要加 timeout 参数").unwrap();
        assert!(store.read_notes().contains("engine.rs:42"));

        // Build rebuild context.
        let ctx = store.build_rebuild_context(&["修复那个 timeout bug".into()]);
        assert!(ctx.contains("正在修复 bug"));
        assert!(ctx.contains("tokio 异步运行时"));
        assert!(ctx.contains("喜欢用 Rust"));
        assert!(ctx.contains("engine.rs:42"));
        assert!(ctx.contains("修复那个 timeout bug"));
        assert!(ctx.contains("下一步"));
    }

    #[test]
    fn hash_path_deterministic() {
        let h1 = hash_path(Path::new("/Users/test/my-project"));
        let h2 = hash_path(Path::new("/Users/test/my-project"));
        assert_eq!(h1, h2);
        assert_ne!(h1, hash_path(Path::new("/Users/test/other-project")));
    }
}
