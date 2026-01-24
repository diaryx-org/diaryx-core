//! CRDT-updating filesystem decorator.
//!
//! This module provides [`CrdtFs`], a decorator that automatically updates the
//! workspace CRDT when filesystem operations occur. This ensures that local file
//! changes are automatically synchronized to the CRDT layer.
//!
//! # Architecture
//!
//! ```text
//! Local Write → CrdtFs.write_file() → Inner FS → Update WorkspaceCrdt
//!                                                       ↓
//!                                              WorkspaceCrdt.observe_updates()
//!                                                       ↓
//!                                              RustSyncBridge (syncs to server)
//! ```
//!
//! # Feature Gate
//!
//! This module requires the `crdt` feature to be enabled.

use std::collections::HashSet;
use std::io::Result;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use crate::crdt::{BodyDocManager, FileMetadata, WorkspaceCrdt};
use crate::frontmatter;
use crate::fs::{AsyncFileSystem, BoxFuture};

/// A filesystem decorator that automatically updates the CRDT on file operations.
///
/// This decorator intercepts filesystem writes and updates the workspace CRDT
/// with file metadata extracted from frontmatter. It supports:
///
/// - Automatic CRDT updates on file write/create
/// - Soft deletion (tombstone) on file delete
/// - Path tracking on file move/rename
/// - Runtime enable/disable toggle
///
/// # Example
///
/// ```ignore
/// use diaryx_core::fs::{CrdtFs, InMemoryFileSystem, SyncToAsyncFs};
/// use diaryx_core::crdt::{WorkspaceCrdt, MemoryStorage};
/// use std::sync::Arc;
///
/// let inner_fs = SyncToAsyncFs::new(InMemoryFileSystem::new());
/// let storage = Arc::new(MemoryStorage::new());
/// let workspace_crdt = Arc::new(WorkspaceCrdt::new(storage.clone()));
/// let body_manager = Arc::new(BodyDocManager::new(storage));
///
/// let crdt_fs = CrdtFs::new(inner_fs, workspace_crdt, body_manager);
///
/// // All writes now automatically update the CRDT
/// crdt_fs.write_file(Path::new("test.md"), "---\ntitle: Test\n---\nContent").await?;
/// ```
pub struct CrdtFs<FS: AsyncFileSystem> {
    /// The underlying filesystem.
    inner: FS,
    /// The workspace CRDT for file metadata.
    workspace_crdt: Arc<WorkspaceCrdt>,
    /// Manager for per-file body documents.
    body_doc_manager: Arc<BodyDocManager>,
    /// Whether CRDT updates are enabled.
    enabled: AtomicBool,
    /// Paths currently being written locally (for loop prevention).
    local_writes_in_progress: RwLock<HashSet<PathBuf>>,
    /// Paths currently being written from sync (skip CRDT updates entirely).
    /// This prevents feedback loops where remote sync writes trigger new CRDT updates.
    sync_writes_in_progress: RwLock<HashSet<PathBuf>>,
}

impl<FS: AsyncFileSystem> CrdtFs<FS> {
    /// Create a new CRDT filesystem decorator.
    pub fn new(
        inner: FS,
        workspace_crdt: Arc<WorkspaceCrdt>,
        body_doc_manager: Arc<BodyDocManager>,
    ) -> Self {
        Self {
            inner,
            workspace_crdt,
            body_doc_manager,
            enabled: AtomicBool::new(true),
            local_writes_in_progress: RwLock::new(HashSet::new()),
            sync_writes_in_progress: RwLock::new(HashSet::new()),
        }
    }

    /// Check if CRDT updates are enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Enable or disable CRDT updates.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }

    /// Get a reference to the workspace CRDT.
    pub fn workspace_crdt(&self) -> &Arc<WorkspaceCrdt> {
        &self.workspace_crdt
    }

    /// Get a reference to the body document manager.
    pub fn body_doc_manager(&self) -> &Arc<BodyDocManager> {
        &self.body_doc_manager
    }

    /// Get a reference to the inner filesystem.
    pub fn inner(&self) -> &FS {
        &self.inner
    }

    /// Check if a path is currently being written locally.
    ///
    /// Used to prevent loops when CRDT observers trigger writes.
    pub fn is_local_write_in_progress(&self, path: &Path) -> bool {
        let writes = self.local_writes_in_progress.read().unwrap();
        writes.contains(&path.to_path_buf())
    }

