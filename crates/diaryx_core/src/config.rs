//! Configuration types for Diaryx.
//!
//! This module provides the [`Config`] struct which stores user preferences
//! and workspace settings. Configuration is persisted as TOML (typically at
//! `~/.config/diaryx/config.toml` on Unix systems).
//!
//! # Key Configuration Fields
//!
//! - `default_workspace`: Primary workspace directory path
//! - `daily_entry_folder`: Optional subfolder for daily entries
//! - `editor`: Preferred editor command
//! - `link_format`: Format for `part_of`/`contents` links
//! - `sync_*`: Cloud synchronization settings
//!
//! # Async-first Design
//!
//! Use `Config::load_from()` with an `AsyncFileSystem` to load config.
//! For synchronous contexts, use the `_sync` variants or wrap with
//! `SyncToAsyncFs` and use `block_on()`.
//!
//! # Example
//!
//! ```ignore
//! use diaryx_core::config::Config;
//! use std::path::PathBuf;
//!
//! // Create a new config
//! let config = Config::new(PathBuf::from("/home/user/diary"));
//!
//! // Load from default location (native only)
//! let config = Config::load()?;
//!
//! // Access config values
//! let daily_dir = config.daily_entry_dir();
//! ```

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::{DiaryxError, Result};
use crate::fs::{AsyncFileSystem, FileSystem, SyncToAsyncFs};
use crate::link_parser::LinkFormat;

/// `Config` is a data structure that represents the parts of Diaryx that the user can configure.
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

    /// Format for `part_of` and `contents` links in frontmatter
    /// Defaults to MarkdownRoot for portable, clickable links
    #[serde(default, skip_serializing_if = "is_default_link_format")]
    pub link_format: LinkFormat,

    // ========================================================================
    // Sync configuration
    // ========================================================================
    /// Sync server URL (e.g., "https://sync.diaryx.org")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_server_url: Option<String>,

    /// Session token for authenticated sync
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_session_token: Option<String>,

    /// Email address used for sync authentication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_email: Option<String>,

    /// Workspace ID for sync (identifies the remote workspace)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_workspace_id: Option<String>,
}

fn is_default_link_format(format: &LinkFormat) -> bool {
    *format == LinkFormat::default()
}

impl Config {
    /// Get the directory where daily entries should be created
    /// Returns daily_entry_folder joined with default_workspace, or just default_workspace
    pub fn daily_entry_dir(&self) -> PathBuf {
        match &self.daily_entry_folder {
            Some(folder) => {
                // Strip leading slashes to ensure proper path joining
                // A leading "/" would make this an absolute path instead of relative
                let normalized = folder.trim_start_matches('/');
                self.default_workspace.join(normalized)
            }
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
            link_format: LinkFormat::default(),
            sync_server_url: None,
            sync_session_token: None,
            sync_email: None,
            sync_workspace_id: None,
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
            link_format: LinkFormat::default(),
            sync_server_url: None,
            sync_session_token: None,
            sync_email: None,
            sync_workspace_id: None,
        }
    }

    // ========================================================================
    // AsyncFileSystem-based methods (work on all platforms including WASM)
    // ========================================================================

    /// Load config from a specific path using an AsyncFileSystem.
    pub async fn load_from<FS: AsyncFileSystem>(fs: &FS, path: &std::path::Path) -> Result<Self> {
        let contents = fs
            .read_to_string(path)
            .await
            .map_err(|e| DiaryxError::FileRead {
                path: path.to_path_buf(),
                source: e,
            })?;

        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }

    /// Save config to a specific path using an AsyncFileSystem.
    pub async fn save_to<FS: AsyncFileSystem>(
        &self,
        fs: &FS,
        path: &std::path::Path,
    ) -> Result<()> {
        // Create parent directory if needed
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs.create_dir_all(parent).await?;
        }

        let contents = toml::to_string_pretty(self)?;
        fs.write_file(path, &contents).await?;
        Ok(())
    }

    /// Load config from an AsyncFileSystem, returning default if not found.
    pub async fn load_from_or_default<FS: AsyncFileSystem>(
        fs: &FS,
        path: &std::path::Path,
        default_workspace: PathBuf,
    ) -> Self {
        match Self::load_from(fs, path).await {
            Ok(config) => config,
            Err(_) => Self::new(default_workspace),
        }
    }

    // ========================================================================
    // Sync wrappers (compatibility layer). Prefer the async APIs above.
    // ========================================================================
    //
    // IMPORTANT:
    // These wrappers are only available on non-WASM targets because they require a
    // blocking executor. On WASM, filesystem access is expected to be async.

    /// Sync wrapper for [`Config::load_from`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_from_sync<FS: FileSystem>(fs: FS, path: &std::path::Path) -> Result<Self> {
        futures_lite::future::block_on(Self::load_from(&SyncToAsyncFs::new(fs), path))
    }

    /// Sync wrapper for [`Config::save_to`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn save_to_sync<FS: FileSystem>(&self, fs: FS, path: &std::path::Path) -> Result<()> {
        futures_lite::future::block_on(self.save_to(&SyncToAsyncFs::new(fs), path))
    }

    /// Sync wrapper for [`Config::load_from_or_default`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_from_or_default_sync<FS: FileSystem>(
        fs: FS,
        path: &std::path::Path,
        default_workspace: PathBuf,
    ) -> Self {
        futures_lite::future::block_on(Self::load_from_or_default(
            &SyncToAsyncFs::new(fs),
            path,
            default_workspace,
        ))
    }
}

// ============================================================================
// Native-only implementation (not available in WASM)
// ============================================================================

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
            link_format: LinkFormat::default(),
            sync_server_url: None,
            sync_session_token: None,
            sync_email: None,
            sync_workspace_id: None,
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
        if let Some(path) = Self::config_path()
            && path.exists()
        {
            let contents = std::fs::read_to_string(&path)?;
            let config: Config = toml::from_str(&contents)?;
            return Ok(config);
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
            std::fs::create_dir_all(parent)?;
        }

        let contents = toml::to_string_pretty(self)?;
        std::fs::write(&path, contents)?;

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
            link_format: LinkFormat::default(),
            sync_server_url: None,
            sync_session_token: None,
            sync_email: None,
            sync_workspace_id: None,
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
            link_format: LinkFormat::default(),
            sync_server_url: None,
            sync_session_token: None,
            sync_email: None,
            sync_workspace_id: None,
        }
    }
}
