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

    /// Get the primary path associated with this event.
    pub fn path(&self) -> &PathBuf {
        match self {
            Self::FileCreated { path, .. } => path,
            Self::FileDeleted { path, .. } => path,
            Self::FileRenamed { new_path, .. } => new_path,
            Self::FileMoved { path, .. } => path,
            Self::MetadataChanged { path, .. } => path,
            Self::ContentsChanged { path, .. } => path,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_created_event() {
        let event = FileSystemEvent::file_created(PathBuf::from("test.md"));
        assert_eq!(event.path(), &PathBuf::from("test.md"));
        assert_eq!(event.event_type(), "FileCreated");
    }

    #[test]
    fn test_file_deleted_event() {
        let event = FileSystemEvent::file_deleted(PathBuf::from("test.md"));
        assert_eq!(event.path(), &PathBuf::from("test.md"));
        assert_eq!(event.event_type(), "FileDeleted");
    }

    #[test]
    fn test_file_renamed_event() {
        let event = FileSystemEvent::file_renamed(PathBuf::from("old.md"), PathBuf::from("new.md"));
        assert_eq!(event.path(), &PathBuf::from("new.md"));
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
}
