//! Manager for multiple per-file body documents.
//!
//! This module provides `BodyDocManager`, which coordinates multiple `BodyDoc`
//! instances for a workspace. It handles loading, caching, and lifecycle
//! management of document CRDTs.
//!
//! # Doc-ID Based Keying
//!
//! Body documents are keyed by stable document IDs (UUIDs) rather than file paths.
//! This means:
//! - Renames don't require renaming body documents (doc_id is stable)
//! - The `rename` method becomes a legacy compatibility wrapper
//! - Use `get_or_create(doc_id)` with the file's doc_id from WorkspaceCrdt

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::body_doc::BodyDoc;
use super::storage::{CrdtStorage, StorageResult};
use super::types::UpdateOrigin;
use crate::fs::FileSystemEvent;

/// Manager for multiple body document CRDTs.
///
/// The manager provides:
/// - Lazy loading of documents on first access
/// - Caching of loaded documents
/// - Batch operations across documents
/// - Coordination with workspace CRDT
///
/// # Example
///
/// ```ignore
/// use diaryx_core::crdt::{BodyDocManager, MemoryStorage};
/// use std::sync::Arc;
///
/// let storage = Arc::new(MemoryStorage::new());
/// let manager = BodyDocManager::new(storage);
///
/// // Get or create a document
/// let doc = manager.get_or_create("notes/hello.md");
/// doc.set_body("# Hello World");
///
/// // Save all modified documents
/// manager.save_all().unwrap();
/// ```
pub struct BodyDocManager {
    storage: Arc<dyn CrdtStorage>,
    docs: RwLock<HashMap<String, Arc<BodyDoc>>>,
    /// Optional callback for emitting filesystem events on remote/sync updates.
    /// This callback is propagated to each BodyDoc when created.
    event_callback: RwLock<Option<Arc<dyn Fn(&FileSystemEvent) + Send + Sync>>>,
}

impl BodyDocManager {
    /// Create a new body document manager.
    pub fn new(storage: Arc<dyn CrdtStorage>) -> Self {
        Self {
            storage,
            docs: RwLock::new(HashMap::new()),
            event_callback: RwLock::new(None),
        }
    }

    /// Set the event callback for emitting filesystem events on remote/sync updates.
    ///
    /// This callback will be propagated to each BodyDoc when it's created or loaded.
    /// Existing loaded documents will NOT receive the callback - only newly loaded ones.
    pub fn set_event_callback(&self, callback: Arc<dyn Fn(&FileSystemEvent) + Send + Sync>) {
        let mut cb = self.event_callback.write().unwrap();
        *cb = Some(callback);
    }

    /// Apply the event callback to a BodyDoc if one is set.
    fn apply_event_callback(&self, doc: &mut BodyDoc) {
        let cb = self.event_callback.read().unwrap();
        if let Some(ref callback) = *cb {
            doc.set_event_callback(Arc::clone(callback));
        }
    }

    /// Get a document by name, loading from storage if necessary.
    ///
    /// Returns None if the document doesn't exist in storage.
    /// Uses double-checked locking to prevent race conditions.
    pub fn get(&self, doc_name: &str) -> Option<Arc<BodyDoc>> {
        // Fast path: check cache with read lock
        {
            let docs = self.docs.read().unwrap();
            if let Some(doc) = docs.get(doc_name) {
                return Some(Arc::clone(doc));
            }
        }

        // Check if document exists in storage before loading
        match self.storage.load_doc(doc_name) {
            Ok(Some(_)) => {
                // Acquire write lock for potential insertion
                let mut docs = self.docs.write().unwrap();

                // Double-check: another thread may have inserted while we waited
                if let Some(doc) = docs.get(doc_name) {
                    return Some(Arc::clone(doc));
                }

                // Document exists, load it
                match BodyDoc::load(Arc::clone(&self.storage), doc_name.to_string()) {
                    Ok(mut doc) => {
                        // Apply event callback if set
                        self.apply_event_callback(&mut doc);
                        let doc = Arc::new(doc);
                        docs.insert(doc_name.to_string(), Arc::clone(&doc));
                        Some(doc)
                    }
                    Err(_) => None,
                }
            }
            _ => None,
        }
    }

