//! Decorator stack builder for composing filesystem decorators.
//!
//! This module provides [`DecoratedFsBuilder`] for building a composable stack
//! of filesystem decorators, and [`DecoratedFs`] which holds the decorated
//! filesystem along with handles for runtime control.
//!
//! # Architecture
//!
//! The decorator stack follows this pattern:
//!
//! ```text
//! EventEmittingFs -> CrdtFs -> BaseFs (OPFS/IndexedDB/Native)
//!                       ↓
//!               WorkspaceCrdt.observe_updates()
//!                       ↓
//!               RustSyncBridge (syncs to server)
//! ```
//!
//! # Feature Gate
//!
//! This module requires the `crdt` feature to be enabled.

use std::sync::Arc;

use crate::crdt::{BodyDocManager, CrdtStorage, MemoryStorage, WorkspaceCrdt};
use crate::fs::AsyncFileSystem;

use super::callback_registry::CallbackRegistry;
use super::crdt_fs::CrdtFs;
use super::event_fs::EventEmittingFs;

/// A fully decorated filesystem with runtime control handles.
///
/// This struct contains:
/// - The decorated filesystem stack
/// - Handles for runtime control (enable/disable CRDT, events)
/// - Access to CRDTs for observer registration
///
/// # Example
///
/// ```ignore
/// use diaryx_core::fs::{DecoratedFsBuilder, InMemoryFileSystem, SyncToAsyncFs};
/// use diaryx_core::crdt::MemoryStorage;
/// use std::sync::Arc;
///
/// let base_fs = SyncToAsyncFs::new(InMemoryFileSystem::new());
/// let storage = Arc::new(MemoryStorage::new());
///
/// let decorated = DecoratedFsBuilder::new(base_fs)
///     .with_crdt(storage)
///     .build();
///
/// // Use the filesystem
/// decorated.fs.write_file(Path::new("test.md"), "content").await?;
///
/// // Access runtime controls
/// decorated.set_crdt_enabled(false);
/// decorated.set_events_enabled(false);
///
/// // Subscribe to events
/// decorated.on_event(Arc::new(|event| println!("{:?}", event)));
/// ```
pub struct DecoratedFs<FS: AsyncFileSystem> {
    /// The fully decorated filesystem stack.
    /// Stack: EventEmittingFs<CrdtFs<FS>>
    pub fs: EventEmittingFs<CrdtFs<FS>>,

    /// The workspace CRDT for file metadata.
    pub workspace_crdt: Arc<WorkspaceCrdt>,

    /// The body document manager for file content.
    pub body_doc_manager: Arc<BodyDocManager>,

    /// The event callback registry.
    pub event_registry: Arc<CallbackRegistry>,

    /// The CRDT storage backend.
    pub storage: Arc<dyn CrdtStorage>,
}

impl<FS: AsyncFileSystem> DecoratedFs<FS> {
    /// Enable or disable CRDT updates.
    pub fn set_crdt_enabled(&self, enabled: bool) {
        self.fs.inner().set_enabled(enabled);
    }

    /// Check if CRDT updates are enabled.
    pub fn is_crdt_enabled(&self) -> bool {
        self.fs.inner().is_enabled()
    }

    /// Enable or disable event emission.
    pub fn set_events_enabled(&self, enabled: bool) {
        self.fs.set_enabled(enabled);
    }

    /// Check if event emission is enabled.
    pub fn is_events_enabled(&self) -> bool {
        self.fs.is_enabled()
    }

    /// Subscribe to filesystem events.
    ///
    /// Returns a subscription ID that can be used to unsubscribe.
    pub fn on_event(
        &self,
        callback: super::callback_registry::EventCallback,
    ) -> super::callback_registry::SubscriptionId {
        self.fs.on_event(callback)
    }

    /// Unsubscribe from filesystem events.
    pub fn off_event(&self, id: super::callback_registry::SubscriptionId) -> bool {
        self.fs.off_event(id)
    }

    /// Get a reference to the inner base filesystem.
    pub fn base_fs(&self) -> &FS {
        self.fs.inner().inner()
    }

    /// Get a reference to the CrdtFs layer.
    pub fn crdt_fs(&self) -> &CrdtFs<FS> {
        self.fs.inner()
    }

    /// Get a reference to the EventEmittingFs layer.
    pub fn event_fs(&self) -> &EventEmittingFs<CrdtFs<FS>> {
        &self.fs
    }
}

// Implement Clone if the inner FS is Clone
impl<FS: AsyncFileSystem + Clone> Clone for DecoratedFs<FS> {
    fn clone(&self) -> Self {
        Self {
            fs: self.fs.clone(),
            workspace_crdt: Arc::clone(&self.workspace_crdt),
            body_doc_manager: Arc::clone(&self.body_doc_manager),
            event_registry: Arc::clone(&self.event_registry),
            storage: Arc::clone(&self.storage),
        }
    }
}

