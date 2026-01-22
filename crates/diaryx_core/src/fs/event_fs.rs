//! Event-emitting filesystem decorator.
//!
//! This module provides [`EventEmittingFs`], a decorator that emits
//! [`FileSystemEvent`]s for all filesystem operations. This enables
//! UI updates and other reactive behaviors.
//!
//! # Architecture
//!
//! ```text
//! File Operation → EventEmittingFs → Inner FS → Emit Event
//!                                                    ↓
//!                                         CallbackRegistry.emit()
//!                                                    ↓
//!                                         JS/UI Callbacks
//! ```

use std::io::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::frontmatter;
use crate::fs::{AsyncFileSystem, BoxFuture};

use super::callback_registry::{CallbackRegistry, EventCallback, SubscriptionId};
use super::events::FileSystemEvent;

/// A filesystem decorator that emits events for all operations.
///
/// This decorator wraps another filesystem and emits events when operations
/// occur. It supports:
///
/// - Subscribing to events via callback functions
/// - Runtime enable/disable toggle
/// - Automatic event type detection (create vs update, rename vs move)
///
/// # Example
///
/// ```ignore
/// use diaryx_core::fs::{EventEmittingFs, InMemoryFileSystem, SyncToAsyncFs, FileSystemEvent};
/// use std::sync::Arc;
///
/// let inner_fs = SyncToAsyncFs::new(InMemoryFileSystem::new());
/// let event_fs = EventEmittingFs::new(inner_fs);
///
/// // Subscribe to events
/// let id = event_fs.on_event(Arc::new(|event| {
///     println!("Event: {:?}", event);
/// }));
///
/// // Operations now emit events
/// event_fs.write_file(Path::new("test.md"), "content").await?;
///
/// // Unsubscribe
/// event_fs.off_event(id);
/// ```
pub struct EventEmittingFs<FS: AsyncFileSystem> {
    /// The underlying filesystem.
    inner: FS,
    /// Registry of event callbacks.
    registry: Arc<CallbackRegistry>,
    /// Whether event emission is enabled.
    enabled: AtomicBool,
}

impl<FS: AsyncFileSystem> EventEmittingFs<FS> {
    /// Create a new event-emitting filesystem decorator.
    pub fn new(inner: FS) -> Self {
        Self {
            inner,
            registry: Arc::new(CallbackRegistry::new()),
            enabled: AtomicBool::new(true),
        }
    }

    /// Create a new event-emitting filesystem with a shared registry.
    pub fn with_registry(inner: FS, registry: Arc<CallbackRegistry>) -> Self {
        Self {
            inner,
            registry,
            enabled: AtomicBool::new(true),
        }
    }

    /// Check if event emission is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Enable or disable event emission.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }

    /// Subscribe to filesystem events.
    ///
    /// Returns a subscription ID that can be used to unsubscribe.
    pub fn on_event(&self, callback: EventCallback) -> SubscriptionId {
        self.registry.subscribe(callback)
    }

    /// Unsubscribe from filesystem events.
    ///
    /// Returns `true` if the subscription was found and removed.
    pub fn off_event(&self, id: SubscriptionId) -> bool {
        self.registry.unsubscribe(id)
    }

    /// Get a reference to the callback registry.
    pub fn registry(&self) -> &Arc<CallbackRegistry> {
        &self.registry
    }

    /// Get a reference to the inner filesystem.
    pub fn inner(&self) -> &FS {
        &self.inner
    }

    /// Emit an event if enabled.
    fn emit(&self, event: FileSystemEvent) {
        if self.is_enabled() {
            self.registry.emit(&event);
        }
    }

    /// Extract frontmatter from content as JSON.
    fn extract_frontmatter(&self, content: &str) -> Option<serde_json::Value> {
        frontmatter::parse_or_empty(content)
            .ok()
            .and_then(|parsed| serde_json::to_value(&parsed.frontmatter).ok())
    }

    /// Get parent path from frontmatter part_of field.
    fn get_parent_from_content(&self, content: &str) -> Option<PathBuf> {
        frontmatter::parse_or_empty(content)
            .ok()
            .and_then(|parsed| {
                parsed
                    .frontmatter
                    .get("part_of")
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from)
            })
    }
}

