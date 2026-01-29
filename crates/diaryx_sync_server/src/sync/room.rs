use diaryx_core::crdt::{BodyDocManager, SqliteStorage, SyncMessage, UpdateOrigin, WorkspaceCrdt};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::{RwLock, broadcast};
use tracing::{debug, error, info, warn};

/// Control messages for session management
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlMessage {
    PeerJoined {
        guest_id: String,
        peer_count: usize,
    },
    PeerLeft {
        guest_id: String,
        peer_count: usize,
    },
    ReadOnlyChanged {
        read_only: bool,
    },
    SessionEnded,
    SyncProgress {
        completed: usize,
        total: usize,
    },
    /// Initial sync has completed - all data has been exchanged
    SyncComplete {
        files_synced: usize,
    },
}

/// Session context for a share session
#[derive(Debug, Clone)]
pub struct SessionContext {
    pub code: String,
    pub owner_user_id: String,
    pub read_only: bool,
}

/// Statistics about the sync state
#[derive(Debug, Clone, Default)]
pub struct SyncStats {
    pub active_connections: usize,
    pub active_rooms: usize,
}

/// Global sync state managing all rooms
pub struct SyncState {
    /// Map of workspace_id to SyncRoom
    rooms: RwLock<HashMap<String, Arc<SyncRoom>>>,
    /// Map of session_code to workspace_id (for session lookups)
    session_to_workspace: RwLock<HashMap<String, String>>,
    /// Base path for workspace databases
    data_dir: PathBuf,
}

impl SyncState {
    /// Create a new SyncState
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            rooms: RwLock::new(HashMap::new()),
            session_to_workspace: RwLock::new(HashMap::new()),
            data_dir,
        }
    }

    /// Get or create a room for a workspace
    pub async fn get_or_create_room(&self, workspace_id: &str) -> Arc<SyncRoom> {
        // Check if room exists
        {
            let rooms = self.rooms.read().await;
            if let Some(room) = rooms.get(workspace_id) {
                return room.clone();
            }
        }

        // Create new room
        let mut rooms = self.rooms.write().await;

        // Double-check after acquiring write lock
        if let Some(room) = rooms.get(workspace_id) {
            return room.clone();
        }

        // Create database path
        let db_path = self.data_dir.join(format!("{}.db", workspace_id));

        let room = match SyncRoom::new(workspace_id, db_path) {
            Ok(r) => Arc::new(r),
            Err(e) => {
                error!("Failed to create sync room for {}: {}", workspace_id, e);
                // Return a fallback in-memory room
                Arc::new(SyncRoom::in_memory(workspace_id))
            }
        };

        rooms.insert(workspace_id.to_string(), room.clone());
        info!("Created sync room for workspace: {}", workspace_id);

        room
    }

    /// Remove a room if it has no active connections
    pub async fn maybe_remove_room(&self, workspace_id: &str) {
        let mut rooms = self.rooms.write().await;

        if let Some(room) = rooms.get(workspace_id) {
            if room.connection_count() == 0 {
                // Save the room state before removing
                if let Err(e) = room.save() {
                    error!("Failed to save room {} before removal: {}", workspace_id, e);
                }
                rooms.remove(workspace_id);
                info!("Removed idle sync room: {}", workspace_id);
            }
        }
    }

    /// Get statistics about the sync state
    pub fn get_stats(&self) -> SyncStats {
        // Note: Using blocking read here for simplicity in sync context
        // In a real async context, you'd want to use try_read or proper async
        let rooms = futures::executor::block_on(self.rooms.read());
        let active_connections: usize = rooms.values().map(|r| r.connection_count()).sum();

        SyncStats {
            active_connections,
            active_rooms: rooms.len(),
        }
    }

    /// Get or create a room for a session, with session context
    pub async fn get_or_create_session_room(
        &self,
        workspace_id: &str,
        session_context: SessionContext,
    ) -> Arc<SyncRoom> {
        // Track session -> workspace mapping
        {
            let mut mapping = self.session_to_workspace.write().await;
            mapping.insert(session_context.code.clone(), workspace_id.to_string());
        }

        // Get or create the room
        let room = self.get_or_create_room(workspace_id).await;

        // Set session context on the room
        room.set_session_context(session_context).await;

        room
    }

    /// Get peer count for a session
    pub async fn get_session_peer_count(&self, session_code: &str) -> Option<usize> {
        let mapping = self.session_to_workspace.read().await;
        let workspace_id = mapping.get(session_code)?;

        let rooms = self.rooms.read().await;
        let room = rooms.get(workspace_id)?;

        Some(room.connection_count())
    }

    /// End a session (notify all connected clients)
    pub async fn end_session(&self, session_code: &str) {
        // Get workspace ID for this session
        let workspace_id = {
            let mapping = self.session_to_workspace.read().await;
            mapping.get(session_code).cloned()
        };

        if let Some(workspace_id) = workspace_id {
            // Get the room and send session ended message
            let rooms = self.rooms.read().await;
            if let Some(room) = rooms.get(&workspace_id) {
                room.broadcast_control_message(ControlMessage::SessionEnded)
                    .await;
                room.clear_session_context().await;
            }

            // Remove session mapping
            let mut mapping = self.session_to_workspace.write().await;
            mapping.remove(session_code);

            info!("Ended session: {}", session_code);
        }
    }

    /// Get room for a session code (for guests joining)
    pub async fn get_room_for_session(&self, session_code: &str) -> Option<Arc<SyncRoom>> {
        let mapping = self.session_to_workspace.read().await;
        let workspace_id = mapping.get(session_code)?;

        let rooms = self.rooms.read().await;
        rooms.get(workspace_id).cloned()
    }

    /// Get an existing room by workspace ID (does not create if not found)
    pub async fn get_room(&self, workspace_id: &str) -> Option<Arc<SyncRoom>> {
        let rooms = self.rooms.read().await;
        rooms.get(workspace_id).cloned()
    }
}

