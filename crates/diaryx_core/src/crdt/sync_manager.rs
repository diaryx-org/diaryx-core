//! Unified sync manager for workspace and body synchronization.
//!
//! This module provides `RustSyncManager`, which replaces all TypeScript sync bridges
//! (bodySyncBridge.ts, rustSyncBridge.ts) with a single unified Rust implementation.
//!
//! # Responsibilities
//!
//! - Workspace metadata sync (replaces rustSyncBridge.ts)
//! - Per-file body sync (replaces bodySyncBridge.ts)
//! - Sync completion tracking (replaces TS debounce logic)
//! - Echo detection (replaces lastKnownBodyContent Map)
//!
//! # Usage
//!
//! ```ignore
//! let manager = RustSyncManager::new(workspace_crdt, body_manager, sync_handler);
//!
//! // Handle incoming workspace message
//! let (response, synced) = manager.handle_workspace_message(&msg, true).await?;
//!
//! // Handle incoming body message
//! let (response, content_changed) = manager.handle_body_message("path.md", &msg, true).await?;
//! ```

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use super::body_doc_manager::BodyDocManager;
use super::sync::{BodySyncProtocol, SyncMessage};
use super::sync_handler::SyncHandler;
use super::types::{FileMetadata, UpdateOrigin};
use super::workspace_doc::WorkspaceCrdt;
use crate::error::{DiaryxError, Result};
use crate::fs::{AsyncFileSystem, FileSystemEvent};

/// Result of handling a sync message.
#[derive(Debug)]
pub struct SyncMessageResult {
    /// Optional response bytes to send back to the server.
    pub response: Option<Vec<u8>>,
    /// List of file paths that were changed by this message.
    pub changed_files: Vec<String>,
    /// Whether sync is now complete (for initial sync tracking).
    pub sync_complete: bool,
}

/// Result of handling a body sync message.
#[derive(Debug)]
pub struct BodySyncResult {
    /// Optional response bytes to send back to the server.
    pub response: Option<Vec<u8>>,
    /// New content if it changed, None if unchanged.
    pub content: Option<String>,
    /// Whether this is an echo of our own update.
    pub is_echo: bool,
}

/// Unified sync manager for workspace and body synchronization.
///
/// This struct replaces all TypeScript sync bridges with a single unified
/// Rust implementation. It handles:
/// - Workspace metadata sync via Y-sync protocol
/// - Per-file body sync via Y-sync protocol
/// - Sync completion tracking
/// - Echo detection to avoid processing our own updates
/// - File locking to prevent concurrent modifications
pub struct RustSyncManager<FS: AsyncFileSystem> {
    // Core CRDT components
    workspace_crdt: Arc<WorkspaceCrdt>,
    body_manager: Arc<BodyDocManager>,
    sync_handler: Arc<SyncHandler<FS>>,

    // Workspace sync state
    workspace_synced: AtomicBool,
    workspace_message_count: Mutex<u32>,

    // Per-file body sync protocols
    body_protocols: RwLock<HashMap<String, BodySyncProtocol>>,
    body_synced: RwLock<HashSet<String>>,

    // Echo detection - tracks last known content to detect our own updates
    last_known_content: RwLock<HashMap<String, String>>,

    // Metadata echo detection - tracks last known metadata to detect our own updates
    last_known_metadata: RwLock<HashMap<String, FileMetadata>>,

    // Initial sync tracking
    initial_sync_complete: AtomicBool,

    // Callback to emit filesystem events (for SendSyncMessage)
    event_callback: RwLock<Option<Arc<dyn Fn(&FileSystemEvent) + Send + Sync>>>,
}

impl<FS: AsyncFileSystem> RustSyncManager<FS> {
    /// Create a new sync manager.
    pub fn new(
        workspace_crdt: Arc<WorkspaceCrdt>,
        body_manager: Arc<BodyDocManager>,
        sync_handler: Arc<SyncHandler<FS>>,
    ) -> Self {
        Self {
            workspace_crdt,
            body_manager,
            sync_handler,
            workspace_synced: AtomicBool::new(false),
            workspace_message_count: Mutex::new(0),
            body_protocols: RwLock::new(HashMap::new()),
            body_synced: RwLock::new(HashSet::new()),
            last_known_content: RwLock::new(HashMap::new()),
            last_known_metadata: RwLock::new(HashMap::new()),
            initial_sync_complete: AtomicBool::new(false),
            event_callback: RwLock::new(None),
        }
    }

