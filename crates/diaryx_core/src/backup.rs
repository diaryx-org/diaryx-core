//! Backup system for persisting workspace data to various targets.
//!
//! This module provides abstractions for backing up workspace data to
//! configurable targets (local drive, cloud storage, etc.).

use crate::fs::FileSystem;
use std::path::{Path, PathBuf};
use std::time::Duration;

// ============================================================================
// Configuration Types
// ============================================================================

/// Configuration for error handling behavior during backup operations.
#[derive(Clone, Debug, Default)]
pub enum FailurePolicy {
    /// Log error and continue with other targets
    #[default]
    Continue,
    /// Retry N times with exponential backoff before continuing
    Retry(u32),
    /// Abort all backup operations on failure
    Abort,
}

/// Result of a backup or restore operation.
#[derive(Debug)]
pub struct BackupResult {
    /// Whether the operation completed successfully
    pub success: bool,
    /// Number of files processed
    pub files_processed: usize,
    /// Error message if the operation failed
    pub error: Option<String>,
}

impl BackupResult {
    /// Create a successful result
    pub fn success(files_processed: usize) -> Self {
        Self {
            success: true,
            files_processed,
            error: None,
        }
    }

    /// Create a failed result
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            success: false,
            files_processed: 0,
            error: Some(error.into()),
        }
    }
}

// ============================================================================
// BackupTarget Trait
// ============================================================================

/// Trait for backup targets (one-way persistence).
///
/// A `BackupTarget` represents a destination where workspace data can be
/// persisted for backup purposes. This is a one-way operation: data flows
/// from the working filesystem to the target.
///
/// For bidirectional sync, see `SyncTarget`.
pub trait BackupTarget: Send + Sync {
    /// Human-readable name for this target (e.g., "Local Backup", "Google Drive")
    fn name(&self) -> &str;

    /// How often this target should be backed up.
    ///
    /// The backup manager will use this to schedule automatic backups.
    fn frequency(&self) -> Duration;

    /// What to do when backup fails.
    fn failure_policy(&self) -> FailurePolicy;

    /// Persist all files from filesystem to this target.
    ///
    /// This should copy all relevant files from the source filesystem
    /// to the backup target.
    fn backup(&self, fs: &dyn FileSystem, workspace_path: &Path) -> BackupResult;

    /// Restore all files from this target into filesystem.
    ///
    /// This should copy all files from the backup target into the
    /// destination filesystem.
    fn restore(&self, fs: &dyn FileSystem, workspace_path: &Path) -> BackupResult;

    /// Check if this target is available/accessible.
    ///
    /// For example, a local drive target might check if the path exists,
    /// while a cloud target might ping the service.
    fn is_available(&self) -> bool;

    /// Get timestamp of last successful backup.
    ///
    /// Returns `None` if no backup has been performed yet.
    fn get_last_sync(&self) -> Option<std::time::SystemTime> {
        None // Default implementation
    }
}

// ============================================================================
// SyncTarget Trait (for bidirectional sync)
// ============================================================================

/// Result of a sync operation.
#[derive(Debug)]
pub struct SyncResult {
    /// Whether the operation completed successfully
    pub success: bool,
    /// Number of files pulled from remote
    pub files_pulled: usize,
    /// Number of files pushed to remote
    pub files_pushed: usize,
    /// List of conflicts that need resolution
    pub conflicts: Vec<Conflict>,
    /// Error message if the operation failed
    pub error: Option<String>,
}

impl SyncResult {
    /// Create a successful sync result
    pub fn success(files_pulled: usize, files_pushed: usize) -> Self {
        Self {
            success: true,
            files_pulled,
            files_pushed,
            conflicts: Vec::new(),
            error: None,
        }
    }

    /// Create a failed sync result
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            success: false,
            files_pulled: 0,
            files_pushed: 0,
            conflicts: Vec::new(),
            error: Some(error.into()),
        }
    }

    /// Create a result with conflicts
    pub fn with_conflicts(conflicts: Vec<Conflict>) -> Self {
        Self {
            success: false,
            files_pulled: 0,
            files_pushed: 0,
            conflicts,
            error: Some("Conflicts detected".to_string()),
        }
    }
}

/// A file conflict between local and remote versions.
#[derive(Debug, Clone)]
pub struct Conflict {
    /// Path to the conflicting file
    pub path: PathBuf,
    /// Local modification timestamp
    pub local_modified: std::time::SystemTime,
    /// Remote modification timestamp
    pub remote_modified: std::time::SystemTime,
}

