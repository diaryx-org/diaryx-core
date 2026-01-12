//! Version history and time-travel functionality for CRDTs.
//!
//! This module provides functionality for:
//! - Viewing document history (list of updates/versions)
//! - Reconstructing document state at any point in history
//! - Comparing versions to see what changed (diffs)
//! - Restoring to a previous version
//!
//! # Example
//!
//! ```ignore
//! use diaryx_core::crdt::{WorkspaceCrdt, MemoryStorage, HistoryManager};
//!
//! let storage = Arc::new(MemoryStorage::new());
//! let workspace = WorkspaceCrdt::new(storage.clone());
//!
//! // Make some changes
//! workspace.set_file("notes.md", FileMetadata::new(Some("Notes")));
//! workspace.save()?;
//!
//! // Get history
//! let history = HistoryManager::new(storage);
//! let entries = history.get_history("workspace", None)?;
//!
//! // Restore to a previous version
//! history.restore_to("workspace", entries[0].update_id)?;
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use yrs::updates::decoder::Decode;
use yrs::{Doc, Map, ReadTxn, StateVector, Transact, Update};

use super::storage::{CrdtStorage, StorageResult};
use super::types::FileMetadata;
use crate::error::DiaryxError;

/// The name of the Y.Map containing file metadata.
const FILES_MAP_NAME: &str = "files";

/// A history entry with metadata about what changed.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct HistoryEntry {
    /// Unique update ID
    pub update_id: i64,

    /// Unix timestamp (milliseconds)
    pub timestamp: i64,

    /// Origin of the change
    pub origin: String,

    /// Files that were changed in this update (if determinable)
    pub files_changed: Vec<String>,
}

/// Type of change made to a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub enum ChangeType {
    /// File was added
    Added,
    /// File was modified
    Modified,
    /// File was deleted (soft delete)
    Deleted,
    /// File was restored from deletion
    Restored,
}

/// Difference between two versions of a file.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct FileDiff {
    /// Path to the file
    pub path: String,

    /// Type of change
    pub change_type: ChangeType,

    /// Metadata before the change (None if file was added)
    pub old_metadata: Option<FileMetadata>,

    /// Metadata after the change (None if file was removed)
    pub new_metadata: Option<FileMetadata>,
}

/// Manager for version history operations.
pub struct HistoryManager {
    storage: Arc<dyn CrdtStorage>,
}

impl HistoryManager {
    /// Create a new history manager with the given storage backend.
    pub fn new(storage: Arc<dyn CrdtStorage>) -> Self {
        Self { storage }
    }

    /// Get the history of updates for a document.
    ///
    /// Returns a list of history entries, newest first.
    pub fn get_history(
        &self,
        doc_name: &str,
        limit: Option<usize>,
    ) -> StorageResult<Vec<HistoryEntry>> {
        let updates = self.storage.get_all_updates(doc_name)?;

        let entries: Vec<HistoryEntry> = updates
            .into_iter()
            .rev() // Newest first
            .take(limit.unwrap_or(usize::MAX))
            .map(|u| HistoryEntry {
                update_id: u.update_id,
                timestamp: u.timestamp,
                origin: u.origin.to_string(),
                files_changed: Vec::new(), // Would need to decode update to determine
            })
            .collect();

        Ok(entries)
    }