    /// Mark a path as being written locally.
    fn mark_local_write_start(&self, path: &Path) {
        let mut writes = self.local_writes_in_progress.write().unwrap();
        writes.insert(path.to_path_buf());
    }

    /// Clear the local write marker for a path.
    fn mark_local_write_end(&self, path: &Path) {
        let mut writes = self.local_writes_in_progress.write().unwrap();
        writes.remove(&path.to_path_buf());
    }

    /// Check if a path is currently being written from sync.
    ///
    /// Sync writes should skip CRDT updates entirely to prevent feedback loops.
    pub fn is_sync_write_in_progress(&self, path: &Path) -> bool {
        let writes = self.sync_writes_in_progress.read().unwrap();
        writes.contains(&path.to_path_buf())
    }

    /// Mark a path as being written from sync (internal implementation).
    fn mark_sync_write_start_internal(&self, path: &Path) {
        let mut writes = self.sync_writes_in_progress.write().unwrap();
        writes.insert(path.to_path_buf());
        log::debug!(
            "CrdtFs: Marked sync write start for {:?} (total: {})",
            path,
            writes.len()
        );
    }

    /// Clear the sync write marker for a path (internal implementation).
    fn mark_sync_write_end_internal(&self, path: &Path) {
        let mut writes = self.sync_writes_in_progress.write().unwrap();
        writes.remove(&path.to_path_buf());
        log::debug!(
            "CrdtFs: Marked sync write end for {:?} (remaining: {})",
            path,
            writes.len()
        );
    }

    /// Extract FileMetadata from file content.
    ///
    /// Parses frontmatter and converts known fields to FileMetadata.
    fn extract_metadata(&self, content: &str) -> FileMetadata {
        match frontmatter::parse_or_empty(content) {
            Ok(parsed) => self.frontmatter_to_metadata(&parsed.frontmatter),
            Err(_) => FileMetadata::default(),
        }
    }

    /// Convert frontmatter to FileMetadata.
    fn frontmatter_to_metadata(
        &self,
        fm: &indexmap::IndexMap<String, serde_yaml::Value>,
    ) -> FileMetadata {
        // Try to convert via JSON for automatic field mapping
        if let Ok(json_value) = serde_json::to_value(fm) {
            if let Ok(metadata) = serde_json::from_value::<FileMetadata>(json_value) {
                return metadata;
            }
        }

        // Fallback: manual extraction of known fields
        let mut metadata = FileMetadata::default();

        if let Some(title) = fm.get("title") {
            metadata.title = title.as_str().map(String::from);
        }
        if let Some(part_of) = fm.get("part_of") {
            metadata.part_of = part_of.as_str().map(String::from);
        }
        if let Some(contents) = fm.get("contents") {
            if let Some(seq) = contents.as_sequence() {
                metadata.contents = Some(
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect(),
                );
            }
        }
        if let Some(audience) = fm.get("audience") {
            if let Some(seq) = audience.as_sequence() {
                metadata.audience = Some(
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect(),
                );
            }
        }
        if let Some(description) = fm.get("description") {
            metadata.description = description.as_str().map(String::from);
        }

        // Store remaining fields in extra
        let known_fields = [
            "title",
            "part_of",
            "contents",
            "audience",
            "description",
            "attachments",
            "deleted",
            "modified_at",
        ];
        for (key, value) in fm {
            if !known_fields.contains(&key.as_str()) {
                if let Ok(json_value) = serde_json::to_value(value) {
                    metadata.extra.insert(key.clone(), json_value);
                }
            }
        }

        metadata.modified_at = chrono::Utc::now().timestamp_millis();
        metadata
    }

    /// Update CRDT with metadata from a file.
    ///
    /// This is skipped if:
    /// - CRDT updates are disabled globally
    /// - The path is marked as a sync write (to prevent feedback loops)
    fn update_crdt_for_file(&self, path: &Path, content: &str) {
        if !self.is_enabled() {
            return;
        }

        // Skip CRDT update if this is a sync write (prevents feedback loops)
        if self.is_sync_write_in_progress(path) {
            log::debug!("CrdtFs: Skipping CRDT update for sync write: {:?}", path);
            return;
        }

        let path_str = path.to_string_lossy().to_string();
        let metadata = self.extract_metadata(content);

        // Update workspace CRDT
        if let Err(e) = self.workspace_crdt.set_file(&path_str, metadata) {
            log::warn!("Failed to update CRDT for {}: {}", path_str, e);
        }

        // Update body doc
        let body = frontmatter::extract_body(content);
        let body_doc = self.body_doc_manager.get_or_create(&path_str);
        let _ = body_doc.set_body(body);
    }