/// How to resolve a conflict.
#[derive(Debug, Clone)]
pub enum Resolution {
    /// Keep the local version
    KeepLocal,
    /// Keep the remote version
    KeepRemote,
    /// Use merged content (for future CRDT support)
    Merge(String),
}

/// Trait for sync targets (bidirectional persistence).
///
/// Extends `BackupTarget` with conflict detection and resolution.
/// Used for cloud sync and multi-device scenarios.
pub trait SyncTarget: BackupTarget {
    /// Pull changes from remote, returning any conflicts.
    fn pull(&self, fs: &dyn FileSystem, workspace_path: &Path) -> SyncResult;

    /// Push local changes to remote.
    fn push(&self, fs: &dyn FileSystem, workspace_path: &Path) -> SyncResult;

    /// Resolve a conflict using the specified resolution strategy.
    fn resolve_conflict(
        &self,
        fs: &dyn FileSystem,
        workspace_path: &Path,
        conflict: &Conflict,
        resolution: Resolution,
    ) -> BackupResult;
}

// ============================================================================
// BackupManager
// ============================================================================

/// Manages multiple backup targets.
///
/// The `BackupManager` coordinates backups across multiple targets,
/// handling scheduling, error policies, and restore prioritization.
pub struct BackupManager {
    targets: Vec<Box<dyn BackupTarget>>,
    primary_index: Option<usize>,
}

impl Default for BackupManager {
    fn default() -> Self {
        Self::new()
    }
}

impl BackupManager {
    /// Create a new empty backup manager.
    pub fn new() -> Self {
        Self {
            targets: Vec::new(),
            primary_index: None,
        }
    }

    /// Add a backup target.
    ///
    /// The first target added becomes the primary by default.
    pub fn add_target(&mut self, target: Box<dyn BackupTarget>) {
        self.targets.push(target);
        if self.primary_index.is_none() {
            self.primary_index = Some(0);
        }
    }

    /// Set the primary target by name.
    ///
    /// The primary target is used for restore operations.
    /// Returns `true` if the target was found and set as primary.
    pub fn set_primary(&mut self, name: &str) -> bool {
        for (i, target) in self.targets.iter().enumerate() {
            if target.name() == name {
                self.primary_index = Some(i);
                return true;
            }
        }
        false
    }

    /// Get the primary target name.
    pub fn primary_name(&self) -> Option<&str> {
        self.primary_index
            .and_then(|i| self.targets.get(i))
            .map(|t| t.name())
    }

    /// Get all target names.
    pub fn target_names(&self) -> Vec<&str> {
        self.targets.iter().map(|t| t.name()).collect()
    }

    /// Backup to all available targets.
    ///
    /// Returns a result for each target, in the same order as added.
    pub fn backup_all(&self, fs: &dyn FileSystem, workspace_path: &Path) -> Vec<BackupResult> {
        let mut results = Vec::with_capacity(self.targets.len());

        for target in &self.targets {
            if !target.is_available() {
                results.push(BackupResult::failure(format!(
                    "Target '{}' is not available",
                    target.name()
                )));
                continue;
            }

            let result = target.backup(fs, workspace_path);

            // Handle failure policy
            if !result.success {
                match target.failure_policy() {
                    FailurePolicy::Abort => {
                        results.push(result);
                        break; // Stop processing further targets
                    }
                    FailurePolicy::Retry(max_retries) => {
                        // Simple retry logic (no exponential backoff in this MVP)
                        let mut final_result = result;
                        for _ in 0..max_retries {
                            final_result = target.backup(fs, workspace_path);
                            if final_result.success {
                                break;
                            }
                        }
                        results.push(final_result);
                    }
                    FailurePolicy::Continue => {
                        results.push(result);
                    }
                }
            } else {
                results.push(result);
            }
        }

        results
    }

    /// Restore from the primary target.
    ///
    /// Returns `None` if no primary target is set.
    pub fn restore_from_primary(
        &self,
        fs: &dyn FileSystem,
        workspace_path: &Path,
    ) -> Option<BackupResult> {
        let primary = self.primary_index.and_then(|i| self.targets.get(i))?;

        if !primary.is_available() {
            return Some(BackupResult::failure(format!(
                "Primary target '{}' is not available",
                primary.name()
            )));
        }

        Some(primary.restore(fs, workspace_path))
    }
}

// ============================================================================
// LocalDriveTarget - Native platforms only
// ============================================================================

/// Backup target that persists to a local directory.
///
/// This copies all workspace files to a specified backup directory.
#[cfg(not(target_arch = "wasm32"))]
pub struct LocalDriveTarget {
    /// Name of this target
    name: String,
    /// Path to the backup directory
    backup_path: PathBuf,
    /// How often to backup
    frequency: Duration,
    /// What to do on failure
    failure_policy: FailurePolicy,
}