    /// Reconstruct document state at a specific update ID.
    ///
    /// This creates a new yrs Doc and applies all updates up to (and including)
    /// the specified update ID to reconstruct the state at that point.
    ///
    /// Note: This replays updates from the beginning, not from the base state.
    /// This ensures accurate historical reconstruction.
    pub fn get_state_at(&self, doc_name: &str, update_id: i64) -> StorageResult<Option<Vec<u8>>> {
        // Get all updates
        let all_updates = self.storage.get_all_updates(doc_name)?;

        // If no updates, return base document state (if any)
        if all_updates.is_empty() {
            return self.storage.load_doc(doc_name);
        }

        // If asking for the latest or beyond, return current state
        if let Some(last) = all_updates.last() {
            if update_id >= last.update_id {
                return self.storage.load_doc(doc_name);
            }
        }

        // Create a new document and apply updates up to the target ID
        let doc = Doc::new();
        let _files_map = doc.get_or_insert_map(FILES_MAP_NAME);

        // Apply updates up to target ID (replaying from beginning)
        for update in all_updates {
            if update.update_id > update_id {
                break;
            }

            let decoded = Update::decode_v1(&update.data).map_err(|e| {
                DiaryxError::Crdt(format!(
                    "Failed to decode update {}: {}",
                    update.update_id, e
                ))
            })?;

            let mut txn = doc.transact_mut();
            txn.apply_update(decoded).map_err(|e| {
                DiaryxError::Crdt(format!(
                    "Failed to apply update {}: {}",
                    update.update_id, e
                ))
            })?;
        }

        // Encode the reconstructed state
        let txn = doc.transact();
        let state = txn.encode_state_as_update_v1(&StateVector::default());

        Ok(Some(state))
    }

    /// Get files from a document state.
    fn get_files_from_state(&self, state: &[u8]) -> StorageResult<HashMap<String, FileMetadata>> {
        let doc = Doc::new();

        let update = Update::decode_v1(state)
            .map_err(|e| DiaryxError::Crdt(format!("Failed to decode state: {}", e)))?;

        {
            let mut txn = doc.transact_mut();
            txn.apply_update(update)
                .map_err(|e| DiaryxError::Crdt(format!("Failed to apply state: {}", e)))?;
        }

        let files_map = doc.get_or_insert_map(FILES_MAP_NAME);
        let txn = doc.transact();

        let mut files = HashMap::new();
        for (key, value) in files_map.iter(&txn) {
            let path = key.to_string();
            let json = value.to_string(&txn);
            if let Ok(metadata) = serde_json::from_str::<FileMetadata>(&json) {
                files.insert(path, metadata);
            }
        }

        Ok(files)
    }

    /// Compute the diff between two versions.
    ///
    /// Returns a list of file changes between the two update IDs.
    pub fn diff(&self, doc_name: &str, from_id: i64, to_id: i64) -> StorageResult<Vec<FileDiff>> {
        // Get state at both points
        let from_state = self.get_state_at(doc_name, from_id)?;
        let to_state = self.get_state_at(doc_name, to_id)?;

        // Parse files from both states
        let from_files = match &from_state {
            Some(state) => self.get_files_from_state(state)?,
            None => HashMap::new(),
        };

        let to_files = match &to_state {
            Some(state) => self.get_files_from_state(state)?,
            None => HashMap::new(),
        };

        // Compute diff
        let mut diffs = Vec::new();

        // Check for added and modified files
        for (path, new_meta) in &to_files {
            match from_files.get(path) {
                None => {
                    // File was added
                    diffs.push(FileDiff {
                        path: path.clone(),
                        change_type: ChangeType::Added,
                        old_metadata: None,
                        new_metadata: Some(new_meta.clone()),
                    });
                }
                Some(old_meta) => {
                    // Check if file was modified
                    if old_meta != new_meta {
                        // Determine change type
                        let change_type = if old_meta.deleted && !new_meta.deleted {
                            ChangeType::Restored
                        } else if !old_meta.deleted && new_meta.deleted {
                            ChangeType::Deleted
                        } else {
                            ChangeType::Modified
                        };

                        diffs.push(FileDiff {
                            path: path.clone(),
                            change_type,
                            old_metadata: Some(old_meta.clone()),
                            new_metadata: Some(new_meta.clone()),
                        });
                    }
                }
            }
        }

        // Check for removed files (in from but not in to)
        for (path, old_meta) in &from_files {
            if !to_files.contains_key(path) {
                diffs.push(FileDiff {
                    path: path.clone(),
                    change_type: ChangeType::Deleted,
                    old_metadata: Some(old_meta.clone()),
                    new_metadata: None,
                });
            }
        }

        // Sort by path for consistent ordering
        diffs.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(diffs)
    }

