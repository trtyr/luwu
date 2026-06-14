//! Tool registry — manages registered tools and produces LLM-ready definitions.
//!
//! The [`ToolRegistry`] holds all tools available to the agent. It can produce
//! [`ToolDefinition`] lists to send to the LLM, and dispatch tool calls to
//! the right handler at execution time.
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
#[derive(Clone)]
pub struct ToolRegistry {
    tools: Arc<HashMap<String, Box<dyn Tool>>>,
    /// Optional file history for rewind — when set, write/edit tools are
    /// intercepted to back up original files before modification.
    file_history: Option<Arc<tokio::sync::Mutex<crate::file_history::FileHistory>>>,
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            tools: Arc::new(HashMap::new()),
            file_history: None,
        }
    }

    /// Attach a file history for rewind support.
    /// Once attached, all write/edit tool calls will be intercepted to
    /// back up the original file before execution.
    pub fn with_file_history(
        mut self,
        fh: Arc<tokio::sync::Mutex<crate::file_history::FileHistory>>,
    ) -> Self {
        self.file_history = Some(fh);
        self
    }

    /// Register a tool. If a tool with the same name already exists, it's replaced.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let tools = Arc::get_mut(&mut self.tools)
            .expect("ToolRegistry::register called after sharing (Arc is shared). Register tools before sharing the registry.");
        tools.insert(tool.name().to_string(), tool);
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

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
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
    fn empty_registry() {
        let reg = ToolRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.tool_names().is_empty());
        assert!(reg.definitions().is_empty());
    }

    #[test]
    fn register_and_get() {
        let mut reg = ToolRegistry::new();
        reg.register(make_tool("bash"));
        assert!(!reg.is_empty());
        assert_eq!(reg.len(), 1);
        assert!(reg.get("bash").is_some());
        assert!(reg.get("nope").is_none());
    }

    #[test]
    fn register_multiple() {
        let mut reg = ToolRegistry::new();
        reg.register(make_tool("bash"));
        reg.register(make_tool("read"));
        reg.register(make_tool("write"));
        assert_eq!(reg.len(), 3);
        assert_eq!(reg.tool_names().len(), 3);
    }

    #[test]
    fn register_replaces_duplicate() {
        let mut reg = ToolRegistry::new();
        reg.register(make_tool("bash"));
        reg.register(make_tool("bash"));
        assert_eq!(reg.len(), 1); // replaced, not appended
    }

    #[test]
    fn definitions_match_registered() {
        let mut reg = ToolRegistry::new();
        reg.register(make_tool("bash"));
        let defs = reg.definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "bash");
        assert_eq!(defs[0].description, "bash tool");
    }

    #[tokio::test]
    async fn execute_unknown_tool_errors() {
        let reg = ToolRegistry::new();
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
        let mut reg = ToolRegistry::new();
        reg.register(make_tool("echo"));
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
        let mut reg = ToolRegistry::new();
        reg.register(make_tool("bash"));
        let cloned = reg.clone();
        assert_eq!(cloned.len(), 1);
        assert!(cloned.get("bash").is_some());
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