impl<FS: AsyncFileSystem> std::fmt::Debug for DecoratedFs<FS> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecoratedFs")
            .field("crdt_enabled", &self.is_crdt_enabled())
            .field("events_enabled", &self.is_events_enabled())
            .field("workspace_crdt", &self.workspace_crdt)
            .field("body_doc_manager", &self.body_doc_manager)
            .finish()
    }
}

/// Builder for constructing a decorated filesystem stack.
///
/// # Example
///
/// ```ignore
/// use diaryx_core::fs::{DecoratedFsBuilder, InMemoryFileSystem, SyncToAsyncFs};
/// use diaryx_core::crdt::MemoryStorage;
/// use std::sync::Arc;
///
/// let base_fs = SyncToAsyncFs::new(InMemoryFileSystem::new());
/// let storage = Arc::new(MemoryStorage::new());
///
/// // Build with storage
/// let decorated = DecoratedFsBuilder::new(base_fs)
///     .with_crdt(storage)
///     .build();
///
/// // Or build with default in-memory storage
/// let decorated = DecoratedFsBuilder::new(another_fs)
///     .build();
/// ```
pub struct DecoratedFsBuilder<FS: AsyncFileSystem> {
    /// The base filesystem to decorate.
    base: FS,
    /// Optional CRDT storage backend.
    storage: Option<Arc<dyn CrdtStorage>>,
    /// Whether to start with CRDT enabled.
    crdt_enabled: bool,
    /// Whether to start with events enabled.
    events_enabled: bool,
}

impl<FS: AsyncFileSystem> DecoratedFsBuilder<FS> {
    /// Create a new builder with the given base filesystem.
    pub fn new(base: FS) -> Self {
        Self {
            base,
            storage: None,
            crdt_enabled: true,
            events_enabled: true,
        }
    }

