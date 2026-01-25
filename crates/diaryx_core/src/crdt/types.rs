//! Core types for CRDT-based synchronization.
//!
//! This module defines the data structures used to represent file metadata,
//! binary attachments, and CRDT updates in the synchronization system.

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use ts_rs::TS;

/// Deserializes a value that should be a string, but may be an integer or other type.
/// Converts non-string values to their string representation.
fn deserialize_string_lenient<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    let value: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(serde_json::Value::String(s)) => Ok(Some(s)),
        Some(serde_json::Value::Number(n)) => Ok(Some(n.to_string())),
        Some(serde_json::Value::Bool(b)) => Ok(Some(b.to_string())),
        Some(serde_json::Value::Null) => Ok(None),
        Some(other) => Err(D::Error::custom(format!(
            "expected string or number, got {:?}",
            other
        ))),
    }
}

/// Metadata for a file in the workspace CRDT.
///
/// This represents the synchronized state of a file's frontmatter properties,
/// stored in a Y.Map within the workspace document.
///
/// ## Doc-ID Based Architecture
///
/// Files are keyed by stable document IDs (UUIDs) rather than file paths.
/// This makes renames trivial property updates rather than delete+create operations.
///
/// The actual filesystem path is derived from the `filename` field and the parent chain:
/// - `filename`: The file's name on disk (e.g., "my-note.md")
/// - `part_of`: Document ID of the parent (or None for root files)
///
/// Use `WorkspaceCrdt::get_path()` to derive the full path from a doc_id.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct FileMetadata {
    /// Filename on disk (e.g., "my-note.md"). Required for non-deleted files.
    /// For files created before the doc-ID migration, this may be empty and
    /// should be derived from the path key during migration.
    #[serde(default)]
    pub filename: String,

    /// Display title from frontmatter
    #[serde(default, deserialize_with = "deserialize_string_lenient")]
    pub title: Option<String>,

    /// Document ID of parent file (e.g., "abc123-uuid"), or None for root files.
    /// Note: For backward compatibility during migration, this may temporarily
    /// contain absolute paths which will be converted to doc_ids.
    pub part_of: Option<String>,

    /// Document IDs of child files.
    /// Note: For backward compatibility during migration, this may temporarily
    /// contain relative paths which will be converted to doc_ids.
    pub contents: Option<Vec<String>>,

    /// Binary attachment references
    pub attachments: Vec<BinaryRef>,

    /// Soft deletion tombstone - if true, file is considered deleted
    pub deleted: bool,

    /// Visibility/access control tags
    pub audience: Option<Vec<String>>,

    /// File description from frontmatter
    #[serde(default, deserialize_with = "deserialize_string_lenient")]
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

    /// Create new FileMetadata with filename and title
    pub fn with_filename(filename: String, title: Option<String>) -> Self {
        Self {
            filename,
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

    /// Convert a title to a normalized filename.
    ///
    /// Rules:
    /// - Lowercase
    /// - Replace spaces and underscores with hyphens
    /// - Remove non-alphanumeric characters (except hyphens)
    /// - Collapse multiple hyphens
    /// - Append .md extension
    ///
    /// Example: "My Note Title" → "my-note-title.md"
    pub fn normalize_title_to_filename(title: &str) -> String {
        let normalized: String = title
            .to_lowercase()
            .chars()
            .map(|c| {
                if c.is_alphanumeric() {
                    c
                } else if c == ' ' || c == '_' {
                    '-'
                } else if c == '-' {
                    c
                } else {
                    // Skip other characters
                    '-'
                }
            })
            .collect();

        // Collapse multiple hyphens and trim leading/trailing hyphens
        let collapsed: String = normalized
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");

        if collapsed.is_empty() {
            "untitled.md".to_string()
        } else {
            format!("{}.md", collapsed)
        }
    }

    /// Check if this metadata uses the legacy path-based format.
    ///
    /// Returns true if part_of contains a path (has '/') rather than a UUID.
    pub fn is_legacy_format(&self) -> bool {
        self.part_of
            .as_ref()
            .is_some_and(|p| p.contains('/') || p.ends_with(".md"))
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
        assert!(meta.filename.is_empty());
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
    fn test_file_metadata_with_filename() {
        let meta = FileMetadata::with_filename("test.md".to_string(), Some("Test".to_string()));
        assert_eq!(meta.filename, "test.md");
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
    fn test_normalize_title_to_filename() {
        assert_eq!(
            FileMetadata::normalize_title_to_filename("My Note Title"),
            "my-note-title.md"
        );
        assert_eq!(
            FileMetadata::normalize_title_to_filename("Hello World!"),
            "hello-world.md"
        );
        assert_eq!(
            FileMetadata::normalize_title_to_filename("Test_File Name"),
            "test-file-name.md"
        );
        assert_eq!(
            FileMetadata::normalize_title_to_filename("  Multiple   Spaces  "),
            "multiple-spaces.md"
        );
        assert_eq!(FileMetadata::normalize_title_to_filename(""), "untitled.md");
        assert_eq!(
            FileMetadata::normalize_title_to_filename("!!!"),
            "untitled.md"
        );
    }

    #[test]
    fn test_is_legacy_format() {
        let mut meta = FileMetadata::default();
        assert!(!meta.is_legacy_format()); // No part_of

        meta.part_of = Some("abc123-uuid".to_string());
        assert!(!meta.is_legacy_format()); // UUID format

        meta.part_of = Some("workspace/index.md".to_string());
        assert!(meta.is_legacy_format()); // Path format

        meta.part_of = Some("index.md".to_string());
        assert!(meta.is_legacy_format()); // Filename with .md
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
