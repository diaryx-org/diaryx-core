//! Storage abstraction for CRDT persistence.
//!
//! This module defines the [`CrdtStorage`] trait which abstracts over different
//! storage backends (SQLite, in-memory) for persisting CRDT documents and updates.

use super::types::{CrdtUpdate, UpdateOrigin};
use crate::error::DiaryxError;

/// Result type for storage operations.
pub type StorageResult<T> = Result<T, DiaryxError>;

/// Trait for CRDT document storage backends.
///
/// Implementations of this trait handle persisting CRDT state and updates
/// to various storage backends (SQLite for native, OPFS for WASM, memory for tests).
///
/// # Storage Model
///
/// The storage maintains two types of data:
/// 1. **Document snapshots**: Compacted full state of a CRDT document
/// 2. **Update log**: Incremental updates for history and sync
///
/// The update log enables:
/// - Version history and time-travel
/// - Efficient sync (send only missing updates)
/// - Undo/redo functionality
pub trait CrdtStorage: Send + Sync {
    /// Load the full document state as a binary blob.
    ///
    /// Returns `None` if the document doesn't exist.
    fn load_doc(&self, name: &str) -> StorageResult<Option<Vec<u8>>>;

    /// Save the full document state.
    ///
    /// This overwrites any existing state for the document.
    fn save_doc(&self, name: &str, state: &[u8]) -> StorageResult<()>;

    /// Delete a document and all its updates.
    fn delete_doc(&self, name: &str) -> StorageResult<()>;

    /// List all document names in storage.
    fn list_docs(&self) -> StorageResult<Vec<String>>;

    /// Append an incremental update to the update log.
    ///
    /// Returns the ID of the newly created update record.
    fn append_update(&self, name: &str, update: &[u8], origin: UpdateOrigin) -> StorageResult<i64> {
        self.append_update_with_device(name, update, origin, None, None)
    }

    /// Append an incremental update with device attribution.
    ///
    /// Returns the ID of the newly created update record.
    fn append_update_with_device(
        &self,
        name: &str,
        update: &[u8],
        origin: UpdateOrigin,
        device_id: Option<&str>,
        device_name: Option<&str>,
    ) -> StorageResult<i64>;

    /// Append multiple updates atomically.
    ///
    /// All updates are applied in a single transaction. If any update fails,
    /// no updates are persisted. This enables atomic operations across multiple
    /// documents (e.g., creating a file updates both workspace and body CRDTs).
    ///
    /// Returns the IDs of all newly created update records in order.
    fn batch_append_updates(
        &self,
        updates: &[(&str, &[u8], UpdateOrigin)],
    ) -> StorageResult<Vec<i64>> {
        // Default implementation: apply updates sequentially (not atomic)
        // Storage backends should override this with proper transaction support
        let mut ids = Vec::with_capacity(updates.len());
        for (name, update, origin) in updates {
            ids.push(self.append_update(name, update, *origin)?);
        }
        Ok(ids)
    }

    /// Get all updates for a document since a given update ID.
    ///
    /// This is used for sync: a client sends their last known update ID,
    /// and receives all updates that happened since then.
    fn get_updates_since(&self, name: &str, since_id: i64) -> StorageResult<Vec<CrdtUpdate>>;

    /// Get all updates for a document.
    fn get_all_updates(&self, name: &str) -> StorageResult<Vec<CrdtUpdate>>;

    /// Get the state of a document at a specific point in history.
    ///
    /// This reconstructs the document state by applying updates up to
    /// (and including) the specified update ID.
    fn get_state_at(&self, name: &str, update_id: i64) -> StorageResult<Option<Vec<u8>>>;

    /// Compact old updates into the document snapshot.
    ///
    /// This merges old updates into the base snapshot, keeping only
    /// the most recent `keep_updates` in the log. This saves space
    /// while preserving recent history.
    fn compact(&self, name: &str, keep_updates: usize) -> StorageResult<()>;

    /// Get the latest update ID for a document.
    ///
    /// Returns 0 if no updates exist.
    fn get_latest_update_id(&self, name: &str) -> StorageResult<i64>;

    /// Rename a document by copying its state and updates to a new name.
    ///
    /// This operation:
    /// 1. Copies the document snapshot from old name to new name
    /// 2. Copies all updates to the new document name
    /// 3. Deletes the old document and its updates
    ///
    /// Used when renaming files to migrate their body CRDT state.
    fn rename_doc(&self, old_name: &str, new_name: &str) -> StorageResult<()>;
}

#[cfg(test)]
mod tests {
    // Tests are in memory_storage.rs using MemoryStorage
}