    /// Set the event callback for emitting filesystem events.
    ///
    /// This callback is used to emit SendSyncMessage events to TypeScript,
    /// which then sends the bytes over WebSocket to the sync server.
    pub fn set_event_callback(&self, callback: Arc<dyn Fn(&FileSystemEvent) + Send + Sync>) {
        let mut cb = self.event_callback.write().unwrap();
        *cb = Some(callback);
    }

    /// Emit a filesystem event via the callback.
    fn emit_event(&self, event: FileSystemEvent) {
        if let Some(ref cb) = *self.event_callback.read().unwrap() {
            cb(&event);
        }
    }

    /// Create and emit a workspace sync message.
    ///
    /// Call this after updating the workspace CRDT to send the changes
    /// to the sync server via TypeScript WebSocket.
    pub fn emit_workspace_update(&self) -> Result<()> {
        let update = self.create_workspace_update(None)?;
        if !update.is_empty() {
            log::debug!(
                "[SyncManager] emit_workspace_update: sending {} bytes",
                update.len()
            );
            self.emit_event(FileSystemEvent::send_sync_message(
                "workspace",
                update,
                false,
            ));
        } else {
            log::debug!("[SyncManager] emit_workspace_update: update is empty, nothing to send");
        }
        Ok(())
    }

    /// Create and emit a body sync message.
    ///
    /// Call this after updating a body CRDT to send the changes
    /// to the sync server via TypeScript WebSocket.
    ///
    /// IMPORTANT: This assumes the body CRDT has already been updated via set_body().
    /// It only encodes the current state - it does NOT call set_body() again.
    ///
    /// The `doc_name` is the canonical file path (e.g., "workspace/notes.md").
    /// The `content` is used only for echo detection tracking.
    pub fn emit_body_update(&self, doc_name: &str, content: &str) -> Result<()> {
        // Track for echo detection (don't update CRDT - it's already updated)
        {
            let mut last_known = self.last_known_content.write().unwrap();
            last_known.insert(doc_name.to_string(), content.to_string());
        }

        // Get the body doc (should already exist and have content set)
        let body_doc = self.body_manager.get_or_create(doc_name);

        // Encode current state as update (without calling set_body again!)
        let update = body_doc.encode_state_as_update();
        if update.is_empty() {
            return Ok(());
        }

        let message = SyncMessage::Update(update).encode();
        self.emit_event(FileSystemEvent::send_sync_message(doc_name, message, true));
        Ok(())
    }

    // =========================================================================
    // Workspace Sync
    // =========================================================================

    /// Handle an incoming WebSocket message for workspace sync.
    ///
    /// Returns a `SyncMessageResult` containing:
    /// - Optional response bytes to send back
    /// - List of changed file paths
    /// - Whether sync is now complete
    ///
    /// If `write_to_disk` is true, changed files will be written to disk
    /// via the SyncHandler.
    pub async fn handle_workspace_message(
        &self,
        message: &[u8],
        write_to_disk: bool,
    ) -> Result<SyncMessageResult> {
        log::debug!(
            "[SyncManager] handle_workspace_message: {} bytes, write_to_disk: {}",
            message.len(),
            write_to_disk
        );

        // Decode all messages in the buffer
        let messages = SyncMessage::decode_all(message)?;
        if messages.is_empty() {
            log::debug!("[SyncManager] No messages decoded");
            return Ok(SyncMessageResult {
                response: None,
                changed_files: Vec::new(),
                sync_complete: false,
            });
        }

        let mut response: Option<Vec<u8>> = None;
        let mut all_changed_files = Vec::new();

        for sync_msg in messages {
            let (msg_response, changed_files) =
                self.handle_single_workspace_message(sync_msg).await?;

            all_changed_files.extend(changed_files);

            // Combine responses
            if let Some(resp) = msg_response {
                if let Some(ref mut existing) = response {
                    existing.extend_from_slice(&resp);
                } else {
                    response = Some(resp);
                }
            }
        }

        // Write changed files to disk if requested
        if write_to_disk && !all_changed_files.is_empty() {
            let files_to_sync: Vec<_> = all_changed_files
                .iter()
                .filter_map(|path| {
                    self.workspace_crdt.get_file(path).and_then(|meta| {
                        // Filter out metadata echoes to prevent feedback loops
                        if self.is_metadata_echo(path, &meta) {
                            log::debug!("[SyncManager] Skipping metadata echo for: {}", path);
                            None
                        } else {
                            Some((path.clone(), meta))
                        }
                    })
                })
                .collect();

            if !files_to_sync.is_empty() {
                let body_mgr_ref = Some(self.body_manager.as_ref());
                self.sync_handler
                    .handle_remote_metadata_update(files_to_sync, body_mgr_ref, true)
                    .await?;
            }
        }

        // Track message count for sync completion detection
        let mut count = self.workspace_message_count.lock().unwrap();
        *count += 1;

        // Consider synced after receiving at least one message
        // (The TypeScript version used a 300ms debounce, but we can track this more precisely)
        let sync_complete = !self.workspace_synced.swap(true, Ordering::SeqCst);
        if sync_complete {
            log::info!("[SyncManager] Workspace sync complete");
            self.initial_sync_complete.store(true, Ordering::SeqCst);
        }

        Ok(SyncMessageResult {
            response,
            changed_files: all_changed_files,
            sync_complete,
        })
    }

