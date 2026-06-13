//! Skill system — reusable workflow packages following the Agent Skills standard.
//!
//! Skills are self-contained capability packages loaded progressively:
//! - Level 1 (always in context): name + description metadata
//! - Level 2 (on activation): full SKILL.md instructions
//! - Level 3 (on demand): scripts/, references/, assets/
//!
//! See <https://agentskills.io/specification> for the open standard.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::error::{LuwuError, Result};

// ─── Skill ──────────────────────────────────────────────────────────

/// A loaded skill following the [Agent Skills standard](https://agentskills.io/specification).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Skill name (1-64 chars, lowercase a-z, 0-9, hyphens only).
    pub name: String,
    /// What this skill does and when to use it (max 1024 chars).
    pub description: String,
    /// Full SKILL.md body (everything after frontmatter).
    pub instructions: String,
    /// Absolute path to the skill directory on disk.
    pub base_path: PathBuf,
}

// ─── Frontmatter ────────────────────────────────────────────────────

/// YAML frontmatter parsed from a SKILL.md file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub compatibility: Option<String>,
    #[serde(default)]
    pub metadata: Option<HashMap<String, String>>,
    #[serde(rename = "allowed-tools", default)]
    pub allowed_tools: Option<String>,
    #[serde(rename = "disable-model-invocation", default)]
    pub disable_model_invocation: Option<bool>,
}

// ─── SkillRegistry ──────────────────────────────────────────────────

/// Scans, loads, and manages all discoverable skills.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRegistry {
    skills: Vec<Skill>,
    /// Map from name to index for O(1) lookup.
    #[serde(skip)]
    index: HashMap<String, usize>,
}

