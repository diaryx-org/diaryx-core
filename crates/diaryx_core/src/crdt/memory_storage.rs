//! In-memory storage implementation for testing.
//!
//! This provides a simple in-memory implementation of [`CrdtStorage`]
//! for use in unit tests and development.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::storage::{CrdtStorage, StorageResult};
use super::types::{CrdtUpdate, UpdateOrigin};

/// In-memory CRDT storage for testing.
///
/// This implementation stores all data in memory using `HashMap` and `Vec`.
/// It's thread-safe via `RwLock` but data is lost when dropped.
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

    fn append_update(
        &self,
        name: &str,
        update: &[u8],
        origin: UpdateOrigin,
    ) -> StorageResult<i64> {
        let id = self.next_update_id();
        let stored = StoredUpdate {
            id,
            data: update.to_vec(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            origin,
        };

        let mut updates = self.updates.write().unwrap();
        updates
            .entry(name.to_string())
            .or_default()
            .push(stored);

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
            })
            .collect())
    }

    fn get_all_updates(&self, name: &str) -> StorageResult<Vec<CrdtUpdate>> {
        self.get_updates_since(name, 0)
    }

    fn get_state_at(&self, name: &str, update_id: i64) -> StorageResult<Option<Vec<u8>>> {
        // For memory storage, we don't implement proper state reconstruction.
        // A real implementation would apply updates up to the given ID.
        // For now, if asking for latest, return current state.
        let updates = self.updates.read().unwrap();
        let doc_updates = updates.get(name);

        if let Some(updates) = doc_updates {
            if let Some(last) = updates.last() {
                if update_id >= last.id {
                    return self.load_doc(name);
                }
            }
        }

        // TODO: Implement proper state reconstruction by replaying updates
        // This requires yrs integration to merge updates
        self.load_doc(name)
    }

    fn compact(&self, name: &str, keep_updates: usize) -> StorageResult<()> {
        let mut updates = self.updates.write().unwrap();

        if let Some(doc_updates) = updates.get_mut(name) {
            if doc_updates.len() > keep_updates {
                // Keep only the last `keep_updates` entries
                let drain_count = doc_updates.len() - keep_updates;
                doc_updates.drain(0..drain_count);
            }
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
        storage.append_update("test", b"update", UpdateOrigin::Local).unwrap();

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

        let id1 = storage.append_update("test", b"update1", UpdateOrigin::Local).unwrap();
        let id2 = storage.append_update("test", b"update2", UpdateOrigin::Remote).unwrap();
        let id3 = storage.append_update("test", b"update3", UpdateOrigin::Sync).unwrap();

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
                .append_update("test", format!("update{}", i).as_bytes(), UpdateOrigin::Local)
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

        let id1 = storage.append_update("test", b"update1", UpdateOrigin::Local).unwrap();
        assert_eq!(storage.get_latest_update_id("test").unwrap(), id1);

        let id2 = storage.append_update("test", b"update2", UpdateOrigin::Local).unwrap();
        assert_eq!(storage.get_latest_update_id("test").unwrap(), id2);
    }
}
