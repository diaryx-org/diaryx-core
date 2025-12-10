use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::{DiaryxError, Result};
use crate::fs::FileSystem;

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

    /// Default template for daily entries (today, yesterday commands)
    /// Falls back to "daily" built-in template if not set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daily_template: Option<String>,
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

    /// Create a new config with the given workspace directory
    pub fn new(default_workspace: PathBuf) -> Self {
        Self {
            default_workspace,
            daily_entry_folder: None,
            editor: None,
            default_template: None,
            daily_template: None,
        }
    }

    /// Create a config with all options specified
    pub fn with_options(
        default_workspace: PathBuf,
        daily_entry_folder: Option<String>,
        editor: Option<String>,
        default_template: Option<String>,
        daily_template: Option<String>,
    ) -> Self {
        Self {
            default_workspace,
            daily_entry_folder,
            editor,
            default_template,
            daily_template,
        }
    }

    // ========================================================================
    // FileSystem-based methods (work on all platforms including WASM)
    // ========================================================================

    /// Load config from a specific path using a FileSystem
    pub fn load_from<FS: FileSystem>(fs: &FS, path: &std::path::Path) -> Result<Self> {
        let contents = fs.read_to_string(path).map_err(|e| DiaryxError::FileRead {
            path: path.to_path_buf(),
            source: e,
        })?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }

    /// Save config to a specific path using a FileSystem
    pub fn save_to<FS: FileSystem>(&self, fs: &FS, path: &std::path::Path) -> Result<()> {
        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs.create_dir_all(parent)?;
            }
        }

        let contents = toml::to_string_pretty(self)?;
        fs.write_file(path, &contents)?;
        Ok(())
    }

    /// Load config from a FileSystem, returning default if not found
    pub fn load_from_or_default<FS: FileSystem>(
        fs: &FS,
        path: &std::path::Path,
        default_workspace: PathBuf,
    ) -> Self {
        Self::load_from(fs, path).unwrap_or_else(|_| Self::new(default_workspace))
    }
}

// ============================================================================
// Native-only implementation (not available in WASM)
// ============================================================================

#[cfg(not(target_arch = "wasm32"))]
use std::fs;

#[cfg(not(target_arch = "wasm32"))]
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
            daily_template: None,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Config {
    /// Get the config file path (~/.config/diaryx/config.toml)
    /// Only available on native platforms
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("diaryx").join("config.toml"))
    }

    /// Load config from default location, or return default if file doesn't exist
    /// Only available on native platforms
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

    /// Save config to default location
    /// Only available on native platforms
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
    /// Only available on native platforms
    pub fn init(default_workspace: PathBuf) -> Result<Self> {
        Self::init_with_options(default_workspace, None)
    }

    /// Initialize config with user-provided values including daily folder
    /// Only available on native platforms
    pub fn init_with_options(
        default_workspace: PathBuf,
        daily_entry_folder: Option<String>,
    ) -> Result<Self> {
        let config = Config {
            default_workspace,
            daily_entry_folder,
            editor: None,
            default_template: None,
            daily_template: None,
        };

        config.save()?;
        Ok(config)
    }
}

// ============================================================================
// WASM-specific implementation
// ============================================================================

#[cfg(target_arch = "wasm32")]
impl Default for Config {
    fn default() -> Self {
        // In WASM, we use a simple default path
        // The actual workspace location will be virtual
        Self {
            default_workspace: PathBuf::from("/workspace"),
            daily_entry_folder: None,
            editor: None,
            default_template: None,
            daily_template: None,
        }
    }
}
