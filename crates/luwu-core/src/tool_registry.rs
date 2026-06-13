//! Tool registry — manages registered tools and produces LLM-ready definitions.
//!
//! The [`ToolRegistry`] holds all tools available to the agent. It can produce
//! [`ToolDefinition`] lists to send to the LLM, and dispatch tool calls to
//! the right handler at execution time.

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
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            tools: Arc::new(HashMap::new()),
        }
    }

    /// Register a tool. If a tool with the same name already exists, it's replaced.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        // We need to get a mutable reference to the inner HashMap.
        // Since we use Arc, we need to check if it's uniquely owned.
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
    /// These are sent to the LLM so it knows what it can call.
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

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
