//! Filesystem events for the decorator pattern.
//!
//! This module defines the events emitted by the [`EventEmittingFs`](super::EventEmittingFs)
//! decorator when filesystem operations occur. These events can be used to trigger
//! UI updates, CRDT synchronization, or other side effects.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

/// Events emitted by filesystem operations.
///
/// These events capture the semantics of filesystem changes, including both
/// the operation type and relevant metadata for each type of change.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
#[serde(tag = "type")]
pub enum FileSystemEvent {
    /// A new file was created.
    FileCreated {
        /// Path of the created file.
        path: PathBuf,
        /// Frontmatter from the file, if any.
        #[serde(default)]
        frontmatter: Option<serde_json::Value>,
        /// Path of the parent index file, if known.
        #[serde(default)]
        parent_path: Option<PathBuf>,
    },

    /// A file was deleted.
    FileDeleted {
        /// Path of the deleted file.
        path: PathBuf,
        /// Path of the parent index file, if known.
        #[serde(default)]
        parent_path: Option<PathBuf>,
    },

    /// A file was renamed (same parent directory).
    FileRenamed {
        /// Original path of the file.
        old_path: PathBuf,
        /// New path of the file.
        new_path: PathBuf,
    },

    /// A file was moved to a different parent directory.
    FileMoved {
        /// Path of the file after the move.
        path: PathBuf,
        /// Original parent directory path.
        #[serde(default)]
        old_parent: Option<PathBuf>,
        /// New parent directory path.
        #[serde(default)]
        new_parent: Option<PathBuf>,
    },

    /// File metadata (frontmatter) was changed.
    MetadataChanged {
        /// Path of the file.
        path: PathBuf,
        /// New frontmatter values.
        frontmatter: serde_json::Value,
    },

    /// File body content was changed.
    ContentsChanged {
        /// Path of the file.
        path: PathBuf,
        /// New body content.
        body: String,
    },

    // === Sync Events ===
    /// Sync session started.
    SyncStarted {
        /// Document name (e.g., "workspace" or file path for body docs).
        doc_name: String,
    },

    /// Initial sync completed.
    SyncCompleted {
        /// Document name.
        doc_name: String,
        /// Number of files synced.
        files_synced: usize,
    },

    /// Sync status changed.
    SyncStatusChanged {
        /// Status: "idle", "connecting", "syncing", "synced", "error".
        status: String,
        /// Optional error message when status is "error".
        #[serde(default)]
        error: Option<String>,
    },

    /// Sync progress update.
    SyncProgress {
        /// Number of files completed.
        completed: usize,
        /// Total number of files to sync.
        total: usize,
    },

    /// Request to send sync message over WebSocket.
    /// Emitted by command handler after CRDT updates.
    SendSyncMessage {
        /// Document name ("workspace" for workspace, file path for body)
        doc_name: String,
        /// Encoded sync message bytes to send (serialized as array of numbers)
        message: Vec<u8>,
        /// Whether this is a body doc (true) or workspace (false)
        is_body: bool,
    },
}

impl FileSystemEvent {
    /// Create a FileCreated event.
    pub fn file_created(path: PathBuf) -> Self {
        Self::FileCreated {
            path,
            frontmatter: None,
            parent_path: None,
        }
    }

    /// Create a FileCreated event with frontmatter.
    pub fn file_created_with_metadata(
        path: PathBuf,
        frontmatter: Option<serde_json::Value>,
        parent_path: Option<PathBuf>,
    ) -> Self {
        Self::FileCreated {
            path,
            frontmatter,
            parent_path,
        }
    }

    /// Create a FileDeleted event.
    pub fn file_deleted(path: PathBuf) -> Self {
        Self::FileDeleted {
            path,
            parent_path: None,
        }
    }

    /// Create a FileDeleted event with parent path.
    pub fn file_deleted_with_parent(path: PathBuf, parent_path: Option<PathBuf>) -> Self {
        Self::FileDeleted { path, parent_path }
    }

    /// Create a FileRenamed event.
    pub fn file_renamed(old_path: PathBuf, new_path: PathBuf) -> Self {
        Self::FileRenamed { old_path, new_path }
    }

    /// Create a FileMoved event.
    pub fn file_moved(
        path: PathBuf,
        old_parent: Option<PathBuf>,
        new_parent: Option<PathBuf>,
    ) -> Self {
        Self::FileMoved {
            path,
            old_parent,
            new_parent,
        }
    }

    /// Create a MetadataChanged event.
    pub fn metadata_changed(path: PathBuf, frontmatter: serde_json::Value) -> Self {
        Self::MetadataChanged { path, frontmatter }
    }