    /// Handle a single workspace sync message.
    async fn handle_single_workspace_message(
        &self,
        msg: SyncMessage,
    ) -> Result<(Option<Vec<u8>>, Vec<String>)> {
        match msg {
            SyncMessage::SyncStep1(remote_sv) => {
                log::debug!(
                    "[SyncManager] Workspace: Received SyncStep1, {} bytes",
                    remote_sv.len()
                );

                // Create SyncStep2 with our updates
                let diff = self.workspace_crdt.encode_diff(&remote_sv)?;
                let step2 = SyncMessage::SyncStep2(diff).encode();

                // Also send our state vector
                let our_sv = self.workspace_crdt.encode_state_vector();
                let step1 = SyncMessage::SyncStep1(our_sv).encode();

                let mut combined = step2;
                combined.extend_from_slice(&step1);

                Ok((Some(combined), Vec::new()))
            }

            SyncMessage::SyncStep2(update) => {
                log::debug!(
                    "[SyncManager] Workspace: Received SyncStep2, {} bytes",
                    update.len()
                );

                let mut changed_files = Vec::new();
                if !update.is_empty() {
                    let (_, files) = self
                        .workspace_crdt
                        .apply_update_tracking_changes(&update, UpdateOrigin::Sync)?;
                    changed_files = files;
                }

                Ok((None, changed_files))
            }

            SyncMessage::Update(update) => {
                log::debug!(
                    "[SyncManager] Workspace: Received Update, {} bytes",
                    update.len()
                );

                let mut changed_files = Vec::new();
                if !update.is_empty() {
                    let (_, files) = self
                        .workspace_crdt
                        .apply_update_tracking_changes(&update, UpdateOrigin::Remote)?;
                    changed_files = files;
                }

                Ok((None, changed_files))
            }
        }
    }

    /// Create a SyncStep1 message for workspace sync.
    pub fn create_workspace_sync_step1(&self) -> Vec<u8> {
        let sv = self.workspace_crdt.encode_state_vector();
        SyncMessage::SyncStep1(sv).encode()
    }

    /// Create an update message for local workspace changes.
    ///
    /// If `since_state_vector` is provided, returns only updates since that state.
    /// Otherwise returns the full state.
    pub fn create_workspace_update(&self, since_state_vector: Option<&[u8]>) -> Result<Vec<u8>> {
        let update = match since_state_vector {
            Some(sv) => self.workspace_crdt.encode_diff(sv)?,
            None => self.workspace_crdt.encode_state_as_update(),
        };

        if update.is_empty() {
            return Ok(Vec::new());
        }

        Ok(SyncMessage::Update(update).encode())
    }

    /// Check if workspace sync is complete.
    pub fn is_workspace_synced(&self) -> bool {
        self.workspace_synced.load(Ordering::SeqCst)
    }

    // =========================================================================
    // Body Sync
    // =========================================================================

