//! Workspace CRDT document for synchronizing file hierarchy.
//!
//! This module provides [`WorkspaceCrdt`], which wraps a yrs [`Doc`] to manage
//! the workspace's file hierarchy as a conflict-free replicated data type.
//!
//! # Doc-ID Based Architecture
//!
//! Files are keyed by stable document IDs (UUIDs) rather than file paths.
//! This makes renames and moves trivial property updates rather than
//! delete+create operations. The actual filesystem path is derived from the
//! `filename` field and the parent chain via `get_path()`.
//!
//! ```text
//! Y.Doc
//! └── Y.Map "files"
//!     ├── "abc123-uuid" → FileMetadata { filename: "index.md", part_of: None, ... }
//!     ├── "def456-uuid" → FileMetadata { filename: "daily.md", part_of: "abc123-uuid", ... }
//!     └── ...
//! ```
//!
//! ## Key Operations
//!
//! - `create_file()` - Create a new file with auto-generated UUID
//! - `get_path(doc_id)` - Derive filesystem path from doc_id chain
//! - `find_by_path(path)` - Find doc_id for a given path
//! - `rename_file(doc_id, new_name)` - Just update filename property
//! - `move_file(doc_id, new_parent)` - Just update part_of property
//!
//! ## Migration
//!
//! For workspaces using the legacy path-based format, `needs_migration()` checks
//! if migration is needed, and `migrate_to_doc_ids()` performs the conversion.
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

    // ==================== Doc-ID Based Operations ====================

    /// Create a new file with a generated UUID as the key.
    ///
    /// This is the primary method for creating files in the doc-ID based system.
    /// Returns the generated doc_id that can be used to reference this file.
    ///
    /// # Arguments
    /// * `metadata` - File metadata (must have `filename` set)
    ///
    /// # Returns
    /// The generated doc_id (UUID) for the new file.
    pub fn create_file(&self, metadata: FileMetadata) -> StorageResult<String> {
        let doc_id = uuid::Uuid::new_v4().to_string();
        self.set_file(&doc_id, metadata)?;
        Ok(doc_id)
    }

    /// Derive the filesystem path from a doc_id by walking the parent chain.
    ///
    /// This reconstructs the full path by traversing the `part_of` references
    /// up to the root and then joining the filenames.
    ///
    /// # Arguments
    /// * `doc_id` - The document ID to get the path for
    ///
    /// # Returns
    /// The derived path, or None if the doc_id doesn't exist or the chain is broken.
    pub fn get_path(&self, doc_id: &str) -> Option<PathBuf> {
        let mut parts = Vec::new();
        let mut current = doc_id.to_string();
        let mut visited = std::collections::HashSet::new();

        // Walk up the parent chain, collecting filenames
        loop {
            // Prevent infinite loops from circular references
            if visited.contains(&current) {
                log::warn!("Circular reference detected in get_path for {}", doc_id);
                return None;
            }
            visited.insert(current.clone());

            let meta = self.get_file(&current)?;

            // For deleted files, we still want to derive the path
            if meta.filename.is_empty() {
                // Legacy entry or corrupted - can't derive path
                log::warn!("Empty filename for doc_id {}", current);
                return None;
            }

            parts.push(meta.filename.clone());

            match meta.part_of {
                Some(parent_id) => {
                    // Check if parent is a UUID (doc-ID system) or a path (legacy)
                    if parent_id.contains('/') || parent_id.ends_with(".md") {
                        // Legacy path-based reference - this is a migration state
                        // For now, we can't fully resolve this
                        log::debug!(
                            "Legacy path reference in part_of: {} for {}",
                            parent_id,
                            current
                        );
                        // Try to find the parent by path
                        if let Some(parent_doc_id) = self.find_by_path_legacy(&parent_id) {
                            current = parent_doc_id;
                        } else {
                            return None;
                        }
                    } else {
                        current = parent_id;
                    }
                }
                None => break, // Reached root
            }
        }

        // Reverse to get path from root to leaf
        parts.reverse();
        Some(PathBuf::from_iter(parts))
    }

    /// Derive a filesystem path from a doc_id using a provided snapshot of files.
    ///
    /// This is similar to `get_path` but uses a pre-captured snapshot instead of
    /// the current CRDT state. This is critical for rename detection where we need
    /// to derive the OLD path using the old state and the NEW path using the new state.
    ///
    /// # Arguments
    /// * `doc_id` - The document ID to derive the path for
    /// * `meta` - The metadata for this doc_id from the snapshot
    /// * `snapshot` - A HashMap of all files from the snapshot
    ///
    /// # Returns
    /// The derived path as a String, or None if the chain is broken.
    fn derive_path_from_snapshot(
        &self,
        doc_id: &str,
        meta: &FileMetadata,
        snapshot: &HashMap<String, FileMetadata>,
    ) -> Option<String> {
        let mut parts = Vec::new();
        let mut current_id = doc_id.to_string();
        let mut current_meta = meta;
        let mut visited = std::collections::HashSet::new();

        loop {
            if visited.contains(&current_id) {
                log::warn!(
                    "Circular reference in derive_path_from_snapshot for {}",
                    doc_id
                );
                return None;
            }
            visited.insert(current_id.clone());

            if current_meta.filename.is_empty() {
                return None;
            }

            parts.push(current_meta.filename.clone());

            match &current_meta.part_of {
                Some(parent_id) => {
                    // Check if parent is a UUID or path
                    if parent_id.contains('/') || parent_id.ends_with(".md") {
                        // Legacy path - can't resolve in snapshot mode
                        return None;
                    }
                    // Look up parent in snapshot
                    match snapshot.get(parent_id) {
                        Some(parent_meta) => {
                            current_id = parent_id.clone();
                            current_meta = parent_meta;
                        }
                        None => return None,
                    }
                }
                None => break, // Reached root
            }
        }

        parts.reverse();
        Some(parts.join("/"))
    }

    /// Find a doc_id by filesystem path.
    ///
    /// This walks the tree to find a file with the matching path.
    /// The path is matched by traversing from root files down through
    /// the `contents` hierarchy.
    ///
    /// # Arguments
    /// * `path` - The path to search for
    ///
    /// # Returns
    /// The doc_id if found, or None.
    pub fn find_by_path(&self, path: &std::path::Path) -> Option<String> {
        let path_components: Vec<&str> = path
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => s.to_str(),
                _ => None,
            })
            .collect();

        if path_components.is_empty() {
            return None;
        }

        // Get all files
        let files = self.list_files();

        // First, find root files (no part_of or part_of is None/empty)
        let root_files: Vec<_> = files
            .iter()
            .filter(|(_, meta)| !meta.deleted && meta.part_of.is_none())
            .collect();

        // Start search from root files
        for (doc_id, meta) in root_files {
            if let Some(found) =
                self.find_by_path_recursive(doc_id, meta, &path_components, 0, &files)
            {
                return Some(found);
            }
        }

        None
    }

    /// Recursive helper for find_by_path.
    fn find_by_path_recursive(
        &self,
        doc_id: &str,
        meta: &FileMetadata,
        path_components: &[&str],
        depth: usize,
        all_files: &[(String, FileMetadata)],
    ) -> Option<String> {
        if depth >= path_components.len() {
            return None;
        }

        // Check if this file's filename matches the current path component
        if meta.filename != path_components[depth] {
            return None;
        }

        // If we've matched all components, this is the file
        if depth == path_components.len() - 1 {
            return Some(doc_id.to_string());
        }

        // Otherwise, search children
        if let Some(ref contents) = meta.contents {
            for child_id in contents {
                // Find the child in all_files
                if let Some((_, child_meta)) = all_files
                    .iter()
                    .find(|(id, m)| id == child_id && !m.deleted)
                {
                    if let Some(found) = self.find_by_path_recursive(
                        child_id,
                        child_meta,
                        path_components,
                        depth + 1,
                        all_files,
                    ) {
                        return Some(found);
                    }
                }
            }
        }

        None
    }

    /// Find a doc_id by legacy path format.
    ///
    /// This is used during migration to resolve path-based `part_of` references.
    fn find_by_path_legacy(&self, path: &str) -> Option<String> {
        // In legacy mode, the keys ARE paths
        let files = self.list_files();
        for (key, meta) in files {
            if key == path && !meta.deleted {
                return Some(key);
            }
        }
        None
    }

    /// Rename a file by updating its filename.
    ///
    /// In the doc-ID system, renames are trivial - just update the filename property.
    /// The doc_id remains stable.
    ///
    /// # Arguments
    /// * `doc_id` - The document ID of the file to rename
    /// * `new_filename` - The new filename (e.g., "new-name.md")
    pub fn rename_file(&self, doc_id: &str, new_filename: &str) -> StorageResult<()> {
        if let Some(mut meta) = self.get_file(doc_id) {
            meta.filename = new_filename.to_string();
            meta.modified_at = chrono::Utc::now().timestamp_millis();
            self.set_file(doc_id, meta)?;
        }
        Ok(())
    }

    /// Move a file by updating its parent reference.
    ///
    /// In the doc-ID system, moves are trivial - just update the part_of property.
    /// The doc_id remains stable.
    ///
    /// # Arguments
    /// * `doc_id` - The document ID of the file to move
    /// * `new_parent_id` - The doc_id of the new parent, or None for root
    pub fn move_file(&self, doc_id: &str, new_parent_id: Option<&str>) -> StorageResult<()> {
        if let Some(mut meta) = self.get_file(doc_id) {
            meta.part_of = new_parent_id.map(String::from);
            meta.modified_at = chrono::Utc::now().timestamp_millis();
            self.set_file(doc_id, meta)?;
        }
        Ok(())
    }

    // ==================== Migration ====================

    /// Check if the workspace needs migration from path-based to doc-ID-based format.
    ///
    /// Returns true if any file key contains a path separator ('/').
    pub fn needs_migration(&self) -> bool {
        self.list_files().iter().any(|(key, _)| key.contains('/'))
    }

    /// Migrate the workspace from path-based to doc-ID-based format.
    ///
    /// This performs the following steps:
    /// 1. Generate UUIDs for all existing files
    /// 2. Extract filename from each path
    /// 3. Convert part_of paths to doc_ids
    /// 4. Convert contents paths to doc_ids
    /// 5. Delete old path-based entries
    /// 6. Create new UUID-based entries
    ///
    /// # Returns
    /// Number of files migrated, or an error.
    pub fn migrate_to_doc_ids(&self) -> StorageResult<usize> {
        use std::collections::HashMap;

        let old_files = self.list_files();

        if old_files.is_empty() {
            return Ok(0);
        }

        // Generate UUIDs for all existing files
        let mut path_to_id: HashMap<String, String> = HashMap::new();
        for (path, _) in &old_files {
            path_to_id.insert(path.clone(), uuid::Uuid::new_v4().to_string());
        }

        // Migrate each file
        for (old_path, mut metadata) in old_files.clone() {
            let new_id = match path_to_id.get(&old_path) {
                Some(id) => id.clone(),
                None => continue,
            };

            // Extract filename from path
            metadata.filename = std::path::Path::new(&old_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            // Convert part_of path to doc_id
            if let Some(ref parent_path) = metadata.part_of {
                // Try to find the parent in our path_to_id mapping
                if let Some(parent_id) = path_to_id.get(parent_path) {
                    metadata.part_of = Some(parent_id.clone());
                } else {
                    // Parent might be a relative path - try to resolve it
                    let parent_dir = std::path::Path::new(&old_path).parent();
                    if let Some(dir) = parent_dir {
                        let absolute_parent = dir.join(parent_path);
                        let absolute_str = absolute_parent.to_string_lossy().to_string();
                        if let Some(parent_id) = path_to_id.get(&absolute_str) {
                            metadata.part_of = Some(parent_id.clone());
                        }
                        // If still not found, leave as-is (will be cleaned up later)
                    }
                }
            }

            // Convert contents paths to doc_ids
            if let Some(ref contents) = metadata.contents {
                let parent_dir = std::path::Path::new(&old_path).parent();
                let new_contents: Vec<String> = contents
                    .iter()
                    .filter_map(|rel_path| {
                        // Resolve relative path to absolute
                        let abs_path = if let Some(dir) = parent_dir {
                            dir.join(rel_path).to_string_lossy().to_string()
                        } else {
                            rel_path.clone()
                        };
                        path_to_id.get(&abs_path).cloned()
                    })
                    .collect();
                metadata.contents = if new_contents.is_empty() {
                    None
                } else {
                    Some(new_contents)
                };
            }

            // Create new entry with UUID key
            self.set_file(&new_id, metadata)?;

            // Remove old path-based entry
            self.remove_file(&old_path)?;
        }

        log::info!(
            "Migrated {} files from path-based to doc-ID-based format",
            path_to_id.len()
        );

        Ok(path_to_id.len())
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
            // Note: apply_update doesn't detect renames, so pass empty slice
            self.emit_diff_events(&files_before, &files_after, &[]);
        }

        // Persist the update to storage
        let update_id = self.storage.append_update(&self.doc_name, update, origin)?;
        Ok(Some(update_id))
    }

    /// Apply an update from a remote peer and return the list of changed file paths.
    ///
    /// This is like `apply_update` but returns the paths of files that changed,
    /// allowing callers to selectively write those files to disk.
    ///
    /// Returns (update_id, changed_paths, renames) where:
    /// - changed_paths includes newly created, deleted, and modified files
    /// - renames is a list of (old_path, new_path) pairs for detected renames
    pub fn apply_update_tracking_changes(
        &self,
        update: &[u8],
        origin: UpdateOrigin,
    ) -> StorageResult<(Option<i64>, Vec<String>, Vec<(String, String)>)> {
        // Capture state before the update
        let files_before: HashMap<String, FileMetadata> = self.list_files().into_iter().collect();

        // Decode and apply the update
        let decoded = Update::decode_v1(update)
            .map_err(|e| DiaryxError::Unsupported(format!("Failed to decode update: {}", e)))?;

        {
            let mut txn = self.doc.transact_mut();
            txn.apply_update(decoded)
                .map_err(|e| DiaryxError::Unsupported(format!("Failed to apply update: {}", e)))?;
        }

        // Capture state after the update
        let files_after: HashMap<String, FileMetadata> = self.list_files().into_iter().collect();

        // Detect doc-ID based renames: same key with different filename
        // In doc-ID mode, the key is a UUID and renames are just filename property updates
        let mut renames: Vec<(String, String)> = Vec::new();
        for (doc_id, new_meta) in &files_after {
            if let Some(old_meta) = files_before.get(doc_id) {
                // Same doc_id, different filename = rename in doc-ID mode
                if old_meta.filename != new_meta.filename
                    && !old_meta.filename.is_empty()
                    && !new_meta.filename.is_empty()
                    && !old_meta.deleted
                    && !new_meta.deleted
                {
                    // Derive old and new paths using the respective snapshots
                    let old_path = self.derive_path_from_snapshot(doc_id, old_meta, &files_before);
                    let new_path = self.derive_path_from_snapshot(doc_id, new_meta, &files_after);

                    if let (Some(old_p), Some(new_p)) = (old_path, new_path) {
                        log::debug!(
                            "[WorkspaceCrdt] Doc-ID rename detected: {} -> {}",
                            old_p,
                            new_p
                        );
                        renames.push((old_p, new_p));
                    }
                }
            }
        }

        // Compute changed paths
        let mut changed_paths = Vec::new();

        // Detect created files (new files that weren't in the previous state)
        for (path, metadata) in &files_after {
            if !files_before.contains_key(path) && !metadata.deleted {
                changed_paths.push(path.clone());
            }
        }

        // Also detect files that were previously deleted but are now restored
        for (path, metadata) in &files_after {
            if let Some(old_meta) = files_before.get(path) {
                if old_meta.deleted && !metadata.deleted && !changed_paths.contains(path) {
                    changed_paths.push(path.clone());
                }
            }
        }

        // Detect deleted files (both newly deleted and already-deleted files that need disk cleanup)
        for (path, _old_meta) in &files_before {
            let is_deleted = files_after.get(path).map(|m| m.deleted).unwrap_or(true);
            // Include if: file is now deleted (whether or not it was before)
            // This ensures disk cleanup happens even if CRDT state was persisted from a previous session
            if is_deleted && !changed_paths.contains(path) {
                changed_paths.push(path.clone());
            }
        }

        // Also include files that only exist in files_after and are deleted
        // (in case they weren't in files_before at all)
        for (path, metadata) in &files_after {
            if metadata.deleted && !files_before.contains_key(path) && !changed_paths.contains(path)
            {
                changed_paths.push(path.clone());
            }
        }

        // Detect metadata changes
        for (path, new_meta) in &files_after {
            if let Some(old_meta) = files_before.get(path) {
                if old_meta != new_meta && !new_meta.deleted && !old_meta.deleted {
                    // Only add if not already in the list
                    if !changed_paths.contains(path) {
                        changed_paths.push(path.clone());
                    }
                }
            }
        }

        // Detect renames: a file deleted + a file created with same part_of
        // We use multiple matching strategies in order of confidence:
        // 1. Same parent AND same title (highest confidence)
        // 2. Same parent AND similar modified_at timestamp (within 5 seconds)
        // 3. Same parent with only ONE candidate pair (fallback)
        // Note: renames vector was already initialized above with doc-ID based renames
        let mut matched_created: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut matched_deleted: std::collections::HashSet<&str> = std::collections::HashSet::new();

        // Get files that are now deleted (were not deleted before, now deleted)
        let deleted_files: Vec<(&String, &FileMetadata)> = files_before
            .iter()
            .filter(|(path, old_meta)| {
                // File must not have been deleted before
                if old_meta.deleted {
                    return false;
                }
                // And must now be deleted (or removed entirely)
                files_after.get(*path).map(|m| m.deleted).unwrap_or(true)
            })
            .collect();

        let created_files: Vec<(&String, &FileMetadata)> = files_after
            .iter()
            .filter(|(path, meta)| !files_before.contains_key(*path) && !meta.deleted)
            .collect();

        log::debug!(
            "[WorkspaceCrdt] Rename detection: {} deleted files, {} created files",
            deleted_files.len(),
            created_files.len()
        );

        // Strategy 1: Match by title (highest confidence)
        for (deleted_path, deleted_meta) in &deleted_files {
            for (created_path, _) in &created_files {
                if matched_created.contains(created_path.as_str()) {
                    continue;
                }

                let created_meta = files_after.get(*created_path);
                let same_part_of =
                    deleted_meta.part_of == created_meta.and_then(|m| m.part_of.clone());
                let same_title = deleted_meta.title.is_some()
                    && deleted_meta.title == created_meta.and_then(|m| m.title.clone());

                if same_part_of && same_title {
                    log::debug!(
                        "[WorkspaceCrdt] Rename by title match: {} -> {}",
                        deleted_path,
                        created_path
                    );
                    renames.push(((*deleted_path).clone(), (*created_path).clone()));
                    matched_created.insert(created_path.as_str());
                    matched_deleted.insert(deleted_path.as_str());
                    break;
                }
            }
        }

        // Strategy 2: Match by modified_at timestamp (within 5 seconds)
        const TIMESTAMP_THRESHOLD_MS: i64 = 5000;
        for (deleted_path, deleted_meta) in &deleted_files {
            if matched_deleted.contains(deleted_path.as_str()) {
                continue;
            }
            for (created_path, _) in &created_files {
                if matched_created.contains(created_path.as_str()) {
                    continue;
                }

                let created_meta = files_after.get(*created_path);
                let same_part_of =
                    deleted_meta.part_of == created_meta.and_then(|m| m.part_of.clone());
                let similar_timestamp = created_meta
                    .map(|m| {
                        (deleted_meta.modified_at - m.modified_at).abs() < TIMESTAMP_THRESHOLD_MS
                    })
                    .unwrap_or(false);

                if same_part_of && similar_timestamp {
                    log::debug!(
                        "[WorkspaceCrdt] Rename by timestamp match: {} -> {} (delta: {}ms)",
                        deleted_path,
                        created_path,
                        created_meta
                            .map(|m| (deleted_meta.modified_at - m.modified_at).abs())
                            .unwrap_or(0)
                    );
                    renames.push(((*deleted_path).clone(), (*created_path).clone()));
                    matched_created.insert(created_path.as_str());
                    matched_deleted.insert(deleted_path.as_str());
                    break;
                }
            }
        }

        // Strategy 3: If there's exactly ONE unmatched created file with a parent that has
        // exactly ONE unmatched deleted file, assume it's a rename
        for (created_path, _) in &created_files {
            if matched_created.contains(created_path.as_str()) {
                continue;
            }

            let created_meta = files_after.get(*created_path);
            let created_parent = created_meta.and_then(|m| m.part_of.clone());

            // Find all unmatched deleted files with the same parent
            let matching_deleted: Vec<_> = deleted_files
                .iter()
                .filter(|(dp, dm)| {
                    !matched_deleted.contains(dp.as_str()) && dm.part_of == created_parent
                })
                .collect();

            // If exactly one match, it's likely a rename
            if matching_deleted.len() == 1 {
                let (deleted_path, _) = matching_deleted[0];
                log::debug!(
                    "[WorkspaceCrdt] Rename by single-pair fallback: {} -> {}",
                    deleted_path,
                    created_path
                );
                renames.push(((*deleted_path).clone(), (*created_path).clone()));
                matched_created.insert(created_path.as_str());
                matched_deleted.insert(deleted_path.as_str());
            }
        }

        log::debug!("[WorkspaceCrdt] Final renames detected: {:?}", renames);

        // Emit events if callback is set
        if origin != UpdateOrigin::Local && self.event_callback.is_some() {
            self.emit_diff_events(&files_before, &files_after, &renames);
        }

        // Persist the update to storage
        let update_id = self.storage.append_update(&self.doc_name, update, origin)?;
        Ok((Some(update_id), changed_paths, renames))
    }

    /// Emit filesystem events for changes between two states.
    ///
    /// This compares the before and after states and emits appropriate events:
    /// - `FileRenamed` for files that were renamed (detected as delete+create with same parent)
    /// - `FileCreated` for new, non-deleted files (excluding renames)
    /// - `FileDeleted` for files that were deleted (excluding renames)
    /// - `MetadataChanged` for files whose metadata changed
    fn emit_diff_events(
        &self,
        before: &HashMap<String, FileMetadata>,
        after: &HashMap<String, FileMetadata>,
        renames: &[(String, String)],
    ) {
        // Collect paths involved in renames to exclude from delete/create events
        let renamed_old_paths: std::collections::HashSet<&str> =
            renames.iter().map(|(old, _)| old.as_str()).collect();
        let renamed_new_paths: std::collections::HashSet<&str> =
            renames.iter().map(|(_, new)| new.as_str()).collect();

        // Emit FileRenamed events first
        for (old_path, new_path) in renames {
            self.emit_event(FileSystemEvent::file_renamed(
                PathBuf::from(old_path),
                PathBuf::from(new_path),
            ));
        }

        // Detect created files (in after but not in before, and not deleted, excluding renames)
        for (path, metadata) in after {
            if !before.contains_key(path)
                && !metadata.deleted
                && !renamed_new_paths.contains(path.as_str())
            {
                self.emit_event(FileSystemEvent::file_created_with_metadata(
                    PathBuf::from(path),
                    Some(self.metadata_to_frontmatter(metadata)),
                    metadata.part_of.as_ref().map(PathBuf::from),
                ));
            }
        }

        // Detect restored files (was deleted, now not deleted)
        for (path, metadata) in after {
            if let Some(old_meta) = before.get(path) {
                if old_meta.deleted && !metadata.deleted {
                    self.emit_event(FileSystemEvent::file_created_with_metadata(
                        PathBuf::from(path),
                        Some(self.metadata_to_frontmatter(metadata)),
                        metadata.part_of.as_ref().map(PathBuf::from),
                    ));
                }
            }
        }

        // Detect deleted files - emit for any file that is marked as deleted (excluding renames)
        // This ensures UI updates even if CRDT state was already persisted
        for (path, old_meta) in before {
            if renamed_old_paths.contains(path.as_str()) {
                continue; // Skip - this was a rename, not a delete
            }
            let is_deleted = after.get(path).map(|m| m.deleted).unwrap_or(true);
            if is_deleted {
                let parent = after
                    .get(path)
                    .and_then(|m| m.part_of.as_ref())
                    .or(old_meta.part_of.as_ref())
                    .map(PathBuf::from);
                self.emit_event(FileSystemEvent::file_deleted_with_parent(
                    PathBuf::from(path),
                    parent,
                ));
            }
        }

        // Also handle files that are only in 'after' and are deleted
        for (path, metadata) in after {
            if metadata.deleted && !before.contains_key(path) {
                self.emit_event(FileSystemEvent::file_deleted_with_parent(
                    PathBuf::from(path),
                    metadata.part_of.as_ref().map(PathBuf::from),
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

    // ==================== Doc-ID Based Tests ====================

    #[test]
    fn test_create_file_returns_uuid() {
        let crdt = create_test_crdt();
        let metadata = FileMetadata::with_filename("test.md".to_string(), Some("Test".to_string()));

        let doc_id = crdt.create_file(metadata.clone()).unwrap();

        // Doc ID should be a valid UUID format (36 chars with dashes)
        assert_eq!(doc_id.len(), 36);
        assert!(doc_id.contains('-'));

        // File should be retrievable by doc_id
        let retrieved = crdt.get_file(&doc_id).unwrap();
        assert_eq!(retrieved.filename, "test.md");
        assert_eq!(retrieved.title, Some("Test".to_string()));
    }

    #[test]
    fn test_get_path_simple() {
        let crdt = create_test_crdt();

        // Create a root file
        let root_meta =
            FileMetadata::with_filename("root.md".to_string(), Some("Root".to_string()));
        let root_id = crdt.create_file(root_meta).unwrap();

        let path = crdt.get_path(&root_id).unwrap();
        assert_eq!(path, PathBuf::from("root.md"));
    }

    #[test]
    fn test_get_path_nested() {
        let crdt = create_test_crdt();

        // Create parent
        let parent_meta =
            FileMetadata::with_filename("parent".to_string(), Some("Parent".to_string()));
        let parent_id = crdt.create_file(parent_meta).unwrap();

        // Create child with parent reference
        let mut child_meta =
            FileMetadata::with_filename("child.md".to_string(), Some("Child".to_string()));
        child_meta.part_of = Some(parent_id.clone());
        let child_id = crdt.create_file(child_meta).unwrap();

        let path = crdt.get_path(&child_id).unwrap();
        assert_eq!(path, PathBuf::from("parent/child.md"));
    }

    #[test]
    fn test_get_path_deeply_nested() {
        let crdt = create_test_crdt();

        // Create: grandparent/parent/child.md
        let gp_meta = FileMetadata::with_filename("grandparent".to_string(), None);
        let gp_id = crdt.create_file(gp_meta).unwrap();

        let mut p_meta = FileMetadata::with_filename("parent".to_string(), None);
        p_meta.part_of = Some(gp_id.clone());
        let p_id = crdt.create_file(p_meta).unwrap();

        let mut child_meta =
            FileMetadata::with_filename("child.md".to_string(), Some("Child".to_string()));
        child_meta.part_of = Some(p_id.clone());
        let child_id = crdt.create_file(child_meta).unwrap();

        let path = crdt.get_path(&child_id).unwrap();
        assert_eq!(path, PathBuf::from("grandparent/parent/child.md"));
    }

    #[test]
    fn test_rename_file() {
        let crdt = create_test_crdt();

        let meta = FileMetadata::with_filename("old-name.md".to_string(), Some("Test".to_string()));
        let doc_id = crdt.create_file(meta).unwrap();

        // Rename
        crdt.rename_file(&doc_id, "new-name.md").unwrap();

        // Check filename updated
        let retrieved = crdt.get_file(&doc_id).unwrap();
        assert_eq!(retrieved.filename, "new-name.md");
        assert_eq!(retrieved.title, Some("Test".to_string())); // Title preserved

        // Path should reflect new filename
        let path = crdt.get_path(&doc_id).unwrap();
        assert_eq!(path, PathBuf::from("new-name.md"));
    }

    #[test]
    fn test_move_file() {
        let crdt = create_test_crdt();

        // Create two parent folders
        let parent1_meta = FileMetadata::with_filename("folder1".to_string(), None);
        let parent1_id = crdt.create_file(parent1_meta).unwrap();

        let parent2_meta = FileMetadata::with_filename("folder2".to_string(), None);
        let parent2_id = crdt.create_file(parent2_meta).unwrap();

        // Create file in folder1
        let mut file_meta =
            FileMetadata::with_filename("file.md".to_string(), Some("Test".to_string()));
        file_meta.part_of = Some(parent1_id.clone());
        let file_id = crdt.create_file(file_meta).unwrap();

        // Move to folder2
        crdt.move_file(&file_id, Some(&parent2_id)).unwrap();

        // Check parent updated
        let retrieved = crdt.get_file(&file_id).unwrap();
        assert_eq!(retrieved.part_of, Some(parent2_id.clone()));

        // Path should reflect new parent
        let path = crdt.get_path(&file_id).unwrap();
        assert_eq!(path, PathBuf::from("folder2/file.md"));
    }

    #[test]
    fn test_needs_migration_false_for_uuids() {
        let crdt = create_test_crdt();

        // Create file with UUID key (doc-ID based)
        let meta = FileMetadata::with_filename("test.md".to_string(), Some("Test".to_string()));
        let _ = crdt.create_file(meta).unwrap();

        // Should not need migration since keys are UUIDs
        assert!(!crdt.needs_migration());
    }

    #[test]
    fn test_needs_migration_true_for_paths() {
        let crdt = create_test_crdt();

        // Simulate legacy path-based entry
        let meta = FileMetadata::with_filename("test.md".to_string(), Some("Test".to_string()));
        crdt.set_file("workspace/notes/test.md", meta).unwrap();

        // Should need migration since key contains '/'
        assert!(crdt.needs_migration());
    }

    #[test]
    fn test_migrate_to_doc_ids() {
        let crdt = create_test_crdt();

        // Create legacy path-based entries
        let mut parent_meta = FileMetadata::new(Some("Parent".to_string()));
        parent_meta.contents = Some(vec!["child.md".to_string()]);
        crdt.set_file("workspace/parent.md", parent_meta).unwrap();

        let mut child_meta = FileMetadata::new(Some("Child".to_string()));
        child_meta.part_of = Some("workspace/parent.md".to_string());
        crdt.set_file("workspace/child.md", child_meta).unwrap();

        // Run migration
        let count = crdt.migrate_to_doc_ids().unwrap();
        assert_eq!(count, 2);

        // Should no longer need migration
        assert!(!crdt.needs_migration());

        // Files should now have filenames set
        let files = crdt.list_files();
        for (doc_id, meta) in &files {
            // Keys should be UUIDs (no slashes)
            assert!(!doc_id.contains('/'), "Key should be UUID: {}", doc_id);
            // Filenames should be set
            assert!(!meta.filename.is_empty(), "Filename should be set");
        }
    }

    #[test]
    fn test_normalize_title_to_filename() {
        assert_eq!(
            FileMetadata::normalize_title_to_filename("My Note"),
            "my-note.md"
        );
        assert_eq!(
            FileMetadata::normalize_title_to_filename("Hello World!"),
            "hello-world.md"
        );
        assert_eq!(
            FileMetadata::normalize_title_to_filename("Test_File Name"),
            "test-file-name.md"
        );
    }
}
