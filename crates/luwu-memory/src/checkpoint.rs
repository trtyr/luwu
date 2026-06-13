//! Checkpoint — structured state snapshot for cycle management.
//!
//! A checkpoint captures the full working state of an agent session
//! in 11 fixed fields. Written by the independent Writer subagent,
//! read during rebuild to restore context.

use serde::{Deserialize, Serialize};

/// Structured state snapshot. Written by Writer, read during rebuild.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Current intent: what the agent is doing right now.
    pub current_intent: String,

    /// Next action: what should happen immediately after rebuild.
    pub next_action: String,

    /// Working constraints: user-requested rules and limits.
    pub constraints: String,

    /// Task tree: goal → subtasks → progress.
    pub task_tree: String,

    /// Current work: files/functions/modules being processed.
    pub current_work: String,

    /// Involved files: read/modified/pending file list.
    pub involved_files: String,

    /// Cross-task discoveries: architecture, API quirks, pitfalls found.
    pub discoveries: String,

    /// Errors encountered and how they were fixed.
    pub errors_and_fixes: String,

    /// Runtime state: branch, env vars, running processes.
    pub runtime_state: String,

    /// Design decisions: why A was chosen over B.
    pub design_decisions: String,

    /// Miscellaneous notes.
    pub notes: String,
}

impl Default for Checkpoint {
    fn default() -> Self {
        Self {
            current_intent: "未知".into(),
            next_action: "未知".into(),
            constraints: "未知".into(),
            task_tree: "未知".into(),
            current_work: "未知".into(),
            involved_files: "未知".into(),
            discoveries: "未知".into(),
            errors_and_fixes: "未知".into(),
            runtime_state: "未知".into(),
            design_decisions: "未知".into(),
            notes: "未知".into(),
        }
    }
}

impl Checkpoint {
    /// Render checkpoint as Markdown with section headers.
    /// This is the format Writer outputs and Rebuild consumes.
    pub fn to_markdown(&self) -> String {
        format!(
            "## 当前意图\n{}\n\n\
             ## 下一步动作\n{}\n\n\
             ## 工作约束\n{}\n\n\
             ## 任务树\n{}\n\n\
             ## 当前工作\n{}\n\n\
             ## 涉及文件\n{}\n\n\
             ## 跨任务发现\n{}\n\n\
             ## 错误与修复\n{}\n\n\
             ## 运行时状态\n{}\n\n\
             ## 设计决策\n{}\n\n\
             ## 杂项笔记\n{}",
            self.current_intent,
            self.next_action,
            self.constraints,
            self.task_tree,
            self.current_work,
            self.involved_files,
            self.discoveries,
            self.errors_and_fixes,
            self.runtime_state,
            self.design_decisions,
            self.notes,
        )
    }

    /// Parse checkpoint from Markdown produced by Writer LLM.
    /// Tolerant parsing — missing sections get "未知".
    pub fn from_markdown(md: &str) -> Self {
        let mut cp = Checkpoint::default();
        let sections = [
            ("## 当前意图", &mut cp.current_intent),
            ("## 下一步动作", &mut cp.next_action),
            ("## 工作约束", &mut cp.constraints),
            ("## 任务树", &mut cp.task_tree),
            ("## 当前工作", &mut cp.current_work),
            ("## 涉及文件", &mut cp.involved_files),
            ("## 跨任务发现", &mut cp.discoveries),
            ("## 错误与修复", &mut cp.errors_and_fixes),
            ("## 运行时状态", &mut cp.runtime_state),
            ("## 设计决策", &mut cp.design_decisions),
            ("## 杂项笔记", &mut cp.notes),
        ];

        let lines: Vec<&str> = md.lines().collect();
        for (header, field) in sections {
            if let Some(start) = lines.iter().position(|l| l.trim() == header) {
                // Collect lines until next ## header or end.
                let mut content_lines = Vec::new();
                for line in lines.iter().skip(start + 1) {
                    let trimmed = line.trim();
                    if trimmed.starts_with("## ") {
                        break;
                    }
                    content_lines.push(*line);
                }
                let content = content_lines.join("\n").trim().to_string();
                if !content.is_empty() {
                    *field = content;
                }
            }
        }

        cp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_markdown() {
        let cp = Checkpoint {
            current_intent: "修复 login 函数的 timeout bug".into(),
            next_action: "在 engine.rs:142 添加 timeout 参数".into(),
            constraints: "用户要求不超过 3 个文件改动".into(),
            task_tree: "修复 timeout bug\n  ├── [完成] 定位问题\n  └── [进行中] 添加参数".into(),
            current_work: "crates/luwu-core/src/engine.rs".into(),
            involved_files: "engine.rs (修改中), lib.rs (待确认)".into(),
            discoveries: "MiniMax API 在超长请求时会返回 502".into(),
            errors_and_fixes: "Box<dyn LlmProvider> 不能 clone → 改成 Arc".into(),
            runtime_state: "分支: feat/memory, 未提交".into(),
            design_decisions: "选 OnceLock 而非 lazy_static — 无额外依赖".into(),
            notes: "用户提到下周要加 MCP 支持".into(),
        };

        let md = cp.to_markdown();
        let parsed = Checkpoint::from_markdown(&md);

        assert_eq!(parsed.current_intent, "修复 login 函数的 timeout bug");
        assert_eq!(parsed.next_action, "在 engine.rs:142 添加 timeout 参数");
        assert_eq!(
            parsed.design_decisions,
            "选 OnceLock 而非 lazy_static — 无额外依赖"
        );
        assert_eq!(parsed.notes, "用户提到下周要加 MCP 支持");
    }

    #[test]
    fn parse_partial_markdown() {
        let md = "## 当前意图\n正在写测试\n\n## 下一步动作\n继续写\n";
        let cp = Checkpoint::from_markdown(md);
        assert_eq!(cp.current_intent, "正在写测试");
        assert_eq!(cp.next_action, "继续写");
        assert_eq!(cp.constraints, "未知"); // Missing section → default
    }
}