    /// Get a document by name, creating it if it doesn't exist.
    /// Uses double-checked locking to prevent race conditions.
    pub fn get_or_create(&self, doc_name: &str) -> Arc<BodyDoc> {
        // Fast path: check cache with read lock
        {
            let docs = self.docs.read().unwrap();
            if let Some(doc) = docs.get(doc_name) {
                return Arc::clone(doc);
            }
        }

        // Acquire write lock for potential insertion
        let mut docs = self.docs.write().unwrap();

        // Double-check: another thread may have inserted while we waited
        if let Some(doc) = docs.get(doc_name) {
            return Arc::clone(doc);
        }

        // Try to load, or create new
        let mut doc = match BodyDoc::load(Arc::clone(&self.storage), doc_name.to_string()) {
            Ok(doc) => doc,
            Err(_) => BodyDoc::new(Arc::clone(&self.storage), doc_name.to_string()),
        };

        // Apply event callback if set
        self.apply_event_callback(&mut doc);

        let doc = Arc::new(doc);
        docs.insert(doc_name.to_string(), Arc::clone(&doc));
        doc
    }

    /// Create a new document, replacing any existing one.
    pub fn create(&self, doc_name: &str) -> Arc<BodyDoc> {
        let mut doc = BodyDoc::new(Arc::clone(&self.storage), doc_name.to_string());

        // Apply event callback if set
        self.apply_event_callback(&mut doc);

        let doc = Arc::new(doc);

        let mut docs = self.docs.write().unwrap();
        docs.insert(doc_name.to_string(), Arc::clone(&doc));
        doc
    }

    /// Check if a document is loaded in the cache.
    pub fn is_loaded(&self, doc_name: &str) -> bool {
        let docs = self.docs.read().unwrap();
        docs.contains_key(doc_name)
    }

    /// Remove a document from the cache.
    ///
    /// This doesn't delete the document from storage, just unloads it from memory.
    pub fn unload(&self, doc_name: &str) -> Option<Arc<BodyDoc>> {
        let mut docs = self.docs.write().unwrap();
        docs.remove(doc_name)
    }

    /// Get all loaded document names.
    pub fn loaded_docs(&self) -> Vec<String> {
        let docs = self.docs.read().unwrap();
        docs.keys().cloned().collect()
    }

    /// Save all loaded documents to storage.
    pub fn save_all(&self) -> StorageResult<()> {
        let docs = self.docs.read().unwrap();
        for doc in docs.values() {
            doc.save()?;
        }
        Ok(())
    }