/// A sync room for a single workspace
pub struct SyncRoom {
    #[allow(dead_code)]
    workspace_id: String,
    /// The CRDT workspace document (metadata only)
    workspace: RwLock<WorkspaceCrdt>,
    /// Manager for per-file body documents
    body_docs: RwLock<BodyDocManager>,
    /// Broadcast channel for workspace updates (binary Y-sync messages)
    broadcast_tx: broadcast::Sender<Vec<u8>>,
    /// Broadcast channel for body updates (file_path, update)
    body_broadcast_tx: broadcast::Sender<(String, Vec<u8>)>,
    /// Broadcast channel for control messages (JSON)
    control_tx: broadcast::Sender<ControlMessage>,
    /// Number of active connections
    connection_count: AtomicUsize,
    /// Storage backend
    storage: Arc<SqliteStorage>,
    /// Session context (if this room is hosting a share session)
    session_context: RwLock<Option<SessionContext>>,
    /// Guest connections (guest_id -> connection tracking)
    guest_connections: RwLock<HashMap<String, ()>>,
    /// Whether the session is read-only
    is_read_only: AtomicBool,
    /// Last response sent per connection, used to detect and break ping-pong loops
    /// Key is a hash of the incoming message, value is the response sent
    last_responses: RwLock<HashMap<u64, Vec<u8>>>,
    /// Clients subscribed to specific body docs (file_path -> connection_ids)
    body_subscriptions: RwLock<HashMap<String, HashSet<String>>>,
    /// Files synced counter for progress tracking (reset on new sync session)
    files_synced: AtomicUsize,
}

impl SyncRoom {
    /// Create a new SyncRoom with SQLite storage
    pub fn new(
        workspace_id: &str,
        db_path: PathBuf,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let storage = Arc::new(SqliteStorage::open(&db_path)?);
        let workspace = WorkspaceCrdt::load_with_name(storage.clone(), workspace_id.to_string())?;
        let body_docs = BodyDocManager::new(storage.clone());

        let (broadcast_tx, _) = broadcast::channel(1024);
        let (body_broadcast_tx, _) = broadcast::channel(1024);
        let (control_tx, _) = broadcast::channel(256);

        Ok(Self {
            workspace_id: workspace_id.to_string(),
            workspace: RwLock::new(workspace),
            body_docs: RwLock::new(body_docs),
            broadcast_tx,
            body_broadcast_tx,
            control_tx,
            connection_count: AtomicUsize::new(0),
            storage,
            session_context: RwLock::new(None),
            guest_connections: RwLock::new(HashMap::new()),
            is_read_only: AtomicBool::new(false),
            last_responses: RwLock::new(HashMap::new()),
            body_subscriptions: RwLock::new(HashMap::new()),
            files_synced: AtomicUsize::new(0),
        })
    }

    /// Create an in-memory SyncRoom (for fallback/testing)
    pub fn in_memory(workspace_id: &str) -> Self {
        let storage =
            Arc::new(SqliteStorage::in_memory().expect("Failed to create in-memory storage"));
        let workspace = WorkspaceCrdt::with_name(storage.clone(), workspace_id.to_string());
        let body_docs = BodyDocManager::new(storage.clone());

        let (broadcast_tx, _) = broadcast::channel(1024);
        let (body_broadcast_tx, _) = broadcast::channel(1024);
        let (control_tx, _) = broadcast::channel(256);

        Self {
            workspace_id: workspace_id.to_string(),
            workspace: RwLock::new(workspace),
            body_docs: RwLock::new(body_docs),
            broadcast_tx,
            body_broadcast_tx,
            control_tx,
            connection_count: AtomicUsize::new(0),
            storage,
            session_context: RwLock::new(None),
            guest_connections: RwLock::new(HashMap::new()),
            is_read_only: AtomicBool::new(false),
            last_responses: RwLock::new(HashMap::new()),
            body_subscriptions: RwLock::new(HashMap::new()),
            files_synced: AtomicUsize::new(0),
        }
    }

    /// Subscribe to room updates
    pub fn subscribe(&self) -> broadcast::Receiver<Vec<u8>> {
        self.connection_count.fetch_add(1, Ordering::SeqCst);
        self.broadcast_tx.subscribe()
    }

    /// Subscribe to control messages
    pub fn subscribe_control(&self) -> broadcast::Receiver<ControlMessage> {
        self.control_tx.subscribe()
    }

    /// Unsubscribe from room updates
    pub fn unsubscribe(&self) {
        self.connection_count.fetch_sub(1, Ordering::SeqCst);
    }

    /// Get the number of active connections
    pub fn connection_count(&self) -> usize {
        self.connection_count.load(Ordering::SeqCst)
    }

    /// Check if room is in read-only mode
    pub fn is_read_only(&self) -> bool {
        self.is_read_only.load(Ordering::SeqCst)
    }

    /// Set session context for this room
    pub async fn set_session_context(&self, context: SessionContext) {
        self.is_read_only.store(context.read_only, Ordering::SeqCst);
        let mut session = self.session_context.write().await;
        *session = Some(context);
    }