// Implement Clone if the inner FS is Clone
impl<FS: AsyncFileSystem + Clone> Clone for EventEmittingFs<FS> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            registry: Arc::clone(&self.registry),
            enabled: AtomicBool::new(self.enabled.load(Ordering::SeqCst)),
        }
    }
}

// AsyncFileSystem implementation - native
#[cfg(not(target_arch = "wasm32"))]
impl<FS: AsyncFileSystem + Send + Sync> AsyncFileSystem for EventEmittingFs<FS> {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        self.inner.read_to_string(path)
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Check if file exists and get old frontmatter (for create vs update detection)
            let old_frontmatter = if self.is_enabled() {
                self.inner
                    .read_to_string(path)
                    .await
                    .ok()
                    .and_then(|old_content| self.extract_frontmatter(&old_content))
            } else {
                None
            };
            let existed =
                old_frontmatter.is_some() || (!self.is_enabled() && self.inner.exists(path).await);

            // Write to inner filesystem
            let result = self.inner.write_file(path, content).await;

            // Emit event if write succeeded
            if result.is_ok() {
                let new_frontmatter = self.extract_frontmatter(content);
                let parent_path = self.get_parent_from_content(content);

                if existed {
                    // File existed - only emit MetadataChanged if frontmatter actually changed
                    if let Some(new_fm) = new_frontmatter {
                        let changed = match &old_frontmatter {
                            Some(old_fm) => old_fm != &new_fm,
                            None => true, // No old frontmatter means it was added
                        };
                        if changed {
                            self.emit(FileSystemEvent::metadata_changed(
                                path.to_path_buf(),
                                new_fm,
                            ));
                        }
                    }
                    // If new file has no frontmatter but old did, that's also a change
                    // but MetadataChanged expects frontmatter, so we skip this case
                } else {
                    // New file - emit file created
                    self.emit(FileSystemEvent::file_created_with_metadata(
                        path.to_path_buf(),
                        new_frontmatter,
                        parent_path,
                    ));
                }
            }

            result
        })
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let result = self.inner.create_new(path, content).await;

            if result.is_ok() {
                let frontmatter = self.extract_frontmatter(content);
                let parent_path = self.get_parent_from_content(content);

                self.emit(FileSystemEvent::file_created_with_metadata(
                    path.to_path_buf(),
                    frontmatter,
                    parent_path,
                ));
            }

            result
        })
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Try to read parent path before deletion
            let parent_path = if self.is_enabled() {
                self.inner
                    .read_to_string(path)
                    .await
                    .ok()
                    .and_then(|content| self.get_parent_from_content(&content))
            } else {
                None
            };

            let result = self.inner.delete_file(path).await;

            if result.is_ok() {
                self.emit(FileSystemEvent::file_deleted_with_parent(
                    path.to_path_buf(),
                    parent_path,
                ));
            }

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
            let result = self.inner.move_file(from, to).await;

            if result.is_ok() {
                // Determine if this is a rename (same parent) or move (different parent)
                let from_parent = from.parent();
                let to_parent = to.parent();

                if from_parent == to_parent {
                    // Same parent - it's a rename
                    self.emit(FileSystemEvent::file_renamed(
                        from.to_path_buf(),
                        to.to_path_buf(),
                    ));
                } else {
                    // Different parent - it's a move
                    self.emit(FileSystemEvent::file_moved(
                        to.to_path_buf(),
                        from_parent.map(PathBuf::from),
                        to_parent.map(PathBuf::from),
                    ));
                }
            }

            result
        })
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        self.inner.read_binary(path)
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        // Binary files don't emit events (they're attachments managed differently)
        self.inner.write_binary(path, content)
    }

    fn list_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        self.inner.list_files(dir)
    }

    fn get_modified_time<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Option<i64>> {
        self.inner.get_modified_time(path)
    }
}

// WASM implementation (without Send + Sync bounds)
#[cfg(target_arch = "wasm32")]
impl<FS: AsyncFileSystem> AsyncFileSystem for EventEmittingFs<FS> {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        self.inner.read_to_string(path)
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Check if file exists and get old frontmatter (for create vs update detection)
            let old_frontmatter = if self.is_enabled() {
                self.inner
                    .read_to_string(path)
                    .await
                    .ok()
                    .and_then(|old_content| self.extract_frontmatter(&old_content))
            } else {
                None
            };
            let existed =
                old_frontmatter.is_some() || (!self.is_enabled() && self.inner.exists(path).await);

