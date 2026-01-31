//! Conflict detection and resolution for sync operations.
//!
//! When both local and remote modify the same file since the last sync,
//! a conflict is detected. This module provides types for representing
//! and resolving these conflicts.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Information about a sync conflict.
///
/// A conflict occurs when the same file was modified on both local and remote
/// since the last successful sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictInfo {
    /// Path to the conflicting file
    pub path: String,

    /// Local modification timestamp (if available)
    pub local_modified_at: Option<i64>,

    /// Remote modification timestamp (if available)
    pub remote_modified_at: Option<DateTime<Utc>>,

    /// Local content hash (if available)
    pub local_hash: Option<String>,

    /// Remote content hash (if available)
    pub remote_hash: Option<String>,
}

impl ConflictInfo {
    /// Check if the content is actually different (hashes don't match).
    ///
    /// If both hashes are available and equal, the files have the same content
    /// despite being modified independently, and the conflict can be auto-resolved.
    pub fn is_content_different(&self) -> bool {
        match (&self.local_hash, &self.remote_hash) {
            (Some(local), Some(remote)) => local != remote,
            // If we can't compare hashes, assume different
            _ => true,
        }
    }

    /// Create a conflict file name for this path.
    ///
    /// For example: `notes/test.md` -> `notes/test.conflict.md`
    pub fn conflict_file_name(&self) -> String {
        if let Some(dot_pos) = self.path.rfind('.') {
            format!(
                "{}.conflict{}",
                &self.path[..dot_pos],
                &self.path[dot_pos..]
            )
        } else {
            format!("{}.conflict", self.path)
        }
    }
}

/// How to resolve a conflict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConflictResolution {
    /// Keep the local version, overwrite remote
    KeepLocal,

    /// Keep the remote version, overwrite local
    KeepRemote,

    /// Merge the content (provide merged content)
    Merge {
        /// The merged content
        content: String,
    },

    /// Keep both versions by creating a conflict file
    KeepBoth,

    /// Skip this conflict (do nothing, will resurface on next sync)
    Skip,
}

impl FromStr for ConflictResolution {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "local" | "keep_local" | "keep-local" => Ok(ConflictResolution::KeepLocal),
            "remote" | "keep_remote" | "keep-remote" => Ok(ConflictResolution::KeepRemote),
            "both" | "keep_both" | "keep-both" => Ok(ConflictResolution::KeepBoth),
            "skip" => Ok(ConflictResolution::Skip),
            _ => Err(()),
        }
    }
}

impl ConflictResolution {
    /// Check if this resolution keeps the local version
    pub fn keeps_local(&self) -> bool {
        matches!(
            self,
            ConflictResolution::KeepLocal
                | ConflictResolution::KeepBoth
                | ConflictResolution::Merge { .. }
        )
    }

    /// Check if this resolution keeps the remote version
    pub fn keeps_remote(&self) -> bool {
        matches!(
            self,
            ConflictResolution::KeepRemote | ConflictResolution::KeepBoth
        )
    }
}

/// Result of applying a conflict resolution.
#[derive(Debug)]
pub struct ConflictResolutionResult {
    /// Whether resolution was successful
    pub success: bool,

    /// Path to the resolved file
    pub path: String,

    /// Path to conflict file if KeepBoth was used
    pub conflict_file_path: Option<String>,

    /// Error message if resolution failed
    pub error: Option<String>,
}

impl ConflictResolutionResult {
    /// Create a successful resolution result
    pub fn success(path: impl Into<String>) -> Self {
        Self {
            success: true,
            path: path.into(),
            conflict_file_path: None,
            error: None,
        }
    }

    /// Create a successful result with conflict file
    pub fn success_with_conflict_file(
        path: impl Into<String>,
        conflict_path: impl Into<String>,
    ) -> Self {
        Self {
            success: true,
            path: path.into(),
            conflict_file_path: Some(conflict_path.into()),
            error: None,
        }
    }

    /// Create a failed resolution result
    pub fn failure(path: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            success: false,
            path: path.into(),
            conflict_file_path: None,
            error: Some(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conflict_info_content_different() {
        // Same hashes - not different
        let conflict = ConflictInfo {
            path: "test.md".to_string(),
            local_modified_at: Some(1000),
            remote_modified_at: None,
            local_hash: Some("abc123".to_string()),
            remote_hash: Some("abc123".to_string()),
        };
        assert!(!conflict.is_content_different());

        // Different hashes - different
        let conflict = ConflictInfo {
            path: "test.md".to_string(),
            local_modified_at: Some(1000),
            remote_modified_at: None,
            local_hash: Some("abc123".to_string()),
            remote_hash: Some("xyz789".to_string()),
        };
        assert!(conflict.is_content_different());

        // Missing hash - assume different
        let conflict = ConflictInfo {
            path: "test.md".to_string(),
            local_modified_at: Some(1000),
            remote_modified_at: None,
            local_hash: Some("abc123".to_string()),
            remote_hash: None,
        };
        assert!(conflict.is_content_different());
    }

    #[test]
    fn test_conflict_file_name() {
        let conflict = ConflictInfo {
            path: "notes/test.md".to_string(),
            local_modified_at: None,
            remote_modified_at: None,
            local_hash: None,
            remote_hash: None,
        };
        assert_eq!(conflict.conflict_file_name(), "notes/test.conflict.md");

        let conflict = ConflictInfo {
            path: "README".to_string(),
            local_modified_at: None,
            remote_modified_at: None,
            local_hash: None,
            remote_hash: None,
        };
        assert_eq!(conflict.conflict_file_name(), "README.conflict");
    }

    #[test]
    fn test_conflict_resolution_from_str() {
        assert!(matches!(
            ConflictResolution::from_str("local"),
            Ok(ConflictResolution::KeepLocal)
        ));
        assert!(matches!(
            ConflictResolution::from_str("keep-remote"),
            Ok(ConflictResolution::KeepRemote)
        ));
        assert!(matches!(
            ConflictResolution::from_str("BOTH"),
            Ok(ConflictResolution::KeepBoth)
        ));
        assert!(matches!(
            ConflictResolution::from_str("skip"),
            Ok(ConflictResolution::Skip)
        ));
        assert!(ConflictResolution::from_str("invalid").is_err());
    }

    #[test]
    fn test_resolution_keeps_versions() {
        assert!(ConflictResolution::KeepLocal.keeps_local());
        assert!(!ConflictResolution::KeepLocal.keeps_remote());

        assert!(!ConflictResolution::KeepRemote.keeps_local());
        assert!(ConflictResolution::KeepRemote.keeps_remote());

        assert!(ConflictResolution::KeepBoth.keeps_local());
        assert!(ConflictResolution::KeepBoth.keeps_remote());

        let merge = ConflictResolution::Merge {
            content: "merged".to_string(),
        };
        assert!(merge.keeps_local());
        assert!(!merge.keeps_remote());
    }
}
