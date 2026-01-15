//! Core types for CRDT-based synchronization.
//!
//! This module defines the data structures used to represent file metadata,
//! binary attachments, and CRDT updates in the synchronization system.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ts_rs::TS;

/// Metadata for a file in the workspace CRDT.
///
/// This represents the synchronized state of a file's frontmatter properties,
/// stored in a Y.Map within the workspace document.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct FileMetadata {
    /// Display title from frontmatter
    pub title: Option<String>,

    /// Absolute path to parent index file (e.g., "workspace/Daily/index.md")
    pub part_of: Option<String>,

    /// Relative paths to child files (e.g., ["2026/index.md", "notes.md"])
    pub contents: Option<Vec<String>>,

    /// Binary attachment references
    pub attachments: Vec<BinaryRef>,

    /// Soft deletion tombstone - if true, file is considered deleted
    pub deleted: bool,

    /// Visibility/access control tags
    pub audience: Option<Vec<String>>,

    /// File description from frontmatter
    pub description: Option<String>,

    /// Additional frontmatter properties not covered by other fields
    pub extra: HashMap<String, serde_json::Value>,

    /// Unix timestamp of last modification (milliseconds)
    pub modified_at: i64,
}

impl FileMetadata {
    /// Create new FileMetadata with the given title
    pub fn new(title: Option<String>) -> Self {
        Self {
            title,
            modified_at: chrono::Utc::now().timestamp_millis(),
            ..Default::default()
        }
    }

    /// Mark this file as deleted (soft delete)
    pub fn mark_deleted(&mut self) {
        self.deleted = true;
        self.modified_at = chrono::Utc::now().timestamp_millis();
    }

    /// Check if this file is an index (has contents)
    pub fn is_index(&self) -> bool {
        self.contents.as_ref().is_some_and(|c| !c.is_empty())
    }
}

/// Reference to a binary attachment file.
///
/// Binary files (images, PDFs, etc.) are stored separately from the CRDT,
/// with only their metadata tracked in the synchronization system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct BinaryRef {
    /// Relative path within workspace (e.g., "_attachments/image.png")
    pub path: String,

    /// Source of the binary: "local", "pending", or external URL
    pub source: String,

    /// SHA-256 hash for deduplication and integrity
    pub hash: String,

    /// MIME type (e.g., "image/png")
    pub mime_type: String,

    /// File size in bytes
    pub size: u64,

    /// Unix timestamp when uploaded (milliseconds)
    pub uploaded_at: Option<i64>,

    /// Soft deletion tombstone
    pub deleted: bool,
}

impl BinaryRef {
    /// Create a new local binary reference
    pub fn new_local(path: String, hash: String, mime_type: String, size: u64) -> Self {
        Self {
            path,
            source: "local".to_string(),
            hash,
            mime_type,
            size,
            uploaded_at: Some(chrono::Utc::now().timestamp_millis()),
            deleted: false,
        }
    }

    /// Create a pending binary reference (not yet uploaded)
    pub fn new_pending(path: String, mime_type: String, size: u64) -> Self {
        Self {
            path,
            source: "pending".to_string(),
            hash: String::new(),
            mime_type,
            size,
            uploaded_at: None,
            deleted: false,
        }
    }
}

/// A CRDT update record, stored for history and sync purposes.
#[derive(Debug, Clone)]
pub struct CrdtUpdate {
    /// Unique identifier for this update
    pub update_id: i64,

    /// Name of the document this update belongs to
    pub doc_name: String,

    /// Binary yrs update data
    pub data: Vec<u8>,

    /// Unix timestamp when this update was created (milliseconds)
    pub timestamp: i64,

    /// Origin of this update (local edit, remote sync, etc.)
    pub origin: UpdateOrigin,

    /// Device ID that created this update (for multi-device attribution)
    pub device_id: Option<String>,

    /// Human-readable device name (e.g., "MacBook Pro", "iPhone")
    pub device_name: Option<String>,
}

/// Origin of a CRDT update, used to distinguish local vs remote changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateOrigin {
    /// Update originated from local user action
    Local,

    /// Update received from a remote peer
    Remote,

    /// Update from initial sync handshake
    Sync,
}

impl std::fmt::Display for UpdateOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpdateOrigin::Local => write!(f, "local"),
            UpdateOrigin::Remote => write!(f, "remote"),
            UpdateOrigin::Sync => write!(f, "sync"),
        }
    }
}

impl std::str::FromStr for UpdateOrigin {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "local" => Ok(UpdateOrigin::Local),
            "remote" => Ok(UpdateOrigin::Remote),
            "sync" => Ok(UpdateOrigin::Sync),
            _ => Err(format!("Unknown update origin: {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_metadata_default() {
        let meta = FileMetadata::default();
        assert!(meta.title.is_none());
        assert!(!meta.deleted);
        assert!(meta.attachments.is_empty());
    }

    #[test]
    fn test_file_metadata_new() {
        let meta = FileMetadata::new(Some("Test".to_string()));
        assert_eq!(meta.title, Some("Test".to_string()));
        assert!(meta.modified_at > 0);
    }

    #[test]
    fn test_file_metadata_mark_deleted() {
        let mut meta = FileMetadata::default();
        let original_time = meta.modified_at;
        std::thread::sleep(std::time::Duration::from_millis(1));
        meta.mark_deleted();
        assert!(meta.deleted);
        assert!(meta.modified_at > original_time);
    }

    #[test]
    fn test_binary_ref_new_local() {
        let binary = BinaryRef::new_local(
            "test.png".to_string(),
            "abc123".to_string(),
            "image/png".to_string(),
            1024,
        );
        assert_eq!(binary.source, "local");
        assert!(binary.uploaded_at.is_some());
    }

    #[test]
    fn test_update_origin_display() {
        assert_eq!(UpdateOrigin::Local.to_string(), "local");
        assert_eq!(UpdateOrigin::Remote.to_string(), "remote");
        assert_eq!(UpdateOrigin::Sync.to_string(), "sync");
    }

    #[test]
    fn test_update_origin_from_str() {
        assert_eq!(
            "local".parse::<UpdateOrigin>().unwrap(),
            UpdateOrigin::Local
        );
        assert_eq!(
            "remote".parse::<UpdateOrigin>().unwrap(),
            UpdateOrigin::Remote
        );
        assert!("invalid".parse::<UpdateOrigin>().is_err());
    }
}
