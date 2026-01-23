//! SQLite-backed storage implementation for CRDT persistence.
//!
//! This module provides a persistent storage backend using SQLite for storing
//! CRDT documents and their update history. It supports state reconstruction
//! for time-travel features.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, params};
use yrs::{Doc, ReadTxn, Transact, Update, updates::decoder::Decode, updates::encoder::Encode};

use super::storage::{CrdtStorage, StorageResult};
use super::types::{CrdtUpdate, UpdateOrigin};
use crate::error::DiaryxError;

/// Row type for file index queries: (path, title, part_of)
type FileIndexRow = (String, Option<String>, Option<String>);

/// SQLite-backed CRDT storage.
///
/// This implementation persists CRDT state and updates to a SQLite database,
/// enabling full version history and time-travel features.
///
/// # Thread Safety
///
/// The connection is wrapped in a `Mutex` for thread-safe access.
/// SQLite itself is used in serialized threading mode.
pub struct SqliteStorage {
    conn: Mutex<Connection>,
}

impl SqliteStorage {
    /// Open or create a SQLite database at the given path.
    ///
    /// This will create the necessary tables if they don't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be opened or if schema
    /// initialization fails.
    pub fn open<P: AsRef<Path>>(path: P) -> StorageResult<Self> {
        let conn = Connection::open(path)?;
        let storage = Self {
            conn: Mutex::new(conn),
        };
        storage.init_schema()?;
        Ok(storage)
    }

    /// Create an in-memory SQLite database for testing.
    ///
    /// Data is lost when the storage is dropped.
    pub fn in_memory() -> StorageResult<Self> {
        let conn = Connection::open_in_memory()?;
        let storage = Self {
            conn: Mutex::new(conn),
        };
        storage.init_schema()?;
        Ok(storage)
    }

    /// Initialize the database schema.
    fn init_schema(&self) -> StorageResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            -- Document snapshots (compacted state)
            CREATE TABLE IF NOT EXISTS documents (
                name TEXT PRIMARY KEY,
                state BLOB NOT NULL,
                state_vector BLOB NOT NULL,
                updated_at INTEGER NOT NULL
            );

            -- Incremental updates (for history)
            -- Note: No foreign key constraint since updates may arrive before document snapshot
            CREATE TABLE IF NOT EXISTS updates (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                doc_name TEXT NOT NULL,
                data BLOB NOT NULL,
                origin TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                device_id TEXT,
                device_name TEXT
            );

            -- Index for efficient sync queries
            CREATE INDEX IF NOT EXISTS idx_updates_doc_id ON updates(doc_name, id);

            -- Metadata for workspace files (queryable without loading CRDT)
            CREATE TABLE IF NOT EXISTS file_index (
                path TEXT PRIMARY KEY,
                title TEXT,
                part_of TEXT,
                deleted INTEGER NOT NULL DEFAULT 0,
                modified_at INTEGER NOT NULL
            );

