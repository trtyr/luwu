//! Tool registry — manages registered tools and produces LLM-ready definitions.
//!
//! The [`ToolRegistry`] uses a **Builder pattern** to separate registration
//! from use. Construct via [`ToolRegistry::builder`], register all tools,
//! then call [`ToolRegistryBuilder::build`] to get an immutable, cheaply-
//! cloneable registry. Registration after build is impossible by design.
//!
//! # Example
//!
//! ```ignore
//! use luwu_core::ToolRegistry;
//!
//! // Imagine a struct MyTool that implements the `Tool` trait.
//! let registry = ToolRegistry::builder()
//!     .register(Box::new(MyTool))
//!     .register(Box::new(OtherTool))
//!     .build();
//! // `registry` is now an immutable, cheaply-cloneable handle.
//! let cloned = registry.clone();
//! tokio::spawn(async move { cloned.execute("my_tool", json!({}), cwd, sid).await });
//! ```
//!
//! For a working example, see the integration tests in the `tests/` directory.
//!
//! File history integration: before executing write/edit tools, the registry
//! calls `track_edit` on the optional `FileHistory` to back up the original file.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::error::Result;
use crate::event::SessionId;
use crate::llm::ToolDefinition;
use crate::tool::{Tool, ToolContext, ToolOutput};

/// Registry of all tools available to the agent.
///
/// Cheaply cloneable (`Arc` inside) and safe to share across async tasks.
/// Tools can only be registered via [`ToolRegistry::builder`].
#[derive(Clone)]
pub struct ToolRegistry {
    tools: Arc<HashMap<String, Box<dyn Tool>>>,
    /// Optional file history for rewind — when set, write/edit tools are
    /// intercepted to back up the original files before modification.
    file_history: Option<Arc<tokio::sync::Mutex<crate::file_history::FileHistory>>>,
}

impl ToolRegistry {
    /// Start building a new registry.
    pub fn builder() -> ToolRegistryBuilder {
        ToolRegistryBuilder::new()
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// List all registered tool names.
    pub fn tool_names(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }

    /// Generate [`ToolDefinition`]s for all registered tools.
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .map(|t| ToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.parameters_schema(),
            })
            .collect()
    }

    /// Execute a tool by name with the given JSON input.
    /// If file history is attached and the tool is write/edit, the target file
    /// is backed up BEFORE execution.
    pub async fn execute(
        &self,
        name: &str,
        input: serde_json::Value,
        working_dir: PathBuf,
        session_id: SessionId,
    ) -> Result<ToolOutput> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| crate::error::LuwuError::Tool(format!("Unknown tool: {name}")))?;

        // ── File history: back up files before write/edit ──
        if let Some(fh) = &self.file_history
            && let Some(file_path) = extract_file_path(name, &input)
        {
            let fh = fh.clone();
            // Use try_lock — don't block the agent if history is being read
            if let Ok(mut guard) = fh.try_lock()
                && let Err(e) = guard.track_edit(&file_path, &session_id.0)
            {
                tracing::warn!(error = %e, file = %file_path, "File history track_edit failed");
            }
        }

        let context = ToolContext {
            working_dir,
            session_id,
        };

        tool.execute(input, context).await
    }

    /// How many tools are registered.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Are there any tools registered?
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

/// Builder for [`ToolRegistry`]. Accumulates tools, then `build()` seals
/// the registry into an immutable, shareable form. This eliminates the
/// `Arc::get_mut` panic risk that comes with `register` after `clone`.
pub struct ToolRegistryBuilder {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistryBuilder {
    /// Create an empty builder.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool. If a tool with the same name already exists, it's replaced.
    /// Can be called multiple times in a chain.
    pub fn register(mut self, tool: Box<dyn Tool>) -> Self {
        self.tools.insert(tool.name().to_string(), tool);
        self
    }

    /// Seal the builder into a [`ToolRegistry`]. After this call, tools
    /// cannot be added or replaced.
    pub fn build(self) -> ToolRegistry {
        ToolRegistry {
            tools: Arc::new(self.tools),
            file_history: None,
        }
    }