            // Write to inner filesystem
            let result = self.inner.write_file(path, content).await;

            // Emit event if write succeeded
            if result.is_ok() {
                let new_frontmatter = self.extract_frontmatter(content);
                let parent_path = self.get_parent_from_content(content);

                if existed {
                    // File existed - only emit MetadataChanged if frontmatter actually changed
                    if let Some(new_fm) = new_frontmatter {
                        let changed = match &old_frontmatter {
                            Some(old_fm) => old_fm != &new_fm,
                            None => true, // No old frontmatter means it was added
                        };
                        if changed {
                            self.emit(FileSystemEvent::metadata_changed(
                                path.to_path_buf(),
                                new_fm,
                            ));
                        }
                    }
                    // If new file has no frontmatter but old did, that's also a change
                    // but MetadataChanged expects frontmatter, so we skip this case
                } else {
                    // New file - emit file created
                    self.emit(FileSystemEvent::file_created_with_metadata(
                        path.to_path_buf(),
                        new_frontmatter,
                        parent_path,
                    ));
                }
            }

            result
        })
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let result = self.inner.create_new(path, content).await;

            if result.is_ok() {
                let frontmatter = self.extract_frontmatter(content);
                let parent_path = self.get_parent_from_content(content);

                self.emit(FileSystemEvent::file_created_with_metadata(
                    path.to_path_buf(),
                    frontmatter,
                    parent_path,
                ));
            }

            result
        })
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Try to read parent path before deletion
            let parent_path = if self.is_enabled() {
                self.inner
                    .read_to_string(path)
                    .await
                    .ok()
                    .and_then(|content| self.get_parent_from_content(&content))
            } else {
                None
            };

            let result = self.inner.delete_file(path).await;

            if result.is_ok() {
                self.emit(FileSystemEvent::file_deleted_with_parent(
                    path.to_path_buf(),
                    parent_path,
                ));
            }

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
            let result = self.inner.move_file(from, to).await;

            if result.is_ok() {
                // Determine if this is a rename (same parent) or move (different parent)
                let from_parent = from.parent();
                let to_parent = to.parent();

                if from_parent == to_parent {
                    // Same parent - it's a rename
                    self.emit(FileSystemEvent::file_renamed(
                        from.to_path_buf(),
                        to.to_path_buf(),
                    ));
                } else {
                    // Different parent - it's a move
                    self.emit(FileSystemEvent::file_moved(
                        to.to_path_buf(),
                        from_parent.map(PathBuf::from),
                        to_parent.map(PathBuf::from),
                    ));
                }
            }

            result
        })
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        self.inner.read_binary(path)
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        // Binary files don't emit events (they're attachments managed differently)
        self.inner.write_binary(path, content)
    }

    fn list_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        self.inner.list_files(dir)
    }

    fn get_modified_time<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Option<i64>> {
        self.inner.get_modified_time(path)
    }
}