    /// Initialize body sync for a document.
    ///
    /// Creates or retrieves the sync protocol for the given document name.
    pub fn init_body_sync(&self, doc_name: &str) {
        let mut protocols = self.body_protocols.write().unwrap();
        if !protocols.contains_key(doc_name) {
            // Try to load existing state from storage
            let body_doc = self.body_manager.get_or_create(doc_name);
            let state = body_doc.encode_state_as_update();

            let protocol = if state.is_empty() {
                BodySyncProtocol::new(doc_name.to_string())
            } else {
                BodySyncProtocol::from_state(doc_name.to_string(), &state)
                    .unwrap_or_else(|_| BodySyncProtocol::new(doc_name.to_string()))
            };

            protocols.insert(doc_name.to_string(), protocol);
            log::debug!("[SyncManager] Initialized body sync for: {}", doc_name);
        }
    }

    /// Close body sync for a document.
    pub fn close_body_sync(&self, doc_name: &str) {
        let mut protocols = self.body_protocols.write().unwrap();
        protocols.remove(doc_name);

        let mut synced = self.body_synced.write().unwrap();
        synced.remove(doc_name);

        log::debug!("[SyncManager] Closed body sync for: {}", doc_name);
    }

    /// Handle an incoming WebSocket message for body sync.
    ///
    /// Returns a `BodySyncResult` containing:
    /// - Optional response bytes to send back
    /// - New content if it changed
    /// - Whether this is an echo of our own update
    pub async fn handle_body_message(
        &self,
        doc_name: &str,
        message: &[u8],
        write_to_disk: bool,
    ) -> Result<BodySyncResult> {
        log::debug!(
            "[SyncManager] handle_body_message: {} for {}, write_to_disk: {}",
            message.len(),
            doc_name,
            write_to_disk
        );

        // Ensure protocol exists
        self.init_body_sync(doc_name);

        // Get the body doc to apply updates
        let body_doc = self.body_manager.get_or_create(doc_name);
        let content_before = body_doc.get_body();

        // Handle the message via the protocol
        let response = {
            let mut protocols = self.body_protocols.write().unwrap();
            let protocol = protocols
                .get_mut(doc_name)
                .ok_or_else(|| DiaryxError::Crdt("Body protocol not found".to_string()))?;

            protocol.handle_message(message)?
        };

        // Apply updates to the body doc
        let messages = SyncMessage::decode_all(message)?;
        for sync_msg in messages {
            match sync_msg {
                SyncMessage::SyncStep2(update) | SyncMessage::Update(update) => {
                    if !update.is_empty() {
                        body_doc.apply_update(&update, UpdateOrigin::Remote)?;
                    }
                }
                SyncMessage::SyncStep1(_) => {
                    // SyncStep1 is handled by the protocol, no update to apply
                }
            }
        }

        let content_after = body_doc.get_body();

        // Check if content changed
        let content_changed = content_before != content_after;

        // Check if this is an echo of our own update
        let is_echo = if content_changed {
            let last_known = self.last_known_content.read().unwrap();
            last_known.get(doc_name) == Some(&content_after)
        } else {
            false
        };

        // Write to disk if content changed and not an echo
        if write_to_disk && content_changed && !is_echo {
            // Get metadata from workspace CRDT if available
            let metadata = self.workspace_crdt.get_file(doc_name);
            self.sync_handler
                .handle_remote_body_update(doc_name, &content_after, metadata.as_ref())
                .await?;
        }

        // Mark as synced
        {
            let mut synced = self.body_synced.write().unwrap();
            synced.insert(doc_name.to_string());
        }

        Ok(BodySyncResult {
            response,
            content: if content_changed && !is_echo {
                Some(content_after)
            } else {
                None
            },
            is_echo,
        })
    }

    /// Create a SyncStep1 message for body sync.
    pub fn create_body_sync_step1(&self, doc_name: &str) -> Vec<u8> {
        self.init_body_sync(doc_name);

        let protocols = self.body_protocols.read().unwrap();
        if let Some(protocol) = protocols.get(doc_name) {
            protocol.create_sync_step1()
        } else {
            // Fallback: create from body doc directly
            let body_doc = self.body_manager.get_or_create(doc_name);
            let sv = body_doc.encode_state_vector();
            SyncMessage::SyncStep1(sv).encode()
        }
    }