    /// Create a restore update that reverts the document to a historical state.
    ///
    /// This doesn't actually apply the restore - it returns an update that can
    /// be applied to bring the document back to the specified version.
    pub fn create_restore_update(&self, doc_name: &str, update_id: i64) -> StorageResult<Vec<u8>> {
        // Get the historical state
        let historical_state = self.get_state_at(doc_name, update_id)?.ok_or_else(|| {
            DiaryxError::Crdt(format!("No state found at update ID {}", update_id))
        })?;

        // Get files from historical state
        let historical_files = self.get_files_from_state(&historical_state)?;

        // Create a new document with the historical files
        let doc = Doc::new();
        let files_map = doc.get_or_insert_map(FILES_MAP_NAME);

        {
            let mut txn = doc.transact_mut();
            for (path, metadata) in historical_files {
                let json = serde_json::to_string(&metadata).unwrap_or_default();
                files_map.insert(&mut txn, path.as_str(), json);
            }
        }

        // Encode the restore state as an update
        let txn = doc.transact();
        Ok(txn.encode_state_as_update_v1(&StateVector::default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crdt::{MemoryStorage, UpdateOrigin};

    fn create_test_doc(storage: &Arc<dyn CrdtStorage>, doc_name: &str) {
        let doc = Doc::new();
        let files_map = doc.get_or_insert_map(FILES_MAP_NAME);

        {
            let mut txn = doc.transact_mut();
            let meta = FileMetadata::new(Some("Test".to_string()));
            let json = serde_json::to_string(&meta).unwrap();
            files_map.insert(&mut txn, "test.md", json);
        }

        let txn = doc.transact();
        let state = txn.encode_state_as_update_v1(&StateVector::default());
        storage.save_doc(doc_name, &state).unwrap();
    }

    #[test]
    fn test_get_history_empty() {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        let history = HistoryManager::new(storage);

        let entries = history.get_history("test", None).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_get_history_with_updates() {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());

        // Add some updates
        storage
            .append_update("test", b"update1", UpdateOrigin::Local)
            .unwrap();
        storage
            .append_update("test", b"update2", UpdateOrigin::Remote)
            .unwrap();
        storage
            .append_update("test", b"update3", UpdateOrigin::Local)
            .unwrap();

        let history = HistoryManager::new(storage);
        let entries = history.get_history("test", None).unwrap();

        assert_eq!(entries.len(), 3);
        // Should be newest first
        assert!(entries[0].update_id > entries[1].update_id);
        assert!(entries[1].update_id > entries[2].update_id);
    }

    #[test]
    fn test_get_history_with_limit() {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());

        for i in 0..10 {
            storage
                .append_update(
                    "test",
                    format!("update{}", i).as_bytes(),
                    UpdateOrigin::Local,
                )
                .unwrap();
        }

        let history = HistoryManager::new(storage);
        let entries = history.get_history("test", Some(3)).unwrap();

        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn test_get_state_at() {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        create_test_doc(&storage, "workspace");

        let history = HistoryManager::new(Arc::clone(&storage));

        // Should return current state when no updates
        let state = history.get_state_at("workspace", 0).unwrap();
        assert!(state.is_some());
    }

    #[test]
    fn test_diff_added_file() {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());

        // Create initial empty state
        let doc1 = Doc::new();
        let _files_map1 = doc1.get_or_insert_map(FILES_MAP_NAME);
        let txn1 = doc1.transact();
        let state1 = txn1.encode_state_as_update_v1(&StateVector::default());
        storage.save_doc("workspace", &state1).unwrap();

        let id1 = storage
            .append_update("workspace", &state1, UpdateOrigin::Local)
            .unwrap();

        // Create state with one file
        let doc2 = Doc::new();
        let files_map2 = doc2.get_or_insert_map(FILES_MAP_NAME);
        {
            let mut txn2 = doc2.transact_mut();
            let meta = FileMetadata::new(Some("New File".to_string()));
            let json = serde_json::to_string(&meta).unwrap();
            files_map2.insert(&mut txn2, "new.md", json);
        }
        let txn2 = doc2.transact();
        let state2 = txn2.encode_state_as_update_v1(&StateVector::default());
        storage.save_doc("workspace", &state2).unwrap();

        let id2 = storage
            .append_update("workspace", &state2, UpdateOrigin::Local)
            .unwrap();

        let history = HistoryManager::new(storage);
        let diffs = history.diff("workspace", id1, id2).unwrap();

        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].path, "new.md");
        assert_eq!(diffs[0].change_type, ChangeType::Added);
        assert!(diffs[0].old_metadata.is_none());
        assert!(diffs[0].new_metadata.is_some());
    }