    /// Update parent's contents array when a child is moved or deleted.
    ///
    /// For rename/move: `new_path` is Some with the new path.
    /// For delete: `new_path` is None.
    fn update_parent_contents(&self, old_path: &str, new_path: Option<&str>) {
        if !self.is_enabled() {
            return;
        }

        let old_metadata = match self.workspace_crdt.get_file(old_path) {
            Some(m) => m,
            None => return,
        };

        if let Some(ref parent_path) = old_metadata.part_of {
            if let Some(mut parent) = self.workspace_crdt.get_file(parent_path) {
                if let Some(ref mut contents) = parent.contents {
                    // Find old filename in contents
                    let old_filename = std::path::Path::new(old_path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(old_path);

                    if let Some(idx) = contents
                        .iter()
                        .position(|e| e == old_filename || e == old_path)
                    {
                        match new_path {
                            Some(np) => {
                                // Rename: replace with new filename
                                let new_filename = std::path::Path::new(np)
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or(np);
                                contents[idx] = new_filename.to_string();
                            }
                            None => {
                                // Delete: remove from contents
                                contents.remove(idx);
                            }
                        }
                        parent.modified_at = chrono::Utc::now().timestamp_millis();
                        let _ = self.workspace_crdt.set_file(parent_path, parent);
                    }
                }
            }
        }
    }
}

// Implement Clone if the inner FS is Clone
impl<FS: AsyncFileSystem + Clone> Clone for CrdtFs<FS> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            workspace_crdt: Arc::clone(&self.workspace_crdt),
            body_doc_manager: Arc::clone(&self.body_doc_manager),
            enabled: AtomicBool::new(self.enabled.load(Ordering::SeqCst)),
            local_writes_in_progress: RwLock::new(HashSet::new()),
            sync_writes_in_progress: RwLock::new(HashSet::new()),
        }
    }
}

// AsyncFileSystem implementation - delegates to inner with CRDT updates
#[cfg(not(target_arch = "wasm32"))]
impl<FS: AsyncFileSystem + Send + Sync> AsyncFileSystem for CrdtFs<FS> {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        self.inner.read_to_string(path)
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Mark local write in progress
            self.mark_local_write_start(path);

            // Write to inner filesystem
            let result = self.inner.write_file(path, content).await;

            // Update CRDT if write succeeded and enabled
            if result.is_ok() {
                self.update_crdt_for_file(path, content);
            }

            // Clear local write marker
            self.mark_local_write_end(path);

            result
        })
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Mark local write in progress
            self.mark_local_write_start(path);

            // Create in inner filesystem
            let result = self.inner.create_new(path, content).await;

            // Update CRDT if creation succeeded and enabled
            if result.is_ok() {
                self.update_crdt_for_file(path, content);
            }

            // Clear local write marker
            self.mark_local_write_end(path);

            result
        })
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Mark local write in progress
            self.mark_local_write_start(path);

            // Delete from inner filesystem
            let result = self.inner.delete_file(path).await;

            // Mark as deleted in CRDT if deletion succeeded and enabled
            if result.is_ok() && self.is_enabled() {
                let path_str = path.to_string_lossy().to_string();

                // Update parent's contents to remove the deleted file
                self.update_parent_contents(&path_str, None);

                if let Err(e) = self.workspace_crdt.delete_file(&path_str) {
                    log::warn!(
                        "Failed to mark file as deleted in CRDT for {}: {}",
                        path_str,
                        e
                    );
                }
            }

            // Clear local write marker
            self.mark_local_write_end(path);

            result
        })
    }

    fn list_md_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        self.inner.list_md_files(dir)
    }

    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        self.inner.exists(path)
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        self.inner.create_dir_all(path)
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        self.inner.is_dir(path)
    }

    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Read content before move (for CRDT update)
            let content = if self.is_enabled() {
                self.inner.read_to_string(from).await.ok()
            } else {
                None
            };

            // Mark both paths as local writes in progress
            self.mark_local_write_start(from);
            self.mark_local_write_start(to);

            // Perform the move
            let result = self.inner.move_file(from, to).await;

            // Update CRDT if move succeeded
            if result.is_ok() && self.is_enabled() {
                let from_str = from.to_string_lossy().to_string();
                let to_str = to.to_string_lossy().to_string();

                // Update parent's contents (replace old path with new path)
                self.update_parent_contents(&from_str, Some(&to_str));

                // Mark old path as deleted
                if let Err(e) = self.workspace_crdt.delete_file(&from_str) {
                    log::warn!("Failed to mark old path as deleted in CRDT: {}", e);
                }

                // Create entry at new path with preserved metadata
                if let Some(content) = content {
                    self.update_crdt_for_file(to, &content);
                }
            }

            // Clear local write markers
            self.mark_local_write_end(from);
            self.mark_local_write_end(to);

            result
        })
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        self.inner.read_binary(path)
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        // Binary files are not tracked in the CRDT (they're attachments)
        self.inner.write_binary(path, content)
    }

    fn list_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        self.inner.list_files(dir)
    }

    fn get_modified_time<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Option<i64>> {
        self.inner.get_modified_time(path)
    }

    // Override sync write markers to track which paths are being written from sync
    fn mark_sync_write_start(&self, path: &Path) {
        self.mark_sync_write_start_internal(path);
    }

    fn mark_sync_write_end(&self, path: &Path) {
        self.mark_sync_write_end_internal(path);
    }
}

