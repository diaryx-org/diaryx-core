//! In-memory storage implementation for testing and WASM.
//!
//! This provides a simple in-memory implementation of [`CrdtStorage`]
//! for use in unit tests, development, and WASM environments.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use yrs::{Doc, ReadTxn, Transact, Update, updates::decoder::Decode};

use super::storage::{CrdtStorage, StorageResult};
use super::types::{CrdtUpdate, UpdateOrigin};

/// Threshold for triggering auto-compaction (number of updates)
const AUTO_COMPACT_THRESHOLD: usize = 1000;

/// Number of updates to keep after auto-compaction
const AUTO_COMPACT_KEEP: usize = 500;

/// In-memory CRDT storage for testing.
///
/// This implementation stores all data in memory using `HashMap` and `Vec`.
/// It's thread-safe via `RwLock` but data is lost when dropped.
///
/// Auto-compaction is triggered when the number of updates for a document
/// exceeds [`AUTO_COMPACT_THRESHOLD`], keeping the most recent
/// [`AUTO_COMPACT_KEEP`] updates.
#[derive(Debug, Default)]
pub struct MemoryStorage {
    /// Document snapshots (name -> binary state)
    docs: Arc<RwLock<HashMap<String, Vec<u8>>>>,

    /// Update logs (name -> list of updates)
    updates: Arc<RwLock<HashMap<String, Vec<StoredUpdate>>>>,

    /// Counter for generating update IDs
    next_id: Arc<RwLock<i64>>,
}

#[derive(Debug, Clone)]
struct StoredUpdate {
    id: i64,
    data: Vec<u8>,
    timestamp: i64,
    origin: UpdateOrigin,
    device_id: Option<String>,
    device_name: Option<String>,
}

impl MemoryStorage {
    /// Create a new empty in-memory storage.
    pub fn new() -> Self {
        Self::default()
    }

    fn next_update_id(&self) -> i64 {
        let mut id = self.next_id.write().unwrap();
        *id += 1;
        *id
    }
}

impl CrdtStorage for MemoryStorage {
    fn load_doc(&self, name: &str) -> StorageResult<Option<Vec<u8>>> {
        let docs = self.docs.read().unwrap();
        Ok(docs.get(name).cloned())
    }

    fn save_doc(&self, name: &str, state: &[u8]) -> StorageResult<()> {
        let mut docs = self.docs.write().unwrap();
        docs.insert(name.to_string(), state.to_vec());
        Ok(())
    }

    fn delete_doc(&self, name: &str) -> StorageResult<()> {
        let mut docs = self.docs.write().unwrap();
        let mut updates = self.updates.write().unwrap();
        docs.remove(name);
        updates.remove(name);
        Ok(())
    }

    fn list_docs(&self) -> StorageResult<Vec<String>> {
        let docs = self.docs.read().unwrap();
        Ok(docs.keys().cloned().collect())
    }

