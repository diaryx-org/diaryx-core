//! Sync engine orchestrator.
//!
//! The SyncEngine coordinates bidirectional sync operations between the local
//! workspace and a cloud storage provider.

use super::change::{LocalChange, RemoteChange, SyncAction, compute_sync_actions};
use super::conflict::{ConflictInfo, ConflictResolution, ConflictResolutionResult};
use super::manifest::{FileSyncState, SyncManifest};
use super::{CloudSyncResult, RemoteFileInfo, SyncProgress, SyncStage, compute_content_hash};
use crate::fs::{AsyncFileSystem, BoxFuture};
use std::path::{Path, PathBuf};

/// Type alias for sync progress callback.
/// Parameters: progress info
pub type SyncProgressCallback<'a> = &'a (dyn Fn(SyncProgress) + Send + Sync);

/// Trait for cloud storage providers that support file-level sync.
///
/// This extends the basic sync capabilities with cloud-specific operations
/// needed for bidirectional synchronization.
pub trait CloudSyncProvider: Send + Sync {
    /// Human-readable name for this provider
    fn name(&self) -> &str;

    /// Unique identifier for this provider instance (e.g., "s3:bucket-name")
    fn provider_id(&self) -> String;

    /// List all files in remote storage
    fn list_remote_files(&self) -> BoxFuture<'_, Result<Vec<RemoteFileInfo>, String>>;

    /// Download a single file from remote storage
    fn download_file(&self, path: &str)
    -> BoxFuture<'_, Result<(Vec<u8>, RemoteFileInfo), String>>;

    /// Upload a single file to remote storage
    fn upload_file(
        &self,
        path: &str,
        content: &[u8],
    ) -> BoxFuture<'_, Result<RemoteFileInfo, String>>;

    /// Delete a file from remote storage
    fn delete_remote_file(&self, path: &str) -> BoxFuture<'_, Result<(), String>>;

    /// Check if the provider is available/connected
    fn is_available(&self) -> bool;
}

/// The sync engine orchestrates bidirectional sync operations.
pub struct SyncEngine<P: CloudSyncProvider> {
    provider: P,
    manifest: SyncManifest,
    manifest_path: PathBuf,
}

impl<P: CloudSyncProvider> SyncEngine<P> {
    /// Create a new sync engine with an empty manifest.
    pub fn new(provider: P, manifest_path: impl Into<PathBuf>) -> Self {
        let manifest = SyncManifest::new(provider.provider_id());
        Self {
            provider,
            manifest,
            manifest_path: manifest_path.into(),
        }
    }

