//! Workspace CRDT document for synchronizing file hierarchy.
//!
//! This module provides [`WorkspaceCrdt`], which wraps a yrs [`Doc`] to manage
//! the workspace's file hierarchy as a conflict-free replicated data type.
//!
//! # Structure
//!
//! The workspace document contains a single Y.Map called "files" that maps
//! file paths to their metadata:
//!
//! ```text
//! Y.Doc
//! └── Y.Map "files"
//!     ├── "workspace/index.md" → FileMetadata { title: "Home", ... }
//!     ├── "workspace/Daily/index.md" → FileMetadata { title: "Daily", ... }
//!     └── ...
//! ```
//!
//! # Synchronization
//!
//! The workspace CRDT supports the Y-sync protocol for synchronization with
//! Hocuspocus servers and other peers. Use [`encode_state_vector`] and
//! [`encode_state_as_update`] for the sync handshake, and [`apply_update`]
//! to integrate remote changes.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Doc, Map, MapRef, Observable, ReadTxn, StateVector, Transact, Update};

use super::storage::{CrdtStorage, StorageResult};
use super::types::{CrdtUpdate, FileMetadata, UpdateOrigin};
use crate::error::DiaryxError;
use crate::fs::FileSystemEvent;

/// The name of the Y.Map containing file metadata.
const FILES_MAP_NAME: &str = "files";

/// The document name used for workspace storage.
const WORKSPACE_DOC_NAME: &str = "workspace";

/// A CRDT document representing the workspace file hierarchy.
///
/// This wraps a yrs [`Doc`] and provides methods for managing file metadata
/// in a conflict-free manner across multiple clients.
pub struct WorkspaceCrdt {
    /// The underlying yrs document
    doc: Doc,

    /// Reference to the files map (cached for efficiency)
    files_map: MapRef,

    /// Storage backend for persistence
    storage: Arc<dyn CrdtStorage>,

    /// Document name for storage operations
    doc_name: String,

    /// Optional callback for emitting filesystem events on remote/sync updates.
    /// This enables unified event handling for both local and remote changes.
    event_callback: Option<Arc<dyn Fn(&FileSystemEvent) + Send + Sync>>,
}

impl WorkspaceCrdt {
    /// Create a new empty workspace CRDT with the given storage backend.
    pub fn new(storage: Arc<dyn CrdtStorage>) -> Self {
        Self::with_name(storage, WORKSPACE_DOC_NAME.to_string())
    }

    /// Create a new workspace CRDT with a custom document name.
    pub fn with_name(storage: Arc<dyn CrdtStorage>, doc_name: String) -> Self {
        let doc = Doc::new();
        let files_map = doc.get_or_insert_map(FILES_MAP_NAME);

        Self {
            doc,
            files_map,
            storage,
            doc_name,
            event_callback: None,
        }
    }

    /// Load an existing workspace CRDT from storage.
    ///
    /// If no document exists in storage, returns a new empty workspace.
    pub fn load(storage: Arc<dyn CrdtStorage>) -> StorageResult<Self> {
        Self::load_with_name(storage, WORKSPACE_DOC_NAME.to_string())
    }

    /// Load a workspace CRDT with a custom document name from storage.
    ///
    /// This loads both the base snapshot (if any) and all incremental updates
    /// to reconstruct the current state.
    pub fn load_with_name(storage: Arc<dyn CrdtStorage>, doc_name: String) -> StorageResult<Self> {
        let doc = Doc::new();

        {
            let mut txn = doc.transact_mut();

            // Try to load base snapshot from storage
            if let Some(state) = storage.load_doc(&doc_name)? {
                let update = Update::decode_v1(&state).map_err(|e| {
                    DiaryxError::Unsupported(format!("Failed to decode CRDT state: {}", e))
                })?;
                txn.apply_update(update).map_err(|e| {
                    DiaryxError::Unsupported(format!("Failed to apply snapshot: {}", e))
                })?;
            }

            // Apply all incremental updates from storage
            // This is critical for WASM where updates are stored but snapshots may not be saved
            let updates = storage.get_all_updates(&doc_name)?;
            for crdt_update in updates {
                if let Ok(update) = Update::decode_v1(&crdt_update.data) {
                    if let Err(e) = txn.apply_update(update) {
                        log::warn!(
                            "Failed to apply stored update {} for {}: {}",
                            crdt_update.update_id,
                            doc_name,
                            e
                        );
                    }
                }
            }
        }

        let files_map = doc.get_or_insert_map(FILES_MAP_NAME);

        Ok(Self {
            doc,
            files_map,
            storage,
            doc_name,
            event_callback: None,
        })
    }

