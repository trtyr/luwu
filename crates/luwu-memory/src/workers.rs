//! Observer / Reflector / Dropper — three-layer memory workers.
//!
//! Inspired by pi-blackhole's observational memory architecture.
//! - Observer: extracts timestamped facts from recent conversation (frequent).
//! - Reflector: synthesizes durable reflections from observations (less frequent).
//! - Dropper: prunes low-value observations when pool exceeds budget (on-demand).
//!
//! `Priority`, `Observation`, `Reflection` are re-exported from
//! `luwu_core::memory_backend` (the microkernel owns these domain types so the
//! `MemoryBackend` trait can be defined without `luwu-core` depending on
//! `luwu-memory`).

// ---- Worker Prompts ----

/// System prompt for the Observer worker.
///
/// The Observer reads recent conversation and extracts timestamped facts.
pub fn observer_prompt() -> &'static str {
    "你是一个观察者（Observer）。你的任务是分析对话历史，提取时间戳事实。\n\n\
    提取规则：\n\
    1. 只提取客观事实：事件、决策、用户偏好、错误发现、代码模式\n\
    2. 每条观察必须简洁（1-2 句话）\n\
    3. 标注优先级：high（关键决策/错误）、medium（有用信息）、low（次要细节）\n\
    4. 标注类别：event（事件）、decision（决策）、preference（偏好）、error（错误）、pattern（模式）\n\
    5. 不要提取对话寒暄、无关闲聊\n\
    6. 不要推断，只记录已发生的事实\n\n\
    输出格式（每条一行 JSON）：\n\
    {\"priority\": \"high\", \"category\": \"decision\", \"content\": \"用户决定使用 SQLite 而非文件存储\"}\n\
    {\"priority\": \"medium\", \"category\": \"error\", \"content\": \"MiniMax provider 在 thinking 模式下返回空 content\"}\n\n\
    直接输出 JSON 行，不要添加额外说明。如果没有值得提取的内容，输出空。"
}

/// System prompt for the Reflector worker.
///
/// The Reflector synthesizes durable reflections from accumulated observations.
pub fn reflector_prompt() -> &'static str {
    "你是一个反思者（Reflector）。你的任务是将多条观察（observations）合成为持久的反思（reflections）。\n\n\
    合成规则：\n\
    1. 识别重复或相关的观察，合并成更通用的洞察\n\
    2. 提取稳定的模式、约束和偏好——这些在未来会话中仍然成立\n\
    3. 去除已经过时或被后续观察推翻的信息\n\
    4. 每条反思应该是一个独立的、持久的事实陈述\n\
    5. 保持简洁——一句话能说清的不要两句\n\n\
    输出格式（每条一行 JSON）：\n\
    {\"content\": \"用户偏好使用确定性算法而非 LLM 调用来做 compaction\", \"source_ids\": [\"abc123\", \"def456\"]}\n\n\
    直接输出 JSON 行。如果没有值得合成的观察，输出空。"
}

/// System prompt for the Dropper worker.
///
/// The Dropper prunes low-value observations to keep the pool within budget.
pub fn dropper_prompt() -> &'static str {
    "你是一个修剪者（Dropper）。你的任务是从观察池中识别低价值的观察，以便修剪。\n\n\
    修剪规则：\n\
    1. 优先修剪 low priority 的观察\n\
    2. 保留所有 high priority 的观察\n\
    3. 已经被 reflections 涵盖的 observations 可以修剪\n\
    4. 过时的信息（被后续观察推翻）可以修剪\n\
    5. 保持观察池在目标大小以内\n\n\
    输出要修剪的观察 ID 列表（每行一个 ID）：\n\
    abc123def456\n\
    789abc012def\n\n\
    如果不需要修剪，输出空。"
}

// ---- Worker Config ----

/// Token thresholds for when each worker activates.
#[derive(Debug, Clone)]
pub struct WorkerThresholds {
    /// Min tokens accumulated before Observer runs.
    pub observe_after_tokens: usize,
    /// Min tokens accumulated before Reflector + Dropper run.
    pub reflect_after_tokens: usize,
    /// Max observation pool size before Dropper prunes.
    pub observations_pool_max: usize,
    /// Target observation pool size after pruning.
    pub observations_pool_target: usize,
}

impl Default for WorkerThresholds {
    fn default() -> Self {
        Self {
            observe_after_tokens: 15_000,
            reflect_after_tokens: 25_000,
            observations_pool_max: 20_000,
            observations_pool_target: 10_000,
        }
    }
}

/// Check if observer should run based on current token count.
pub fn should_observe(tokens_since_last: usize, thresholds: &WorkerThresholds) -> bool {
    tokens_since_last >= thresholds.observe_after_tokens
}

/// Check if reflector + dropper should run based on current token count.
pub fn should_reflect(tokens_since_last: usize, thresholds: &WorkerThresholds) -> bool {
    tokens_since_last >= thresholds.reflect_after_tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    // These types live in `luwu_core::memory_backend` (moved there so
    // the `MemoryBackend` trait can reference them without `luwu-core`
    // depending on `luwu-memory`). The production worker functions no
    // longer need them directly, but the test module does.
    use luwu_core::memory_backend::{Observation, Priority, Reflection};

    #[test]
    fn test_observation_creation() {
        let obs = Observation::new(Priority::High, "decision", "User chose SQLite over files");
        assert_eq!(obs.id.len(), 12);
        assert_eq!(obs.priority, Priority::High);
        assert_eq!(obs.category, "decision");
    }

    #[test]
    fn test_reflection_creation() {
        let refl = Reflection::new(
            "User prefers deterministic approaches",
            vec!["abc123".to_string(), "def456".to_string()],
        );
        assert_eq!(refl.id.len(), 12);
        assert_eq!(refl.source_ids.len(), 2);
    }

    #[test]
    fn test_worker_thresholds() {
        let t = WorkerThresholds::default();
        assert!(should_observe(16_000, &t));
        assert!(!should_observe(14_000, &t));
        assert!(should_reflect(26_000, &t));
        assert!(!should_reflect(24_000, &t));
    }

    #[test]
    fn test_prompts_not_empty() {
        assert!(observer_prompt().contains("Observer"));
        assert!(reflector_prompt().contains("Reflector"));
        assert!(dropper_prompt().contains("Dropper"));
    }
}