    fn append_update_with_device(
        &self,
        name: &str,
        update: &[u8],
        origin: UpdateOrigin,
        device_id: Option<&str>,
        device_name: Option<&str>,
    ) -> StorageResult<i64> {
        let id = self.next_update_id();
        let stored = StoredUpdate {
            id,
            data: update.to_vec(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            origin,
            device_id: device_id.map(String::from),
            device_name: device_name.map(String::from),
        };

        let mut updates = self.updates.write().unwrap();
        let doc_updates = updates.entry(name.to_string()).or_default();
        doc_updates.push(stored);

        // Auto-compact if we've exceeded the threshold
        if doc_updates.len() > AUTO_COMPACT_THRESHOLD {
            let drain_count = doc_updates.len() - AUTO_COMPACT_KEEP;
            doc_updates.drain(0..drain_count);
        }

        Ok(id)
    }

    fn get_updates_since(&self, name: &str, since_id: i64) -> StorageResult<Vec<CrdtUpdate>> {
        let updates = self.updates.read().unwrap();
        let doc_updates = updates.get(name).map(|u| u.as_slice()).unwrap_or(&[]);

        Ok(doc_updates
            .iter()
            .filter(|u| u.id > since_id)
            .map(|u| CrdtUpdate {
                update_id: u.id,
                doc_name: name.to_string(),
                data: u.data.clone(),
                timestamp: u.timestamp,
                origin: u.origin,
                device_id: u.device_id.clone(),
                device_name: u.device_name.clone(),
            })
            .collect())
    }

    fn get_all_updates(&self, name: &str) -> StorageResult<Vec<CrdtUpdate>> {
        self.get_updates_since(name, 0)
    }

    fn get_state_at(&self, name: &str, update_id: i64) -> StorageResult<Option<Vec<u8>>> {
        // Load base document snapshot
        let base_state = self.load_doc(name)?;

        // Get updates up to the specified ID
        let updates_lock = self.updates.read().unwrap();
        let doc_updates: Vec<Vec<u8>> = updates_lock
            .get(name)
            .map(|updates| {
                updates
                    .iter()
                    .filter(|u| u.id <= update_id)
                    .map(|u| u.data.clone())
                    .collect()
            })
            .unwrap_or_default();

        // If no base state and no updates, return None
        if base_state.is_none() && doc_updates.is_empty() {
            return Ok(None);
        }

        // Create a new doc and apply all state
        let doc = Doc::new();
        {
            let mut txn = doc.transact_mut();

            // Apply base state if it exists
            if let Some(state) = &base_state
                && let Ok(update) = Update::decode_v1(state)
            {
                if let Err(e) = txn.apply_update(update) {
                    log::warn!("Failed to apply base state for {}: {}", name, e);
                }
            }

            // Apply incremental updates up to the specified ID
            for update_data in doc_updates {
                if let Ok(update) = Update::decode_v1(&update_data) {
                    if let Err(e) = txn.apply_update(update) {
                        log::warn!("Failed to apply incremental update for {}: {}", name, e);
                    }
                }
            }
        }

        // Encode final state
        let txn = doc.transact();
        Ok(Some(txn.encode_state_as_update_v1(&Default::default())))
    }

    fn compact(&self, name: &str, keep_updates: usize) -> StorageResult<()> {
        let mut updates = self.updates.write().unwrap();

        if let Some(doc_updates) = updates.get_mut(name)
            && doc_updates.len() > keep_updates
        {
            // Keep only the last `keep_updates` entries
            let drain_count = doc_updates.len() - keep_updates;
            doc_updates.drain(0..drain_count);
        }

        Ok(())
    }

    fn get_latest_update_id(&self, name: &str) -> StorageResult<i64> {
        let updates = self.updates.read().unwrap();
        Ok(updates
            .get(name)
            .and_then(|u| u.last())
            .map(|u| u.id)
            .unwrap_or(0))
    }

    fn rename_doc(&self, old_name: &str, new_name: &str) -> StorageResult<()> {
        // Copy document snapshot
        {
            let mut docs = self.docs.write().unwrap();
            if let Some(state) = docs.remove(old_name) {
                docs.insert(new_name.to_string(), state);
            }
        }

        // Copy updates with new doc_name
        {
            let mut updates = self.updates.write().unwrap();
            if let Some(old_updates) = updates.remove(old_name) {
                let new_updates: Vec<StoredUpdate> = old_updates
                    .into_iter()
                    .map(|u| StoredUpdate {
                        id: u.id,
                        data: u.data,
                        timestamp: u.timestamp,
                        origin: u.origin,
                        device_id: u.device_id,
                        device_name: u.device_name,
                    })
                    .collect();
                updates.insert(new_name.to_string(), new_updates);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_and_load_doc() {
        let storage = MemoryStorage::new();
        let data = b"test document state";

        storage.save_doc("test", data).unwrap();
        let loaded = storage.load_doc("test").unwrap();

        assert_eq!(loaded, Some(data.to_vec()));
    }

    #[test]
    fn test_load_nonexistent_doc() {
        let storage = MemoryStorage::new();
        let loaded = storage.load_doc("nonexistent").unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_delete_doc() {
        let storage = MemoryStorage::new();
        storage.save_doc("test", b"data").unwrap();
        storage
            .append_update("test", b"update", UpdateOrigin::Local)
            .unwrap();

        storage.delete_doc("test").unwrap();

        assert!(storage.load_doc("test").unwrap().is_none());
        assert!(storage.get_all_updates("test").unwrap().is_empty());
    }

    #[test]
    fn test_list_docs() {
        let storage = MemoryStorage::new();
        storage.save_doc("doc1", b"data1").unwrap();
        storage.save_doc("doc2", b"data2").unwrap();

        let mut docs = storage.list_docs().unwrap();
        docs.sort();

        assert_eq!(docs, vec!["doc1", "doc2"]);
    }

    #[test]
    fn test_append_and_get_updates() {
        let storage = MemoryStorage::new();

        let id1 = storage
            .append_update("test", b"update1", UpdateOrigin::Local)
            .unwrap();
        let id2 = storage
            .append_update("test", b"update2", UpdateOrigin::Remote)
            .unwrap();
        let id3 = storage
            .append_update("test", b"update3", UpdateOrigin::Sync)
            .unwrap();

        assert!(id1 < id2);
        assert!(id2 < id3);

        let all = storage.get_all_updates("test").unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].origin, UpdateOrigin::Local);
        assert_eq!(all[1].origin, UpdateOrigin::Remote);

        let since_id1 = storage.get_updates_since("test", id1).unwrap();
        assert_eq!(since_id1.len(), 2);
        assert_eq!(since_id1[0].update_id, id2);
    }

    #[test]
    fn test_compact() {
        let storage = MemoryStorage::new();

        for i in 0..10 {
            storage
                .append_update(
                    "test",
                    format!("update{}", i).as_bytes(),
                    UpdateOrigin::Local,
                )
                .unwrap();
        }

        assert_eq!(storage.get_all_updates("test").unwrap().len(), 10);

        storage.compact("test", 3).unwrap();

        let remaining = storage.get_all_updates("test").unwrap();
        assert_eq!(remaining.len(), 3);
    }

    #[test]
    fn test_get_latest_update_id() {
        let storage = MemoryStorage::new();

        assert_eq!(storage.get_latest_update_id("test").unwrap(), 0);

        let id1 = storage
            .append_update("test", b"update1", UpdateOrigin::Local)
            .unwrap();
        assert_eq!(storage.get_latest_update_id("test").unwrap(), id1);

        let id2 = storage
            .append_update("test", b"update2", UpdateOrigin::Local)
            .unwrap();
        assert_eq!(storage.get_latest_update_id("test").unwrap(), id2);
    }

    #[test]
    fn test_get_state_at_reconstructs_history() {
        use yrs::{GetString, Text, Transact};

        let storage = MemoryStorage::new();

        // Create a Y.Doc and make some changes, storing updates
        let doc = Doc::new();
        let text = doc.get_or_insert_text("content");

        // First update: add "Hello"
        let update1 = {
            let mut txn = doc.transact_mut();
            text.insert(&mut txn, 0, "Hello");
            txn.encode_update_v1()
        };
        let id1 = storage
            .append_update("test", &update1, UpdateOrigin::Local)
            .unwrap();

        // Second update: add " World"
        let update2 = {
            let mut txn = doc.transact_mut();
            text.insert(&mut txn, 5, " World");
            txn.encode_update_v1()
        };
        let id2 = storage
            .append_update("test", &update2, UpdateOrigin::Local)
            .unwrap();

        // Third update: add "!"
        let update3 = {
            let mut txn = doc.transact_mut();
            text.insert(&mut txn, 11, "!");
            txn.encode_update_v1()
        };
        let _id3 = storage
            .append_update("test", &update3, UpdateOrigin::Local)
            .unwrap();

        // Verify current state is "Hello World!"
        {
            let txn = doc.transact();
            assert_eq!(text.get_string(&txn), "Hello World!");
        }

        // Get state at id1 - should only have "Hello"
        let state_at_1 = storage.get_state_at("test", id1).unwrap().unwrap();
        let doc_at_1 = Doc::new();
        {
            let mut txn = doc_at_1.transact_mut();
            let update = Update::decode_v1(&state_at_1).unwrap();
            txn.apply_update(update).unwrap();
        }
        let text_at_1 = doc_at_1.get_or_insert_text("content");
        {
            let txn = doc_at_1.transact();
            assert_eq!(text_at_1.get_string(&txn), "Hello");
        }

        // Get state at id2 - should have "Hello World"
        let state_at_2 = storage.get_state_at("test", id2).unwrap().unwrap();
        let doc_at_2 = Doc::new();
        {
            let mut txn = doc_at_2.transact_mut();
            let update = Update::decode_v1(&state_at_2).unwrap();
            txn.apply_update(update).unwrap();
        }
        let text_at_2 = doc_at_2.get_or_insert_text("content");
        {
            let txn = doc_at_2.transact();
            assert_eq!(text_at_2.get_string(&txn), "Hello World");
        }
    }

    #[test]
    fn test_get_state_at_nonexistent() {
        let storage = MemoryStorage::new();

        // No doc, no updates - should return None
        let result = storage.get_state_at("nonexistent", 1).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_rename_doc() {
        let storage = MemoryStorage::new();

        // Create a doc with state and updates
        storage.save_doc("old_name", b"test state").unwrap();
        storage
            .append_update("old_name", b"update1", UpdateOrigin::Local)
            .unwrap();
        storage
            .append_update("old_name", b"update2", UpdateOrigin::Remote)
            .unwrap();

        // Verify old name exists
        assert!(storage.load_doc("old_name").unwrap().is_some());
        assert_eq!(storage.get_all_updates("old_name").unwrap().len(), 2);

        // Rename
        storage.rename_doc("old_name", "new_name").unwrap();

        // Old name should be gone
        assert!(storage.load_doc("old_name").unwrap().is_none());
        assert!(storage.get_all_updates("old_name").unwrap().is_empty());

        // New name should have the content
        assert_eq!(
            storage.load_doc("new_name").unwrap(),
            Some(b"test state".to_vec())
        );
        let updates = storage.get_all_updates("new_name").unwrap();
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].origin, UpdateOrigin::Local);
        assert_eq!(updates[1].origin, UpdateOrigin::Remote);
    }

    #[test]
    fn test_rename_doc_nonexistent() {
        let storage = MemoryStorage::new();

        // Renaming a nonexistent doc should not error
        let result = storage.rename_doc("nonexistent", "new_name");
        assert!(result.is_ok());

        // Both should be empty
        assert!(storage.load_doc("nonexistent").unwrap().is_none());
        assert!(storage.load_doc("new_name").unwrap().is_none());
    }
}
