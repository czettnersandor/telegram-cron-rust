use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub chat_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JobConfig {
    pub schedule: String,
    pub script: String,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// CLI arguments forwarded to the script verbatim.
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment variables injected into the script process.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

fn default_timeout() -> u64 {
    60
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    pub telegram: TelegramConfig,
    /// Base directory for scripts. Relative paths are resolved from the
    /// config file's parent directory. Defaults to that same parent.
    #[serde(default)]
    pub scripts_dir: Option<String>,
    pub jobs: HashMap<String, JobConfig>,
}

/// Load configuration from a YAML file.
pub fn load_config(path: &Path) -> Result<AppConfig, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
    serde_yaml::from_str(&content)
        .map_err(|e| format!("Cannot parse {}: {}", path.display(), e))
}

/// Resolve the scripts base directory from config.
pub fn get_scripts_base(config: &AppConfig, config_path: &Path) -> PathBuf {
    if let Some(dir) = &config.scripts_dir {
        let p = PathBuf::from(dir);
        if p.is_absolute() {
            return p;
        }
        if let Some(parent) = config_path.parent() {
            return parent.join(&p);
        }
        return p;
    }
    config_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}