    /// Clear session context
    pub async fn clear_session_context(&self) {
        let mut session = self.session_context.write().await;
        *session = None;
        self.is_read_only.store(false, Ordering::SeqCst);

        // Clear guest connections
        let mut guests = self.guest_connections.write().await;
        guests.clear();
    }

    /// Get session context
    pub async fn get_session_context(&self) -> Option<SessionContext> {
        let session = self.session_context.read().await;
        session.clone()
    }

    /// Add a guest connection
    pub async fn add_guest(&self, guest_id: &str) {
        let mut guests = self.guest_connections.write().await;
        guests.insert(guest_id.to_string(), ());

        let peer_count = self.connection_count();
        self.broadcast_control_message(ControlMessage::PeerJoined {
            guest_id: guest_id.to_string(),
            peer_count,
        })
        .await;
    }

    /// Remove a guest connection
    pub async fn remove_guest(&self, guest_id: &str) {
        let mut guests = self.guest_connections.write().await;
        guests.remove(guest_id);

        let peer_count = self.connection_count();
        self.broadcast_control_message(ControlMessage::PeerLeft {
            guest_id: guest_id.to_string(),
            peer_count,
        })
        .await;
    }

    /// Set read-only mode and broadcast to all clients
    pub async fn set_read_only(&self, read_only: bool) {
        self.is_read_only.store(read_only, Ordering::SeqCst);

        // Update session context if present
        if let Some(mut context) = self.get_session_context().await {
            context.read_only = read_only;
            self.set_session_context(context).await;
        }

        self.broadcast_control_message(ControlMessage::ReadOnlyChanged { read_only })
            .await;
    }

    /// Broadcast a control message to all connected clients
    pub async fn broadcast_control_message(&self, msg: ControlMessage) {
        let _ = self.control_tx.send(msg);
    }

    /// Handle an incoming Y-sync message and return response if any
    pub async fn handle_message(&self, msg: &[u8]) -> Option<Vec<u8>> {
        // Decode the message
        let sync_messages = match SyncMessage::decode_all(msg) {
            Ok(msgs) => msgs,
            Err(e) => {
                warn!("Failed to decode sync message: {}", e);
                return None;
            }
        };

        let mut responses = Vec::new();

        for sync_msg in sync_messages {
            match sync_msg {
                SyncMessage::SyncStep1(state_vector) => {
                    // Client is initiating sync - reset progress counter
                    self.files_synced.store(0, Ordering::SeqCst);

                    // Client is initiating sync, respond with our diff
                    // Handle empty/invalid state vectors by sending full state
                    let workspace = self.workspace.read().await;
                    let diff_result = if state_vector.is_empty() {
                        // Empty state vector - client has no state, send full state
                        debug!("Received empty state vector for workspace, sending full state");
                        Ok(workspace.encode_state_as_update())
                    } else {
                        workspace.encode_diff(&state_vector)
                    };

                    match diff_result {
                        Ok(diff) => {
                            let response = SyncMessage::SyncStep2(diff).encode();
                            responses.extend(response);

                            // Also send our state vector so client can send us their diff
                            let our_sv = workspace.encode_state_vector();
                            let sv_msg = SyncMessage::SyncStep1(our_sv).encode();
                            responses.extend(sv_msg);
                        }
                        Err(e) => {
                            // Fallback: try sending full state on any decode error
                            warn!(
                                "Failed to encode workspace diff: {}, falling back to full state",
                                e
                            );
                            let full_state = workspace.encode_state_as_update();
                            let response = SyncMessage::SyncStep2(full_state).encode();
                            responses.extend(response);

                            let our_sv = workspace.encode_state_vector();
                            let sv_msg = SyncMessage::SyncStep1(our_sv).encode();
                            responses.extend(sv_msg);
                        }
                    }
                }
                SyncMessage::SyncStep2(diff) => {
                    // Client sent us their diff, apply it and track changed files
                    let workspace = self.workspace.write().await;
                    match workspace.apply_update_tracking_changes(&diff, UpdateOrigin::Remote) {
                        Ok((_, changed_files, _)) => {
                            // Update progress counter and broadcast
                            if !changed_files.is_empty() {
                                let newly_synced = changed_files.len();
                                let total_synced =
                                    self.files_synced.fetch_add(newly_synced, Ordering::SeqCst)
                                        + newly_synced;
                                let total_files = workspace.file_count();

                                debug!(
                                    "Sync progress: {}/{} files (SyncStep2, {} new)",
                                    total_synced, total_files, newly_synced
                                );

                                self.broadcast_control_message(ControlMessage::SyncProgress {
                                    completed: total_synced,
                                    total: total_files,
                                })
                                .await;
                            }
                        }
                        Err(e) => {
                            warn!("Failed to apply sync step 2: {}", e);
                        }
                    }
                }
                SyncMessage::Update(update) => {
                    // Apply the update and track changed files
                    let changed_count;
                    let total_files;
                    {
                        let workspace = self.workspace.write().await;
                        match workspace.apply_update_tracking_changes(&update, UpdateOrigin::Remote)
                        {
                            Ok((_, changed_files, _)) => {
                                changed_count = changed_files.len();
                                total_files = workspace.file_count();
                            }
                            Err(e) => {
                                warn!("Failed to apply update: {}", e);
                                continue;
                            }
                        }
                    }

                    // Update progress counter and broadcast if files changed
                    if changed_count > 0 {
                        let total_synced =
                            self.files_synced.fetch_add(changed_count, Ordering::SeqCst)
                                + changed_count;

                        debug!(
                            "Sync progress: {}/{} files (Update, {} new)",
                            total_synced, total_files, changed_count
                        );

                        self.broadcast_control_message(ControlMessage::SyncProgress {
                            completed: total_synced,
                            total: total_files,
                        })
                        .await;
                    }

                    // Broadcast to other clients
                    let broadcast_msg = SyncMessage::Update(update).encode();
                    let _ = self.broadcast_tx.send(broadcast_msg);
                }
            }
        }

        if responses.is_empty() {
            None
        } else {
            // Detect ping-pong loops: hash the incoming message and check if we'd send the same response
            let msg_hash = {
                let mut hasher = DefaultHasher::new();
                msg.hash(&mut hasher);
                hasher.finish()
            };

            let mut last_responses = self.last_responses.write().await;

            if let Some(last_response) = last_responses.get(&msg_hash) {
                if last_response == &responses {
                    debug!("Skipping duplicate response to break sync loop");
                    return None;
                }
            }

            // Store this response for loop detection
            last_responses.insert(msg_hash, responses.clone());

            // Limit the cache size to prevent memory leaks
            if last_responses.len() > 100 {
                // Clear old entries (simple approach - just clear all)
                last_responses.clear();
                last_responses.insert(msg_hash, responses.clone());
            }

            Some(responses)
        }
    }

