use std::path::PathBuf;
use thiserror::Error;

/// Unified error type for diaryx operations
#[derive(Debug, Error)]
pub enum DiaryxError {
    // IO errors
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to read file '{path}': {source}")]
    FileRead {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Failed to write file '{path}': {source}")]
    FileWrite {
        path: PathBuf,
        source: std::io::Error,
    },

    // Frontmatter errors
    #[error("YAML parsing error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("No frontmatter found in '{0}'")]
    NoFrontmatter(PathBuf),

    #[error("Invalid frontmatter structure in '{0}'")]
    InvalidFrontmatter(PathBuf),

    // Date errors
    #[error("Invalid date format: '{0}'. Try 'today', 'yesterday', 'last friday', '3 days ago', or 'YYYY-MM-DD'")]
    InvalidDateFormat(String),

    // Config errors
    #[error("Config parse error: {0}")]
    ConfigParse(#[from] toml::de::Error),

    #[error("Config serialize error: {0}")]
    ConfigSerialize(#[from] toml::ser::Error),

    #[error("Could not determine config directory")]
    NoConfigDir,

    #[error("Configuration not initialized. Run 'diaryx init' first.")]
    ConfigNotInitialized,

    // Editor errors
    #[error("No editor found. Set $EDITOR, $VISUAL, or configure editor in config file")]
    NoEditorFound,

    #[error("Failed to launch editor '{editor}': {source}")]
    EditorLaunchFailed {
        editor: String,
        source: std::io::Error,
    },

    #[error("Editor exited with code {0}")]
    EditorExited(i32),

    // Workspace errors
    #[error("Workspace not found at '{0}'")]
    WorkspaceNotFound(PathBuf),

    #[error("Workspace already exists at '{0}'")]
    WorkspaceAlreadyExists(PathBuf),
}

/// Result type alias for diaryx operations
pub type Result<T> = std::result::Result<T, DiaryxError>;