    /// How many tools have been registered so far.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Has no tool been registered yet?
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl Default for ToolRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract the file path from a tool's JSON arguments for write/edit tools.
/// Returns None for tools that don't modify files.
fn extract_file_path(tool_name: &str, input: &serde_json::Value) -> Option<String> {
    match tool_name {
        "write" | "edit" => input
            .get("path")
            .or_else(|| input.get("file_path"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "bash" => None, // bash is too unpredictable to track
        _ => None,
    }
}

// File history attachment is a setter on the immutable registry, since
// it doesn't change the tool set.
impl ToolRegistry {
    /// Attach a file history for rewind support. Returns a new registry
    /// (the original is unchanged, since `ToolRegistry` is cheaply cloneable).
    /// Once attached, all write/edit tool calls will be intercepted to
    /// back up the original file before execution.
    pub fn with_file_history(
        &self,
        fh: Arc<tokio::sync::Mutex<crate::file_history::FileHistory>>,
    ) -> Self {
        Self {
            tools: Arc::clone(&self.tools),
            file_history: Some(fh),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::SessionId;
    use async_trait::async_trait;

    struct EchoTool {
        name: String,
        desc: String,
    }

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            &self.name
        }
        fn description(&self) -> &str {
            &self.desc
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {}
            })
        }
        async fn execute(&self, input: serde_json::Value, _ctx: ToolContext) -> Result<ToolOutput> {
            Ok(ToolOutput::text(format!("echo: {input}")))
        }
    }

    fn make_tool(name: &str) -> Box<dyn Tool> {
        Box::new(EchoTool {
            name: name.into(),
            desc: format!("{name} tool"),
        })
    }

    #[test]
    fn empty_builder() {
        let b = ToolRegistryBuilder::new();
        assert!(b.is_empty());
        assert_eq!(b.len(), 0);
    }

    #[test]
    fn empty_registry() {
        let reg = ToolRegistry::builder().build();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.tool_names().is_empty());
        assert!(reg.definitions().is_empty());
    }

    #[test]
    fn builder_register_and_get() {
        let reg = ToolRegistry::builder()
            .register(make_tool("bash"))
            .build();
        assert!(!reg.is_empty());
        assert_eq!(reg.len(), 1);
        assert!(reg.get("bash").is_some());
        assert!(reg.get("nope").is_none());
    }

    #[test]
    fn builder_register_multiple() {
        let reg = ToolRegistry::builder()
            .register(make_tool("bash"))
            .register(make_tool("read"))
            .register(make_tool("write"))
            .build();
        assert_eq!(reg.len(), 3);
        assert_eq!(reg.tool_names().len(), 3);
    }

    #[test]
    fn builder_register_replaces_duplicate() {
        let reg = ToolRegistry::builder()
            .register(make_tool("bash"))
            .register(make_tool("bash"))
            .build();
        assert_eq!(reg.len(), 1); // replaced, not appended
    }

    #[test]
    fn builder_definitions_match() {
        let reg = ToolRegistry::builder()
            .register(make_tool("bash"))
            .build();
        let defs = reg.definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "bash");
        assert_eq!(defs[0].description, "bash tool");
    }

    #[tokio::test]
    async fn execute_unknown_tool_errors() {
        let reg = ToolRegistry::builder().build();
        let result = reg
            .execute(
                "ghost",
                serde_json::json!({}),
                PathBuf::from("/tmp"),
                SessionId("s1".to_string()),
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn execute_calls_tool() {
        let reg = ToolRegistry::builder()
            .register(make_tool("echo"))
            .build();
        let output = reg
            .execute(
                "echo",
                serde_json::json!({"msg": "hi"}),
                PathBuf::from("/tmp"),
                SessionId("s1".to_string()),
            )
            .await
            .unwrap();
        assert!(output.content.contains("hi"));
    }

    #[test]
    fn clone_shares_registry() {
        let reg = ToolRegistry::builder()
            .register(make_tool("bash"))
            .build();
        let cloned = reg.clone();
        assert_eq!(cloned.len(), 1);
        assert!(cloned.get("bash").is_some());
    }

    /// P0 risk: Arc::get_mut panic on register after clone is now structurally impossible.
    #[test]
    fn no_panic_register_after_clone() {
        let reg = ToolRegistry::builder()
            .register(make_tool("bash"))
            .build();
        let _cloned = reg.clone();
        // There is no `register` method on `ToolRegistry` anymore — the builder
        // is the only way to add tools, and it cannot be cloned.
        // This test passes by virtue of compilation: the unsafe pattern is gone.
    }

    #[test]
    fn extract_file_path_write() {
        let input = serde_json::json!({"path": "src/main.rs", "content": "fn main() {}"});
        assert_eq!(
            extract_file_path("write", &input),
            Some("src/main.rs".to_string())
        );
    }

    #[test]
    fn extract_file_path_edit() {
        let input = serde_json::json!({"path": "src/lib.rs", "old_text": "a", "new_text": "b"});
        assert_eq!(
            extract_file_path("edit", &input),
            Some("src/lib.rs".to_string())
        );
    }

    #[test]
    fn extract_file_path_bash_returns_none() {
        let input = serde_json::json!({"command": "ls"});
        assert_eq!(extract_file_path("bash", &input), None);
    }

    #[test]
    fn extract_file_path_file_path_key() {
        let input = serde_json::json!({"file_path": "src/util.rs"});
        assert_eq!(
            extract_file_path("write", &input),
            Some("src/util.rs".to_string())
        );
    }
}