    /// Create an update message for local body changes.
    pub fn create_body_update(&self, doc_name: &str, content: &str) -> Result<Vec<u8>> {
        // Update content in body doc
        let body_doc = self.body_manager.get_or_create(doc_name);
        body_doc.set_body(content)?;

        // Track for echo detection
        {
            let mut last_known = self.last_known_content.write().unwrap();
            last_known.insert(doc_name.to_string(), content.to_string());
        }

        // Get full state as update
        let update = body_doc.encode_state_as_update();
        if update.is_empty() {
            return Ok(Vec::new());
        }

        Ok(SyncMessage::Update(update).encode())
    }

    /// Check if body sync is complete for a document.
    pub fn is_body_synced(&self, doc_name: &str) -> bool {
        let synced = self.body_synced.read().unwrap();
        synced.contains(doc_name)
    }

    // =========================================================================
    // Echo Detection
    // =========================================================================

    /// Check if content change is an echo of our own edit.
    pub fn is_echo(&self, path: &str, content: &str) -> bool {
        let last_known = self.last_known_content.read().unwrap();
        last_known.get(path) == Some(&content.to_string())
    }

    /// Track content for echo detection.
    pub fn track_content(&self, path: &str, content: &str) {
        let mut last_known = self.last_known_content.write().unwrap();
        last_known.insert(path.to_string(), content.to_string());
    }

    /// Clear tracked content (e.g., when closing a file).
    pub fn clear_tracked_content(&self, path: &str) {
        let mut last_known = self.last_known_content.write().unwrap();
        last_known.remove(path);
    }

    /// Check if metadata change is an echo of our own edit (ignoring modified_at).
    pub fn is_metadata_echo(&self, path: &str, metadata: &FileMetadata) -> bool {
        let last_known = self.last_known_metadata.read().unwrap();
        if let Some(known) = last_known.get(path) {
            // Compare all fields except modified_at
            known.title == metadata.title
                && known.part_of == metadata.part_of
                && known.contents == metadata.contents
                && known.attachments == metadata.attachments
                && known.deleted == metadata.deleted
                && known.audience == metadata.audience
                && known.description == metadata.description
                && known.extra == metadata.extra
        } else {
            false
        }
    }

    /// Track metadata for echo detection.
    pub fn track_metadata(&self, path: &str, metadata: &FileMetadata) {
        let mut last_known = self.last_known_metadata.write().unwrap();
        last_known.insert(path.to_string(), metadata.clone());
    }

    /// Clear tracked metadata (e.g., when closing a file).
    pub fn clear_tracked_metadata(&self, path: &str) {
        let mut last_known = self.last_known_metadata.write().unwrap();
        last_known.remove(path);
    }

    // =========================================================================
    // Sync State
    // =========================================================================

    /// Mark initial sync as complete.
    pub fn mark_sync_complete(&self) {
        self.initial_sync_complete.store(true, Ordering::SeqCst);
        self.workspace_synced.store(true, Ordering::SeqCst);
        log::info!("[SyncManager] Initial sync marked complete");
    }

    /// Check if initial sync is complete.
    pub fn is_sync_complete(&self) -> bool {
        self.initial_sync_complete.load(Ordering::SeqCst)
    }

    /// Get list of active body syncs.
    pub fn get_active_syncs(&self) -> Vec<String> {
        let protocols = self.body_protocols.read().unwrap();
        protocols.keys().cloned().collect()
    }

    // =========================================================================
    // Path Handling (delegates to SyncHandler)
    // =========================================================================

    /// Get the storage path for a canonical path.
    pub fn get_storage_path(&self, canonical_path: &str) -> PathBuf {
        self.sync_handler.get_storage_path(canonical_path)
    }

    /// Get the canonical path from a storage path.
    pub fn get_canonical_path(&self, storage_path: &str) -> String {
        self.sync_handler.get_canonical_path(storage_path)
    }

    /// Check if we're in guest mode.
    pub fn is_guest(&self) -> bool {
        self.sync_handler.is_guest()
    }

    // =========================================================================
    // Cleanup
    // =========================================================================