// WASM implementation (without Send + Sync bounds)
#[cfg(target_arch = "wasm32")]
impl<FS: AsyncFileSystem> AsyncFileSystem for CrdtFs<FS> {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        self.inner.read_to_string(path)
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Mark local write in progress
            self.mark_local_write_start(path);

            // Write to inner filesystem
            let result = self.inner.write_file(path, content).await;

            // Update CRDT if write succeeded and enabled
            if result.is_ok() {
                self.update_crdt_for_file(path, content);
            }

            // Clear local write marker
            self.mark_local_write_end(path);

            result
        })
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Mark local write in progress
            self.mark_local_write_start(path);

            // Create in inner filesystem
            let result = self.inner.create_new(path, content).await;

            // Update CRDT if creation succeeded and enabled
            if result.is_ok() {
                self.update_crdt_for_file(path, content);
            }

            // Clear local write marker
            self.mark_local_write_end(path);

            result
        })
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Mark local write in progress
            self.mark_local_write_start(path);

            // Delete from inner filesystem
            let result = self.inner.delete_file(path).await;

            // Mark as deleted in CRDT if deletion succeeded and enabled
            if result.is_ok() && self.is_enabled() {
                let path_str = path.to_string_lossy().to_string();

                // Update parent's contents to remove the deleted file
                self.update_parent_contents(&path_str, None);

                if let Err(e) = self.workspace_crdt.delete_file(&path_str) {
                    log::warn!(
                        "Failed to mark file as deleted in CRDT for {}: {}",
                        path_str,
                        e
                    );
                }
            }

            // Clear local write marker
            self.mark_local_write_end(path);

            result
        })
    }

    fn list_md_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        self.inner.list_md_files(dir)
    }

    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        self.inner.exists(path)
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        self.inner.create_dir_all(path)
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        self.inner.is_dir(path)
    }

    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Read content before move (for CRDT update)
            let content = if self.is_enabled() {
                self.inner.read_to_string(from).await.ok()
            } else {
                None
            };

            // Mark both paths as local writes in progress
            self.mark_local_write_start(from);
            self.mark_local_write_start(to);

            // Perform the move
            let result = self.inner.move_file(from, to).await;

            // Update CRDT if move succeeded
            if result.is_ok() && self.is_enabled() {
                let from_str = from.to_string_lossy().to_string();
                let to_str = to.to_string_lossy().to_string();

                // Update parent's contents (replace old path with new path)
                self.update_parent_contents(&from_str, Some(&to_str));

                // Mark old path as deleted
                if let Err(e) = self.workspace_crdt.delete_file(&from_str) {
                    log::warn!("Failed to mark old path as deleted in CRDT: {}", e);
                }

                // Create entry at new path with preserved metadata
                if let Some(content) = content {
                    self.update_crdt_for_file(to, &content);
                }
            }

            // Clear local write markers
            self.mark_local_write_end(from);
            self.mark_local_write_end(to);

            result
        })
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        self.inner.read_binary(path)
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        // Binary files are not tracked in the CRDT (they're attachments)
        self.inner.write_binary(path, content)
    }

    fn list_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        self.inner.list_files(dir)
    }

    fn get_modified_time<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Option<i64>> {
        self.inner.get_modified_time(path)
    }

    // Override sync write markers to track which paths are being written from sync
    fn mark_sync_write_start(&self, path: &Path) {
        self.mark_sync_write_start_internal(path);
    }

    fn mark_sync_write_end(&self, path: &Path) {
        self.mark_sync_write_end_internal(path);
    }
}