    /// Set the event callback for emitting filesystem events on remote/sync updates.
    ///
    /// When set, this callback will be invoked with `FileSystemEvent`s whenever
    /// `apply_update()` is called with a non-Local origin. This enables unified
    /// event handling where the UI responds the same way to both local and remote changes.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut crdt = WorkspaceCrdt::load(storage)?;
    /// crdt.set_event_callback(Arc::new(|event| {
    ///     println!("Remote change: {:?}", event);
    /// }));
    /// ```
    pub fn set_event_callback(&mut self, callback: Arc<dyn Fn(&FileSystemEvent) + Send + Sync>) {
        self.event_callback = Some(callback);
    }

    /// Emit a filesystem event to the registered callback, if any.
    fn emit_event(&self, event: FileSystemEvent) {
        if let Some(ref cb) = self.event_callback {
            cb(&event);
        }
    }

    /// Get the underlying yrs document.
    pub fn doc(&self) -> &Doc {
        &self.doc
    }

    /// Get the document name used for storage.
    pub fn doc_name(&self) -> &str {
        &self.doc_name
    }

    /// Get a reference to the storage backend.
    pub fn storage(&self) -> &Arc<dyn CrdtStorage> {
        &self.storage
    }

    // ==================== File Operations ====================

    /// Get metadata for a file at the given path.
    pub fn get_file(&self, path: &str) -> Option<FileMetadata> {
        let txn = self.doc.transact();

        self.files_map.get(&txn, path).and_then(|value| {
            let json = value.to_string(&txn);
            serde_json::from_str(&json).ok()
        })
    }

    /// Set metadata for a file at the given path.
    ///
    /// This will create a new entry or update an existing one.
    /// The change is automatically recorded in the update history.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails to persist to storage.
    pub fn set_file(&self, path: &str, metadata: FileMetadata) -> StorageResult<()> {
        // Get state vector before the change
        let sv_before = {
            let txn = self.doc.transact();
            txn.state_vector()
        };

        // Make the change
        {
            let mut txn = self.doc.transact_mut();
            let json = serde_json::to_string(&metadata).unwrap_or_default();
            self.files_map.insert(&mut txn, path, json);
        }

        // Capture the incremental update and store it
        let update = {
            let txn = self.doc.transact();
            txn.encode_state_as_update_v1(&sv_before)
        };

        if !update.is_empty() {
            self.storage
                .append_update(&self.doc_name, &update, UpdateOrigin::Local)?;
        }
        Ok(())
    }

    /// Mark a file as deleted (soft delete).
    ///
    /// This sets the `deleted` flag to true rather than removing the entry,
    /// which is important for proper CRDT tombstone handling.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails to persist to storage.
    pub fn delete_file(&self, path: &str) -> StorageResult<()> {
        if let Some(mut metadata) = self.get_file(path) {
            metadata.mark_deleted();
            self.set_file(path, metadata)?;
        }
        Ok(())
    }

    /// Remove a file entry completely from the CRDT.
    ///
    /// **Warning**: This should generally not be used. Prefer [`delete_file`]
    /// for proper tombstone handling. Use this only for garbage collection
    /// of very old deleted entries.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails to persist to storage.
    pub fn remove_file(&self, path: &str) -> StorageResult<()> {
        // Get state vector before the change
        let sv_before = {
            let txn = self.doc.transact();
            txn.state_vector()
        };

        // Make the change
        {
            let mut txn = self.doc.transact_mut();
            self.files_map.remove(&mut txn, path);
        }

        // Capture the incremental update and store it
        let update = {
            let txn = self.doc.transact();
            txn.encode_state_as_update_v1(&sv_before)
        };

        if !update.is_empty() {
            self.storage
                .append_update(&self.doc_name, &update, UpdateOrigin::Local)?;
        }
        Ok(())
    }

