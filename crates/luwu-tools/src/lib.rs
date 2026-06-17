//! Built-in tool implementations for luwu.
//!
//! This crate provides the standard set of tools that implement
//! [`Tool`](luwu_core::Tool) from `luwu-core`:
//!
//! - `bash` — run shell commands
//! - `read` — read file contents / list directories
//! - `write` — create or completely overwrite files
//! - `edit` — make precise text replacements in existing files
//! - `grep` — grep / search file contents
//! - `web_fetch` — fetch web pages and extract readable content
//! - `todo` — task management (create/update/list/get/delete)
//! - `memory` — persistent memory (search/write/delete)

pub mod bash;
pub mod edit;
pub mod error;
pub mod grep;
pub mod memory;
pub mod read;
pub mod todo;
pub mod web_fetch;
pub mod write;

pub mod hashline;
use luwu_core::Tool;
use luwu_core::memory_backend::MemoryBackendFactory;

/// Build the list of built-in tools. The caller must provide a
/// `MemoryBackendFactory` so the `memory` tool can be wired to a concrete
/// backend (typically `MemoryStore` from `luwu-memory`). The factory pattern
/// keeps `luwu-tools` decoupled from `luwu-memory` (review P2 #22).
pub fn all_builtin_tools(memory_factory: MemoryBackendFactory) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(bash::BashTool::new()),
        Box::new(read::ReadTool::new()),
        Box::new(write::WriteTool::new()),
        Box::new(edit::EditTool::new()),
        Box::new(grep::GrepTool::new()),
        Box::new(web_fetch::WebFetchTool::new()),
        Box::new(memory::MemoryTool::new(memory_factory)),
        Box::new(todo::TodoTool::new()),
    ]
}
