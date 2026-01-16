//! Cloud sync module for bidirectional file synchronization.
//!
//! This module provides abstractions for syncing workspace files with cloud storage
//! providers (S3, Google Drive) while integrating with the CRDT system for conflict
//! resolution and real-time updates.
//!
//! # Architecture
//!
//! ```text
//! Cloud Storage (S3/GDrive)
//!         ↑↓
//!    SyncEngine (file-level bidirectional sync)
//!         ↑↓
//!    WorkspaceCrdt + BodyDocManager (CRDT layer)
//!         ↑↓
//!    AsyncFileSystem
//! ```
//!
//! # Key Components
//!
//! - [`SyncManifest`] - Tracks sync state per file (hashes, timestamps, versions)
//! - [`SyncEngine`] - Orchestrates the sync process
//! - [`LocalChange`] / [`RemoteChange`] - Represents detected changes
//! - [`Conflict`] / [`ConflictResolution`] - Handles conflicts between local and remote

mod change;
/// Conflict detection and resolution types
pub mod conflict;
/// Sync engine orchestrator
pub mod engine;
/// Sync manifest for tracking file state
pub mod manifest;

pub use change::{LocalChange, RemoteChange, SyncAction, SyncDirection};
pub use conflict::{ConflictInfo, ConflictResolution};
pub use engine::{CloudSyncProvider, SyncEngine};
pub use manifest::{FileSyncState, SyncManifest};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Information about a file in remote storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteFileInfo {
    /// Path relative to the sync root
    pub path: String,
    /// File size in bytes
    pub size: u64,
    /// Last modification time
    pub modified_at: DateTime<Utc>,
    /// Provider-specific version identifier (S3 ETag, GDrive revision ID)
    pub etag: Option<String>,
    /// Content hash if provided by the provider
    pub content_hash: Option<String>,
}

/// Result of a sync operation.
#[derive(Debug)]
pub struct CloudSyncResult {
    /// Whether the operation completed successfully
    pub success: bool,
    /// Number of files uploaded to remote
    pub files_uploaded: usize,
    /// Number of files downloaded from remote
    pub files_downloaded: usize,
    /// Number of files deleted
    pub files_deleted: usize,
    /// Conflicts that need user resolution
    pub conflicts: Vec<ConflictInfo>,
    /// Error message if the operation failed
    pub error: Option<String>,
}

impl CloudSyncResult {
    /// Create a successful result
    pub fn success(uploaded: usize, downloaded: usize, deleted: usize) -> Self {
        Self {
            success: true,
            files_uploaded: uploaded,
            files_downloaded: downloaded,
            files_deleted: deleted,
            conflicts: Vec::new(),
            error: None,
        }
    }

    /// Create a failed result
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            success: false,
            files_uploaded: 0,
            files_downloaded: 0,
            files_deleted: 0,
            conflicts: Vec::new(),
            error: Some(error.into()),
        }
    }

    /// Create a result with conflicts
    pub fn with_conflicts(conflicts: Vec<ConflictInfo>) -> Self {
        Self {
            success: false,
            files_uploaded: 0,
            files_downloaded: 0,
            files_deleted: 0,
            conflicts,
            error: Some("Conflicts detected - user resolution required".to_string()),
        }
    }
}

/// Progress information for sync operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncProgress {
    /// Current stage of the sync operation
    pub stage: SyncStage,
    /// Current item being processed (1-indexed)
    pub current: usize,
    /// Total items to process in this stage
    pub total: usize,
    /// Overall percentage complete (0-100)
    pub percent: u8,
    /// Optional detail message (e.g., filename being processed)
    pub message: Option<String>,
}

/// Stages of a sync operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncStage {
    /// Scanning local files for changes
    DetectingLocal,
    /// Fetching remote file list
    DetectingRemote,
    /// Uploading files to remote
    Uploading,
    /// Downloading files from remote
    Downloading,
    /// Deleting files
    Deleting,
    /// Sync complete
    Complete,
    /// Sync failed
    Error,
}

impl SyncStage {
    /// Get a human-readable description of this stage
    pub fn description(&self) -> &'static str {
        match self {
            SyncStage::DetectingLocal => "Scanning local files...",
            SyncStage::DetectingRemote => "Fetching remote files...",
            SyncStage::Uploading => "Uploading...",
            SyncStage::Downloading => "Downloading...",
            SyncStage::Deleting => "Cleaning up...",
            SyncStage::Complete => "Sync complete!",
            SyncStage::Error => "Sync failed",
        }
    }
}

/// Compute SHA-256 hash of content.
pub fn compute_content_hash(content: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Use a simple hash for now; can upgrade to SHA-256 when sha2 crate is added
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_content_hash() {
        let hash1 = compute_content_hash(b"hello world");
        let hash2 = compute_content_hash(b"hello world");
        let hash3 = compute_content_hash(b"different content");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_cloud_sync_result_success() {
        let result = CloudSyncResult::success(5, 3, 1);
        assert!(result.success);
        assert_eq!(result.files_uploaded, 5);
        assert_eq!(result.files_downloaded, 3);
        assert_eq!(result.files_deleted, 1);
        assert!(result.error.is_none());
    }

    #[test]
    fn test_cloud_sync_result_failure() {
        let result = CloudSyncResult::failure("Network error");
        assert!(!result.success);
        assert_eq!(result.error, Some("Network error".to_string()));
    }
}