    /// Reset all sync state.
    pub fn reset(&self) {
        self.workspace_synced.store(false, Ordering::SeqCst);
        self.initial_sync_complete.store(false, Ordering::SeqCst);

        {
            let mut count = self.workspace_message_count.lock().unwrap();
            *count = 0;
        }

        {
            let mut protocols = self.body_protocols.write().unwrap();
            protocols.clear();
        }

        {
            let mut synced = self.body_synced.write().unwrap();
            synced.clear();
        }

        {
            let mut last_known = self.last_known_content.write().unwrap();
            last_known.clear();
        }

        {
            let mut last_known = self.last_known_metadata.write().unwrap();
            last_known.clear();
        }

        log::info!("[SyncManager] Reset complete");
    }
}

impl<FS: AsyncFileSystem> std::fmt::Debug for RustSyncManager<FS> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RustSyncManager")
            .field("workspace_synced", &self.workspace_synced)
            .field("initial_sync_complete", &self.initial_sync_complete)
            .field("active_body_syncs", &self.get_active_syncs().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crdt::MemoryStorage;
    use crate::crdt::storage::CrdtStorage;
    use crate::fs::SyncToAsyncFs;
    use crate::test_utils::MockFileSystem;

    fn create_test_manager() -> RustSyncManager<SyncToAsyncFs<MockFileSystem>> {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        let workspace_crdt = Arc::new(WorkspaceCrdt::new(Arc::clone(&storage)));
        let body_manager = Arc::new(BodyDocManager::new(Arc::clone(&storage)));
        let fs = SyncToAsyncFs::new(MockFileSystem::new());
        let sync_handler = Arc::new(SyncHandler::new(fs));

        RustSyncManager::new(workspace_crdt, body_manager, sync_handler)
    }

    #[test]
    fn test_workspace_sync_step1() {
        let manager = create_test_manager();
        let step1 = manager.create_workspace_sync_step1();

        // Should be a valid SyncStep1 message
        assert!(!step1.is_empty());
        assert_eq!(step1[0], 0); // SYNC type
        assert_eq!(step1[1], 0); // STEP1 subtype
    }

    #[test]
    fn test_body_sync_init() {
        let manager = create_test_manager();

        // Initially no body syncs
        assert!(manager.get_active_syncs().is_empty());

        // Init body sync
        manager.init_body_sync("test.md");
        assert_eq!(manager.get_active_syncs(), vec!["test.md"]);

        // Close body sync
        manager.close_body_sync("test.md");
        assert!(manager.get_active_syncs().is_empty());
    }

    #[test]
    fn test_echo_detection() {
        let manager = create_test_manager();

        // Track content
        manager.track_content("test.md", "Hello world");

        // Should detect echo
        assert!(manager.is_echo("test.md", "Hello world"));

        // Should not detect different content as echo
        assert!(!manager.is_echo("test.md", "Different content"));

        // Clear and check
        manager.clear_tracked_content("test.md");
        assert!(!manager.is_echo("test.md", "Hello world"));
    }

    #[test]
    fn test_metadata_echo_detection() {
        use crate::crdt::FileMetadata;

        let manager = create_test_manager();

        // Create metadata
        let mut meta = FileMetadata::new(Some("Test".to_string()));
        meta.part_of = Some("parent/index.md".to_string());

        // Track metadata
        manager.track_metadata("test.md", &meta);

        // Should detect echo with same content (even if modified_at differs)
        let mut meta2 = meta.clone();
        meta2.modified_at = 999999; // Different timestamp
        assert!(manager.is_metadata_echo("test.md", &meta2));

        // Should not detect different content as echo
        let mut meta3 = meta.clone();
        meta3.title = Some("Different".to_string());
        assert!(!manager.is_metadata_echo("test.md", &meta3));

        // Clear and check
        manager.clear_tracked_metadata("test.md");
        assert!(!manager.is_metadata_echo("test.md", &meta));
    }

    #[test]
    fn test_sync_state() {
        let manager = create_test_manager();

        // Initially not synced
        assert!(!manager.is_sync_complete());
        assert!(!manager.is_workspace_synced());

        // Mark complete
        manager.mark_sync_complete();
        assert!(manager.is_sync_complete());
        assert!(manager.is_workspace_synced());

        // Reset
        manager.reset();
        assert!(!manager.is_sync_complete());
        assert!(!manager.is_workspace_synced());
    }
}