    /// List all files in the workspace.
    ///
    /// Returns a vector of (path, metadata) tuples for all files,
    /// including deleted ones (check `metadata.deleted`).
    pub fn list_files(&self) -> Vec<(String, FileMetadata)> {
        let txn = self.doc.transact();

        self.files_map
            .iter(&txn)
            .filter_map(|(key, value)| {
                let path = key.to_string();
                let json = value.to_string(&txn);
                let metadata: FileMetadata = serde_json::from_str(&json).ok()?;
                Some((path, metadata))
            })
            .collect()
    }

    /// List all non-deleted files in the workspace.
    pub fn list_active_files(&self) -> Vec<(String, FileMetadata)> {
        self.list_files()
            .into_iter()
            .filter(|(_, meta)| !meta.deleted)
            .collect()
    }

    /// Get the number of files in the workspace (including deleted).
    pub fn file_count(&self) -> usize {
        let txn = self.doc.transact();
        self.files_map.len(&txn) as usize
    }

    // ==================== Sync Operations ====================

    /// Encode the current state vector for sync handshake.
    ///
    /// Send this to a remote peer to initiate synchronization.
    /// The remote peer will use it to compute what updates you're missing.
    pub fn encode_state_vector(&self) -> Vec<u8> {
        let txn = self.doc.transact();
        txn.state_vector().encode_v1()
    }

    /// Encode the full document state as an update.
    ///
    /// This returns a binary blob that can be applied to another document
    /// to bring it up to date with this one.
    pub fn encode_state_as_update(&self) -> Vec<u8> {
        let txn = self.doc.transact();
        txn.encode_state_as_update_v1(&StateVector::default())
    }

    /// Encode only the updates that the remote peer is missing.
    ///
    /// Given the remote peer's state vector, this computes and returns
    /// only the updates they don't have yet.
    pub fn encode_diff(&self, remote_state_vector: &[u8]) -> StorageResult<Vec<u8>> {
        let sv = StateVector::decode_v1(remote_state_vector).map_err(|e| {
            DiaryxError::Unsupported(format!("Failed to decode state vector: {}", e))
        })?;

        let txn = self.doc.transact();
        Ok(txn.encode_state_as_update_v1(&sv))
    }

    /// Apply an update from a remote peer.
    ///
    /// Returns the update ID if the update was persisted to storage.
    ///
    /// For non-Local origins (Remote, Sync), this method will detect what changed
    /// and emit corresponding `FileSystemEvent`s via the event callback. This enables
    /// unified event handling where the UI responds the same way to both local and
    /// remote changes.
    pub fn apply_update(&self, update: &[u8], origin: UpdateOrigin) -> StorageResult<Option<i64>> {
        // Only emit events for remote/sync updates (Local updates emit via CrdtFs)
        let should_emit = origin != UpdateOrigin::Local && self.event_callback.is_some();

        // Capture state before the update (only if we need to emit events)
        let files_before: HashMap<String, FileMetadata> = if should_emit {
            self.list_files().into_iter().collect()
        } else {
            HashMap::new()
        };

        // Decode and apply the update
        let decoded = Update::decode_v1(update)
            .map_err(|e| DiaryxError::Unsupported(format!("Failed to decode update: {}", e)))?;

        {
            let mut txn = self.doc.transact_mut();
            txn.apply_update(decoded)
                .map_err(|e| DiaryxError::Unsupported(format!("Failed to apply update: {}", e)))?;
        }

        // Diff and emit events for changes
        if should_emit {
            let files_after: HashMap<String, FileMetadata> =
                self.list_files().into_iter().collect();
            self.emit_diff_events(&files_before, &files_after);
        }

        // Persist the update to storage
        let update_id = self.storage.append_update(&self.doc_name, update, origin)?;
        Ok(Some(update_id))
    }