    /// Save a specific document to storage.
    pub fn save(&self, doc_name: &str) -> StorageResult<bool> {
        let docs = self.docs.read().unwrap();
        if let Some(doc) = docs.get(doc_name) {
            doc.save()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Apply a remote update to a document.
    ///
    /// Creates the document if it doesn't exist.
    pub fn apply_update(
        &self,
        doc_name: &str,
        update: &[u8],
        origin: UpdateOrigin,
    ) -> StorageResult<Option<i64>> {
        let doc = self.get_or_create(doc_name);
        doc.apply_update(update, origin)
    }

    /// Get the sync state (state vector) for a document.
    pub fn get_sync_state(&self, doc_name: &str) -> Option<Vec<u8>> {
        self.get(doc_name).map(|doc| doc.encode_state_vector())
    }

    /// Get the full state as an update for a document.
    pub fn get_full_state(&self, doc_name: &str) -> Option<Vec<u8>> {
        self.get(doc_name).map(|doc| doc.encode_state_as_update())
    }

    /// Get the diff between a document's current state and a remote state vector.
    pub fn get_diff(&self, doc_name: &str, remote_state_vector: &[u8]) -> StorageResult<Vec<u8>> {
        let doc = self.get_or_create(doc_name);
        doc.encode_diff(remote_state_vector)
    }

    /// Get the number of loaded documents.
    pub fn loaded_count(&self) -> usize {
        let docs = self.docs.read().unwrap();
        docs.len()
    }

    /// Clear all documents from the cache.
    pub fn clear(&self) {
        let mut docs = self.docs.write().unwrap();
        docs.clear();
    }

    /// Rename a body document by copying its content to a new name and deleting the old one.
    ///
    /// This operation:
    /// 1. Renames the document in storage (snapshot + updates)
    /// 2. Renames any cached document in memory
    ///
    /// Used when a file is renamed to migrate its body CRDT to the new path.
    pub fn rename(&self, old_name: &str, new_name: &str) -> StorageResult<()> {
        log::debug!("BodyDocManager: renaming {} to {}", old_name, new_name);

        // First rename in storage
        self.storage.rename_doc(old_name, new_name)?;

        // Then update the cache - remove old entry and add under new name
        let mut docs = self.docs.write().unwrap();
        if let Some(doc) = docs.remove(old_name) {
            // Update the doc_name in the BodyDoc itself
            doc.set_doc_name(new_name.to_string());
            docs.insert(new_name.to_string(), doc);
        }

        log::debug!(
            "BodyDocManager: rename complete {} -> {}",
            old_name,
            new_name
        );
        Ok(())
    }

    /// Delete a body document from storage and cache.
    ///
    /// This removes both the document snapshot and all updates from storage,
    /// and unloads any cached version from memory.
    pub fn delete(&self, doc_name: &str) -> StorageResult<()> {
        log::debug!("BodyDocManager: deleting {}", doc_name);

        // Delete from storage
        self.storage.delete_doc(doc_name)?;

        // Remove from cache
        let mut docs = self.docs.write().unwrap();
        docs.remove(doc_name);

        Ok(())
    }
}

impl std::fmt::Debug for BodyDocManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let docs = self.docs.read().unwrap();
        f.debug_struct("BodyDocManager")
            .field("loaded_docs", &docs.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crdt::MemoryStorage;

    fn create_manager() -> BodyDocManager {
        let storage = Arc::new(MemoryStorage::new());
        BodyDocManager::new(storage)
    }

    #[test]
    fn test_get_or_create_new_doc() {
        let manager = create_manager();

        let doc = manager.get_or_create("test.md");
        assert_eq!(doc.doc_name(), "test.md");
        assert_eq!(doc.get_body(), "");
    }

    #[test]
    fn test_get_returns_cached_doc() {
        let manager = create_manager();

        let doc1 = manager.get_or_create("test.md");
        let _ = doc1.set_body("Hello");

        let doc2 = manager.get("test.md").unwrap();
        assert_eq!(doc2.get_body(), "Hello");

        // Should be the same Arc
        assert!(Arc::ptr_eq(&doc1, &doc2));
    }

    #[test]
    fn test_get_nonexistent_returns_none() {
        let manager = create_manager();
        assert!(manager.get("nonexistent.md").is_none());
    }

    #[test]
    fn test_create_replaces_existing() {
        let manager = create_manager();

        let doc1 = manager.get_or_create("test.md");
        let _ = doc1.set_body("Original");

        let doc2 = manager.create("test.md");
        assert_eq!(doc2.get_body(), "");
        assert!(!Arc::ptr_eq(&doc1, &doc2));
    }

    #[test]
    fn test_is_loaded() {
        let manager = create_manager();

        assert!(!manager.is_loaded("test.md"));
        manager.get_or_create("test.md");
        assert!(manager.is_loaded("test.md"));
    }

    #[test]
    fn test_unload() {
        let manager = create_manager();

        manager.get_or_create("test.md");
        assert!(manager.is_loaded("test.md"));

        manager.unload("test.md");
        assert!(!manager.is_loaded("test.md"));
    }

    #[test]
    fn test_loaded_docs() {
        let manager = create_manager();

        manager.get_or_create("doc1.md");
        manager.get_or_create("doc2.md");
        manager.get_or_create("doc3.md");

        let loaded = manager.loaded_docs();
        assert_eq!(loaded.len(), 3);
        assert!(loaded.contains(&"doc1.md".to_string()));
        assert!(loaded.contains(&"doc2.md".to_string()));
        assert!(loaded.contains(&"doc3.md".to_string()));
    }

    #[test]
    fn test_save_and_reload() {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        let manager = BodyDocManager::new(Arc::clone(&storage));

        // Create and populate a document
        let doc = manager.get_or_create("test.md");
        let _ = doc.set_body("Persistent content");
        manager.save_all().unwrap();

        // Clear cache and reload
        manager.clear();
        assert!(!manager.is_loaded("test.md"));

        let reloaded = manager.get("test.md").unwrap();
        assert_eq!(reloaded.get_body(), "Persistent content");
    }

    #[test]
    fn test_apply_update_creates_doc() {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());

        // Create a source document with content
        let source_doc = BodyDoc::new(Arc::clone(&storage), "source.md".to_string());
        let _ = source_doc.set_body("Synced content");
        let update = source_doc.encode_state_as_update();

        // Apply to manager (creates new doc)
        let manager = BodyDocManager::new(Arc::clone(&storage));
        manager
            .apply_update("target.md", &update, UpdateOrigin::Remote)
            .unwrap();

        let target = manager.get("target.md").unwrap();
        assert_eq!(target.get_body(), "Synced content");
    }

    #[test]
    fn test_loaded_count() {
        let manager = create_manager();

        assert_eq!(manager.loaded_count(), 0);

        manager.get_or_create("doc1.md");
        assert_eq!(manager.loaded_count(), 1);

        manager.get_or_create("doc2.md");
        assert_eq!(manager.loaded_count(), 2);

        manager.unload("doc1.md");
        assert_eq!(manager.loaded_count(), 1);
    }

    #[test]
    fn test_clear() {
        let manager = create_manager();

        manager.get_or_create("doc1.md");
        manager.get_or_create("doc2.md");
        assert_eq!(manager.loaded_count(), 2);

        manager.clear();
        assert_eq!(manager.loaded_count(), 0);
    }

    #[test]
    fn test_get_sync_state() {
        let manager = create_manager();

        // Non-existent doc returns None
        assert!(manager.get_sync_state("nonexistent.md").is_none());

        // Existing doc returns state vector
        manager.get_or_create("test.md");
        let state = manager.get_sync_state("test.md");
        assert!(state.is_some());
    }

    #[test]
    fn test_sync_between_managers() {
        let storage1 = Arc::new(MemoryStorage::new());
        let storage2 = Arc::new(MemoryStorage::new());

        let manager1 = BodyDocManager::new(storage1);
        let manager2 = BodyDocManager::new(storage2);

        // Edit on manager1
        let doc1 = manager1.get_or_create("shared.md");
        let _ = doc1.set_body("Hello from manager1");

        // Sync to manager2
        let update = manager1.get_full_state("shared.md").unwrap();
        manager2
            .apply_update("shared.md", &update, UpdateOrigin::Remote)
            .unwrap();

        // Verify sync
        let doc2 = manager2.get("shared.md").unwrap();
        assert_eq!(doc2.get_body(), "Hello from manager1");
    }

    #[test]
    fn test_rename_preserves_content() {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        let manager = BodyDocManager::new(Arc::clone(&storage));

        // Create and populate a document
        let doc = manager.get_or_create("old_name.md");
        let _ = doc.set_body("Important content");
        let _ = doc.save();

        // Rename
        manager.rename("old_name.md", "new_name.md").unwrap();

        // Old name should be gone
        assert!(!manager.is_loaded("old_name.md"));
        assert!(storage.load_doc("old_name.md").unwrap().is_none());

        // New name should have the content
        assert!(manager.is_loaded("new_name.md"));
        let renamed_doc = manager.get("new_name.md").unwrap();
        assert_eq!(renamed_doc.get_body(), "Important content");
        assert_eq!(renamed_doc.doc_name(), "new_name.md");
    }

    #[test]
    fn test_rename_uncached_doc() {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        let manager = BodyDocManager::new(Arc::clone(&storage));

        // Create document, save it, then clear cache
        let doc = manager.get_or_create("old_name.md");
        let _ = doc.set_body("Persisted content");
        let _ = doc.save();
        manager.clear();
        assert!(!manager.is_loaded("old_name.md"));

        // Rename (doc not in cache)
        manager.rename("old_name.md", "new_name.md").unwrap();

        // Old storage entry should be gone
        assert!(storage.load_doc("old_name.md").unwrap().is_none());

        // New storage entry should exist with content
        assert!(storage.load_doc("new_name.md").unwrap().is_some());
    }

    #[test]
    fn test_delete_removes_from_storage_and_cache() {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        let manager = BodyDocManager::new(Arc::clone(&storage));

        // Create and save a document
        let doc = manager.get_or_create("to_delete.md");
        let _ = doc.set_body("Soon to be deleted");
        let _ = doc.save();

        // Verify it exists
        assert!(manager.is_loaded("to_delete.md"));
        assert!(storage.load_doc("to_delete.md").unwrap().is_some());

        // Delete
        manager.delete("to_delete.md").unwrap();

        // Should be gone from both cache and storage
        assert!(!manager.is_loaded("to_delete.md"));
        assert!(storage.load_doc("to_delete.md").unwrap().is_none());
    }
}