    /// Create a sync engine with an existing manifest.
    pub fn with_manifest(
        provider: P,
        manifest: SyncManifest,
        manifest_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            provider,
            manifest,
            manifest_path: manifest_path.into(),
        }
    }

    /// Load the manifest from the filesystem.
    pub async fn load_manifest(&mut self, fs: &dyn AsyncFileSystem) -> Result<(), String> {
        match SyncManifest::load_from_file(fs, &self.manifest_path).await {
            Ok(manifest) => {
                self.manifest = manifest;
                Ok(())
            }
            Err(e) => {
                // If manifest doesn't exist, that's OK - start fresh
                if e.contains("not found") || e.contains("No such file") {
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Save the manifest to the filesystem.
    pub async fn save_manifest(&self, fs: &dyn AsyncFileSystem) -> Result<(), String> {
        // Ensure parent directory exists
        if let Some(parent) = self.manifest_path.parent() {
            fs.create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create manifest directory: {}", e))?;
        }
        self.manifest.save_to_file(fs, &self.manifest_path).await
    }

    /// Get a reference to the current manifest.
    pub fn manifest(&self) -> &SyncManifest {
        &self.manifest
    }

    /// Get the provider ID.
    pub fn provider_id(&self) -> String {
        self.provider.provider_id()
    }

    /// Check if the provider is available.
    pub fn is_available(&self) -> bool {
        self.provider.is_available()
    }

    /// Detect local changes since the last sync.
    pub async fn detect_local_changes(
        &self,
        fs: &dyn AsyncFileSystem,
        workspace_path: &Path,
    ) -> Result<Vec<LocalChange>, String> {
        let mut changes = Vec::new();

        // List all markdown files in workspace
        let files = fs
            .list_all_files_recursive(workspace_path)
            .await
            .map_err(|e| format!("Failed to list files: {}", e))?;

        let mut current_paths = Vec::new();

        for file_path in files {
            // Skip directories
            if fs.is_dir(&file_path).await {
                continue;
            }

            // Get relative path from workspace
            let relative_path = match file_path.strip_prefix(workspace_path) {
                Ok(rel) => rel.to_string_lossy().to_string(),
                Err(_) => continue,
            };

            // Skip hidden files and folders (anything with a component starting with '.')
            if relative_path
                .split('/')
                .any(|component| component.starts_with('.'))
            {
                continue;
            }

            // Sync markdown files and files in _attachments folders
            let is_markdown = relative_path.ends_with(".md");
            let is_attachment = relative_path
                .split('/')
                .any(|component| component == "_attachments");

            if !is_markdown && !is_attachment {
                continue;
            }

            current_paths.push(relative_path.clone());

            // Read file content and compute hash
            // Use binary read for attachments, string read for markdown
            let content_bytes = if is_attachment {
                match fs.read_binary(&file_path).await {
                    Ok(bytes) => bytes,
                    Err(_) => continue,
                }
            } else {
                match fs.read_to_string(&file_path).await {
                    Ok(c) => c.into_bytes(),
                    Err(_) => continue,
                }
            };

            let content_hash = compute_content_hash(&content_bytes);

            // Get file modification time (use current time as fallback)
            let modified_at = chrono::Utc::now().timestamp();

            // Check against manifest
            match self.manifest.get_file(&relative_path) {
                None => {
                    // New file
                    changes.push(LocalChange::Created {
                        path: relative_path,
                        content_hash,
                        modified_at,
                    });
                }
                Some(state) => {
                    // Check if modified
                    if state.content_hash != content_hash {
                        changes.push(LocalChange::Modified {
                            path: relative_path,
                            content_hash,
                            modified_at,
                            previous_hash: state.content_hash.clone(),
                        });
                    }
                }
            }
        }

        // Check for deletions
        for path in self.manifest.get_locally_deleted(&current_paths) {
            changes.push(LocalChange::Deleted { path });
        }

        Ok(changes)
    }

    /// Detect remote changes since the last sync.
    pub async fn detect_remote_changes(&self) -> Result<Vec<RemoteChange>, String> {
        let mut changes = Vec::new();

        let remote_files = self.provider.list_remote_files().await?;
        let mut remote_paths: std::collections::HashSet<String> = std::collections::HashSet::new();

        for info in remote_files {
            // Sync markdown files and files in _attachments folders
            let is_markdown = info.path.ends_with(".md");
            let is_attachment = info.path.split('/').any(|c| c == "_attachments");

            if !is_markdown && !is_attachment {
                continue;
            }

            remote_paths.insert(info.path.clone());

            match self.manifest.get_file(&info.path) {
                None => {
                    // New remote file
                    changes.push(RemoteChange::Created { info });
                }
                Some(state) => {
                    // Check if modified (compare by etag or modified time)
                    let is_modified = match (&info.etag, &state.remote_version) {
                        (Some(new_etag), Some(old_etag)) => new_etag != old_etag,
                        _ => {
                            // Fall back to timestamp comparison
                            info.modified_at.timestamp() > state.synced_at.timestamp()
                        }
                    };

                    if is_modified {
                        changes.push(RemoteChange::Modified {
                            info,
                            previous_version: state.remote_version.clone(),
                        });
                    }
                }
            }
        }

        // Check for remote deletions
        for (path, _) in &self.manifest.files {
            if !remote_paths.contains(path) {
                changes.push(RemoteChange::Deleted { path: path.clone() });
            }
        }

        Ok(changes)
    }

    /// Perform a full bidirectional sync.
    pub async fn sync(
        &mut self,
        fs: &dyn AsyncFileSystem,
        workspace_path: &Path,
    ) -> CloudSyncResult {
        // Note: We don't check is_available() here because it may use blocking I/O
        // which would panic in an async context. Instead, we let the actual operations
        // fail with descriptive errors if the provider isn't reachable.

        // Detect changes on both sides
        let local_changes = match self.detect_local_changes(fs, workspace_path).await {
            Ok(changes) => changes,
            Err(e) => {
                return CloudSyncResult::failure(format!("Failed to detect local changes: {}", e));
            }
        };

        let remote_changes = match self.detect_remote_changes().await {
            Ok(changes) => changes,
            Err(e) => {
                return CloudSyncResult::failure(format!("Failed to detect remote changes: {}", e));
            }
        };

        // Compute sync actions
        let actions = compute_sync_actions(&local_changes, &remote_changes);

        // Check for conflicts first
        let conflicts: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                SyncAction::Conflict { info } => Some(info.clone()),
                _ => None,
            })
            .collect();

        if !conflicts.is_empty() {
            return CloudSyncResult::with_conflicts(conflicts);
        }

        // Execute non-conflict actions
        let mut uploaded = 0;
        let mut downloaded = 0;
        let mut deleted = 0;

        for action in actions {
            match action {
                SyncAction::Upload { path } => {
                    match self.upload_file(fs, workspace_path, &path).await {
                        Ok(_) => uploaded += 1,
                        Err(e) => {
                            return CloudSyncResult::failure(format!(
                                "Failed to upload {}: {}",
                                path, e
                            ));
                        }
                    }
                }
                SyncAction::Download { path, remote_info } => {
                    match self
                        .download_file(fs, workspace_path, &path, &remote_info)
                        .await
                    {
                        Ok(_) => downloaded += 1,
                        Err(e) => {
                            return CloudSyncResult::failure(format!(
                                "Failed to download {}: {}",
                                path, e
                            ));
                        }
                    }
                }
                SyncAction::Delete { path, direction } => {
                    match direction {
                        super::change::SyncDirection::Upload => {
                            // Delete from remote
                            if let Err(e) = self.provider.delete_remote_file(&path).await {
                                return CloudSyncResult::failure(format!(
                                    "Failed to delete {} from remote: {}",
                                    path, e
                                ));
                            }
                        }
                        super::change::SyncDirection::Download => {
                            // Delete from local
                            let full_path = workspace_path.join(&path);
                            if let Err(e) = fs.delete_file(&full_path).await {
                                return CloudSyncResult::failure(format!(
                                    "Failed to delete {} locally: {}",
                                    path, e
                                ));
                            }
                        }
                    }
                    self.manifest.remove_file(&path);
                    deleted += 1;
                }
                SyncAction::Conflict { .. } => {
                    // Already handled above
                }
            }
        }

        // Mark sync complete and save manifest
        self.manifest.mark_synced();
        if let Err(e) = self.save_manifest(fs).await {
            return CloudSyncResult::failure(format!("Failed to save manifest: {}", e));
        }

        CloudSyncResult::success(uploaded, downloaded, deleted)
    }

    /// Perform a full bidirectional sync with progress reporting.
    pub async fn sync_with_progress<F>(
        &mut self,
        fs: &dyn AsyncFileSystem,
        workspace_path: &Path,
        on_progress: F,
    ) -> CloudSyncResult
    where
        F: Fn(SyncProgress) + Send + Sync,
    {
        // Note: We don't check is_available() here because it may use blocking I/O
        // which would panic in an async context. Instead, we let the actual operations
        // fail with descriptive errors if the provider isn't reachable.

        // Stage 1: Detect local changes (0-20%)
        on_progress(SyncProgress {
            stage: SyncStage::DetectingLocal,
            current: 0,
            total: 0,
            percent: 5,
            message: Some("Scanning local files...".to_string()),
        });

        let local_changes = match self.detect_local_changes(fs, workspace_path).await {
            Ok(changes) => changes,
            Err(e) => {
                on_progress(SyncProgress {
                    stage: SyncStage::Error,
                    current: 0,
                    total: 0,
                    percent: 0,
                    message: Some(format!("Failed to detect local changes: {}", e)),
                });
                return CloudSyncResult::failure(format!("Failed to detect local changes: {}", e));
            }
        };

        on_progress(SyncProgress {
            stage: SyncStage::DetectingLocal,
            current: local_changes.len(),
            total: local_changes.len(),
            percent: 15,
            message: Some(format!("Found {} local changes", local_changes.len())),
        });

        // Stage 2: Detect remote changes (20-40%)
        on_progress(SyncProgress {
            stage: SyncStage::DetectingRemote,
            current: 0,
            total: 0,
            percent: 20,
            message: Some("Fetching remote files...".to_string()),
        });

        let remote_changes = match self.detect_remote_changes().await {
            Ok(changes) => changes,
            Err(e) => {
                on_progress(SyncProgress {
                    stage: SyncStage::Error,
                    current: 0,
                    total: 0,
                    percent: 0,
                    message: Some(format!("Failed to detect remote changes: {}", e)),
                });
                return CloudSyncResult::failure(format!("Failed to detect remote changes: {}", e));
            }
        };

        on_progress(SyncProgress {
            stage: SyncStage::DetectingRemote,
            current: remote_changes.len(),
            total: remote_changes.len(),
            percent: 35,
            message: Some(format!("Found {} remote changes", remote_changes.len())),
        });

        // Compute sync actions
        let actions = compute_sync_actions(&local_changes, &remote_changes);

        // Check for conflicts first
        let conflicts: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                SyncAction::Conflict { info } => Some(info.clone()),
                _ => None,
            })
            .collect();

        if !conflicts.is_empty() {
            on_progress(SyncProgress {
                stage: SyncStage::Error,
                current: 0,
                total: conflicts.len(),
                percent: 40,
                message: Some(format!("{} conflict(s) detected", conflicts.len())),
            });
            return CloudSyncResult::with_conflicts(conflicts);
        }

        // Count actions by type
        let uploads: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, SyncAction::Upload { .. }))
            .collect();
        let downloads: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, SyncAction::Download { .. }))
            .collect();
        let deletes: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, SyncAction::Delete { .. }))
            .collect();

        let total_actions = uploads.len() + downloads.len() + deletes.len();
        if total_actions == 0 {
            on_progress(SyncProgress {
                stage: SyncStage::Complete,
                current: 0,
                total: 0,
                percent: 100,
                message: Some("Already in sync!".to_string()),
            });
            return CloudSyncResult::success(0, 0, 0);
        }

        // Execute actions with progress (40-95%)
        let mut uploaded = 0;
        let mut downloaded = 0;
        let mut deleted = 0;
        let mut completed = 0;

        // Stage 3: Upload files (40-60%)
        for action in &actions {
            if let SyncAction::Upload { path } = action {
                let percent = 40 + (completed * 20 / total_actions.max(1)) as u8;
                on_progress(SyncProgress {
                    stage: SyncStage::Uploading,
                    current: uploaded + 1,
                    total: uploads.len(),
                    percent,
                    message: Some(format!("Uploading {}", path)),
                });

                match self.upload_file(fs, workspace_path, path).await {
                    Ok(_) => {
                        uploaded += 1;
                        completed += 1;
                    }
                    Err(e) => {
                        on_progress(SyncProgress {
                            stage: SyncStage::Error,
                            current: uploaded,
                            total: uploads.len(),
                            percent,
                            message: Some(format!("Failed to upload {}: {}", path, e)),
                        });
                        return CloudSyncResult::failure(format!(
                            "Failed to upload {}: {}",
                            path, e
                        ));
                    }
                }
            }
        }

        // Stage 4: Download files (60-80%)
        for action in &actions {
            if let SyncAction::Download { path, remote_info } = action {
                let percent = 60 + (completed * 20 / total_actions.max(1)) as u8;
                on_progress(SyncProgress {
                    stage: SyncStage::Downloading,
                    current: downloaded + 1,
                    total: downloads.len(),
                    percent,
                    message: Some(format!("Downloading {}", path)),
                });

                match self
                    .download_file(fs, workspace_path, path, remote_info)
                    .await
                {
                    Ok(_) => {
                        downloaded += 1;
                        completed += 1;
                    }
                    Err(e) => {
                        on_progress(SyncProgress {
                            stage: SyncStage::Error,
                            current: downloaded,
                            total: downloads.len(),
                            percent,
                            message: Some(format!("Failed to download {}: {}", path, e)),
                        });
                        return CloudSyncResult::failure(format!(
                            "Failed to download {}: {}",
                            path, e
                        ));
                    }
                }
            }
        }

        // Stage 5: Delete files (80-95%)
        for action in &actions {
            if let SyncAction::Delete { path, direction } = action {
                let percent = 80 + (completed * 15 / total_actions.max(1)) as u8;
                on_progress(SyncProgress {
                    stage: SyncStage::Deleting,
                    current: deleted + 1,
                    total: deletes.len(),
                    percent,
                    message: Some(format!("Deleting {}", path)),
                });

                match direction {
                    super::change::SyncDirection::Upload => {
                        if let Err(e) = self.provider.delete_remote_file(path).await {
                            on_progress(SyncProgress {
                                stage: SyncStage::Error,
                                current: deleted,
                                total: deletes.len(),
                                percent,
                                message: Some(format!(
                                    "Failed to delete {} from remote: {}",
                                    path, e
                                )),
                            });
                            return CloudSyncResult::failure(format!(
                                "Failed to delete {} from remote: {}",
                                path, e
                            ));
                        }
                    }
                    super::change::SyncDirection::Download => {
                        let full_path = workspace_path.join(path);
                        if let Err(e) = fs.delete_file(&full_path).await {
                            on_progress(SyncProgress {
                                stage: SyncStage::Error,
                                current: deleted,
                                total: deletes.len(),
                                percent,
                                message: Some(format!("Failed to delete {} locally: {}", path, e)),
                            });
                            return CloudSyncResult::failure(format!(
                                "Failed to delete {} locally: {}",
                                path, e
                            ));
                        }
                    }
                }
                self.manifest.remove_file(path);
                deleted += 1;
                completed += 1;
            }
        }

        // Mark sync complete and save manifest
        self.manifest.mark_synced();
        if let Err(e) = self.save_manifest(fs).await {
            on_progress(SyncProgress {
                stage: SyncStage::Error,
                current: 0,
                total: 0,
                percent: 95,
                message: Some(format!("Failed to save manifest: {}", e)),
            });
            return CloudSyncResult::failure(format!("Failed to save manifest: {}", e));
        }

        // Complete!
        on_progress(SyncProgress {
            stage: SyncStage::Complete,
            current: total_actions,
            total: total_actions,
            percent: 100,
            message: Some(format!(
                "Sync complete: {} uploaded, {} downloaded, {} deleted",
                uploaded, downloaded, deleted
            )),
        });

        CloudSyncResult::success(uploaded, downloaded, deleted)
    }

    /// Upload a single file to remote storage.
    async fn upload_file(
        &mut self,
        fs: &dyn AsyncFileSystem,
        workspace_path: &Path,
        relative_path: &str,
    ) -> Result<(), String> {
        let full_path = workspace_path.join(relative_path);

        // Check if this is an attachment (binary file)
        let is_attachment = relative_path
            .split('/')
            .any(|component| component == "_attachments");

        let content_bytes = if is_attachment {
            fs.read_binary(&full_path)
                .await
                .map_err(|e| format!("Failed to read binary file: {}", e))?
        } else {
            let content = fs
                .read_to_string(&full_path)
                .await
                .map_err(|e| format!("Failed to read file: {}", e))?;
            content.into_bytes()
        };

        let content_hash = compute_content_hash(&content_bytes);

        let remote_info = self
            .provider
            .upload_file(relative_path, &content_bytes)
            .await?;

        // Update manifest
        let state =
            FileSyncState::new(relative_path, &content_hash, chrono::Utc::now().timestamp())
                .with_remote_version(remote_info.etag.unwrap_or_default())
                .with_size(content_bytes.len() as u64);

        self.manifest.set_file(relative_path, state);

        Ok(())
    }

    /// Download a file from remote storage to local.
    async fn download_file(
        &mut self,
        fs: &dyn AsyncFileSystem,
        workspace_path: &Path,
        relative_path: &str,
        _remote_info: &RemoteFileInfo,
    ) -> Result<(), String> {
        let (content_bytes, updated_info) = self.provider.download_file(relative_path).await?;

        let full_path = workspace_path.join(relative_path);

        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            fs.create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        }

        // Check if this is an attachment (binary file)
        let is_attachment = relative_path
            .split('/')
            .any(|component| component == "_attachments");

        if is_attachment {
            // Write binary content directly
            fs.write_binary(&full_path, &content_bytes)
                .await
                .map_err(|e| format!("Failed to write binary file: {}", e))?;
        } else {
            // Write as text for markdown files
            let content =
                String::from_utf8(content_bytes.clone()).map_err(|_| "Invalid UTF-8 content")?;

            fs.write_file(&full_path, &content)
                .await
                .map_err(|e| format!("Failed to write file: {}", e))?;
        }

        // Update manifest
        let content_hash = compute_content_hash(&content_bytes);
        let state =
            FileSyncState::new(relative_path, &content_hash, chrono::Utc::now().timestamp())
                .with_remote_version(updated_info.etag.unwrap_or_default())
                .with_size(content_bytes.len() as u64);

        self.manifest.set_file(relative_path, state);

        Ok(())
    }

    /// Resolve a conflict with the specified resolution strategy.
    pub async fn resolve_conflict(
        &mut self,
        fs: &dyn AsyncFileSystem,
        workspace_path: &Path,
        conflict: &ConflictInfo,
        resolution: ConflictResolution,
    ) -> ConflictResolutionResult {
        let full_path = workspace_path.join(&conflict.path);

        match resolution {
            ConflictResolution::KeepLocal => {
                // Upload local version to remote
                match self.upload_file(fs, workspace_path, &conflict.path).await {
                    Ok(_) => ConflictResolutionResult::success(&conflict.path),
                    Err(e) => ConflictResolutionResult::failure(&conflict.path, e),
                }
            }
            ConflictResolution::KeepRemote => {
                // Download remote version
                let remote_info = RemoteFileInfo {
                    path: conflict.path.clone(),
                    size: 0,
                    modified_at: conflict.remote_modified_at.unwrap_or_else(chrono::Utc::now),
                    etag: None,
                    content_hash: conflict.remote_hash.clone(),
                };

                match self
                    .download_file(fs, workspace_path, &conflict.path, &remote_info)
                    .await
                {
                    Ok(_) => ConflictResolutionResult::success(&conflict.path),
                    Err(e) => ConflictResolutionResult::failure(&conflict.path, e),
                }
            }
            ConflictResolution::Merge { content } => {
                // Write merged content locally
                if let Err(e) = fs.write_file(&full_path, &content).await {
                    return ConflictResolutionResult::failure(
                        &conflict.path,
                        format!("Failed to write merged content: {}", e),
                    );
                }

                // Upload merged version
                match self.upload_file(fs, workspace_path, &conflict.path).await {
                    Ok(_) => ConflictResolutionResult::success(&conflict.path),
                    Err(e) => ConflictResolutionResult::failure(&conflict.path, e),
                }
            }
            ConflictResolution::KeepBoth => {
                // Download remote version to conflict file
                let conflict_path = conflict.conflict_file_name();

                // Check if this is an attachment (binary file)
                let is_attachment = conflict
                    .path
                    .split('/')
                    .any(|component| component == "_attachments");

                // Download to conflict file
                match self.provider.download_file(&conflict.path).await {
                    Ok((content_bytes, _)) => {
                        let conflict_full_path = workspace_path.join(&conflict_path);

                        let write_result = if is_attachment {
                            fs.write_binary(&conflict_full_path, &content_bytes).await
                        } else {
                            let content = String::from_utf8_lossy(&content_bytes);
                            fs.write_file(&conflict_full_path, &content).await
                        };

                        if let Err(e) = write_result {
                            return ConflictResolutionResult::failure(
                                &conflict.path,
                                format!("Failed to write conflict file: {}", e),
                            );
                        }

                        // Upload local version to remote (keeping local as primary)
                        match self.upload_file(fs, workspace_path, &conflict.path).await {
                            Ok(_) => ConflictResolutionResult::success_with_conflict_file(
                                &conflict.path,
                                &conflict_path,
                            ),
                            Err(e) => ConflictResolutionResult::failure(&conflict.path, e),
                        }
                    }
                    Err(e) => ConflictResolutionResult::failure(&conflict.path, e),
                }
            }
            ConflictResolution::Skip => {
                // Do nothing
                ConflictResolutionResult::success(&conflict.path)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock provider for testing
    struct MockProvider {
        files: std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                files: std::sync::Mutex::new(std::collections::HashMap::new()),
            }
        }
    }

    impl CloudSyncProvider for MockProvider {
        fn name(&self) -> &str {
            "mock"
        }

        fn provider_id(&self) -> String {
            "mock:test".to_string()
        }

        fn list_remote_files(&self) -> BoxFuture<'_, Result<Vec<RemoteFileInfo>, String>> {
            Box::pin(async move {
                let files = self.files.lock().unwrap();
                Ok(files
                    .keys()
                    .map(|path| RemoteFileInfo {
                        path: path.clone(),
                        size: 100,
                        modified_at: chrono::Utc::now(),
                        etag: Some(format!("etag-{}", path)),
                        content_hash: None,
                    })
                    .collect())
            })
        }

        fn download_file(
            &self,
            path: &str,
        ) -> BoxFuture<'_, Result<(Vec<u8>, RemoteFileInfo), String>> {
            let path = path.to_string();
            Box::pin(async move {
                let files = self.files.lock().unwrap();
                match files.get(&path) {
                    Some(content) => Ok((
                        content.clone(),
                        RemoteFileInfo {
                            path: path.clone(),
                            size: content.len() as u64,
                            modified_at: chrono::Utc::now(),
                            etag: Some(format!("etag-{}", path)),
                            content_hash: None,
                        },
                    )),
                    None => Err(format!("File not found: {}", path)),
                }
            })
        }

        fn upload_file(
            &self,
            path: &str,
            content: &[u8],
        ) -> BoxFuture<'_, Result<RemoteFileInfo, String>> {
            let path = path.to_string();
            let content = content.to_vec();
            Box::pin(async move {
                let mut files = self.files.lock().unwrap();
                files.insert(path.clone(), content.clone());
                Ok(RemoteFileInfo {
                    path: path.clone(),
                    size: content.len() as u64,
                    modified_at: chrono::Utc::now(),
                    etag: Some(format!("etag-{}", path)),
                    content_hash: None,
                })
            })
        }

        fn delete_remote_file(&self, path: &str) -> BoxFuture<'_, Result<(), String>> {
            let path = path.to_string();
            Box::pin(async move {
                let mut files = self.files.lock().unwrap();
                files.remove(&path);
                Ok(())
            })
        }

        fn is_available(&self) -> bool {
            true
        }
    }

    #[test]
    fn test_sync_engine_creation() {
        let provider = MockProvider::new();
        let engine = SyncEngine::new(provider, "/tmp/manifest.json");

        assert_eq!(engine.provider_id(), "mock:test");
        assert!(engine.is_available());
        assert!(engine.manifest().files.is_empty());
    }
}