    /// Emit filesystem events for changes between two states.
    ///
    /// This compares the before and after states and emits appropriate events:
    /// - `FileCreated` for new, non-deleted files
    /// - `FileDeleted` for files that were deleted (or newly marked as deleted)
    /// - `MetadataChanged` for files whose metadata changed
    fn emit_diff_events(
        &self,
        before: &HashMap<String, FileMetadata>,
        after: &HashMap<String, FileMetadata>,
    ) {
        // Detect created files (in after but not in before, and not deleted)
        for (path, metadata) in after {
            if !before.contains_key(path) && !metadata.deleted {
                self.emit_event(FileSystemEvent::file_created_with_metadata(
                    PathBuf::from(path),
                    Some(self.metadata_to_frontmatter(metadata)),
                    metadata.part_of.as_ref().map(PathBuf::from),
                ));
            }
        }

        // Detect deleted files (was not deleted before, is deleted now or removed)
        for (path, old_meta) in before {
            let is_deleted = after.get(path).map(|m| m.deleted).unwrap_or(true);
            if !old_meta.deleted && is_deleted {
                self.emit_event(FileSystemEvent::file_deleted_with_parent(
                    PathBuf::from(path),
                    old_meta.part_of.as_ref().map(PathBuf::from),
                ));
            }
        }

        // Detect metadata changes (file exists in both, metadata differs, not deleted)
        for (path, new_meta) in after {
            if let Some(old_meta) = before.get(path) {
                if old_meta != new_meta && !new_meta.deleted && !old_meta.deleted {
                    self.emit_event(FileSystemEvent::metadata_changed(
                        PathBuf::from(path),
                        self.metadata_to_frontmatter(new_meta),
                    ));
                }
            }
        }
    }

    /// Convert FileMetadata to a serde_json::Value for event frontmatter.
    fn metadata_to_frontmatter(&self, metadata: &FileMetadata) -> serde_json::Value {
        // Serialize the metadata to JSON, handling any errors gracefully
        serde_json::to_value(metadata).unwrap_or_else(|_| {
            // Fallback: create a minimal object with just the title
            serde_json::json!({
                "title": metadata.title
            })
        })
    }

    // ==================== Persistence ====================

    /// Save the current document state to storage.
    pub fn save(&self) -> StorageResult<()> {
        let state = self.encode_state_as_update();
        self.storage.save_doc(&self.doc_name, &state)
    }

    /// Reload the document state from storage, discarding local changes.
    pub fn reload(&mut self) -> StorageResult<()> {
        if let Some(state) = self.storage.load_doc(&self.doc_name)? {
            let update = Update::decode_v1(&state).map_err(|e| {
                DiaryxError::Unsupported(format!("Failed to decode CRDT state: {}", e))
            })?;

            // Create a fresh doc and apply the stored state
            self.doc = Doc::new();
            self.files_map = self.doc.get_or_insert_map(FILES_MAP_NAME);
            let mut txn = self.doc.transact_mut();
            txn.apply_update(update)
                .map_err(|e| DiaryxError::Unsupported(format!("Failed to apply update: {}", e)))?;
        }
        Ok(())
    }

    // ==================== History ====================

    /// Get all updates from storage for this document.
    pub fn get_history(&self) -> StorageResult<Vec<CrdtUpdate>> {
        self.storage.get_all_updates(&self.doc_name)
    }

    /// Get updates since a specific update ID.
    pub fn get_updates_since(&self, since_id: i64) -> StorageResult<Vec<CrdtUpdate>> {
        self.storage.get_updates_since(&self.doc_name, since_id)
    }

    /// Get the latest update ID.
    pub fn get_latest_update_id(&self) -> StorageResult<i64> {
        self.storage.get_latest_update_id(&self.doc_name)
    }

    // ==================== Observers ====================

    /// Subscribe to document updates.
    ///
    /// The callback receives the binary update data whenever the document changes.
    /// Returns a subscription that will unsubscribe when dropped.
    ///
    /// # Panics
    ///
    /// Panics if unable to acquire transaction for observing.
    pub fn observe_updates<F>(&self, callback: F) -> yrs::Subscription
    where
        F: Fn(&[u8]) + 'static,
    {
        self.doc
            .observe_update_v1(move |_txn, event| {
                callback(&event.update);
            })
            .expect("Failed to observe document updates")
    }

