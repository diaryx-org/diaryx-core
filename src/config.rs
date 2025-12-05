use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::error::{DiaryxError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Default workspace directory
    /// This is the main directory for your workspace/journal
    #[serde(alias = "base_dir")]
    pub default_workspace: PathBuf,

    /// Subfolder within the workspace for daily entries (optional)
    /// If not set, daily entries are created in the workspace root
    /// Example: "Daily" or "Journal/Daily"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daily_entry_folder: Option<String>,

    /// Preferred editor (falls back to $EDITOR if not set)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,

    /// Default template to use when creating entries
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_template: Option<String>,
}

impl Config {
    /// Get the directory where daily entries should be created
    /// Returns daily_entry_folder joined with default_workspace, or just default_workspace
    pub fn daily_entry_dir(&self) -> PathBuf {
        match &self.daily_entry_folder {
            Some(folder) => self.default_workspace.join(folder),
            None => self.default_workspace.clone(),
        }
    }

    /// Alias for backwards compatibility
    pub fn base_dir(&self) -> &PathBuf {
        &self.default_workspace
    }
}

impl Default for Config {
    fn default() -> Self {
        let default_base = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("diaryx");

        Self {
            default_workspace: default_base,
            daily_entry_folder: None,
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
    pub fn init(default_workspace: PathBuf) -> Result<Self> {
        Self::init_with_options(default_workspace, None)
    }

    /// Initialize config with user-provided values including daily folder
    pub fn init_with_options(
        default_workspace: PathBuf,
        daily_entry_folder: Option<String>,
    ) -> Result<Self> {
        let config = Config {
            default_workspace,
            daily_entry_folder,
            editor: None,
            default_template: None,
        };

        config.save()?;
        Ok(config)
    }
}
