//! Auto-Consolidation — merge memory entries when files exceed capacity.
//!
//! When a memory file (global.md, project.md, corrections.md) grows too
//! large, a Writer LLM merges similar entries into a compact version.
//! This prevents unbounded growth while preserving key information.

use std::path::{Path, PathBuf};

/// Which memory file type to check/consolidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryFileType {
    Global,
    Project,
    Corrections,
}

impl MemoryFileType {
    /// Human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Project => "project",
            Self::Corrections => "corrections",
        }
    }
}

/// Configuration for consolidation thresholds.
#[derive(Debug, Clone)]
pub struct ConsolidationConfig {
    /// Max characters before global.md triggers consolidation.
    pub global_threshold: usize,
    /// Max characters before project.md triggers consolidation.
    pub project_threshold: usize,
    /// Max characters before corrections.md triggers consolidation.
    pub corrections_threshold: usize,
}

impl Default for ConsolidationConfig {
    fn default() -> Self {
        Self {
            global_threshold: 8000,
            project_threshold: 8000,
            corrections_threshold: 8000,
        }
    }
}

/// Indicates a file needs consolidation.
#[derive(Debug, Clone)]
pub struct ConsolidationNeeded {
    /// Which file type.
    pub file_type: MemoryFileType,
    /// Current file size in characters.
    pub current_size: usize,
    /// The threshold that was exceeded.
    pub threshold: usize,
    /// Path to the file.
    pub path: PathBuf,
}

/// Result of a consolidation operation.
#[derive(Debug, Clone)]
pub struct ConsolidationResult {
    /// Which file was consolidated.
    pub file_type: MemoryFileType,
    /// Size before consolidation.
    pub original_size: usize,
    /// Size after consolidation.
    pub consolidated_size: usize,
    /// Number of entries before.
    pub entry_count_before: usize,
    /// Number of entries after.
    pub entry_count_after: usize,
}

/// Checks memory files for consolidation eligibility.
#[derive(Default)]
pub struct ConsolidationChecker {
    config: ConsolidationConfig,
}

impl ConsolidationChecker {
    /// Create with custom config.
    pub fn new(config: ConsolidationConfig) -> Self {
        Self { config }
    }

    /// Check a single file against its threshold.
    pub fn check(&self, file_type: MemoryFileType, path: &Path) -> Option<ConsolidationNeeded> {
        if !path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(path).ok()?;
        let size = content.chars().count();
        let threshold = match file_type {
            MemoryFileType::Global => self.config.global_threshold,
            MemoryFileType::Project => self.config.project_threshold,
            MemoryFileType::Corrections => self.config.corrections_threshold,
        };

        if size > threshold {
            Some(ConsolidationNeeded {
                file_type,
                current_size: size,
                threshold,
                path: path.to_path_buf(),
            })
        } else {
            None
        }
    }

    /// Check all three memory files. Returns those needing consolidation.
    pub fn check_all(
        &self,
        global_path: &Path,
        project_path: &Path,
        corrections_path: &Path,
    ) -> Vec<ConsolidationNeeded> {
        let mut results = Vec::new();
        if let Some(c) = self.check(MemoryFileType::Global, global_path) {
            results.push(c);
        }
        if let Some(c) = self.check(MemoryFileType::Project, project_path) {
            results.push(c);
        }
        if let Some(c) = self.check(MemoryFileType::Corrections, corrections_path) {
            results.push(c);
        }
        results
    }
}

/// System prompt for the consolidation Writer LLM.
///
/// Instructs the LLM to merge similar memory entries into a compact version,
/// preserving key facts while removing redundancy.
pub fn consolidation_prompt() -> &'static str {
    "你是一个记忆合并助手。下面是 Agent 的记忆文件内容，包含多条用 § 符号分隔的记忆条目。\n\
    请将这些条目合并为更精简的版本：\n\
    1. 合并重复或高度相似的条目\n\
    2. 保留所有关键事实信息（用户偏好、项目决策、错误教训）\n\
    3. 去除过时或已被新条目取代的信息\n\
    4. 保持 § 分隔符格式\n\
    5. 保持 HTML 注释时间戳格式（取合并条目中最新的时间戳）\n\
    6. 目标：将内容压缩到原来的 50-60%\n\n\
    直接输出合并后的内容，不要添加额外说明。"
}

/// Apply consolidation result: write the consolidated content back to file.
pub fn apply_consolidation(
    needed: &ConsolidationNeeded,
    consolidated_content: &str,
) -> ConsolidationResult {
    let original = std::fs::read_to_string(&needed.path).unwrap_or_default();
    let original_size = original.chars().count();

    let count = |text: &str| {
        text.split('§')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .count()
    };

    let entry_count_before = count(&original);

    std::fs::write(&needed.path, consolidated_content).ok();

    let consolidated_size = consolidated_content.chars().count();
    let entry_count_after = count(consolidated_content);

    ConsolidationResult {
        file_type: needed.file_type,
        original_size,
        consolidated_size,
        entry_count_before,
        entry_count_after,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_under_threshold() {
        let dir = std::env::temp_dir().join(format!(
            "luwu_test_consolidation_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("global.md");
        std::fs::write(&path, "short content").unwrap();

        let checker = ConsolidationChecker::default();
        assert!(checker.check(MemoryFileType::Global, &path).is_none());
    }

    #[test]
    fn test_check_over_threshold() {
        let dir = std::env::temp_dir().join(format!(
            "luwu_test_consolidation2_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("global.md");
        // Write 10000 chars — over the 8000 default threshold.
        let big_content = "x".repeat(10000);
        std::fs::write(&path, &big_content).unwrap();

        let checker = ConsolidationChecker::default();
        let result = checker.check(MemoryFileType::Global, &path).unwrap();
        assert_eq!(result.file_type, MemoryFileType::Global);
        assert!(result.current_size > 8000);
    }

    #[test]
    fn test_consolidation_prompt_not_empty() {
        let prompt = consolidation_prompt();
        assert!(prompt.contains("§"));
        assert!(prompt.contains("合并"));
    }
}