    /// Subscribe to changes in the files map.
    ///
    /// The callback receives the path and new metadata (or None if removed)
    /// for each changed file.
    pub fn observe_files<F>(&self, callback: F) -> yrs::Subscription
    where
        F: Fn(Vec<(String, Option<FileMetadata>)>) + 'static,
    {
        self.files_map.observe(move |txn, event| {
            let changes: Vec<(String, Option<FileMetadata>)> = event
                .keys(txn)
                .iter()
                .map(|(key, change)| {
                    let path = key.to_string();
                    match change {
                        yrs::types::EntryChange::Inserted(value)
                        | yrs::types::EntryChange::Updated(_, value) => {
                            let json = value.clone().cast::<String>().unwrap_or_default();
                            let metadata: Option<FileMetadata> = serde_json::from_str(&json).ok();
                            (path, metadata)
                        }
                        yrs::types::EntryChange::Removed(_) => (path, None),
                    }
                })
                .collect();

            if !changes.is_empty() {
                callback(changes);
            }
        })
    }
}

impl std::fmt::Debug for WorkspaceCrdt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceCrdt")
            .field("doc_name", &self.doc_name)
            .field("file_count", &self.file_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crdt::MemoryStorage;

    fn create_test_crdt() -> WorkspaceCrdt {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        WorkspaceCrdt::new(storage)
    }

    #[test]
    fn test_new_workspace_is_empty() {
        let crdt = create_test_crdt();
        assert_eq!(crdt.file_count(), 0);
        assert!(crdt.list_files().is_empty());
    }

    #[test]
    fn test_set_and_get_file() {
        let crdt = create_test_crdt();

        let metadata = FileMetadata::new(Some("Test File".to_string()));
        crdt.set_file("test.md", metadata.clone()).unwrap();

        let retrieved = crdt.get_file("test.md").unwrap();
        assert_eq!(retrieved.title, Some("Test File".to_string()));
    }

    #[test]
    fn test_get_nonexistent_file() {
        let crdt = create_test_crdt();
        assert!(crdt.get_file("nonexistent.md").is_none());
    }

    #[test]
    fn test_update_file() {
        let crdt = create_test_crdt();

        let mut metadata = FileMetadata::new(Some("Original".to_string()));
        crdt.set_file("test.md", metadata.clone()).unwrap();

        metadata.title = Some("Updated".to_string());
        crdt.set_file("test.md", metadata).unwrap();

        let retrieved = crdt.get_file("test.md").unwrap();
        assert_eq!(retrieved.title, Some("Updated".to_string()));
        assert_eq!(crdt.file_count(), 1);
    }

    #[test]
    fn test_delete_file() {
        let crdt = create_test_crdt();

        let metadata = FileMetadata::new(Some("To Delete".to_string()));
        crdt.set_file("test.md", metadata).unwrap();

        crdt.delete_file("test.md").unwrap();

        let retrieved = crdt.get_file("test.md").unwrap();
        assert!(retrieved.deleted);
        assert_eq!(crdt.file_count(), 1);
    }

    #[test]
    fn test_list_active_files() {
        let crdt = create_test_crdt();

        crdt.set_file("active.md", FileMetadata::new(Some("Active".to_string())))
            .unwrap();
        crdt.set_file("deleted.md", FileMetadata::new(Some("Deleted".to_string())))
            .unwrap();
        crdt.delete_file("deleted.md").unwrap();

        let all = crdt.list_files();
        assert_eq!(all.len(), 2);

        let active = crdt.list_active_files();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].0, "active.md");
    }

    #[test]
    fn test_remove_file() {
        let crdt = create_test_crdt();

        crdt.set_file("test.md", FileMetadata::new(Some("Test".to_string())))
            .unwrap();
        assert_eq!(crdt.file_count(), 1);

        crdt.remove_file("test.md").unwrap();
        assert_eq!(crdt.file_count(), 0);
        assert!(crdt.get_file("test.md").is_none());
    }

    #[test]
    fn test_encode_and_apply_update() {
        let crdt1 = create_test_crdt();
        let crdt2 = create_test_crdt();

        crdt1
            .set_file("file1.md", FileMetadata::new(Some("File 1".to_string())))
            .unwrap();
        crdt1
            .set_file("file2.md", FileMetadata::new(Some("File 2".to_string())))
            .unwrap();

        let update = crdt1.encode_state_as_update();
        crdt2.apply_update(&update, UpdateOrigin::Remote).unwrap();

        assert_eq!(crdt2.file_count(), 2);
        assert!(crdt2.get_file("file1.md").is_some());
        assert!(crdt2.get_file("file2.md").is_some());
    }

    #[test]
    fn test_encode_diff() {
        let crdt1 = create_test_crdt();
        let crdt2 = create_test_crdt();

        crdt1
            .set_file("file1.md", FileMetadata::new(Some("File 1".to_string())))
            .unwrap();

        let update = crdt1.encode_state_as_update();
        crdt2.apply_update(&update, UpdateOrigin::Sync).unwrap();

        crdt1
            .set_file("file2.md", FileMetadata::new(Some("File 2".to_string())))
            .unwrap();

        let sv = crdt2.encode_state_vector();
        let diff = crdt1.encode_diff(&sv).unwrap();

        crdt2.apply_update(&diff, UpdateOrigin::Remote).unwrap();

        assert_eq!(crdt2.file_count(), 2);
    }

    #[test]
    fn test_save_and_load() {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());

        {
            let crdt1 = WorkspaceCrdt::new(Arc::clone(&storage));
            crdt1
                .set_file("file1.md", FileMetadata::new(Some("File 1".to_string())))
                .unwrap();
            crdt1
                .set_file("file2.md", FileMetadata::new(Some("File 2".to_string())))
                .unwrap();
            crdt1.save().unwrap();
        }

        let crdt2 = WorkspaceCrdt::load(storage).unwrap();
        assert_eq!(crdt2.file_count(), 2);
        assert_eq!(
            crdt2.get_file("file1.md").unwrap().title,
            Some("File 1".to_string())
        );
    }

    #[test]
    fn test_concurrent_edits_merge() {
        let storage1: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        let storage2: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());

        let crdt1 = WorkspaceCrdt::new(storage1);
        let crdt2 = WorkspaceCrdt::new(storage2);

        crdt1
            .set_file(
                "file1.md",
                FileMetadata::new(Some("From CRDT1".to_string())),
            )
            .unwrap();
        crdt2
            .set_file(
                "file2.md",
                FileMetadata::new(Some("From CRDT2".to_string())),
            )
            .unwrap();

        let update1 = crdt1.encode_state_as_update();
        let update2 = crdt2.encode_state_as_update();

        crdt1.apply_update(&update2, UpdateOrigin::Remote).unwrap();
        crdt2.apply_update(&update1, UpdateOrigin::Remote).unwrap();

        assert_eq!(crdt1.file_count(), 2);
        assert_eq!(crdt2.file_count(), 2);
        assert!(crdt1.get_file("file1.md").is_some());
        assert!(crdt1.get_file("file2.md").is_some());
        assert!(crdt2.get_file("file1.md").is_some());
        assert!(crdt2.get_file("file2.md").is_some());
    }

    #[test]
    fn test_file_metadata_with_contents() {
        let crdt = create_test_crdt();

        let mut metadata = FileMetadata::new(Some("Index".to_string()));
        metadata.part_of = None;
        metadata.contents = Some(vec!["child1.md".to_string(), "child2.md".to_string()]);
        metadata.audience = Some(vec!["public".to_string()]);

        crdt.set_file("index.md", metadata).unwrap();

        let retrieved = crdt.get_file("index.md").unwrap();
        assert_eq!(retrieved.contents.unwrap().len(), 2);
        assert_eq!(retrieved.audience.unwrap(), vec!["public"]);
    }

    #[test]
    fn test_observer_fires_on_change() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let crdt = create_test_crdt();
        let changes = Rc::new(RefCell::new(Vec::new()));
        let changes_clone = Rc::clone(&changes);

        let _sub = crdt.observe_files(move |file_changes| {
            changes_clone.borrow_mut().extend(file_changes);
        });

        crdt.set_file("test.md", FileMetadata::new(Some("Test".to_string())))
            .unwrap();

        let captured = changes.borrow();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].0, "test.md");
    }
}
