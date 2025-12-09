use std::path::PathBuf;

use serde::Serialize;
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

/// A serializable representation of DiaryxError for IPC (e.g., Tauri)
#[derive(Debug, Clone, Serialize)]
pub struct SerializableError {
    /// Error kind/variant name
    pub kind: String,
    /// Human-readable error message
    pub message: String,
    /// Associated path (if applicable)
    pub path: Option<PathBuf>,
}

impl From<&DiaryxError> for SerializableError {
    fn from(err: &DiaryxError) -> Self {
        let kind = match err {
            DiaryxError::Io(_) => "Io",
            DiaryxError::FileRead { .. } => "FileRead",
            DiaryxError::FileWrite { .. } => "FileWrite",
            DiaryxError::Yaml(_) => "Yaml",
            DiaryxError::NoFrontmatter(_) => "NoFrontmatter",
            DiaryxError::InvalidFrontmatter(_) => "InvalidFrontmatter",
            DiaryxError::InvalidDateFormat(_) => "InvalidDateFormat",
            DiaryxError::ConfigParse(_) => "ConfigParse",
            DiaryxError::ConfigSerialize(_) => "ConfigSerialize",
            DiaryxError::NoConfigDir => "NoConfigDir",
            DiaryxError::ConfigNotInitialized => "ConfigNotInitialized",
            DiaryxError::NoEditorFound => "NoEditorFound",
            DiaryxError::EditorLaunchFailed { .. } => "EditorLaunchFailed",
            DiaryxError::EditorExited(_) => "EditorExited",
            DiaryxError::WorkspaceNotFound(_) => "WorkspaceNotFound",
            DiaryxError::WorkspaceAlreadyExists(_) => "WorkspaceAlreadyExists",
        }
        .to_string();

        let path = match err {
            DiaryxError::FileRead { path, .. } => Some(path.clone()),
            DiaryxError::FileWrite { path, .. } => Some(path.clone()),
            DiaryxError::NoFrontmatter(path) => Some(path.clone()),
            DiaryxError::InvalidFrontmatter(path) => Some(path.clone()),
            DiaryxError::WorkspaceNotFound(path) => Some(path.clone()),
            DiaryxError::WorkspaceAlreadyExists(path) => Some(path.clone()),
            _ => None,
        };

        Self {
            kind,
            message: err.to_string(),
            path,
        }
    }
}

impl From<DiaryxError> for SerializableError {
    fn from(err: DiaryxError) -> Self {
        SerializableError::from(&err)
    }
}

impl DiaryxError {
    /// Convert to a serializable representation for IPC
    pub fn to_serializable(&self) -> SerializableError {
        SerializableError::from(self)
    }
}