            -- Index for querying non-deleted files
            CREATE INDEX IF NOT EXISTS idx_file_index_deleted ON file_index(deleted);
            "#,
        )?;
        Ok(())
    }

    /// Reconstruct CRDT state by applying updates up to a given ID.
    ///
    /// This creates a fresh Y.Doc and applies all updates from the base
    /// snapshot plus incremental updates up to the specified ID.
    fn reconstruct_state(&self, name: &str, up_to_id: i64) -> StorageResult<Option<Vec<u8>>> {
        let conn = self.conn.lock().unwrap();

        // Load base snapshot
        let base_state: Option<Vec<u8>> = conn
            .query_row(
                "SELECT state FROM documents WHERE name = ?",
                params![name],
                |row| row.get(0),
            )
            .ok();

        // Get updates up to the specified ID
        let mut stmt = conn
            .prepare("SELECT data FROM updates WHERE doc_name = ? AND id <= ? ORDER BY id ASC")?;
        let updates: Vec<Vec<u8>> = stmt
            .query_map(params![name, up_to_id], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        // If no base state and no updates, return None
        if base_state.is_none() && updates.is_empty() {
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
                let _ = txn.apply_update(update);
            }

            // Apply incremental updates
            for update_data in updates {
                if let Ok(update) = Update::decode_v1(&update_data) {
                    let _ = txn.apply_update(update);
                }
            }
        }

        // Encode final state
        let txn = doc.transact();
        Ok(Some(txn.encode_state_as_update_v1(&Default::default())))
    }

    /// Update the file index from a decoded FileMetadata.
    ///
    /// This keeps a queryable index of files without needing to load the CRDT.
    pub fn update_file_index(
        &self,
        path: &str,
        title: Option<&str>,
        part_of: Option<&str>,
        deleted: bool,
        modified_at: i64,
    ) -> StorageResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO file_index (path, title, part_of, deleted, modified_at)
             VALUES (?, ?, ?, ?, ?)",
            params![path, title, part_of, deleted as i32, modified_at],
        )?;
        Ok(())
    }

    /// Query active (non-deleted) files from the index.
    pub fn query_active_files(&self) -> StorageResult<Vec<FileIndexRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT path, title, part_of FROM file_index WHERE deleted = 0 ORDER BY path",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// Remove a file from the index entirely.
    pub fn remove_from_file_index(&self, path: &str) -> StorageResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM file_index WHERE path = ?", params![path])?;
        Ok(())
    }
}

impl std::fmt::Debug for SqliteStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteStorage").finish_non_exhaustive()
    }
}

