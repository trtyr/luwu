//! todo tool — lightweight task management for the agent.
//!
//! Inspired by Claude Code's TodoWrite and pydantic-ai-todo:
//! - Tasks have status: pending → in_progress → completed (+ deleted tombstone)
//! - Dependencies via blockedBy[]
//! - Session-scoped storage (JSON file in ~/.luwu/sessions/<id>/tasks.json)
//! - Actions: create, update, list, get, delete, clear

use async_trait::async_trait;
use luwu_core::{Result, Tool, ToolContext, ToolOutput};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::debug;

// ── Task data model ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: u32,
    pub subject: String,
    #[serde(default)]
    pub description: String,
    pub status: TaskStatus,
    #[serde(default)]
    pub blocked_by: Vec<u32>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Deleted,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskStatus::Pending => write!(f, "pending"),
            TaskStatus::InProgress => write!(f, "in_progress"),
            TaskStatus::Completed => write!(f, "completed"),
            TaskStatus::Deleted => write!(f, "deleted"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct TaskStore {
    tasks: Vec<Task>,
    next_id: u32,
}

// ── Tool ──

pub struct TodoTool;

impl Default for TodoTool {
    fn default() -> Self {
        Self
    }
}

impl TodoTool {
    pub fn new() -> Self {
        Self
    }

    fn store_path(context: &ToolContext) -> PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".luwu")
            .join("sessions")
            .join(&context.session_id.0)
            .join("tasks.json")
    }

    fn load_store(path: &PathBuf) -> TaskStore {
        match std::fs::read_to_string(path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => TaskStore::default(),
        }
    }

    fn save_store(path: &PathBuf, store: &TaskStore) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(store) {
            let _ = std::fs::write(path, json);
        }
    }
}

#[async_trait]
impl Tool for TodoTool {
    fn name(&self) -> &str {
        "todo"
    }