impl<FS: AsyncFileSystem> std::fmt::Debug for CrdtFs<FS> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CrdtFs")
            .field("enabled", &self.is_enabled())
            .field("workspace_crdt", &self.workspace_crdt)
            .field("body_doc_manager", &self.body_doc_manager)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crdt::{CrdtStorage, MemoryStorage};
    use crate::fs::{InMemoryFileSystem, SyncToAsyncFs};

    fn create_test_crdt_fs() -> CrdtFs<SyncToAsyncFs<InMemoryFileSystem>> {
        let inner = SyncToAsyncFs::new(InMemoryFileSystem::new());
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        let workspace_crdt = Arc::new(WorkspaceCrdt::new(Arc::clone(&storage)));
        let body_manager = Arc::new(BodyDocManager::new(storage));
        CrdtFs::new(inner, workspace_crdt, body_manager)
    }

    #[test]
    fn test_write_updates_crdt() {
        let fs = create_test_crdt_fs();
        let content = "---\ntitle: Test\npart_of: index.md\n---\nBody content";

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test.md"), content).await.unwrap();
        });

        // Check CRDT was updated
        let metadata = fs.workspace_crdt.get_file("test.md").unwrap();
        assert_eq!(metadata.title, Some("Test".to_string()));
        assert_eq!(metadata.part_of, Some("index.md".to_string()));
    }

    #[test]
    fn test_delete_marks_deleted_in_crdt() {
        let fs = create_test_crdt_fs();
        let content = "---\ntitle: Test\n---\nBody";

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test.md"), content).await.unwrap();
            fs.delete_file(Path::new("test.md")).await.unwrap();
        });

        // Check file is marked as deleted in CRDT
        let metadata = fs.workspace_crdt.get_file("test.md").unwrap();
        assert!(metadata.deleted);
    }

    #[test]
    fn test_disabled_skips_crdt_updates() {
        let fs = create_test_crdt_fs();
        fs.set_enabled(false);

        let content = "---\ntitle: Test\n---\nBody";

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test.md"), content).await.unwrap();
        });

        // CRDT should not have the file
        assert!(fs.workspace_crdt.get_file("test.md").is_none());
    }

    #[test]
    fn test_toggle_enabled() {
        let fs = create_test_crdt_fs();

        assert!(fs.is_enabled());
        fs.set_enabled(false);
        assert!(!fs.is_enabled());
        fs.set_enabled(true);
        assert!(fs.is_enabled());
    }

    #[test]
    fn test_local_write_tracking() {
        let fs = create_test_crdt_fs();

        assert!(!fs.is_local_write_in_progress(Path::new("test.md")));

        fs.mark_local_write_start(Path::new("test.md"));
        assert!(fs.is_local_write_in_progress(Path::new("test.md")));

        fs.mark_local_write_end(Path::new("test.md"));
        assert!(!fs.is_local_write_in_progress(Path::new("test.md")));
    }

    #[test]
    fn test_sync_write_tracking() {
        let fs = create_test_crdt_fs();

        assert!(!fs.is_sync_write_in_progress(Path::new("test.md")));

        fs.mark_sync_write_start(Path::new("test.md"));
        assert!(fs.is_sync_write_in_progress(Path::new("test.md")));

        fs.mark_sync_write_end(Path::new("test.md"));
        assert!(!fs.is_sync_write_in_progress(Path::new("test.md")));
    }

    #[test]
    fn test_sync_write_skips_crdt_update() {
        let fs = create_test_crdt_fs();
        let content = "---\ntitle: Sync Write Test\n---\nBody content";

        // First, write without sync marker - should update CRDT
        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test1.md"), content).await.unwrap();
        });
        assert!(fs.workspace_crdt.get_file("test1.md").is_some());

        // Now, mark sync write and write - should NOT update CRDT
        fs.mark_sync_write_start(Path::new("test2.md"));
        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test2.md"), content).await.unwrap();
        });
        fs.mark_sync_write_end(Path::new("test2.md"));

        // File should exist on disk but NOT in CRDT
        assert!(futures_lite::future::block_on(
            fs.exists(Path::new("test2.md"))
        ));
        assert!(
            fs.workspace_crdt.get_file("test2.md").is_none(),
            "CRDT should not have been updated for sync write"
        );
    }
}