    #[test]
    fn test_diff_deleted_file() {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());

        // Create state with one file
        let doc1 = Doc::new();
        let files_map1 = doc1.get_or_insert_map(FILES_MAP_NAME);
        {
            let mut txn1 = doc1.transact_mut();
            let meta = FileMetadata::new(Some("To Delete".to_string()));
            let json = serde_json::to_string(&meta).unwrap();
            files_map1.insert(&mut txn1, "delete.md", json);
        }
        let txn1 = doc1.transact();
        let state1 = txn1.encode_state_as_update_v1(&StateVector::default());
        storage.save_doc("workspace", &state1).unwrap();

        let id1 = storage
            .append_update("workspace", &state1, UpdateOrigin::Local)
            .unwrap();

        // Create state with file marked as deleted
        let doc2 = Doc::new();
        let files_map2 = doc2.get_or_insert_map(FILES_MAP_NAME);
        {
            let mut txn2 = doc2.transact_mut();
            let mut meta = FileMetadata::new(Some("To Delete".to_string()));
            meta.deleted = true;
            let json = serde_json::to_string(&meta).unwrap();
            files_map2.insert(&mut txn2, "delete.md", json);
        }
        let txn2 = doc2.transact();
        let state2 = txn2.encode_state_as_update_v1(&StateVector::default());
        storage.save_doc("workspace", &state2).unwrap();

        let id2 = storage
            .append_update("workspace", &state2, UpdateOrigin::Local)
            .unwrap();

        let history = HistoryManager::new(storage);
        let diffs = history.diff("workspace", id1, id2).unwrap();

        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].path, "delete.md");
        assert_eq!(diffs[0].change_type, ChangeType::Deleted);
    }

    #[test]
    fn test_create_restore_update() {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());

        // Create initial state
        let doc1 = Doc::new();
        let files_map1 = doc1.get_or_insert_map(FILES_MAP_NAME);
        {
            let mut txn1 = doc1.transact_mut();
            let meta = FileMetadata::new(Some("Original".to_string()));
            let json = serde_json::to_string(&meta).unwrap();
            files_map1.insert(&mut txn1, "file.md", json);
        }
        let txn1 = doc1.transact();
        let state1 = txn1.encode_state_as_update_v1(&StateVector::default());
        drop(txn1); // Explicitly drop transaction
        storage.save_doc("workspace", &state1).unwrap();

        let id1 = storage
            .append_update("workspace", &state1, UpdateOrigin::Local)
            .unwrap();

        let history = HistoryManager::new(storage);

        // Create restore update for the initial state
        let restore_update = history.create_restore_update("workspace", id1).unwrap();

        // Verify the restore update is not empty
        assert!(!restore_update.is_empty());

        // Verify the restore update can be decoded
        let decoded = Update::decode_v1(&restore_update);
        assert!(decoded.is_ok());
    }
}
