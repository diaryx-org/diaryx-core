//! Change detection types for sync operations.
//!
//! This module defines the types used to represent local and remote changes,
//! and the actions that need to be taken during sync.

use super::RemoteFileInfo;
use super::conflict::ConflictInfo;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A change detected in the local workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LocalChange {
    /// A new file was created locally
    Created {
        /// Path to the file
        path: String,
        /// Content hash
        content_hash: String,
        /// Modification timestamp
        modified_at: i64,
    },
    /// An existing file was modified locally
    Modified {
        /// Path to the file
        path: String,
        /// New content hash
        content_hash: String,
        /// Modification timestamp
        modified_at: i64,
        /// Previous content hash from manifest
        previous_hash: String,
    },
    /// A file was deleted locally
    Deleted {
        /// Path to the deleted file
        path: String,
    },
}

impl LocalChange {
    /// Get the path of the changed file
    pub fn path(&self) -> &str {
        match self {
            LocalChange::Created { path, .. } => path,
            LocalChange::Modified { path, .. } => path,
            LocalChange::Deleted { path } => path,
        }
    }

    /// Get the content hash if available
    pub fn content_hash(&self) -> Option<&str> {
        match self {
            LocalChange::Created { content_hash, .. } => Some(content_hash),
            LocalChange::Modified { content_hash, .. } => Some(content_hash),
            LocalChange::Deleted { .. } => None,
        }
    }

    /// Get the modification timestamp if available
    pub fn modified_at(&self) -> Option<i64> {
        match self {
            LocalChange::Created { modified_at, .. } => Some(*modified_at),
            LocalChange::Modified { modified_at, .. } => Some(*modified_at),
            LocalChange::Deleted { .. } => None,
        }
    }
}

/// A change detected in remote storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RemoteChange {
    /// A new file was created remotely
    Created {
        /// Remote file information
        info: RemoteFileInfo,
    },
    /// An existing file was modified remotely
    Modified {
        /// Updated remote file information
        info: RemoteFileInfo,
        /// Previous version identifier
        previous_version: Option<String>,
    },
    /// A file was deleted remotely
    Deleted {
        /// Path to the deleted file
        path: String,
    },
}

impl RemoteChange {
    /// Get the path of the changed file
    pub fn path(&self) -> &str {
        match self {
            RemoteChange::Created { info } => &info.path,
            RemoteChange::Modified { info, .. } => &info.path,
            RemoteChange::Deleted { path } => path,
        }
    }

    /// Get the modification timestamp if available
    pub fn modified_at(&self) -> Option<DateTime<Utc>> {
        match self {
            RemoteChange::Created { info } => Some(info.modified_at),
            RemoteChange::Modified { info, .. } => Some(info.modified_at),
            RemoteChange::Deleted { .. } => None,
        }
    }

    /// Get the content hash if available
    pub fn content_hash(&self) -> Option<&str> {
        match self {
            RemoteChange::Created { info } => info.content_hash.as_deref(),
            RemoteChange::Modified { info, .. } => info.content_hash.as_deref(),
            RemoteChange::Deleted { .. } => None,
        }
    }
}

/// Direction of sync operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncDirection {
    /// From local to remote (upload)
    Upload,
    /// From remote to local (download)
    Download,
}

/// An action to be taken during sync.
#[derive(Debug, Clone)]
pub enum SyncAction {
    /// Upload a file to remote storage
    Upload {
        /// Path to upload
        path: String,
    },
    /// Download a file from remote storage
    Download {
        /// Path to download
        path: String,
        /// Remote file info
        remote_info: RemoteFileInfo,
    },
    /// Delete a file
    Delete {
        /// Path to delete
        path: String,
        /// Where to delete from
        direction: SyncDirection,
    },
    /// A conflict that needs resolution
    Conflict {
        /// Conflict information
        info: ConflictInfo,
    },
}

impl SyncAction {
    /// Get the path this action affects
    pub fn path(&self) -> &str {
        match self {
            SyncAction::Upload { path } => path,
            SyncAction::Download { path, .. } => path,
            SyncAction::Delete { path, .. } => path,
            SyncAction::Conflict { info } => &info.path,
        }
    }

    /// Check if this is an upload action
    pub fn is_upload(&self) -> bool {
        matches!(self, SyncAction::Upload { .. })
    }

    /// Check if this is a download action
    pub fn is_download(&self) -> bool {
        matches!(self, SyncAction::Download { .. })
    }

    /// Check if this is a conflict
    pub fn is_conflict(&self) -> bool {
        matches!(self, SyncAction::Conflict { .. })
    }
}

