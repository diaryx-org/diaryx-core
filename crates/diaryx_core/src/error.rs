use std::path::PathBuf;

use serde::Serialize;
use thiserror::Error;

/// Unified error type for Diaryx operations
///
/// Many of these are necessary because of the abstracted FileSystem in `fs.rs`.
#[derive(Debug, Error)]
pub enum DiaryxError {
    /// General error for any kind of I/O issue not otherwise documented here.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// A kind of error representing a failed file read.
    ///
    /// Can occur due to:
    /// - insufficient permissions
    /// - locking/concurrent access
    /// - resource issues
    ///
    /// Diaryx should display an error message if a file cannot be read.
    #[error("Failed to read file '{path}': {source}")]
    FileRead {
        /// Path to the file that failed to be read
        path: PathBuf,
        /// std::io error that caused this error
        source: std::io::Error,
    },

    /// A kind of error representing a failed file write.
    ///
    /// Can occur due to:
    /// - insufficient permissions
    /// - locking/concurrent access
    /// - resource issues
    ///
    /// Diaryx should display an error message if a file cannot be written.
    #[error("Failed to write file '{path}': {source}")]
    FileWrite {
        /// Path to file that failed to be written
        path: PathBuf,
        /// std::io error that caused this error
        source: std::io::Error,
    },

    /// An error that occured while serializing or deserializing YAML data from the frontmatter.
    ///
    /// Inherited from `serde_yaml::Error`
    #[error("YAML parsing error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    /// An error that occurs when no frontmatter is found in a file.
    ///
    /// Diaryx should gracefully work around this error by doing things such as the following:
    /// - If attempting to read frontmatter, simply display empty values for all possible.
    /// - If trying to write to frontmatter, initialize it first by adding `---` delimiters before and after.
    #[error("No frontmatter found in '{0}'")]
    NoFrontmatter(PathBuf),

    /// Error from invalid/unparseable frontmatter.
    ///
    /// Diaryx should gracefully work around this error whenever possible.
    #[error("Invalid frontmatter structure in '{0}'")]
    InvalidFrontmatter(PathBuf),

    /// Date errors
    #[error(
        "Invalid date format: '{0}'. Try 'today', 'yesterday', 'last friday', '3 days ago', or 'YYYY-MM-DD'"
    )]
    InvalidDateFormat(String),

    /// Error that occurs when deserializing config.toml file.
    ///
    /// Inherited from `toml::de::Error`
    #[error("Config parse error: {0}")]
    ConfigParse(#[from] toml::de::Error),

    /// Config failed to serialize.
    ///
    /// Inherited from `toml::ser::Error`.
    #[error("Config serialize error: {0}")]
    ConfigSerialize(#[from] toml::ser::Error),

    /// Error indicating a failure to find config directory.
    /// Diaryx should fall back to default config when this occurs.
    #[error("Could not determine config directory")]
    NoConfigDir,

    /// Error from missing config.
    #[error("Configuration not initialized. Run 'diaryx init' first.")]
    ConfigNotInitialized,

    /// Error when editor is not configured for `diaryx open` commands and similar.
    /// This should be rare, because in Diaryx CLI it tries common editors like `nano` before returning this.
    #[error("No editor found. Set $EDITOR, $VISUAL, or configure editor in config file")]
    NoEditorFound,

    /// Error from failing to launch an editor.
    /// Common in WASI or other environments that don't support forking a process.
    #[error("Failed to launch editor '{editor}': {source}")]
    EditorLaunchFailed {
        /// Name of editor command that failed
        editor: String,
        /// std::io error that caused this error
        source: std::io::Error,
    },

    /// Error for when editor fails for some reason.
    /// Should be passed onto the user.
    #[error("Editor exited with code {0}")]
    EditorExited(i32),

    /// Error for when workspace is not found.
    /// Should give an error message to user, then possibly fall back to default config.
    #[error("Workspace not found at '{0}'")]
    WorkspaceNotFound(PathBuf),

    /// When creating a workspace, workspace already exists.
    /// Should give a message to user.
    #[error("Workspace already exists at '{0}'")]
    WorkspaceAlreadyExists(PathBuf),

    /// Error for when template is not defined.
    /// Should give a message to the user.
    #[error("Template not found: '{0}'")]
    TemplateNotFound(String),

    /// Error for when trying to create a template that already exists.
    #[error("Template already exists: '{0}'")]
    TemplateAlreadyExists(PathBuf),

    /// Error for invalid path structure (e.g., missing parent directory or filename).
    #[error("Invalid path '{path}': {message}")]
    InvalidPath {
        /// Path that is invalid
        path: PathBuf,
        /// Description of what's wrong with the path
        message: String,
    },
}

/// Result type alias for Diaryx operations
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
            DiaryxError::TemplateNotFound(_) => "TemplateNotFound",
            DiaryxError::TemplateAlreadyExists(_) => "TemplateAlreadyExists",
            DiaryxError::InvalidPath { .. } => "InvalidPath",
        }
        .to_string();

        let path = match err {
            DiaryxError::FileRead { path, .. } => Some(path.clone()),
            DiaryxError::FileWrite { path, .. } => Some(path.clone()),
            DiaryxError::NoFrontmatter(path) => Some(path.clone()),
            DiaryxError::InvalidFrontmatter(path) => Some(path.clone()),
            DiaryxError::WorkspaceNotFound(path) => Some(path.clone()),
            DiaryxError::WorkspaceAlreadyExists(path) => Some(path.clone()),
            DiaryxError::TemplateAlreadyExists(path) => Some(path.clone()),
            DiaryxError::InvalidPath { path, .. } => Some(path.clone()),
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