impl SkillRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            skills: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// Scan global + project skill directories and return a populated registry.
    ///
    /// Scan order (later entries override earlier on name collision):
    /// 1. `<luwu_home>/skills/` (global)
    /// 2. `<project_dir>/.luwu/skills/` (project-local, higher priority)
    pub fn discover(luwu_home: &Path, project_dir: &Path) -> Result<Self> {
        let mut registry = Self::new();

        let global_dir = luwu_home.join("skills");
        if global_dir.is_dir() {
            registry.scan_directory(&global_dir)?;
        }

        let project_dir_skills = project_dir.join(".luwu").join("skills");
        if project_dir_skills.is_dir() {
            registry.scan_directory(&project_dir_skills)?;
        }

        info!(
            "Skill discovery complete: {} skills loaded",
            registry.skills.len()
        );
        Ok(registry)
    }

    /// Scan a single directory for skill folders.
    fn scan_directory(&mut self, dir: &Path) -> Result<()> {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(err) => {
                warn!("Cannot read skill directory {}: {err}", dir.display());
                return Ok(());
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let skill_file = path.join("SKILL.md");
            if !skill_file.is_file() {
                // Also check for bare .md files in the root (Agent Skills standard alt format).
                continue;
            }

            match Self::load_skill(&path) {
                Ok(skill) => {
                    debug!("Loaded skill '{}' from {}", skill.name, path.display());
                    self.insert_or_replace(skill);
                }
                Err(err) => {
                    warn!("Failed to load skill from {}: {err}", path.display());
                }
            }
        }

        Ok(())
    }

    /// Load a single skill from its directory.
    fn load_skill(dir: &Path) -> Result<Skill> {
        let skill_file = dir.join("SKILL.md");
        let raw = std::fs::read_to_string(&skill_file).map_err(|e| {
            LuwuError::Io(std::io::Error::other(format!(
                "Cannot read {}: {e}",
                skill_file.display()
            )))
        })?;

        let (frontmatter, instructions) = Self::parse_skill_md(&raw)?;

        // Validate name.
        Self::validate_name(&frontmatter.name, dir)?;

        // Validate description.
        if frontmatter.description.is_empty() {
            return Err(LuwuError::Config(format!(
                "Skill '{}' has empty description — skills without description are not loaded",
                frontmatter.name
            )));
        }
        if frontmatter.description.len() > 1024 {
            warn!(
                "Skill '{}' description exceeds 1024 chars ({}), will be truncated by agents",
                frontmatter.name,
                frontmatter.description.len()
            );
        }

        Ok(Skill {
            name: frontmatter.name,
            description: frontmatter.description,
            instructions,
            base_path: dir
                .to_path_buf()
                .canonicalize()
                .unwrap_or_else(|_| dir.to_path_buf()),
        })
    }

    /// Parse a SKILL.md into (frontmatter, body).
    fn parse_skill_md(content: &str) -> Result<(SkillFrontmatter, String)> {
        let content = content.trim_start();

        // Must start with --- for YAML frontmatter.
        if !content.starts_with("---") {
            return Err(LuwuError::Config(
                "SKILL.md must start with YAML frontmatter (---)".to_string(),
            ));
        }

        // Find closing ---.
        let rest = &content[3..];
        let close_pos = rest.find("\n---").ok_or_else(|| {
            LuwuError::Config("SKILL.md frontmatter missing closing ---".to_string())
        })?;

        let yaml_str = &rest[..close_pos];
        let body = rest[close_pos + 4..].trim().to_string();

        let frontmatter: SkillFrontmatter = serde_yaml::from_str(yaml_str)
            .map_err(|e| LuwuError::Config(format!("Invalid SKILL.md frontmatter: {e}")))?;

        Ok((frontmatter, body))
    }

    /// Validate skill name per Agent Skills spec.
    fn validate_name(name: &str, dir: &Path) -> Result<()> {
        if name.is_empty() || name.len() > 64 {
            return Err(LuwuError::Config(format!(
                "Skill name must be 1-64 chars, got '{}' ({} chars)",
                name,
                name.len()
            )));
        }
        if name.starts_with('-') || name.ends_with('-') {
            return Err(LuwuError::Config(format!(
                "Skill name '{name}' cannot start or end with hyphen"
            )));
        }
        if name.contains("--") {
            return Err(LuwuError::Config(format!(
                "Skill name '{name}' cannot contain consecutive hyphens"
            )));
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(LuwuError::Config(format!(
                "Skill name '{name}' must only contain lowercase a-z, 0-9, and hyphens"
            )));
        }

        // Warn (not error) if name doesn't match directory name.
        if let Some(dir_name) = dir.file_name().map(|n| n.to_string_lossy())
            && dir_name != name
        {
            warn!(
                "Skill name '{name}' doesn't match directory name '{dir_name}' (warning, still loaded)"
            );
        }

        Ok(())
    }

    /// Insert a skill, replacing any existing one with the same name.
    fn insert_or_replace(&mut self, skill: Skill) {
        if let Some(&idx) = self.index.get(&skill.name) {
            self.skills[idx] = skill;
        } else {
            let idx = self.skills.len();
            self.index.insert(skill.name.clone(), idx);
            self.skills.push(skill);
        }
    }

    /// Rebuild the name → index lookup map after deserialization.
    pub fn rebuild_index(&mut self) {
        self.index.clear();
        for (i, skill) in self.skills.iter().enumerate() {
            self.index.insert(skill.name.clone(), i);
        }
    }

    // ─── Accessors ──────────────────────────────────────────────────

    /// Look up a skill by name.
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.index.get(name).map(|&idx| &self.skills[idx])
    }

    /// List all loaded skills.
    pub fn list(&self) -> &[Skill] {
        &self.skills
    }

    /// Number of loaded skills.
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Whether any skills are loaded.
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Generate the Level-1 metadata prompt for system prompt injection.
    ///
    /// Returns a string like:
    /// ```text
    /// ## Available Skills
    /// When a task matches a skill's description, load and follow its instructions:
    ///
    /// - deploy: Deploy project to staging or production...
    /// - code-review: Perform code review with quantitative metrics...
    /// ```
    pub fn skill_metadata_prompt(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }

        let mut out = String::from(
            "## Available Skills\n\n\
             When a task matches a skill's description, follow its instructions. \
             To load a skill's full instructions, use `read` on its SKILL.md file.\n",
        );

        for skill in &self.skills {
            out.push_str(&format!("- {}: {}\n", skill.name, skill.description));
        }

        out
    }

    /// Get the path to a skill's SKILL.md for on-demand loading.
    pub fn skill_file_path(&self, name: &str) -> Option<PathBuf> {
        self.get(name).map(|s| s.base_path.join("SKILL.md"))
    }

    /// List files in a skill's directory (for the detail API endpoint).
    pub fn skill_files(&self, name: &str) -> Vec<String> {
        let Some(skill) = self.get(name) else {
            return Vec::new();
        };

        let mut files = Vec::new();
        self.walk_dir(&skill.base_path, &skill.base_path, &mut files);
        files
    }

    fn walk_dir(&self, base: &Path, dir: &Path, files: &mut Vec<String>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Ok(relative) = path.strip_prefix(base) {
                    files.push(relative.to_string_lossy().to_string());
                    if path.is_dir() {
                        self.walk_dir(base, &path, files);
                    }
                }
            }
        }
    }

    /// Check if the assistant's text contains a reference to a skill.
    /// Returns the skill name if found.
    ///
    /// Detects patterns like:
    /// - `[skill:deploy]`
    /// - `using skill "deploy"`
    /// - `use the deploy skill`
    pub fn detect_skill_reference(&self, text: &str) -> Option<String> {
        let text_lower = text.to_lowercase();

        // Pattern 1: [skill:name]
        for skill in &self.skills {
            if text_lower.contains(&format!("[skill:{}]", skill.name)) {
                return Some(skill.name.clone());
            }
        }

        // Pattern 2: "use the <name> skill" / "using skill <name>"
        for skill in &self.skills {
            if text_lower.contains(&format!("{} skill", skill.name))
                || text_lower.contains(&format!("skill {}", skill.name))
                || text_lower.contains(&format!("skill \"{}\"", skill.name))
            {
                return Some(skill.name.clone());
            }
        }

        None
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}