    /// Create a ContentsChanged event.
    pub fn contents_changed(path: PathBuf, body: String) -> Self {
        Self::ContentsChanged { path, body }
    }

    /// Create a SyncStarted event.
    pub fn sync_started(doc_name: String) -> Self {
        Self::SyncStarted { doc_name }
    }

    /// Create a SyncCompleted event.
    pub fn sync_completed(doc_name: String, files_synced: usize) -> Self {
        Self::SyncCompleted {
            doc_name,
            files_synced,
        }
    }

    /// Create a SyncStatusChanged event.
    pub fn sync_status_changed(status: impl Into<String>, error: Option<String>) -> Self {
        Self::SyncStatusChanged {
            status: status.into(),
            error,
        }
    }

    /// Create a SyncProgress event.
    pub fn sync_progress(completed: usize, total: usize) -> Self {
        Self::SyncProgress { completed, total }
    }

    /// Create a SendSyncMessage event.
    pub fn send_sync_message(doc_name: impl Into<String>, message: Vec<u8>, is_body: bool) -> Self {
        Self::SendSyncMessage {
            doc_name: doc_name.into(),
            message,
            is_body,
        }
    }

    /// Get the primary path associated with this event.
    /// Returns None for sync events which don't have a path.
    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            Self::FileCreated { path, .. } => Some(path),
            Self::FileDeleted { path, .. } => Some(path),
            Self::FileRenamed { new_path, .. } => Some(new_path),
            Self::FileMoved { path, .. } => Some(path),
            Self::MetadataChanged { path, .. } => Some(path),
            Self::ContentsChanged { path, .. } => Some(path),
            // Sync events don't have a primary path
            Self::SyncStarted { .. } => None,
            Self::SyncCompleted { .. } => None,
            Self::SyncStatusChanged { .. } => None,
            Self::SyncProgress { .. } => None,
            Self::SendSyncMessage { .. } => None,
        }
    }

    /// Get the event type as a string.
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::FileCreated { .. } => "FileCreated",
            Self::FileDeleted { .. } => "FileDeleted",
            Self::FileRenamed { .. } => "FileRenamed",
            Self::FileMoved { .. } => "FileMoved",
            Self::MetadataChanged { .. } => "MetadataChanged",
            Self::ContentsChanged { .. } => "ContentsChanged",
            Self::SyncStarted { .. } => "SyncStarted",
            Self::SyncCompleted { .. } => "SyncCompleted",
            Self::SyncStatusChanged { .. } => "SyncStatusChanged",
            Self::SyncProgress { .. } => "SyncProgress",
            Self::SendSyncMessage { .. } => "SendSyncMessage",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_created_event() {
        let path = PathBuf::from("test.md");
        let event = FileSystemEvent::file_created(path.clone());
        assert_eq!(event.path(), Some(&path));
        assert_eq!(event.event_type(), "FileCreated");
    }

    #[test]
    fn test_file_deleted_event() {
        let path = PathBuf::from("test.md");
        let event = FileSystemEvent::file_deleted(path.clone());
        assert_eq!(event.path(), Some(&path));
        assert_eq!(event.event_type(), "FileDeleted");
    }

    #[test]
    fn test_file_renamed_event() {
        let new_path = PathBuf::from("new.md");
        let event = FileSystemEvent::file_renamed(PathBuf::from("old.md"), new_path.clone());
        assert_eq!(event.path(), Some(&new_path));
        assert_eq!(event.event_type(), "FileRenamed");
    }

    #[test]
    fn test_event_serialization() {
        let event = FileSystemEvent::file_created_with_metadata(
            PathBuf::from("test.md"),
            Some(serde_json::json!({"title": "Test"})),
            Some(PathBuf::from("index.md")),
        );

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("FileCreated"));
        assert!(json.contains("test.md"));

        let parsed: FileSystemEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.event_type(), "FileCreated");
    }

    #[test]
    fn test_sync_events() {
        let event = FileSystemEvent::sync_started("workspace".to_string());
        assert_eq!(event.event_type(), "SyncStarted");
        assert!(event.path().is_none());

        let event = FileSystemEvent::sync_completed("workspace".to_string(), 10);
        assert_eq!(event.event_type(), "SyncCompleted");

        let event = FileSystemEvent::sync_status_changed("synced", None);
        assert_eq!(event.event_type(), "SyncStatusChanged");

        let event =
            FileSystemEvent::sync_status_changed("error", Some("Connection failed".to_string()));
        assert_eq!(event.event_type(), "SyncStatusChanged");

        let event = FileSystemEvent::sync_progress(5, 10);
        assert_eq!(event.event_type(), "SyncProgress");
    }
}