    /// Get the full state for a new client
    pub async fn get_full_state(&self) -> Vec<u8> {
        let workspace = self.workspace.read().await;
        let state = workspace.encode_state_as_update();
        SyncMessage::SyncStep2(state).encode()
    }

    /// Get our state vector for sync initiation
    pub async fn get_state_vector(&self) -> Vec<u8> {
        let workspace = self.workspace.read().await;
        let sv = workspace.encode_state_vector();
        SyncMessage::SyncStep1(sv).encode()
    }

    /// Save the room state to storage
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // The workspace auto-saves on updates, but we can force a save here
        // For SQLite storage, updates are persisted immediately
        Ok(())
    }

    // ==================== Body Document Operations ====================

    /// Subscribe to body document updates for a specific file
    pub async fn subscribe_body(
        &self,
        file_path: &str,
        client_id: &str,
    ) -> broadcast::Receiver<(String, Vec<u8>)> {
        // Track subscription using async lock
        let mut subs = self.body_subscriptions.write().await;
        subs.entry(file_path.to_string())
            .or_default()
            .insert(client_id.to_string());

        self.body_broadcast_tx.subscribe()
    }

    /// Subscribe to ALL body broadcasts (for multiplexed connections).
    ///
    /// Returns receiver that gets ALL body updates. The caller is responsible
    /// for filtering based on which files the client is subscribed to.
    /// This is used by multiplexed body sync to receive updates for all files
    /// over a single WebSocket connection.
    pub fn subscribe_all_bodies(&self) -> broadcast::Receiver<(String, Vec<u8>)> {
        self.body_broadcast_tx.subscribe()
    }

    /// Unsubscribe from body document updates for a specific file
    pub async fn unsubscribe_body(&self, file_path: &str, client_id: &str) {
        let mut subs = self.body_subscriptions.write().await;
        if let Some(clients) = subs.get_mut(file_path) {
            clients.remove(client_id);
            if clients.is_empty() {
                subs.remove(file_path);
            }
        }
    }

    /// Handle an incoming Y-sync message for a body document
    pub async fn handle_body_message(&self, file_path: &str, msg: &[u8]) -> Option<Vec<u8>> {
        // Decode the message
        let sync_messages = match SyncMessage::decode_all(msg) {
            Ok(msgs) => msgs,
            Err(e) => {
                warn!(
                    "Failed to decode body sync message for {}: {}",
                    file_path, e
                );
                return None;
            }
        };

        let mut responses = Vec::new();
        // Use read lock - BodyDocManager::get_or_create handles its own internal locking
        let body_docs = self.body_docs.read().await;
        let doc = body_docs.get_or_create(file_path);

        for sync_msg in sync_messages {
            match sync_msg {
                SyncMessage::SyncStep1(state_vector) => {
                    // Client is initiating sync, respond with our diff
                    // Handle empty/invalid state vectors by sending full state
                    let diff_result = if state_vector.is_empty() {
                        // Empty state vector - client has no state, send full state
                        debug!(
                            "Received empty state vector for {}, sending full state",
                            file_path
                        );
                        Ok(doc.encode_state_as_update())
                    } else {
                        doc.encode_diff(&state_vector)
                    };

                    match diff_result {
                        Ok(diff) => {
                            let response = SyncMessage::SyncStep2(diff).encode();
                            responses.extend(response);

                            // Also send our state vector so client can send us their diff
                            let our_sv = doc.encode_state_vector();
                            let sv_msg = SyncMessage::SyncStep1(our_sv).encode();
                            responses.extend(sv_msg);
                        }
                        Err(e) => {
                            // Fallback: try sending full state on any decode error
                            warn!(
                                "Failed to encode body diff for {}: {}, falling back to full state",
                                file_path, e
                            );
                            let full_state = doc.encode_state_as_update();
                            let response = SyncMessage::SyncStep2(full_state).encode();
                            responses.extend(response);

                            let our_sv = doc.encode_state_vector();
                            let sv_msg = SyncMessage::SyncStep1(our_sv).encode();
                            responses.extend(sv_msg);
                        }
                    }
                }
                SyncMessage::SyncStep2(diff) => {
                    // Client sent us their diff, apply it
                    if let Err(e) = doc.apply_update(&diff, UpdateOrigin::Remote) {
                        warn!("Failed to apply body sync step 2 for {}: {}", file_path, e);
                    }
                }
                SyncMessage::Update(update) => {
                    // Apply the update
                    if let Err(e) = doc.apply_update(&update, UpdateOrigin::Remote) {
                        warn!("Failed to apply body update for {}: {}", file_path, e);
                        continue;
                    }

                    // Broadcast to other clients subscribed to this file
                    let broadcast_msg = SyncMessage::Update(update).encode();
                    let _ = self
                        .body_broadcast_tx
                        .send((file_path.to_string(), broadcast_msg));
                }
            }
        }

        if responses.is_empty() {
            None
        } else {
            Some(responses)
        }
    }

    /// Get the full body state for a new client
    pub async fn get_body_full_state(&self, file_path: &str) -> Vec<u8> {
        // Use read lock - get_or_create handles its own internal locking
        let body_docs = self.body_docs.read().await;
        let doc = body_docs.get_or_create(file_path);
        let state = doc.encode_state_as_update();
        SyncMessage::SyncStep2(state).encode()
    }

    /// Get body state vector for sync initiation
    pub async fn get_body_state_vector(&self, file_path: &str) -> Vec<u8> {
        // Use read lock - get_or_create handles its own internal locking
        let body_docs = self.body_docs.read().await;
        let doc = body_docs.get_or_create(file_path);
        let sv = doc.encode_state_vector();
        SyncMessage::SyncStep1(sv).encode()
    }

    /// Check if a client is subscribed to a specific body document
    pub async fn is_subscribed_to_body(&self, file_path: &str, client_id: &str) -> bool {
        let subs = self.body_subscriptions.read().await;
        subs.get(file_path)
            .map(|clients| clients.contains(client_id))
            .unwrap_or(false)
    }

    /// Get list of files a client is subscribed to
    pub async fn get_client_body_subscriptions(&self, client_id: &str) -> Vec<String> {
        let subs = self.body_subscriptions.read().await;
        subs.iter()
            .filter_map(|(path, clients)| {
                if clients.contains(client_id) {
                    Some(path.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get body document content (for debugging/inspection)
    pub async fn get_body_content(&self, file_path: &str) -> Option<String> {
        let body_docs = self.body_docs.read().await;
        body_docs.get(file_path).map(|doc| doc.get_body())
    }

    /// Get the number of files in the workspace (for user data check)
    pub async fn get_file_count(&self) -> usize {
        let workspace = self.workspace.read().await;
        workspace.file_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diaryx_core::crdt::FileMetadata;

    // =========================================================================
    // SyncRoom Creation Tests
    // =========================================================================

    #[test]
    fn test_sync_room_in_memory_creation() {
        let room = SyncRoom::in_memory("test-workspace");
        assert_eq!(room.connection_count(), 0);
        assert!(!room.is_read_only());
    }

    #[tokio::test]
    async fn test_sync_room_initial_state_empty() {
        let room = SyncRoom::in_memory("test-workspace");
        let file_count = room.get_file_count().await;
        assert_eq!(file_count, 0);
    }

    // =========================================================================
    // Connection Tracking Tests
    // =========================================================================

    #[test]
    fn test_subscribe_increments_connection_count() {
        let room = SyncRoom::in_memory("test-workspace");
        assert_eq!(room.connection_count(), 0);

        let _rx1 = room.subscribe();
        assert_eq!(room.connection_count(), 1);

        let _rx2 = room.subscribe();
        assert_eq!(room.connection_count(), 2);
    }

    #[test]
    fn test_unsubscribe_decrements_connection_count() {
        let room = SyncRoom::in_memory("test-workspace");

        let _rx = room.subscribe();
        assert_eq!(room.connection_count(), 1);

        room.unsubscribe();
        assert_eq!(room.connection_count(), 0);
    }

    // =========================================================================
    // Session Context Tests
    // =========================================================================

    #[tokio::test]
    async fn test_session_context_initially_none() {
        let room = SyncRoom::in_memory("test-workspace");
        let context = room.get_session_context().await;
        assert!(context.is_none());
    }

    #[tokio::test]
    async fn test_set_session_context() {
        let room = SyncRoom::in_memory("test-workspace");

        let context = SessionContext {
            code: "ABC123".to_string(),
            owner_user_id: "user-1".to_string(),
            read_only: false,
        };

        room.set_session_context(context.clone()).await;

        let retrieved = room.get_session_context().await.unwrap();
        assert_eq!(retrieved.code, "ABC123");
        assert_eq!(retrieved.owner_user_id, "user-1");
        assert!(!retrieved.read_only);
    }

    #[tokio::test]
    async fn test_session_context_read_only_flag() {
        let room = SyncRoom::in_memory("test-workspace");
        assert!(!room.is_read_only());

        let context = SessionContext {
            code: "ABC123".to_string(),
            owner_user_id: "user-1".to_string(),
            read_only: true,
        };

        room.set_session_context(context).await;
        assert!(room.is_read_only());
    }

    #[tokio::test]
    async fn test_clear_session_context() {
        let room = SyncRoom::in_memory("test-workspace");

        let context = SessionContext {
            code: "ABC123".to_string(),
            owner_user_id: "user-1".to_string(),
            read_only: true,
        };

        room.set_session_context(context).await;
        assert!(room.is_read_only());

        room.clear_session_context().await;
        assert!(room.get_session_context().await.is_none());
        assert!(!room.is_read_only()); // Should reset to false
    }

    #[tokio::test]
    async fn test_set_read_only() {
        let room = SyncRoom::in_memory("test-workspace");

        // Set up a session first
        let context = SessionContext {
            code: "ABC123".to_string(),
            owner_user_id: "user-1".to_string(),
            read_only: false,
        };
        room.set_session_context(context).await;

        // Change read-only state
        room.set_read_only(true).await;
        assert!(room.is_read_only());

        // Session context should also be updated
        let updated_context = room.get_session_context().await.unwrap();
        assert!(updated_context.read_only);
    }

    // =========================================================================
    // Guest Connection Tests
    // =========================================================================

    #[tokio::test]
    async fn test_add_and_remove_guest() {
        let room = SyncRoom::in_memory("test-workspace");

        // Subscribe to control messages to verify broadcasts
        let mut control_rx = room.subscribe_control();

        room.add_guest("guest-1").await;

        // Should receive PeerJoined message
        let msg = control_rx.try_recv();
        assert!(msg.is_ok());
        match msg.unwrap() {
            ControlMessage::PeerJoined { guest_id, .. } => {
                assert_eq!(guest_id, "guest-1");
            }
            _ => panic!("Expected PeerJoined message"),
        }

        room.remove_guest("guest-1").await;

        // Should receive PeerLeft message
        let msg = control_rx.try_recv();
        assert!(msg.is_ok());
        match msg.unwrap() {
            ControlMessage::PeerLeft { guest_id, .. } => {
                assert_eq!(guest_id, "guest-1");
            }
            _ => panic!("Expected PeerLeft message"),
        }
    }

    // =========================================================================
    // Sync Protocol Tests - handle_message()
    // =========================================================================

    #[tokio::test]
    async fn test_handle_sync_step1_empty_workspace() {
        let room = SyncRoom::in_memory("test-workspace");

        // Create a SyncStep1 message with empty state vector (new client)
        let empty_sv = Vec::new();
        let msg = SyncMessage::SyncStep1(empty_sv).encode();

        let response = room.handle_message(&msg).await;

        // Should get a response (SyncStep2 with full state + SyncStep1 with server's SV)
        assert!(response.is_some());
        let response_data = response.unwrap();

        // Decode the response
        let response_msgs = SyncMessage::decode_all(&response_data).unwrap();

        // Should have at least SyncStep2 and SyncStep1
        assert!(response_msgs.len() >= 2);

        // First message should be SyncStep2
        assert!(matches!(response_msgs[0], SyncMessage::SyncStep2(_)));

        // Second message should be SyncStep1 (server's state vector)
        assert!(matches!(response_msgs[1], SyncMessage::SyncStep1(_)));
    }

    #[tokio::test]
    async fn test_handle_sync_step1_with_data() {
        let room = SyncRoom::in_memory("test-workspace");

        // Add some data to the room first
        {
            let workspace = room.workspace.write().await;
            workspace
                .set_file("test.md", FileMetadata::new(Some("Test".to_string())))
                .unwrap();
        }

        // Client sends SyncStep1 with empty state vector
        let empty_sv = Vec::new();
        let msg = SyncMessage::SyncStep1(empty_sv).encode();

        let response = room.handle_message(&msg).await;
        assert!(response.is_some());

        let response_data = response.unwrap();
        let response_msgs = SyncMessage::decode_all(&response_data).unwrap();

        // SyncStep2 should contain the file data
        match &response_msgs[0] {
            SyncMessage::SyncStep2(diff) => {
                // Non-empty diff (> 2 bytes) because room has data
                assert!(diff.len() > 2, "Diff should contain file data");
            }
            _ => panic!("Expected SyncStep2"),
        }
    }

    #[tokio::test]
    async fn test_handle_sync_step2_applies_update() {
        let room = SyncRoom::in_memory("test-workspace");

        // Create a separate workspace with data
        let client_storage = Arc::new(SqliteStorage::in_memory().unwrap());
        let client_workspace = WorkspaceCrdt::new(client_storage);
        client_workspace
            .set_file(
                "client-file.md",
                FileMetadata::new(Some("Client File".to_string())),
            )
            .unwrap();

        // Get the client's full state as an update
        let client_update = client_workspace.encode_state_as_update();

        // Send as SyncStep2
        let msg = SyncMessage::SyncStep2(client_update).encode();
        let _response = room.handle_message(&msg).await;

        // Room should now have the file
        let file_count = room.get_file_count().await;
        assert_eq!(file_count, 1);
    }

    #[tokio::test]
    async fn test_handle_update_message() {
        let room = SyncRoom::in_memory("test-workspace");

        // Subscribe to broadcasts to verify the update is forwarded
        let mut broadcast_rx = room.subscribe();

        // Create an update from a separate workspace
        let client_storage = Arc::new(SqliteStorage::in_memory().unwrap());
        let client_workspace = WorkspaceCrdt::new(client_storage);
        client_workspace
            .set_file("new-file.md", FileMetadata::new(Some("New".to_string())))
            .unwrap();

        let update = client_workspace.encode_state_as_update();
        let msg = SyncMessage::Update(update).encode();

        let _response = room.handle_message(&msg).await;

        // Room should have the file
        let file_count = room.get_file_count().await;
        assert_eq!(file_count, 1);

        // Update should be broadcast to other subscribers
        let broadcast = broadcast_rx.try_recv();
        assert!(broadcast.is_ok());
    }

    // =========================================================================
    // Loop Detection Tests
    // =========================================================================

    #[tokio::test]
    async fn test_loop_detection_blocks_duplicate_response() {
        let room = SyncRoom::in_memory("test-workspace");

        // Send the same SyncStep1 message twice
        let empty_sv = Vec::new();
        let msg = SyncMessage::SyncStep1(empty_sv).encode();

        // First call should return a response
        let response1 = room.handle_message(&msg).await;
        assert!(response1.is_some());

        // Second identical call should return None (loop detected)
        let response2 = room.handle_message(&msg).await;
        assert!(
            response2.is_none(),
            "Duplicate message should be blocked by loop detection"
        );
    }

    #[tokio::test]
    async fn test_loop_detection_allows_different_messages() {
        let room = SyncRoom::in_memory("test-workspace");

        // First message
        let msg1 = SyncMessage::SyncStep1(Vec::new()).encode();
        let response1 = room.handle_message(&msg1).await;
        assert!(response1.is_some());

        // Different message (with actual state vector)
        let client_storage = Arc::new(SqliteStorage::in_memory().unwrap());
        let client_workspace = WorkspaceCrdt::new(client_storage);
        let sv = client_workspace.encode_state_vector();
        let msg2 = SyncMessage::SyncStep1(sv).encode();

        let response2 = room.handle_message(&msg2).await;
        // Different message should get a response
        assert!(response2.is_some());
    }

    // =========================================================================
    // Body Document Sync Tests
    // =========================================================================

    #[tokio::test]
    async fn test_handle_body_sync_step1() {
        let room = SyncRoom::in_memory("test-workspace");

        // Send SyncStep1 for a body document
        let empty_sv = Vec::new();
        let msg = SyncMessage::SyncStep1(empty_sv).encode();

        let response = room.handle_body_message("test.md", &msg).await;
        assert!(response.is_some());

        let response_data = response.unwrap();
        let response_msgs = SyncMessage::decode_all(&response_data).unwrap();

        // Should have SyncStep2 and SyncStep1
        assert!(response_msgs.len() >= 2);
        assert!(matches!(response_msgs[0], SyncMessage::SyncStep2(_)));
        assert!(matches!(response_msgs[1], SyncMessage::SyncStep1(_)));
    }

    #[tokio::test]
    async fn test_handle_body_sync_step2_applies_content() {
        let room = SyncRoom::in_memory("test-workspace");

        // Create a client body doc with content
        let client_storage = Arc::new(SqliteStorage::in_memory().unwrap());
        let client_body_manager = BodyDocManager::new(client_storage);
        let client_doc = client_body_manager.get_or_create("test.md");
        let _ = client_doc.set_body("Hello from client!");

        // Get the full state
        let client_update = client_doc.encode_state_as_update();
        let msg = SyncMessage::SyncStep2(client_update).encode();

        let _response = room.handle_body_message("test.md", &msg).await;

        // Room should now have the body content
        let content = room.get_body_content("test.md").await;
        assert_eq!(content, Some("Hello from client!".to_string()));
    }

    #[tokio::test]
    async fn test_handle_body_update_broadcasts() {
        let room = SyncRoom::in_memory("test-workspace");

        // Subscribe to body broadcasts
        let mut body_rx = room.subscribe_all_bodies();

        // Create an update
        let client_storage = Arc::new(SqliteStorage::in_memory().unwrap());
        let client_body_manager = BodyDocManager::new(client_storage);
        let client_doc = client_body_manager.get_or_create("test.md");
        let _ = client_doc.set_body("Updated content");

        let update = client_doc.encode_state_as_update();
        let msg = SyncMessage::Update(update).encode();

        let _response = room.handle_body_message("test.md", &msg).await;

        // Should broadcast to subscribers
        let broadcast = body_rx.try_recv();
        assert!(broadcast.is_ok());

        let (file_path, _data) = broadcast.unwrap();
        assert_eq!(file_path, "test.md");
    }

    // =========================================================================
    // Body Subscription Tests
    // =========================================================================

    #[tokio::test]
    async fn test_body_subscription_tracking() {
        let room = SyncRoom::in_memory("test-workspace");

        // Initially not subscribed
        assert!(!room.is_subscribed_to_body("test.md", "client-1").await);

        // Subscribe
        let _rx = room.subscribe_body("test.md", "client-1").await;
        assert!(room.is_subscribed_to_body("test.md", "client-1").await);

        // Unsubscribe
        room.unsubscribe_body("test.md", "client-1").await;
        assert!(!room.is_subscribed_to_body("test.md", "client-1").await);
    }

    #[tokio::test]
    async fn test_get_client_body_subscriptions() {
        let room = SyncRoom::in_memory("test-workspace");

        let _rx1 = room.subscribe_body("file1.md", "client-1").await;
        let _rx2 = room.subscribe_body("file2.md", "client-1").await;
        let _rx3 = room.subscribe_body("file3.md", "client-2").await;

        let client1_subs = room.get_client_body_subscriptions("client-1").await;
        assert_eq!(client1_subs.len(), 2);
        assert!(client1_subs.contains(&"file1.md".to_string()));
        assert!(client1_subs.contains(&"file2.md".to_string()));

        let client2_subs = room.get_client_body_subscriptions("client-2").await;
        assert_eq!(client2_subs.len(), 1);
        assert!(client2_subs.contains(&"file3.md".to_string()));
    }

    // =========================================================================
    // State Vector / Full State Tests
    // =========================================================================

    #[tokio::test]
    async fn test_get_state_vector() {
        let room = SyncRoom::in_memory("test-workspace");

        let sv_msg = room.get_state_vector().await;
        let decoded = SyncMessage::decode_all(&sv_msg).unwrap();

        assert_eq!(decoded.len(), 1);
        assert!(matches!(decoded[0], SyncMessage::SyncStep1(_)));
    }

    #[tokio::test]
    async fn test_get_full_state() {
        let room = SyncRoom::in_memory("test-workspace");

        // Add some data
        {
            let workspace = room.workspace.write().await;
            workspace
                .set_file("test.md", FileMetadata::new(Some("Test".to_string())))
                .unwrap();
        }

        let full_state_msg = room.get_full_state().await;
        let decoded = SyncMessage::decode_all(&full_state_msg).unwrap();

        assert_eq!(decoded.len(), 1);
        match &decoded[0] {
            SyncMessage::SyncStep2(state) => {
                assert!(state.len() > 2, "Full state should contain data");
            }
            _ => panic!("Expected SyncStep2"),
        }
    }

    #[tokio::test]
    async fn test_get_body_state_vector() {
        let room = SyncRoom::in_memory("test-workspace");

        let sv_msg = room.get_body_state_vector("test.md").await;
        let decoded = SyncMessage::decode_all(&sv_msg).unwrap();

        assert_eq!(decoded.len(), 1);
        assert!(matches!(decoded[0], SyncMessage::SyncStep1(_)));
    }

    #[tokio::test]
    async fn test_get_body_full_state() {
        let room = SyncRoom::in_memory("test-workspace");

        // Add body content
        {
            let body_docs = room.body_docs.read().await;
            let doc = body_docs.get_or_create("test.md");
            let _ = doc.set_body("Test content");
        }

        let full_state_msg = room.get_body_full_state("test.md").await;
        let decoded = SyncMessage::decode_all(&full_state_msg).unwrap();

        assert_eq!(decoded.len(), 1);
        match &decoded[0] {
            SyncMessage::SyncStep2(state) => {
                assert!(state.len() > 2, "Full state should contain body data");
            }
            _ => panic!("Expected SyncStep2"),
        }
    }

    // =========================================================================
    // Control Message Broadcast Tests
    // =========================================================================

    #[tokio::test]
    async fn test_broadcast_control_message() {
        let room = SyncRoom::in_memory("test-workspace");

        let mut control_rx = room.subscribe_control();

        room.broadcast_control_message(ControlMessage::SyncComplete { files_synced: 42 })
            .await;

        let msg = control_rx.try_recv();
        assert!(msg.is_ok());

        match msg.unwrap() {
            ControlMessage::SyncComplete { files_synced } => {
                assert_eq!(files_synced, 42);
            }
            _ => panic!("Expected SyncComplete message"),
        }
    }

    #[tokio::test]
    async fn test_sync_progress_tracking() {
        let room = SyncRoom::in_memory("test-workspace");

        let mut control_rx = room.subscribe_control();

        // Create a client workspace with multiple files
        let client_storage = Arc::new(SqliteStorage::in_memory().unwrap());
        let client_workspace = WorkspaceCrdt::new(client_storage);
        client_workspace
            .set_file("file1.md", FileMetadata::new(Some("File 1".to_string())))
            .unwrap();
        client_workspace
            .set_file("file2.md", FileMetadata::new(Some("File 2".to_string())))
            .unwrap();

        // First, initiate sync with SyncStep1 (resets progress counter)
        let empty_sv = Vec::new();
        let init_msg = SyncMessage::SyncStep1(empty_sv).encode();
        let _ = room.handle_message(&init_msg).await;

        // Send update with the files
        let update = client_workspace.encode_state_as_update();
        let msg = SyncMessage::SyncStep2(update).encode();
        let _ = room.handle_message(&msg).await;

        // Should have received SyncProgress message
        // (may need to drain initial messages first)
        let mut found_progress = false;
        while let Ok(ctrl_msg) = control_rx.try_recv() {
            if let ControlMessage::SyncProgress { completed, total } = ctrl_msg {
                assert!(completed > 0);
                assert!(total > 0);
                found_progress = true;
                break;
            }
        }
        assert!(found_progress, "Should have received SyncProgress message");
    }

    // =========================================================================
    // ControlMessage Serialization Tests
    // =========================================================================

    #[test]
    fn test_control_message_peer_joined_serialization() {
        let msg = ControlMessage::PeerJoined {
            guest_id: "guest-123".to_string(),
            peer_count: 3,
        };

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("peer_joined"));
        assert!(json.contains("guest-123"));
        assert!(json.contains("3"));

        let deserialized: ControlMessage = serde_json::from_str(&json).unwrap();
        match deserialized {
            ControlMessage::PeerJoined {
                guest_id,
                peer_count,
            } => {
                assert_eq!(guest_id, "guest-123");
                assert_eq!(peer_count, 3);
            }
            _ => panic!("Expected PeerJoined"),
        }
    }

    #[test]
    fn test_control_message_sync_complete_serialization() {
        let msg = ControlMessage::SyncComplete { files_synced: 100 };

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("sync_complete"));
        assert!(json.contains("100"));

        let deserialized: ControlMessage = serde_json::from_str(&json).unwrap();
        match deserialized {
            ControlMessage::SyncComplete { files_synced } => {
                assert_eq!(files_synced, 100);
            }
            _ => panic!("Expected SyncComplete"),
        }
    }

    #[test]
    fn test_control_message_session_ended_serialization() {
        let msg = ControlMessage::SessionEnded;

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("session_ended"));

        let deserialized: ControlMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, ControlMessage::SessionEnded));
    }
}
