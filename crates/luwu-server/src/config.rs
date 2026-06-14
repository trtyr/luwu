//! Configuration — shared between CLI and server.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Config structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub default: DefaultConfig,
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
    #[serde(default)]
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DefaultConfig {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level: trace, debug, info, warn, error
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Output format: "pretty" (human-readable) or "json"
    #[serde(default = "default_log_format")]
    pub format: String,

    /// Optional file path for logs (JSON with daily rotation). None = stderr only.
    #[serde(default)]
    pub file: Option<String>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: default_log_format(),
            file: None,
        }
    }
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "pretty".to_string()
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ResolvedConfig {
    pub provider_name: String,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u64>,
}

// ---------------------------------------------------------------------------
// Methods
// ---------------------------------------------------------------------------

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let path = config_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let content =
            fs::read_to_string(path).map_err(|e| ConfigError::Io(path.to_path_buf(), e))?;
        toml::from_str(&content).map_err(|e| ConfigError::Parse(path.to_path_buf(), e))
    }

    pub fn resolve(&self, provider_name: Option<&str>) -> Result<ResolvedConfig, ConfigError> {
        let name = provider_name
            .or(self.default.provider.as_deref())
            .ok_or(ConfigError::NoDefaultProvider)?;

        let provider = self
            .providers
            .get(name)
            .ok_or_else(|| ConfigError::ProviderNotFound(name.to_string()))?;

        let model = provider
            .model
            .as_deref()
            .or(self.default.model.as_deref())
            .unwrap_or("gpt-4o-mini")
            .to_string();

        let base_url = provider
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

        // ── Field validation (Phase 4.4) ──
        if provider.api_key.trim().is_empty() {
            return Err(ConfigError::InvalidConfig(format!(
                "Provider '{name}' has an empty api_key"
            )));
        }
        if base_url.is_empty()
            || !(base_url.starts_with("http://") || base_url.starts_with("https://"))
        {
            return Err(ConfigError::InvalidConfig(format!(
                "Provider '{name}' base_url must be a valid http(s) URL, got: {base_url}"
            )));
        }
        if let Some(t) = provider.temperature
            && !(0.0..=2.0).contains(&t)
        {
            return Err(ConfigError::InvalidConfig(format!(
                "Provider '{name}' temperature must be 0.0–2.0, got: {t}"
            )));
        }
        if let Some(mt) = provider.max_tokens
            && mt == 0
        {
            return Err(ConfigError::InvalidConfig(format!(
                "Provider '{name}' max_tokens must be > 0"
            )));
        }

        Ok(ResolvedConfig {
            provider_name: name.to_string(),
            api_key: provider.api_key.clone(),
            base_url,
            model,
            temperature: provider.temperature,
            max_tokens: provider.max_tokens,
        })
    }
}

pub fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".luwu")
        .join("config.toml")
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error at {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("Failed to parse config at {0}: {1}")]
    Parse(PathBuf, toml::de::Error),
    #[error("No default provider configured")]
    NoDefaultProvider,
    #[error("Provider '{0}' not found")]
    ProviderNotFound(String),
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}