#[cfg(not(target_arch = "wasm32"))]
impl LocalDriveTarget {
    /// Create a new local drive backup target.
    pub fn new(name: impl Into<String>, backup_path: PathBuf) -> Self {
        Self {
            name: name.into(),
            backup_path,
            frequency: Duration::from_secs(300), // 5 minutes default
            failure_policy: FailurePolicy::Continue,
        }
    }

    /// Set the backup frequency.
    pub fn with_frequency(mut self, frequency: Duration) -> Self {
        self.frequency = frequency;
        self
    }

    /// Set the failure policy.
    pub fn with_failure_policy(mut self, policy: FailurePolicy) -> Self {
        self.failure_policy = policy;
        self
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl BackupTarget for LocalDriveTarget {
    fn name(&self) -> &str {
        &self.name
    }

    fn frequency(&self) -> Duration {
        self.frequency
    }

    fn failure_policy(&self) -> FailurePolicy {
        self.failure_policy.clone()
    }

    fn backup(&self, fs: &dyn FileSystem, workspace_path: &Path) -> BackupResult {
        use std::fs as std_fs;

        // Ensure backup directory exists
        if let Err(e) = std_fs::create_dir_all(&self.backup_path) {
            return BackupResult::failure(format!("Failed to create backup directory: {}", e));
        }

        // Get all files in workspace
        let files = match fs.list_all_files_recursive(workspace_path) {
            Ok(files) => files,
            Err(e) => return BackupResult::failure(format!("Failed to list files: {}", e)),
        };

        let mut files_processed = 0;

        for file_path in files {
            // Skip directories
            if fs.is_dir(&file_path) {
                continue;
            }

            // Calculate relative path from workspace
            let relative = match file_path.strip_prefix(workspace_path) {
                Ok(rel) => rel,
                Err(_) => continue,
            };

            let dest_path = self.backup_path.join(relative);

            // Ensure parent directory exists
            if let Some(parent) = dest_path.parent() {
                if let Err(e) = std_fs::create_dir_all(parent) {
                    return BackupResult::failure(format!(
                        "Failed to create directory {:?}: {}",
                        parent, e
                    ));
                }
            }

            // Copy file content
            let content = match fs.read_to_string(&file_path) {
                Ok(content) => content,
                Err(_) => {
                    // Try binary read
                    match fs.read_binary(&file_path) {
                        Ok(bytes) => {
                            if let Err(e) = std_fs::write(&dest_path, &bytes) {
                                return BackupResult::failure(format!(
                                    "Failed to write binary file {:?}: {}",
                                    dest_path, e
                                ));
                            }
                            files_processed += 1;
                            continue;
                        }
                        Err(e) => {
                            return BackupResult::failure(format!(
                                "Failed to read file {:?}: {}",
                                file_path, e
                            ))
                        }
                    }
                }
            };

            if let Err(e) = std_fs::write(&dest_path, &content) {
                return BackupResult::failure(format!(
                    "Failed to write file {:?}: {}",
                    dest_path, e
                ));
            }

            files_processed += 1;
        }

        BackupResult::success(files_processed)
    }

    fn restore(&self, fs: &dyn FileSystem, workspace_path: &Path) -> BackupResult {
        use std::fs as std_fs;

        if !self.backup_path.exists() {
            return BackupResult::failure("Backup directory does not exist");
        }

        let mut files_processed = 0;

        // Walk the backup directory
        fn visit_dir(
            dir: &Path,
            backup_root: &Path,
            workspace_path: &Path,
            fs: &dyn FileSystem,
            files_processed: &mut usize,
        ) -> Result<(), String> {
            let entries = std_fs::read_dir(dir)
                .map_err(|e| format!("Failed to read directory {:?}: {}", dir, e))?;

            for entry in entries {
                let entry =
                    entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
                let path = entry.path();

                if path.is_dir() {
                    visit_dir(&path, backup_root, workspace_path, fs, files_processed)?;
                } else {
                    let relative = path
                        .strip_prefix(backup_root)
                        .map_err(|_| "Failed to calculate relative path")?;
                    let dest_path = workspace_path.join(relative);

                    // Ensure parent directory exists in target filesystem
                    if let Some(parent) = dest_path.parent() {
                        fs.create_dir_all(parent)
                            .map_err(|e| format!("Failed to create directory {:?}: {}", parent, e))?;
                    }

                    // Read and write file
                    let content = std_fs::read_to_string(&path)
                        .or_else(|_| {
                            std_fs::read(&path).map(|bytes| {
                                // For binary files, we'll handle separately
                                String::from_utf8_lossy(&bytes).into_owned()
                            })
                        })
                        .map_err(|e| format!("Failed to read file {:?}: {}", path, e))?;

                    fs.write_file(&dest_path, &content)
                        .map_err(|e| format!("Failed to write file {:?}: {}", dest_path, e))?;

                    *files_processed += 1;
                }
            }

            Ok(())
        }

        match visit_dir(
            &self.backup_path,
            &self.backup_path,
            workspace_path,
            fs,
            &mut files_processed,
        ) {
            Ok(()) => BackupResult::success(files_processed),
            Err(e) => BackupResult::failure(e),
        }
    }

    fn is_available(&self) -> bool {
        // Check if we can access the parent directory
        self.backup_path
            .parent()
            .map(|p| p.exists() || p.as_os_str().is_empty())
            .unwrap_or(true)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backup_result_success() {
        let result = BackupResult::success(10);
        assert!(result.success);
        assert_eq!(result.files_processed, 10);
        assert!(result.error.is_none());
    }

    #[test]
    fn test_backup_result_failure() {
        let result = BackupResult::failure("Something went wrong");
        assert!(!result.success);
        assert_eq!(result.files_processed, 0);
        assert_eq!(result.error, Some("Something went wrong".to_string()));
    }

    #[test]
    fn test_backup_manager_empty() {
        let manager = BackupManager::new();
        assert!(manager.target_names().is_empty());
        assert!(manager.primary_name().is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn test_local_drive_target_creation() {
        let target = LocalDriveTarget::new("Test Backup", PathBuf::from("/tmp/backup"))
            .with_frequency(Duration::from_secs(60))
            .with_failure_policy(FailurePolicy::Retry(3));

        assert_eq!(target.name(), "Test Backup");
        assert_eq!(target.frequency(), Duration::from_secs(60));
        matches!(target.failure_policy(), FailurePolicy::Retry(3));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn test_backup_manager_add_target() {
        let mut manager = BackupManager::new();
        let target = LocalDriveTarget::new("Test", PathBuf::from("/tmp/backup"));
        manager.add_target(Box::new(target));

        assert_eq!(manager.target_names(), vec!["Test"]);
        assert_eq!(manager.primary_name(), Some("Test"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn test_backup_manager_set_primary() {
        let mut manager = BackupManager::new();
        manager.add_target(Box::new(LocalDriveTarget::new(
            "First",
            PathBuf::from("/tmp/first"),
        )));
        manager.add_target(Box::new(LocalDriveTarget::new(
            "Second",
            PathBuf::from("/tmp/second"),
        )));

        assert_eq!(manager.primary_name(), Some("First"));
        assert!(manager.set_primary("Second"));
        assert_eq!(manager.primary_name(), Some("Second"));
        assert!(!manager.set_primary("NonExistent"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn test_backup_and_restore_integration() {
        use crate::fs::InMemoryFileSystem;
        use tempfile::tempdir;

        // Create a workspace with some files
        let fs = InMemoryFileSystem::new();
        let workspace = PathBuf::from("/workspace");
        fs.create_dir_all(&workspace).unwrap();
        fs.write_file(&workspace.join("test.md"), "# Hello World").unwrap();
        fs.write_file(&workspace.join("subdir/nested.md"), "Nested content").unwrap();

        // Create backup target pointing to temp directory
        let backup_dir = tempdir().unwrap();
        let target = LocalDriveTarget::new("Test Backup", backup_dir.path().to_path_buf());

        // Backup
        let result = target.backup(&fs, &workspace);
        assert!(result.success, "Backup failed: {:?}", result.error);
        assert_eq!(result.files_processed, 2);

        // Verify files exist in backup
        assert!(backup_dir.path().join("test.md").exists());
        assert!(backup_dir.path().join("subdir/nested.md").exists());

        // Create a fresh filesystem and restore into it
        let fs2 = InMemoryFileSystem::new();
        fs2.create_dir_all(&workspace).unwrap();
        
        let restore_result = target.restore(&fs2, &workspace);
        assert!(restore_result.success, "Restore failed: {:?}", restore_result.error);
        assert_eq!(restore_result.files_processed, 2);

        // Verify restored content
        let content = fs2.read_to_string(&workspace.join("test.md")).unwrap();
        assert_eq!(content, "# Hello World");
        
        let nested = fs2.read_to_string(&workspace.join("subdir/nested.md")).unwrap();
        assert_eq!(nested, "Nested content");
    }
}