impl<FS: AsyncFileSystem> std::fmt::Debug for EventEmittingFs<FS> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventEmittingFs")
            .field("enabled", &self.is_enabled())
            .field("registry", &self.registry)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{InMemoryFileSystem, SyncToAsyncFs};
    use std::sync::atomic::AtomicUsize;

    fn create_test_event_fs() -> EventEmittingFs<SyncToAsyncFs<InMemoryFileSystem>> {
        let inner = SyncToAsyncFs::new(InMemoryFileSystem::new());
        EventEmittingFs::new(inner)
    }

    #[test]
    fn test_write_emits_file_created() {
        let fs = create_test_event_fs();
        let created_count = Arc::new(AtomicUsize::new(0));

        let counter = Arc::clone(&created_count);
        fs.on_event(Arc::new(move |event| {
            if matches!(event, FileSystemEvent::FileCreated { .. }) {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }));

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test.md"), "---\ntitle: Test\n---\nBody")
                .await
                .unwrap();
        });

        assert_eq!(created_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_write_existing_emits_metadata_changed_only_when_frontmatter_changes() {
        let fs = create_test_event_fs();
        let changed_count = Arc::new(AtomicUsize::new(0));

        // Create file first
        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test.md"), "---\ntitle: First\n---\nBody")
                .await
                .unwrap();
        });

        // Now subscribe
        let counter = Arc::clone(&changed_count);
        fs.on_event(Arc::new(move |event| {
            if matches!(event, FileSystemEvent::MetadataChanged { .. }) {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }));

        // Update frontmatter - should emit MetadataChanged
        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test.md"), "---\ntitle: Updated\n---\nBody")
                .await
                .unwrap();
        });
        assert_eq!(changed_count.load(Ordering::SeqCst), 1);

        // Update body only - should NOT emit MetadataChanged
        futures_lite::future::block_on(async {
            fs.write_file(
                Path::new("test.md"),
                "---\ntitle: Updated\n---\nBody changed!",
            )
            .await
            .unwrap();
        });
        assert_eq!(changed_count.load(Ordering::SeqCst), 1); // Still 1, not 2

        // Update frontmatter again - should emit MetadataChanged
        futures_lite::future::block_on(async {
            fs.write_file(
                Path::new("test.md"),
                "---\ntitle: Final Title\n---\nBody changed!",
            )
            .await
            .unwrap();
        });
        assert_eq!(changed_count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_delete_emits_file_deleted() {
        let fs = create_test_event_fs();
        let deleted_count = Arc::new(AtomicUsize::new(0));

        // Create file first
        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test.md"), "content")
                .await
                .unwrap();
        });

        // Subscribe
        let counter = Arc::clone(&deleted_count);
        fs.on_event(Arc::new(move |event| {
            if matches!(event, FileSystemEvent::FileDeleted { .. }) {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }));

        // Delete
        futures_lite::future::block_on(async {
            fs.delete_file(Path::new("test.md")).await.unwrap();
        });

        assert_eq!(deleted_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_disabled_skips_events() {
        let fs = create_test_event_fs();
        fs.set_enabled(false);

        let event_count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&event_count);
        fs.on_event(Arc::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        }));

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test.md"), "content")
                .await
                .unwrap();
        });

        assert_eq!(event_count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_unsubscribe() {
        let fs = create_test_event_fs();
        let event_count = Arc::new(AtomicUsize::new(0));

        let counter = Arc::clone(&event_count);
        let id = fs.on_event(Arc::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        }));

        // First write
        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test1.md"), "content")
                .await
                .unwrap();
        });
        assert_eq!(event_count.load(Ordering::SeqCst), 1);

        // Unsubscribe
        assert!(fs.off_event(id));

        // Second write
        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test2.md"), "content")
                .await
                .unwrap();
        });

        // Count should not have increased
        assert_eq!(event_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_move_same_parent_emits_renamed() {
        let fs = create_test_event_fs();
        let renamed_count = Arc::new(AtomicUsize::new(0));

        // Create file
        futures_lite::future::block_on(async {
            fs.write_file(Path::new("dir/old.md"), "content")
                .await
                .unwrap();
        });

        // Subscribe
        let counter = Arc::clone(&renamed_count);
        fs.on_event(Arc::new(move |event| {
            if matches!(event, FileSystemEvent::FileRenamed { .. }) {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }));

        // Move within same directory
        futures_lite::future::block_on(async {
            fs.move_file(Path::new("dir/old.md"), Path::new("dir/new.md"))
                .await
                .unwrap();
        });

        assert_eq!(renamed_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_move_different_parent_emits_moved() {
        let fs = create_test_event_fs();
        let moved_count = Arc::new(AtomicUsize::new(0));

        // Create file
        futures_lite::future::block_on(async {
            fs.create_dir_all(Path::new("dir1")).await.unwrap();
            fs.create_dir_all(Path::new("dir2")).await.unwrap();
            fs.write_file(Path::new("dir1/file.md"), "content")
                .await
                .unwrap();
        });

        // Subscribe
        let counter = Arc::clone(&moved_count);
        fs.on_event(Arc::new(move |event| {
            if matches!(event, FileSystemEvent::FileMoved { .. }) {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }));

        // Move to different directory
        futures_lite::future::block_on(async {
            fs.move_file(Path::new("dir1/file.md"), Path::new("dir2/file.md"))
                .await
                .unwrap();
        });

        assert_eq!(moved_count.load(Ordering::SeqCst), 1);
    }
}