impl CrdtStorage for SqliteStorage {
    fn load_doc(&self, name: &str) -> StorageResult<Option<Vec<u8>>> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT state FROM documents WHERE name = ?",
            params![name],
            |row| row.get(0),
        );

        match result {
            Ok(state) => Ok(Some(state)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(DiaryxError::Database(e)),
        }
    }

    fn save_doc(&self, name: &str, state: &[u8]) -> StorageResult<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();

        // Extract state vector from the state
        let state_vector = {
            let doc = Doc::new();
            {
                let mut txn = doc.transact_mut();
                if let Ok(update) = Update::decode_v1(state) {
                    let _ = txn.apply_update(update);
                }
            }
            let txn = doc.transact();
            txn.state_vector().encode_v1()
        };

        conn.execute(
            "INSERT OR REPLACE INTO documents (name, state, state_vector, updated_at)
             VALUES (?, ?, ?, ?)",
            params![name, state, state_vector, now],
        )?;
        Ok(())
    }

    fn delete_doc(&self, name: &str) -> StorageResult<()> {
        let conn = self.conn.lock().unwrap();
        // Delete updates first (foreign key)
        conn.execute("DELETE FROM updates WHERE doc_name = ?", params![name])?;
        conn.execute("DELETE FROM documents WHERE name = ?", params![name])?;
        Ok(())
    }

    fn list_docs(&self) -> StorageResult<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT name FROM documents ORDER BY name")?;
        let names = stmt
            .query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(names)
    }

    fn append_update_with_device(
        &self,
        name: &str,
        update: &[u8],
        origin: UpdateOrigin,
        device_id: Option<&str>,
        device_name: Option<&str>,
    ) -> StorageResult<i64> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let origin_str = origin.to_string();

        conn.execute(
            "INSERT INTO updates (doc_name, data, origin, timestamp, device_id, device_name) VALUES (?, ?, ?, ?, ?, ?)",
            params![name, update, origin_str, now, device_id, device_name],
        )?;

        Ok(conn.last_insert_rowid())
    }

    fn get_updates_since(&self, name: &str, since_id: i64) -> StorageResult<Vec<CrdtUpdate>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, data, origin, timestamp, device_id, device_name FROM updates
             WHERE doc_name = ? AND id > ?
             ORDER BY id ASC",
        )?;

        let updates = stmt
            .query_map(params![name, since_id], |row| {
                let origin_str: String = row.get(2)?;
                Ok(CrdtUpdate {
                    update_id: row.get(0)?,
                    doc_name: name.to_string(),
                    data: row.get(1)?,
                    timestamp: row.get(3)?,
                    origin: origin_str.parse().unwrap_or(UpdateOrigin::Local),
                    device_id: row.get(4)?,
                    device_name: row.get(5)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(updates)
    }

    fn get_all_updates(&self, name: &str) -> StorageResult<Vec<CrdtUpdate>> {
        self.get_updates_since(name, 0)
    }

    fn get_state_at(&self, name: &str, update_id: i64) -> StorageResult<Option<Vec<u8>>> {
        self.reconstruct_state(name, update_id)
    }

    fn compact(&self, name: &str, keep_updates: usize) -> StorageResult<()> {
        let mut conn = self.conn.lock().unwrap();

        // Get the current full state by reconstructing from base + all updates
        // This is done outside the transaction since it's read-only
        let full_state = {
            // Get base state
            let base_state: Option<Vec<u8>> = conn
                .query_row(
                    "SELECT state FROM documents WHERE name = ?",
                    params![name],
                    |row| row.get(0),
                )
                .ok();

            // Get all updates
            let mut stmt =
                conn.prepare("SELECT data FROM updates WHERE doc_name = ? ORDER BY id ASC")?;
            let updates: Vec<Vec<u8>> = stmt
                .query_map(params![name], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();

            if base_state.is_none() && updates.is_empty() {
                return Ok(());
            }

            // Reconstruct full state
            let doc = Doc::new();
            {
                let mut txn = doc.transact_mut();
                if let Some(state) = &base_state
                    && let Ok(update) = Update::decode_v1(state)
                {
                    let _ = txn.apply_update(update);
                }
                for update_data in &updates {
                    if let Ok(update) = Update::decode_v1(update_data) {
                        let _ = txn.apply_update(update);
                    }
                }
            }

            let txn = doc.transact();
            txn.encode_state_as_update_v1(&Default::default())
        };

        // Count updates
        let update_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM updates WHERE doc_name = ?",
            params![name],
            |row| row.get(0),
        )?;

        if update_count as usize <= keep_updates {
            return Ok(());
        }

        // Find the cutoff ID - keep the last `keep_updates` updates
        let cutoff_id: i64 = conn
            .query_row(
                "SELECT id FROM updates WHERE doc_name = ? ORDER BY id DESC LIMIT 1 OFFSET ?",
                params![name, keep_updates - 1],
                |row| row.get(0),
            )
            .unwrap_or(0);

        // Compute the state vector before starting transaction
        let now = chrono::Utc::now().timestamp_millis();
        let state_vector = {
            let doc = Doc::new();
            {
                let mut txn = doc.transact_mut();
                if let Ok(update) = Update::decode_v1(&full_state) {
                    let _ = txn.apply_update(update);
                }
            }
            let txn = doc.transact();
            txn.state_vector().encode_v1()
        };

        // Use a transaction to ensure atomicity of delete + update
        // This prevents data loss if the process crashes mid-operation
        let tx = conn.transaction()?;

        // IMPORTANT: Save the new snapshot FIRST, then delete old updates
        // This ensures we never lose data even if the transaction is interrupted
        tx.execute(
            "INSERT OR REPLACE INTO documents (name, state, state_vector, updated_at)
             VALUES (?, ?, ?, ?)",
            params![name, full_state, state_vector, now],
        )?;

        // Now safe to delete old updates since snapshot is saved
        tx.execute(
            "DELETE FROM updates WHERE doc_name = ? AND id < ?",
            params![name, cutoff_id],
        )?;

        // Commit the transaction - either both operations succeed or neither
        tx.commit()?;

        Ok(())
    }

    fn batch_append_updates(
        &self,
        updates: &[(&str, &[u8], UpdateOrigin)],
    ) -> StorageResult<Vec<i64>> {
        if updates.is_empty() {
            return Ok(vec![]);
        }

        let mut conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();

        // Use a SQL transaction for atomicity
        let tx = conn.transaction()?;
        let mut ids = Vec::with_capacity(updates.len());

        {
            let mut stmt = tx.prepare(
                "INSERT INTO updates (doc_name, data, origin, timestamp) VALUES (?, ?, ?, ?)",
            )?;

            for (name, update, origin) in updates {
                let origin_str = origin.to_string();
                stmt.execute(params![*name, *update, origin_str, now])?;
                ids.push(tx.last_insert_rowid());
            }
        }

        tx.commit()?;
        Ok(ids)
    }

    fn get_latest_update_id(&self, name: &str) -> StorageResult<i64> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT id FROM updates WHERE doc_name = ? ORDER BY id DESC LIMIT 1",
            params![name],
            |row| row.get(0),
        );

        match result {
            Ok(id) => Ok(id),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
            Err(e) => Err(DiaryxError::Database(e)),
        }
    }

    fn rename_doc(&self, old_name: &str, new_name: &str) -> StorageResult<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        // Rename document snapshot
        tx.execute(
            "UPDATE documents SET name = ? WHERE name = ?",
            params![new_name, old_name],
        )?;

        // Rename updates to point to new doc_name
        tx.execute(
            "UPDATE updates SET doc_name = ? WHERE doc_name = ?",
            params![new_name, old_name],
        )?;

        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yrs::Map;

    #[test]
    fn test_sqlite_save_and_load_doc() {
        let storage = SqliteStorage::in_memory().unwrap();
        let data = b"test document state";

        storage.save_doc("test", data).unwrap();
        let loaded = storage.load_doc("test").unwrap();

        assert!(loaded.is_some());
    }

    #[test]
    fn test_sqlite_load_nonexistent_doc() {
        let storage = SqliteStorage::in_memory().unwrap();
        let loaded = storage.load_doc("nonexistent").unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_sqlite_delete_doc() {
        let storage = SqliteStorage::in_memory().unwrap();
        storage.save_doc("test", b"data").unwrap();
        storage
            .append_update("test", b"update", UpdateOrigin::Local)
            .unwrap();

        storage.delete_doc("test").unwrap();

        assert!(storage.load_doc("test").unwrap().is_none());
        assert!(storage.get_all_updates("test").unwrap().is_empty());
    }

    #[test]
    fn test_sqlite_list_docs() {
        let storage = SqliteStorage::in_memory().unwrap();
        storage.save_doc("doc1", b"data1").unwrap();
        storage.save_doc("doc2", b"data2").unwrap();

        let docs = storage.list_docs().unwrap();

        assert_eq!(docs, vec!["doc1", "doc2"]);
    }

    #[test]
    fn test_sqlite_append_and_get_updates() {
        let storage = SqliteStorage::in_memory().unwrap();

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
    fn test_sqlite_get_latest_update_id() {
        let storage = SqliteStorage::in_memory().unwrap();

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
    fn test_sqlite_file_index() {
        let storage = SqliteStorage::in_memory().unwrap();

        storage
            .update_file_index("/test.md", Some("Test"), None, false, 1000)
            .unwrap();
        storage
            .update_file_index("/deleted.md", Some("Deleted"), None, true, 2000)
            .unwrap();
        storage
            .update_file_index("/child.md", Some("Child"), Some("/test.md"), false, 3000)
            .unwrap();

        let active = storage.query_active_files().unwrap();
        assert_eq!(active.len(), 2);
        assert_eq!(active[0].0, "/child.md");
        assert_eq!(active[1].0, "/test.md");

        storage.remove_from_file_index("/child.md").unwrap();
        let active = storage.query_active_files().unwrap();
        assert_eq!(active.len(), 1);
    }

    #[test]
    fn test_sqlite_compact_with_yrs() {
        let storage = SqliteStorage::in_memory().unwrap();

        // Create a real yrs document and generate updates
        let doc = Doc::new();
        let map = doc.get_or_insert_map("files");

        // Generate multiple updates
        for i in 0..10 {
            {
                let mut txn = doc.transact_mut();
                map.insert(&mut txn, format!("file{}", i), format!("value{}", i));
            }
            // Encode just this update
            let txn = doc.transact();
            let update = txn.encode_state_as_update_v1(&Default::default());
            storage
                .append_update("test", &update, UpdateOrigin::Local)
                .unwrap();
        }

        assert_eq!(storage.get_all_updates("test").unwrap().len(), 10);

        // Compact to keep only 3 updates
        storage.compact("test", 3).unwrap();

        let remaining = storage.get_all_updates("test").unwrap();
        assert_eq!(remaining.len(), 3);

        // Verify the document can still be reconstructed
        let state = storage.load_doc("test").unwrap();
        assert!(state.is_some());
    }

    #[test]
    fn test_sqlite_state_reconstruction() {
        let storage = SqliteStorage::in_memory().unwrap();

        // Create initial state
        let doc1 = Doc::new();
        let map1 = doc1.get_or_insert_map("files");
        {
            let mut txn = doc1.transact_mut();
            map1.insert(&mut txn, "file1", "value1");
        }
        let state1 = {
            let txn = doc1.transact();
            txn.encode_state_as_update_v1(&Default::default())
        };
        storage.save_doc("test", &state1).unwrap();

        // Add incremental updates
        {
            let mut txn = doc1.transact_mut();
            map1.insert(&mut txn, "file2", "value2");
        }
        let update1 = {
            let txn = doc1.transact();
            txn.encode_state_as_update_v1(&Default::default())
        };
        let id1 = storage
            .append_update("test", &update1, UpdateOrigin::Local)
            .unwrap();

        {
            let mut txn = doc1.transact_mut();
            map1.insert(&mut txn, "file3", "value3");
        }
        let update2 = {
            let txn = doc1.transact();
            txn.encode_state_as_update_v1(&Default::default())
        };
        let _id2 = storage
            .append_update("test", &update2, UpdateOrigin::Local)
            .unwrap();

        // Reconstruct state at id1 (should have file1 and file2, but not file3)
        let reconstructed = storage.get_state_at("test", id1).unwrap();
        assert!(reconstructed.is_some());

        // Verify reconstructed state
        let doc2 = Doc::new();
        {
            let mut txn = doc2.transact_mut();
            if let Ok(update) = Update::decode_v1(&reconstructed.unwrap()) {
                let _ = txn.apply_update(update);
            }
        }
        let map2 = doc2.get_or_insert_map("files");
        let txn = doc2.transact();

        // file1 and file2 should exist
        assert!(map2.get(&txn, "file1").is_some());
        assert!(map2.get(&txn, "file2").is_some());
    }

    #[test]
    fn test_sqlite_batch_append_updates() {
        let storage = SqliteStorage::in_memory().unwrap();

        // Batch append updates to multiple documents
        let updates: Vec<(&str, &[u8], UpdateOrigin)> = vec![
            ("doc1", b"update1", UpdateOrigin::Local),
            ("doc2", b"update2", UpdateOrigin::Local),
            ("doc1", b"update3", UpdateOrigin::Remote),
        ];

        let ids = storage.batch_append_updates(&updates).unwrap();
        assert_eq!(ids.len(), 3);
        assert!(ids[0] < ids[1]);
        assert!(ids[1] < ids[2]);

        // Verify updates were persisted to correct docs
        let doc1_updates = storage.get_all_updates("doc1").unwrap();
        assert_eq!(doc1_updates.len(), 2);
        assert_eq!(doc1_updates[0].origin, UpdateOrigin::Local);
        assert_eq!(doc1_updates[1].origin, UpdateOrigin::Remote);

        let doc2_updates = storage.get_all_updates("doc2").unwrap();
        assert_eq!(doc2_updates.len(), 1);
        assert_eq!(doc2_updates[0].origin, UpdateOrigin::Local);
    }

    #[test]
    fn test_sqlite_batch_append_empty() {
        let storage = SqliteStorage::in_memory().unwrap();

        // Empty batch should return empty vec
        let ids = storage.batch_append_updates(&[]).unwrap();
        assert!(ids.is_empty());
    }
}