/// Detect conflicts between local and remote changes.
///
/// A conflict occurs when the same file was modified on both sides since the last sync.
pub fn detect_conflicts(
    local_changes: &[LocalChange],
    remote_changes: &[RemoteChange],
) -> Vec<ConflictInfo> {
    let mut conflicts = Vec::new();

    for local in local_changes {
        // Skip deletions for conflict detection
        if matches!(local, LocalChange::Deleted { .. }) {
            continue;
        }

        for remote in remote_changes {
            // Skip deletions for conflict detection
            if matches!(remote, RemoteChange::Deleted { .. }) {
                continue;
            }

            if local.path() == remote.path() {
                // Both sides modified the same file
                conflicts.push(ConflictInfo {
                    path: local.path().to_string(),
                    local_modified_at: local.modified_at(),
                    remote_modified_at: remote.modified_at(),
                    local_hash: local.content_hash().map(String::from),
                    remote_hash: remote.content_hash().map(String::from),
                });
            }
        }
    }

    conflicts
}

/// Compute sync actions from local and remote changes.
///
/// This function determines what operations need to be performed to sync,
/// handling conflicts appropriately.
pub fn compute_sync_actions(
    local_changes: &[LocalChange],
    remote_changes: &[RemoteChange],
) -> Vec<SyncAction> {
    let conflicts = detect_conflicts(local_changes, remote_changes);
    let conflict_paths: std::collections::HashSet<String> =
        conflicts.iter().map(|c| c.path.clone()).collect();

    let mut actions = Vec::new();

    // Add conflict actions first
    for conflict in conflicts {
        actions.push(SyncAction::Conflict { info: conflict });
    }

    // Process local changes (excluding conflicts)
    for change in local_changes {
        if conflict_paths.contains(change.path()) {
            continue;
        }

        match change {
            LocalChange::Created { path, .. } | LocalChange::Modified { path, .. } => {
                actions.push(SyncAction::Upload { path: path.clone() });
            }
            LocalChange::Deleted { path } => {
                actions.push(SyncAction::Delete {
                    path: path.clone(),
                    direction: SyncDirection::Upload, // Delete from remote
                });
            }
        }
    }

    // Process remote changes (excluding conflicts)
    for change in remote_changes {
        if conflict_paths.contains(change.path()) {
            continue;
        }

        match change {
            RemoteChange::Created { info } | RemoteChange::Modified { info, .. } => {
                actions.push(SyncAction::Download {
                    path: info.path.clone(),
                    remote_info: info.clone(),
                });
            }
            RemoteChange::Deleted { path } => {
                actions.push(SyncAction::Delete {
                    path: path.clone(),
                    direction: SyncDirection::Download, // Delete from local
                });
            }
        }
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_remote_info(path: &str) -> RemoteFileInfo {
        RemoteFileInfo {
            path: path.to_string(),
            size: 100,
            modified_at: Utc::now(),
            etag: None,
            content_hash: Some("remote_hash".to_string()),
        }
    }

    #[test]
    fn test_detect_no_conflicts() {
        let local = vec![LocalChange::Created {
            path: "a.md".to_string(),
            content_hash: "hash_a".to_string(),
            modified_at: 1000,
        }];
        let remote = vec![RemoteChange::Created {
            info: make_remote_info("b.md"),
        }];

        let conflicts = detect_conflicts(&local, &remote);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_detect_conflict() {
        let local = vec![LocalChange::Modified {
            path: "shared.md".to_string(),
            content_hash: "local_hash".to_string(),
            modified_at: 2000,
            previous_hash: "old_hash".to_string(),
        }];
        let remote = vec![RemoteChange::Modified {
            info: make_remote_info("shared.md"),
            previous_version: None,
        }];

        let conflicts = detect_conflicts(&local, &remote);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].path, "shared.md");
    }

    #[test]
    fn test_compute_sync_actions() {
        let local = vec![
            LocalChange::Created {
                path: "new_local.md".to_string(),
                content_hash: "h1".to_string(),
                modified_at: 1000,
            },
            LocalChange::Deleted {
                path: "deleted_local.md".to_string(),
            },
        ];
        let remote = vec![RemoteChange::Created {
            info: make_remote_info("new_remote.md"),
        }];

        let actions = compute_sync_actions(&local, &remote);

        assert_eq!(actions.len(), 3);

        // Should have: 1 upload, 1 delete (remote), 1 download
        let uploads: Vec<_> = actions.iter().filter(|a| a.is_upload()).collect();
        let downloads: Vec<_> = actions.iter().filter(|a| a.is_download()).collect();
        let deletes: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, SyncAction::Delete { .. }))
            .collect();

        assert_eq!(uploads.len(), 1);
        assert_eq!(downloads.len(), 1);
        assert_eq!(deletes.len(), 1);
    }

    #[test]
    fn test_conflict_excludes_from_actions() {
        let local = vec![LocalChange::Modified {
            path: "conflict.md".to_string(),
            content_hash: "local".to_string(),
            modified_at: 2000,
            previous_hash: "old".to_string(),
        }];
        let remote = vec![RemoteChange::Modified {
            info: make_remote_info("conflict.md"),
            previous_version: None,
        }];

        let actions = compute_sync_actions(&local, &remote);

        // Should only have conflict action, no upload/download for the conflicting file
        assert_eq!(actions.len(), 1);
        assert!(actions[0].is_conflict());
    }
}
