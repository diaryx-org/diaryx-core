//! Per-file document body CRDT.
//!
//! This module provides `BodyDoc`, a Y.Doc for collaborative editing of
//! individual file contents. Each file in the workspace can have its own
//! BodyDoc for real-time sync of markdown content.

use std::path::PathBuf;
use std::sync::Arc;

use yrs::{
    Doc, GetString, Map, Observable, ReadTxn, Text, Transact, Update, updates::decoder::Decode,
    updates::encoder::Encode,
};

use super::storage::{CrdtStorage, StorageResult};
use super::types::UpdateOrigin;
use crate::error::DiaryxError;
use crate::fs::FileSystemEvent;

/// Name of the Y.Text holding the document body content.
const BODY_TEXT_NAME: &str = "body";

/// Name of the Y.Map holding frontmatter properties.
const FRONTMATTER_MAP_NAME: &str = "frontmatter";

/// A CRDT document for a single file's body content.
///
/// Each file in the workspace can have its own BodyDoc for collaborative
/// editing. The document contains:
/// - A Y.Text for the markdown body content
/// - A Y.Map for frontmatter properties (optional structured access)
///
/// # Example
///
/// ```ignore
/// use diaryx_core::crdt::{BodyDoc, MemoryStorage};
/// use std::sync::Arc;
///
/// let storage = Arc::new(MemoryStorage::new());
/// let doc = BodyDoc::new(storage, "workspace/notes/hello.md".to_string());
///
/// // Set content
/// doc.set_body("# Hello World\n\nThis is my note.");
///
/// // Get content
/// let body = doc.get_body();
/// assert!(body.starts_with("# Hello"));
/// ```
pub struct BodyDoc {
    doc: Doc,
    body_text: yrs::TextRef,
    frontmatter_map: yrs::MapRef,
    storage: Arc<dyn CrdtStorage>,
    doc_name: String,
    /// Optional callback for emitting filesystem events on remote/sync updates.
    event_callback: Option<Arc<dyn Fn(&FileSystemEvent) + Send + Sync>>,
}

impl BodyDoc {
    /// Create a new empty body document.
    ///
    /// The document name should be the file path (e.g., "workspace/notes/hello.md").
    pub fn new(storage: Arc<dyn CrdtStorage>, doc_name: String) -> Self {
        let doc = Doc::new();
        let body_text = doc.get_or_insert_text(BODY_TEXT_NAME);
        let frontmatter_map = doc.get_or_insert_map(FRONTMATTER_MAP_NAME);

        Self {
            doc,
            body_text,
            frontmatter_map,
            storage,
            doc_name,
            event_callback: None,
        }
    }

    /// Load a body document from storage, or create a new one if it doesn't exist.
    pub fn load(storage: Arc<dyn CrdtStorage>, doc_name: String) -> StorageResult<Self> {
        let doc = Doc::new();
        let body_text = doc.get_or_insert_text(BODY_TEXT_NAME);
        let frontmatter_map = doc.get_or_insert_map(FRONTMATTER_MAP_NAME);

        // Try to load existing state
        if let Some(state) = storage.load_doc(&doc_name)?
            && let Ok(update) = Update::decode_v1(&state)
        {
            let mut txn = doc.transact_mut();
            if let Err(e) = txn.apply_update(update) {
                log::warn!(
                    "Failed to apply stored state for body doc {}: {}",
                    doc_name,
                    e
                );
            }
        }

        Ok(Self {
            doc,
            body_text,
            frontmatter_map,
            storage,
            doc_name,
            event_callback: None,
        })
    }

    /// Set the event callback for emitting filesystem events on remote/sync updates.
    ///
    /// When set, this callback will be invoked with `ContentsChanged` events whenever
    /// `apply_update()` is called with a non-Local origin.
    pub fn set_event_callback(&mut self, callback: Arc<dyn Fn(&FileSystemEvent) + Send + Sync>) {
        self.event_callback = Some(callback);
    }

    /// Emit a filesystem event to the registered callback, if any.
    fn emit_event(&self, event: FileSystemEvent) {
        if let Some(ref cb) = self.event_callback {
            cb(&event);
        }
    }

