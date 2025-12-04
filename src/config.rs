use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::error::{DiaryxError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Base directory for diary entries
    pub base_dir: PathBuf,

    /// Preferred editor (falls back to $EDITOR if not set)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,

    /// Default template to use when creating entries
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_template: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        let default_base = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("diaryx");

        Self {
            base_dir: default_base,
            editor: None,
            default_template: None,
        }
    }
}

impl Config {
    /// Get the config file path (~/.config/diaryx/config.toml)
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("diaryx").join("config.toml"))
    }

    /// Load config from file, or return default if file doesn't exist
    pub fn load() -> Result<Self> {
        if let Some(path) = Self::config_path() {
            if path.exists() {
                let contents = fs::read_to_string(&path)?;
                let config: Config = toml::from_str(&contents)?;
                return Ok(config);
            }
        }

        // Return default config if file doesn't exist
        Ok(Config::default())
    }

    /// Save config to file
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path().ok_or(DiaryxError::NoConfigDir)?;

        // Create config directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let contents = toml::to_string_pretty(self)?;
        fs::write(&path, contents)?;

        Ok(())
    }

    /// Initialize config with user-provided values
    pub fn init(base_dir: PathBuf) -> Result<Self> {
        let config = Config {
            base_dir,
            editor: None,
            default_template: None,
        };

        config.save()?;
        Ok(config)
    }
}