    /// Set the CRDT storage backend.
    ///
    /// If not called, an in-memory storage will be used.
    pub fn with_crdt(mut self, storage: Arc<dyn CrdtStorage>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Set whether CRDT updates should be enabled initially.
    ///
    /// Default: `true`
    pub fn crdt_enabled(mut self, enabled: bool) -> Self {
        self.crdt_enabled = enabled;
        self
    }

    /// Set whether event emission should be enabled initially.
    ///
    /// Default: `true`
    pub fn events_enabled(mut self, enabled: bool) -> Self {
        self.events_enabled = enabled;
        self
    }

    /// Build the decorated filesystem stack.
    ///
    /// This creates:
    /// 1. WorkspaceCrdt and BodyDocManager from the storage
    /// 2. CrdtFs wrapping the base filesystem
    /// 3. EventEmittingFs wrapping the CrdtFs
    ///
    /// Returns a `DecoratedFs` with handles for runtime control.
    pub fn build(self) -> DecoratedFs<FS> {
        // Use provided storage or create in-memory storage
        let storage: Arc<dyn CrdtStorage> = self
            .storage
            .unwrap_or_else(|| Arc::new(MemoryStorage::new()));

        // Create CRDT structures
        let workspace_crdt = Arc::new(WorkspaceCrdt::new(Arc::clone(&storage)));
        let body_doc_manager = Arc::new(BodyDocManager::new(Arc::clone(&storage)));

        // Build the decorator stack
        let crdt_fs = CrdtFs::new(
            self.base,
            Arc::clone(&workspace_crdt),
            Arc::clone(&body_doc_manager),
        );
        crdt_fs.set_enabled(self.crdt_enabled);

        let event_registry = Arc::new(CallbackRegistry::new());
        let event_fs = EventEmittingFs::with_registry(crdt_fs, Arc::clone(&event_registry));
        event_fs.set_enabled(self.events_enabled);

        DecoratedFs {
            fs: event_fs,
            workspace_crdt,
            body_doc_manager,
            event_registry,
            storage,
        }
    }

    /// Build the decorated filesystem stack, loading existing CRDT state from storage.
    ///
    /// Unlike `build()`, this loads any existing state from the storage backend,
    /// which is important for persistence across sessions.
    ///
    /// # Errors
    ///
    /// Returns an error if loading from storage fails.
    pub fn build_with_load(self) -> crate::error::Result<DecoratedFs<FS>> {
        // Use provided storage or create in-memory storage
        let storage: Arc<dyn CrdtStorage> = self
            .storage
            .unwrap_or_else(|| Arc::new(MemoryStorage::new()));

        // Create CRDT structures, loading from storage
        let workspace_crdt = Arc::new(WorkspaceCrdt::load(Arc::clone(&storage))?);
        let body_doc_manager = Arc::new(BodyDocManager::new(Arc::clone(&storage)));

        // Build the decorator stack
        let crdt_fs = CrdtFs::new(
            self.base,
            Arc::clone(&workspace_crdt),
            Arc::clone(&body_doc_manager),
        );
        crdt_fs.set_enabled(self.crdt_enabled);

        let event_registry = Arc::new(CallbackRegistry::new());
        let event_fs = EventEmittingFs::with_registry(crdt_fs, Arc::clone(&event_registry));
        event_fs.set_enabled(self.events_enabled);

        Ok(DecoratedFs {
            fs: event_fs,
            workspace_crdt,
            body_doc_manager,
            event_registry,
            storage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{InMemoryFileSystem, SyncToAsyncFs};
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn create_test_base_fs() -> SyncToAsyncFs<InMemoryFileSystem> {
        SyncToAsyncFs::new(InMemoryFileSystem::new())
    }

    #[test]
    fn test_build_with_default_storage() {
        let base = create_test_base_fs();
        let decorated = DecoratedFsBuilder::new(base).build();

        assert!(decorated.is_crdt_enabled());
        assert!(decorated.is_events_enabled());
    }

    #[test]
    fn test_build_with_custom_storage() {
        let base = create_test_base_fs();
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());

        let decorated = DecoratedFsBuilder::new(base).with_crdt(storage).build();

        assert!(decorated.is_crdt_enabled());
    }

    #[test]
    fn test_build_with_disabled_crdt() {
        let base = create_test_base_fs();
        let decorated = DecoratedFsBuilder::new(base).crdt_enabled(false).build();

        assert!(!decorated.is_crdt_enabled());
    }

    #[test]
    fn test_build_with_disabled_events() {
        let base = create_test_base_fs();
        let decorated = DecoratedFsBuilder::new(base).events_enabled(false).build();

        assert!(!decorated.is_events_enabled());
    }

    #[test]
    fn test_runtime_toggle_crdt() {
        let base = create_test_base_fs();
        let decorated = DecoratedFsBuilder::new(base).build();

        assert!(decorated.is_crdt_enabled());
        decorated.set_crdt_enabled(false);
        assert!(!decorated.is_crdt_enabled());
        decorated.set_crdt_enabled(true);
        assert!(decorated.is_crdt_enabled());
    }

    #[test]
    fn test_runtime_toggle_events() {
        let base = create_test_base_fs();
        let decorated = DecoratedFsBuilder::new(base).build();

        assert!(decorated.is_events_enabled());
        decorated.set_events_enabled(false);
        assert!(!decorated.is_events_enabled());
        decorated.set_events_enabled(true);
        assert!(decorated.is_events_enabled());
    }

    #[test]
    fn test_event_subscription() {
        let base = create_test_base_fs();
        let decorated = DecoratedFsBuilder::new(base).build();

        let event_count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&event_count);

        let id = decorated.on_event(Arc::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        }));

        futures_lite::future::block_on(async {
            decorated
                .fs
                .write_file(Path::new("test.md"), "content")
                .await
                .unwrap();
        });

        assert_eq!(event_count.load(Ordering::SeqCst), 1);

        // Unsubscribe
        assert!(decorated.off_event(id));

        futures_lite::future::block_on(async {
            decorated
                .fs
                .write_file(Path::new("test2.md"), "content")
                .await
                .unwrap();
        });

        // Count should not have increased
        assert_eq!(event_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_crdt_updates_on_write() {
        let base = create_test_base_fs();
        let decorated = DecoratedFsBuilder::new(base).build();

        futures_lite::future::block_on(async {
            decorated
                .fs
                .write_file(
                    Path::new("test.md"),
                    "---\ntitle: Test File\n---\nBody content",
                )
                .await
                .unwrap();
        });

        // Check CRDT was updated
        let metadata = decorated.workspace_crdt.get_file("test.md").unwrap();
        assert_eq!(metadata.title, Some("Test File".to_string()));
    }

    #[test]
    fn test_crdt_disabled_skips_updates() {
        let base = create_test_base_fs();
        let decorated = DecoratedFsBuilder::new(base).crdt_enabled(false).build();

        futures_lite::future::block_on(async {
            decorated
                .fs
                .write_file(Path::new("test.md"), "---\ntitle: Test\n---\nBody")
                .await
                .unwrap();
        });

        // CRDT should not have the file
        assert!(decorated.workspace_crdt.get_file("test.md").is_none());
    }

    #[test]
    fn test_build_with_load() {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());

        // First build - create some data
        {
            let base = create_test_base_fs();
            let decorated = DecoratedFsBuilder::new(base)
                .with_crdt(Arc::clone(&storage))
                .build();

            futures_lite::future::block_on(async {
                decorated
                    .fs
                    .write_file(Path::new("test.md"), "---\ntitle: Persistent\n---\nBody")
                    .await
                    .unwrap();
            });

            // Save state
            decorated.workspace_crdt.save().unwrap();
        }

        // Second build - load existing data
        {
            let base = create_test_base_fs();
            let decorated = DecoratedFsBuilder::new(base)
                .with_crdt(Arc::clone(&storage))
                .build_with_load()
                .unwrap();

            // Should have the data from the first session
            let metadata = decorated.workspace_crdt.get_file("test.md").unwrap();
            assert_eq!(metadata.title, Some("Persistent".to_string()));
        }
    }
}