    fn description(&self) -> &str {
        "Manages a structured task list for tracking multi-step work. \
         Use this to plan and track progress on complex tasks — research, design, implementation. \
         \
         Actions: \
         - create: Add a new task. Requires 'subject'. Optional: description, blockedBy (task IDs). \
         - update: Change a task's status or fields. Requires 'id'. \
         - list: Show all tasks (optionally filtered by status). \
         - get: Show details of a specific task. Requires 'id'. \
         - delete: Remove a task (soft delete — tombstoned). Requires 'id'. \
         - clear: Reset all tasks. \
         \
         Task statuses: pending → in_progress → completed (+ deleted). \
         Tasks can depend on others via blockedBy — a task should not start until its blockers are completed."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "update", "list", "get", "delete", "clear"],
                    "description": "The action to perform."
                },
                "id": {
                    "type": "number",
                    "description": "Task ID (required for update, get, delete)."
                },
                "subject": {
                    "type": "string",
                    "description": "Task subject line (required for create)."
                },
                "description": {
                    "type": "string",
                    "description": "Long-form task description (optional for create/update)."
                },
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed", "deleted"],
                    "description": "New status (for update)."
                },
                "blockedBy": {
                    "type": "array",
                    "items": {"type": "number"},
                    "description": "Task IDs this task depends on (for create/update)."
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, input: serde_json::Value, context: ToolContext) -> Result<ToolOutput> {
        debug!("Tool executing: todo");
        let action = input.get("action").and_then(|v| v.as_str()).unwrap_or("");
        let path = Self::store_path(&context);
        let mut store = Self::load_store(&path);

        match action {
            "create" => {
                let subject = input.get("subject").and_then(|v| v.as_str()).unwrap_or("");
                if subject.is_empty() {
                    return Ok(ToolOutput::error("'subject' is required for create."));
                }
                let description = input
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let blocked_by: Vec<u32> = input
                    .get("blockedBy")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_u64().map(|n| n as u32))
                            .collect()
                    })
                    .unwrap_or_default();

                store.next_id += 1;
                let id = store.next_id;
                let task = Task {
                    id,
                    subject: subject.to_string(),
                    description: description.to_string(),
                    status: TaskStatus::Pending,
                    blocked_by,
                    owner: None,
                    metadata: serde_json::Value::Null,
                };
                store.tasks.push(task);
                Self::save_store(&path, &store);
                Ok(ToolOutput::text(format!("Created task #{id}: {subject}")))
            }

            "update" => {
                let id = input.get("id").and_then(|v| v.as_u64()).map(|n| n as u32);
                let Some(id) = id else {
                    return Ok(ToolOutput::error("'id' is required for update."));
                };
                let task = store.tasks.iter_mut().find(|t| t.id == id);
                let Some(task) = task else {
                    return Ok(ToolOutput::error(format!("Task #{id} not found.")));
                };

                let mut changes = Vec::new();
                if let Some(status) = input.get("status").and_then(|v| v.as_str()) {
                    task.status = match status {
                        "pending" => TaskStatus::Pending,
                        "in_progress" => TaskStatus::InProgress,
                        "completed" => TaskStatus::Completed,
                        "deleted" => TaskStatus::Deleted,
                        _ => {
                            return Ok(ToolOutput::error(format!(
                                "Unknown status: {status}"
                            )))
                        }
                    };
                    changes.push(format!("status → {status}"));
                }
                if let Some(subject) = input.get("subject").and_then(|v| v.as_str()) {
                    task.subject = subject.to_string();
                    changes.push("subject".to_string());
                }
                if let Some(desc) = input.get("description").and_then(|v| v.as_str()) {
                    task.description = desc.to_string();
                    changes.push("description".to_string());
                }
                if let Some(blocked) = input.get("blockedBy").and_then(|v| v.as_array()) {
                    task.blocked_by = blocked
                        .iter()
                        .filter_map(|v| v.as_u64().map(|n| n as u32))
                        .collect();
                    changes.push("blockedBy".to_string());
                }
                if let Some(owner) = input.get("owner").and_then(|v| v.as_str()) {
                    task.owner = Some(owner.to_string());
                    changes.push("owner".to_string());
                }

                // Extract display values before releasing the mutable borrow.
                let (task_id, task_subject) = (task.id, task.subject.clone());

                Self::save_store(&path, &store);
                if changes.is_empty() {
                    Ok(ToolOutput::text(format!(
                        "Task #{task_id}: {task_subject} (no changes)"
                    )))
                } else {
                    Ok(ToolOutput::text(format!(
                        "Updated task #{task_id}: {task_subject} ({})",
                        changes.join(", ")
                    )))
                }
            }

            "list" => {
                let filter = input.get("status").and_then(|v| v.as_str());
                let active: Vec<&Task> = store
                    .tasks
                    .iter()
                    .filter(|t| t.status != TaskStatus::Deleted)
                    .filter(|t| {
                        filter.map_or(true, |f| {
                            t.status
                                == match f {
                                    "pending" => TaskStatus::Pending,
                                    "in_progress" => TaskStatus::InProgress,
                                    "completed" => TaskStatus::Completed,
                                    _ => return false,
                                }
                        })
                    })
                    .collect();

                if active.is_empty() {
                    let note = filter
                        .map(|f| format!(" with status '{f}'"))
                        .unwrap_or_default();
                    return Ok(ToolOutput::text(format!("No tasks{note}.")));
                }

                let mut lines = Vec::new();
                for t in &active {
                    let marker = match t.status {
                        TaskStatus::Pending => "○",
                        TaskStatus::InProgress => "◑",
                        TaskStatus::Completed => "●",
                        TaskStatus::Deleted => "✗",
                    };
                    let blockers = if t.blocked_by.is_empty() {
                        String::new()
                    } else {
                        format!(" (blocked by {:?})", t.blocked_by)
                    };
                    lines.push(format!(
                        "  {marker} #{:<3} {}{}",
                        t.id, t.subject, blockers
                    ));
                }
                lines.push(format!(
                    "\n{} task{} total.",
                    active.len(),
                    if active.len() > 1 { "s" } else { "" }
                ));
                Ok(ToolOutput::text(lines.join("\n")))
            }

            "get" => {
                let id = input.get("id").and_then(|v| v.as_u64()).map(|n| n as u32);
                let Some(id) = id else {
                    return Ok(ToolOutput::error("'id' is required for get."));
                };
                let task = store.tasks.iter().find(|t| t.id == id);
                let Some(task) = task else {
                    return Ok(ToolOutput::error(format!("Task #{id} not found.")));
                };
                let blocked = if task.blocked_by.is_empty() {
                    "none".to_string()
                } else {
                    format!("{:?}", task.blocked_by)
                };
                Ok(ToolOutput::text(format!(
                    "Task #{}\n  Subject: {}\n  Status: {}\n  Blocked by: {}\n  Description: {}",
                    task.id, task.subject, task.status, blocked, task.description
                )))
            }

            "delete" => {
                let id = input.get("id").and_then(|v| v.as_u64()).map(|n| n as u32);
                let Some(id) = id else {
                    return Ok(ToolOutput::error("'id' is required for delete."));
                };
                let task = store.tasks.iter_mut().find(|t| t.id == id);
                let Some(task) = task else {
                    return Ok(ToolOutput::error(format!("Task #{id} not found.")));
                };
                let subject = task.subject.clone();
                task.status = TaskStatus::Deleted;
                Self::save_store(&path, &store);
                Ok(ToolOutput::text(format!("Deleted task #{id}: {subject}")))
            }

            "clear" => {
                store.tasks.clear();
                store.next_id = 0;
                Self::save_store(&path, &store);
                Ok(ToolOutput::text("All tasks cleared."))
            }

            _ => Ok(ToolOutput::error(format!(
                "Unknown action: '{action}'. Valid: create, update, list, get, delete, clear."
            ))),
        }
    }
}
