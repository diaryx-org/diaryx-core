//! Sync manifest for tracking file synchronization state.
//!
//! The manifest stores metadata about each synced file, allowing the sync engine
//! to detect changes since the last sync and avoid unnecessary transfers.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Manifest tracking the sync state of all files.
///
/// This is stored locally (and optionally in cloud storage) to track
/// what has been synced and detect changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncManifest {
    /// Version of the manifest format for future compatibility
    pub version: u32,

    /// When the last sync was performed
    pub last_sync: Option<DateTime<Utc>>,

    /// Provider identifier (e.g., "s3:bucket-name" or "gdrive:folder-id")
    pub provider_id: String,

    /// Per-file sync state
    pub files: HashMap<String, FileSyncState>,

    /// Provider-specific cursor for incremental sync (e.g., GDrive change token)
    #[serde(default)]
    pub cursor: Option<String>,
}

impl SyncManifest {
    /// Current manifest format version
    pub const CURRENT_VERSION: u32 = 1;

    /// Create a new empty manifest for a provider
    pub fn new(provider_id: impl Into<String>) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            last_sync: None,
            provider_id: provider_id.into(),
            files: HashMap::new(),
            cursor: None,
        }
    }

    /// Get the sync state for a file
    pub fn get_file(&self, path: &str) -> Option<&FileSyncState> {
        self.files.get(path)
    }

    /// Update or insert the sync state for a file
    pub fn set_file(&mut self, path: impl Into<String>, state: FileSyncState) {
        self.files.insert(path.into(), state);
    }

    /// Remove a file from the manifest (after deletion)
    pub fn remove_file(&mut self, path: &str) -> Option<FileSyncState> {
        self.files.remove(path)
    }

    /// Mark the sync as complete with current timestamp
    pub fn mark_synced(&mut self) {
        self.last_sync = Some(Utc::now());
    }

    /// Check if a file needs to be uploaded based on local modification
    pub fn needs_upload(&self, path: &str, local_modified_at: i64, content_hash: &str) -> bool {
        match self.files.get(path) {
            None => true, // New file, needs upload
            Some(state) => {
                // Check if modified since last sync
                local_modified_at > state.local_modified_at || state.content_hash != content_hash
            }
        }
    }

    /// Get paths of files that were in manifest but are now missing locally
    pub fn get_locally_deleted(&self, current_paths: &[String]) -> Vec<String> {
        let current_set: std::collections::HashSet<_> = current_paths.iter().collect();
        self.files
            .keys()
            .filter(|path| !current_set.contains(path))
            .cloned()
            .collect()
    }

    /// Load manifest from JSON string
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Serialize manifest to JSON string
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Load manifest from a file path
    pub async fn load_from_file(
        fs: &dyn crate::fs::AsyncFileSystem,
        path: &Path,
    ) -> Result<Self, String> {
        let content = fs
            .read_to_string(path)
            .await
            .map_err(|e| format!("Failed to read manifest: {}", e))?;
        Self::from_json(&content).map_err(|e| format!("Failed to parse manifest: {}", e))
    }

    /// Save manifest to a file path
    pub async fn save_to_file(
        &self,
        fs: &dyn crate::fs::AsyncFileSystem,
        path: &Path,
    ) -> Result<(), String> {
        let content = self
            .to_json()
            .map_err(|e| format!("Failed to serialize manifest: {}", e))?;
        fs.write_file(path, &content)
            .await
            .map_err(|e| format!("Failed to write manifest: {}", e))
    }
}

/// Sync state for a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSyncState {
    /// Path relative to workspace root
    pub path: String,

    /// SHA-256 hash of content at last sync
    pub content_hash: String,

    /// Timestamp when this file was last synced
    pub synced_at: DateTime<Utc>,

    /// Provider-specific version ID (S3 ETag, GDrive revision ID)
    #[serde(default)]
    pub remote_version: Option<String>,

    /// Local `modified_at` timestamp at last sync
    pub local_modified_at: i64,

    /// File size at last sync
    #[serde(default)]
    pub size: u64,
}

impl FileSyncState {
    /// Create a new file sync state
    pub fn new(
        path: impl Into<String>,
        content_hash: impl Into<String>,
        local_modified_at: i64,
    ) -> Self {
        Self {
            path: path.into(),
            content_hash: content_hash.into(),
            synced_at: Utc::now(),
            remote_version: None,
            local_modified_at,
            size: 0,
        }
    }

    /// Set the remote version identifier
    pub fn with_remote_version(mut self, version: impl Into<String>) -> Self {
        self.remote_version = Some(version.into());
        self
    }

    /// Set the file size
    pub fn with_size(mut self, size: u64) -> Self {
        self.size = size;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_new() {
        let manifest = SyncManifest::new("s3:my-bucket");
        assert_eq!(manifest.version, SyncManifest::CURRENT_VERSION);
        assert_eq!(manifest.provider_id, "s3:my-bucket");
        assert!(manifest.files.is_empty());
        assert!(manifest.last_sync.is_none());
    }

    #[test]
    fn test_manifest_file_operations() {
        let mut manifest = SyncManifest::new("test");

        // Add a file
        let state = FileSyncState::new("notes/test.md", "abc123", 1000);
        manifest.set_file("notes/test.md", state);

        // Get the file
        let retrieved = manifest.get_file("notes/test.md").unwrap();
        assert_eq!(retrieved.content_hash, "abc123");
        assert_eq!(retrieved.local_modified_at, 1000);

        // Remove the file
        let removed = manifest.remove_file("notes/test.md");
        assert!(removed.is_some());
        assert!(manifest.get_file("notes/test.md").is_none());
    }

    #[test]
    fn test_needs_upload() {
        let mut manifest = SyncManifest::new("test");
        let state = FileSyncState::new("test.md", "hash123", 1000);
        manifest.set_file("test.md", state);

        // New file needs upload
        assert!(manifest.needs_upload("new.md", 500, "anything"));

        // Unmodified file doesn't need upload
        assert!(!manifest.needs_upload("test.md", 1000, "hash123"));

        // Modified timestamp needs upload
        assert!(manifest.needs_upload("test.md", 2000, "hash123"));

        // Changed content needs upload
        assert!(manifest.needs_upload("test.md", 1000, "different_hash"));
    }

    #[test]
    fn test_get_locally_deleted() {
        let mut manifest = SyncManifest::new("test");
        manifest.set_file("a.md", FileSyncState::new("a.md", "h1", 100));
        manifest.set_file("b.md", FileSyncState::new("b.md", "h2", 200));
        manifest.set_file("c.md", FileSyncState::new("c.md", "h3", 300));

        // Only a.md and c.md exist now
        let current = vec!["a.md".to_string(), "c.md".to_string()];
        let deleted = manifest.get_locally_deleted(&current);

        assert_eq!(deleted, vec!["b.md".to_string()]);
    }

    #[test]
    fn test_json_roundtrip() {
        let mut manifest = SyncManifest::new("s3:test-bucket");
        manifest.set_file(
            "notes/test.md",
            FileSyncState::new("notes/test.md", "abc123", 1000)
                .with_remote_version("etag-xyz")
                .with_size(256),
        );
        manifest.mark_synced();

        let json = manifest.to_json().unwrap();
        let parsed = SyncManifest::from_json(&json).unwrap();

        assert_eq!(parsed.provider_id, "s3:test-bucket");
        assert!(parsed.last_sync.is_some());
        let file = parsed.get_file("notes/test.md").unwrap();
        assert_eq!(file.content_hash, "abc123");
        assert_eq!(file.remote_version, Some("etag-xyz".to_string()));
    }
}