    /// Get the document name (file path).
    pub fn doc_name(&self) -> &str {
        &self.doc_name
    }

    // ==================== Body Content Operations ====================

    /// Get the full body content as a string.
    pub fn get_body(&self) -> String {
        let txn = self.doc.transact();
        self.body_text.get_string(&txn)
    }

    /// Set the body content, using minimal diff operations.
    ///
    /// Instead of delete-all + insert-all (which breaks CRDT sync), this method
    /// calculates the minimal diff between current and new content, applying
    /// only the necessary insert/delete operations. This ensures that Y.js
    /// operation IDs are preserved where content hasn't changed, allowing
    /// proper CRDT merging across clients.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails to persist to storage.
    pub fn set_body(&self, content: &str) -> StorageResult<()> {
        // Get current content and state vector before the change
        let (current, sv_before) = {
            let txn = self.doc.transact();
            (self.body_text.get_string(&txn), txn.state_vector())
        };

        // If content is the same, no-op
        if current == content {
            return Ok(());
        }

        // Calculate minimal diff using common prefix/suffix approach
        let current_chars: Vec<char> = current.chars().collect();
        let new_chars: Vec<char> = content.chars().collect();

        // Find common prefix length
        let common_prefix = current_chars
            .iter()
            .zip(new_chars.iter())
            .take_while(|(a, b)| a == b)
            .count();

        // Find common suffix length (but don't overlap with prefix)
        let remaining_current = current_chars.len() - common_prefix;
        let remaining_new = new_chars.len() - common_prefix;
        let common_suffix = current_chars[common_prefix..]
            .iter()
            .rev()
            .zip(new_chars[common_prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .take(remaining_current.min(remaining_new))
            .count();

        // Calculate the range to delete and text to insert
        let delete_start = common_prefix;
        let delete_end = current_chars.len() - common_suffix;
        let insert_start = common_prefix;
        let insert_end = new_chars.len() - common_suffix;

        // Apply the minimal changes
        {
            let mut txn = self.doc.transact_mut();

            // Delete the changed portion (if any)
            if delete_end > delete_start {
                // Y.js uses byte offsets, so convert char positions to Y.js positions
                // For TextRef, we need the length in Y.js units
                let delete_len = (delete_end - delete_start) as u32;
                self.body_text
                    .remove_range(&mut txn, delete_start as u32, delete_len);
            }

            // Insert the new portion (if any)
            if insert_end > insert_start {
                let insert_text: String = new_chars[insert_start..insert_end].iter().collect();
                self.body_text
                    .insert(&mut txn, delete_start as u32, &insert_text);
            }
        }

        // Capture the incremental update and store it
        self.record_update(&sv_before)
    }

    /// Insert text at a specific position.
    /// The change is automatically recorded in the update history.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails to persist to storage.
    pub fn insert_at(&self, index: u32, text: &str) -> StorageResult<()> {
        let sv_before = {
            let txn = self.doc.transact();
            txn.state_vector()
        };

        {
            let mut txn = self.doc.transact_mut();
            self.body_text.insert(&mut txn, index, text);
        }

        self.record_update(&sv_before)
    }

    /// Delete a range of text.
    /// The change is automatically recorded in the update history.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails to persist to storage.
    pub fn delete_range(&self, index: u32, length: u32) -> StorageResult<()> {
        let sv_before = {
            let txn = self.doc.transact();
            txn.state_vector()
        };

        {
            let mut txn = self.doc.transact_mut();
            self.body_text.remove_range(&mut txn, index, length);
        }

        self.record_update(&sv_before)
    }

    /// Helper to record an update in storage after a mutation.
    fn record_update(&self, sv_before: &yrs::StateVector) -> StorageResult<()> {
        let update = {
            let txn = self.doc.transact();
            txn.encode_state_as_update_v1(sv_before)
        };

        if !update.is_empty() {
            self.storage
                .append_update(&self.doc_name, &update, UpdateOrigin::Local)?;
        }
        Ok(())
    }

    /// Get the length of the body content.
    pub fn body_len(&self) -> u32 {
        let txn = self.doc.transact();
        self.body_text.len(&txn)
    }

    // ==================== Frontmatter Operations ====================

    /// Get a frontmatter property value as a string.
    pub fn get_frontmatter(&self, key: &str) -> Option<String> {
        let txn = self.doc.transact();
        self.frontmatter_map
            .get(&txn, key)
            .and_then(|v| v.cast::<String>().ok())
    }

    /// Set a frontmatter property.
    /// The change is automatically recorded in the update history.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails to persist to storage.
    pub fn set_frontmatter(&self, key: &str, value: &str) -> StorageResult<()> {
        let sv_before = {
            let txn = self.doc.transact();
            txn.state_vector()
        };

        {
            let mut txn = self.doc.transact_mut();
            self.frontmatter_map.insert(&mut txn, key, value);
        }

        self.record_update(&sv_before)
    }

    /// Remove a frontmatter property.
    /// The change is automatically recorded in the update history.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails to persist to storage.
    pub fn remove_frontmatter(&self, key: &str) -> StorageResult<()> {
        let sv_before = {
            let txn = self.doc.transact();
            txn.state_vector()
        };

        {
            let mut txn = self.doc.transact_mut();
            self.frontmatter_map.remove(&mut txn, key);
        }

        self.record_update(&sv_before)
    }

    /// Get all frontmatter keys.
    pub fn frontmatter_keys(&self) -> Vec<String> {
        let txn = self.doc.transact();
        self.frontmatter_map.keys(&txn).map(String::from).collect()
    }

    // ==================== Sync Operations ====================

    /// Encode the current state vector for sync.
    pub fn encode_state_vector(&self) -> Vec<u8> {
        let txn = self.doc.transact();
        txn.state_vector().encode_v1()
    }

    /// Encode the full state as an update.
    pub fn encode_state_as_update(&self) -> Vec<u8> {
        let txn = self.doc.transact();
        txn.encode_state_as_update_v1(&Default::default())
    }

    /// Encode the diff between current state and a remote state vector.
    pub fn encode_diff(&self, remote_state_vector: &[u8]) -> StorageResult<Vec<u8>> {
        let sv = yrs::StateVector::decode_v1(remote_state_vector)
            .map_err(|e| DiaryxError::Crdt(format!("Failed to decode state vector: {}", e)))?;
        let txn = self.doc.transact();
        Ok(txn.encode_state_as_update_v1(&sv))
    }

    /// Apply an update from a remote peer.
    ///
    /// For non-Local origins (Remote, Sync), this method will emit a `ContentsChanged`
    /// event via the event callback. This enables unified event handling where the UI
    /// responds the same way to both local and remote changes.
    pub fn apply_update(&self, update: &[u8], origin: UpdateOrigin) -> StorageResult<Option<i64>> {
        // Only emit events for remote/sync updates (Local updates emit via CrdtFs)
        let should_emit = origin != UpdateOrigin::Local && self.event_callback.is_some();

        let decoded = Update::decode_v1(update)
            .map_err(|e| DiaryxError::Crdt(format!("Failed to decode update: {}", e)))?;

        {
            let mut txn = self.doc.transact_mut();
            txn.apply_update(decoded)
                .map_err(|e| DiaryxError::Crdt(format!("Failed to apply update: {}", e)))?;
        }

        // Emit ContentsChanged event for remote body updates
        if should_emit {
            let content = self.get_body();
            self.emit_event(FileSystemEvent::contents_changed(
                PathBuf::from(&self.doc_name),
                content,
            ));
        }

        // Persist the update
        let update_id = self.storage.append_update(&self.doc_name, update, origin)?;
        Ok(Some(update_id))
    }

    // ==================== Persistence ====================

    /// Save the current state to storage.
    pub fn save(&self) -> StorageResult<()> {
        let state = self.encode_state_as_update();
        self.storage.save_doc(&self.doc_name, &state)
    }

    /// Reload state from storage.
    pub fn reload(&mut self) -> StorageResult<()> {
        if let Some(state) = self.storage.load_doc(&self.doc_name)?
            && let Ok(update) = Update::decode_v1(&state)
        {
            let mut txn = self.doc.transact_mut();
            if let Err(e) = txn.apply_update(update) {
                log::warn!("Failed to reload body doc {}: {}", self.doc_name, e);
            }
        }
        Ok(())
    }

    // ==================== History ====================

    /// Get the update history for this document.
    pub fn get_history(&self) -> StorageResult<Vec<super::types::CrdtUpdate>> {
        self.storage.get_all_updates(&self.doc_name)
    }

    /// Get updates since a given ID.
    pub fn get_updates_since(&self, since_id: i64) -> StorageResult<Vec<super::types::CrdtUpdate>> {
        self.storage.get_updates_since(&self.doc_name, since_id)
    }

    // ==================== Observers ====================

    /// Observe text changes in the body.
    ///
    /// The callback is called whenever the body text changes.
    /// It receives the transaction and text event.
    pub fn observe_body<F>(&self, callback: F) -> yrs::Subscription
    where
        F: Fn() + 'static,
    {
        self.body_text.observe(move |_txn, _event| {
            callback();
        })
    }

    /// Observe changes to the underlying document.
    pub fn observe_updates<F>(&self, callback: F) -> yrs::Subscription
    where
        F: Fn(&[u8]) + 'static,
    {
        self.doc
            .observe_update_v1(move |_, event| {
                callback(&event.update);
            })
            .expect("Failed to observe document updates")
    }
}

impl std::fmt::Debug for BodyDoc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BodyDoc")
            .field("doc_name", &self.doc_name)
            .field("body_len", &self.body_len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crdt::MemoryStorage;

    fn create_body_doc(name: &str) -> BodyDoc {
        let storage = Arc::new(MemoryStorage::new());
        BodyDoc::new(storage, name.to_string())
    }

    #[test]
    fn test_new_body_doc_is_empty() {
        let doc = create_body_doc("test.md");
        assert_eq!(doc.get_body(), "");
        assert_eq!(doc.body_len(), 0);
    }

    #[test]
    fn test_set_and_get_body() {
        let doc = create_body_doc("test.md");

        let content = "# Hello World\n\nThis is content.";
        doc.set_body(content).unwrap();
        assert_eq!(doc.get_body(), content);
        assert_eq!(doc.body_len(), content.len() as u32);
    }

    #[test]
    fn test_replace_body() {
        let doc = create_body_doc("test.md");

        doc.set_body("Original content").unwrap();
        doc.set_body("New content").unwrap();

        assert_eq!(doc.get_body(), "New content");
    }

    #[test]
    fn test_insert_at() {
        let doc = create_body_doc("test.md");

        doc.set_body("Hello World").unwrap();
        doc.insert_at(6, "Beautiful ").unwrap();

        assert_eq!(doc.get_body(), "Hello Beautiful World");
    }

    #[test]
    fn test_delete_range() {
        let doc = create_body_doc("test.md");

        doc.set_body("Hello Beautiful World").unwrap();
        doc.delete_range(6, 10).unwrap(); // Remove "Beautiful "

        assert_eq!(doc.get_body(), "Hello World");
    }

    #[test]
    fn test_frontmatter_operations() {
        let doc = create_body_doc("test.md");

        // Set properties
        doc.set_frontmatter("title", "My Title").unwrap();
        doc.set_frontmatter("author", "John Doe").unwrap();

        // Get properties
        assert_eq!(doc.get_frontmatter("title"), Some("My Title".to_string()));
        assert_eq!(doc.get_frontmatter("author"), Some("John Doe".to_string()));
        assert_eq!(doc.get_frontmatter("nonexistent"), None);

        // List keys
        let keys = doc.frontmatter_keys();
        assert!(keys.contains(&"title".to_string()));
        assert!(keys.contains(&"author".to_string()));

        // Remove property
        doc.remove_frontmatter("author").unwrap();
        assert_eq!(doc.get_frontmatter("author"), None);
    }

    #[test]
    fn test_save_and_load() {
        let storage = Arc::new(MemoryStorage::new());
        let doc_name = "test.md".to_string();

        // Create and populate
        {
            let doc = BodyDoc::new(storage.clone(), doc_name.clone());
            doc.set_body("# Persistent Content").unwrap();
            doc.set_frontmatter("title", "Saved Title").unwrap();
            doc.save().unwrap();
        }

        // Load and verify
        {
            let doc = BodyDoc::load(storage, doc_name).unwrap();
            assert_eq!(doc.get_body(), "# Persistent Content");
            assert_eq!(
                doc.get_frontmatter("title"),
                Some("Saved Title".to_string())
            );
        }
    }

    #[test]
    fn test_sync_between_docs() {
        let storage1 = Arc::new(MemoryStorage::new());
        let storage2 = Arc::new(MemoryStorage::new());

        let doc1 = BodyDoc::new(storage1, "test.md".to_string());
        let doc2 = BodyDoc::new(storage2, "test.md".to_string());

        // Edit on doc1
        doc1.set_body("Content from doc1").unwrap();
        doc1.set_frontmatter("source", "doc1").unwrap();

        // Sync to doc2
        let update = doc1.encode_state_as_update();
        doc2.apply_update(&update, UpdateOrigin::Remote).unwrap();

        // Verify sync
        assert_eq!(doc2.get_body(), "Content from doc1");
        assert_eq!(doc2.get_frontmatter("source"), Some("doc1".to_string()));
    }

    #[test]
    fn test_concurrent_edits() {
        let storage1 = Arc::new(MemoryStorage::new());
        let storage2 = Arc::new(MemoryStorage::new());

        let doc1 = BodyDoc::new(storage1, "test.md".to_string());
        let doc2 = BodyDoc::new(storage2, "test.md".to_string());

        // Both start with same content
        doc1.set_body("Hello World").unwrap();
        let initial = doc1.encode_state_as_update();
        doc2.apply_update(&initial, UpdateOrigin::Remote).unwrap();

        // Concurrent edits
        doc1.insert_at(0, "A: ").unwrap(); // "A: Hello World"
        doc2.insert_at(11, "!").unwrap(); // "Hello World!"

        // Exchange updates
        let update1 = doc1.encode_state_as_update();
        let update2 = doc2.encode_state_as_update();

        doc1.apply_update(&update2, UpdateOrigin::Remote).unwrap();
        doc2.apply_update(&update1, UpdateOrigin::Remote).unwrap();

        // Both should converge to same result
        assert_eq!(doc1.get_body(), doc2.get_body());
        // Result should contain both edits
        let body = doc1.get_body();
        assert!(body.contains("A: "));
        assert!(body.contains("!"));
    }

    #[test]
    fn test_encode_diff() {
        let storage1 = Arc::new(MemoryStorage::new());
        let storage2 = Arc::new(MemoryStorage::new());

        let doc1 = BodyDoc::new(storage1, "test.md".to_string());
        let doc2 = BodyDoc::new(storage2, "test.md".to_string());

        // Initial sync
        doc1.set_body("Initial content").unwrap();
        let initial = doc1.encode_state_as_update();
        doc2.apply_update(&initial, UpdateOrigin::Remote).unwrap();

        // Doc2 captures state vector
        let sv2 = doc2.encode_state_vector();

        // Doc1 makes more changes
        doc1.insert_at(0, "NEW: ").unwrap();

        // Get only the diff
        let diff = doc1.encode_diff(&sv2).unwrap();

        // Apply diff to doc2
        doc2.apply_update(&diff, UpdateOrigin::Remote).unwrap();

        assert_eq!(doc2.get_body(), "NEW: Initial content");
    }

    #[test]
    fn test_observer_fires_on_change() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let doc = create_body_doc("test.md");
        let changed = Arc::new(AtomicBool::new(false));
        let changed_clone = changed.clone();

        let _sub = doc.observe_updates(move |_update| {
            changed_clone.store(true, Ordering::SeqCst);
        });

        doc.set_body("Trigger change").unwrap();

        assert!(changed.load(Ordering::SeqCst));
    }

    #[test]
    fn test_doc_name() {
        let doc = create_body_doc("workspace/notes/hello.md");
        assert_eq!(doc.doc_name(), "workspace/notes/hello.md");
    }
}
