//! Luwu — 陆吾，昆仑山的管家。
//!
//! This crate defines the core traits and types for the luwu agent framework.
//! Nothing here depends on any specific LLM provider, tool implementation,
//! or storage backend — those are all plugins.
//!
//! # Architecture
//!
//! The core is organized around a few key abstractions:
//!
//! - [`LlmProvider`] — trait for streaming LLM completions
//! - [`Tool`] — trait for agent tools (bash, file ops, search, etc.)
//! - [`Storage`] — trait for session persistence
//! - [`EventBus`] — pub/sub for agent lifecycle events
//!
//! Everything else is a type that these traits produce or consume.

pub mod engine;
pub mod cycle;
pub mod error;
pub mod event;
pub mod llm;
pub mod message;
pub mod session;
pub mod session_manager;
pub mod storage;
pub mod tool;
pub mod tool_registry;
pub mod prompt;
pub mod skill;

// Re-export the core types for convenience.
pub use cycle::{CycleAction, CycleState};
pub use engine::{CancelToken, TurnEngine, TurnResult};
pub use error::{LuwuError, Result};
pub use event::{Event, EventBus, SessionId, TurnEvent, TurnId};
pub use llm::{LlmEvent, LlmProvider, LlmRequest, LlmUsage, ToolDefinition};
pub use message::{ContentPart, Message, Role};
pub use session::{SessionData, SessionMeta};
pub use session_manager::{ManagedSession, SessionManager, SessionSummary, TrySetRunningError};
pub use storage::Storage;
pub use tool::{Tool, ToolContext, ToolOutput};
pub use tool_registry::ToolRegistry;
pub use prompt::{default_system_prompt, system_prompt_with_tools, system_prompt_with_tools_and_skills, writer_system_prompt};
pub use skill::{Skill, SkillFrontmatter, SkillRegistry};
