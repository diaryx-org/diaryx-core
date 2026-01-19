use diaryx_core::crdt::{SqliteStorage, SyncMessage, UpdateOrigin, WorkspaceCrdt};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tracing::{error, info, warn};

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
    /// Base path for workspace databases
    data_dir: PathBuf,
}

impl SyncState {
    /// Create a new SyncState
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            rooms: RwLock::new(HashMap::new()),
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
}

/// A sync room for a single workspace
pub struct SyncRoom {
    #[allow(dead_code)]
    workspace_id: String,
    /// The CRDT workspace document
    workspace: RwLock<WorkspaceCrdt>,
    /// Broadcast channel for updates
    broadcast_tx: broadcast::Sender<Vec<u8>>,
    /// Number of active connections
    connection_count: std::sync::atomic::AtomicUsize,
    /// Storage backend (kept for potential future use)
    #[allow(dead_code)]
    storage: Arc<SqliteStorage>,
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

        let (broadcast_tx, _) = broadcast::channel(1024);

        Ok(Self {
            workspace_id: workspace_id.to_string(),
            workspace: RwLock::new(workspace),
            broadcast_tx,
            connection_count: std::sync::atomic::AtomicUsize::new(0),
            storage,
        })
    }

    /// Create an in-memory SyncRoom (for fallback/testing)
    pub fn in_memory(workspace_id: &str) -> Self {
        let storage =
            Arc::new(SqliteStorage::in_memory().expect("Failed to create in-memory storage"));
        let workspace = WorkspaceCrdt::with_name(storage.clone(), workspace_id.to_string());

        let (broadcast_tx, _) = broadcast::channel(1024);

        Self {
            workspace_id: workspace_id.to_string(),
            workspace: RwLock::new(workspace),
            broadcast_tx,
            connection_count: std::sync::atomic::AtomicUsize::new(0),
            storage,
        }
    }

    /// Subscribe to room updates
    pub fn subscribe(&self) -> broadcast::Receiver<Vec<u8>> {
        self.connection_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.broadcast_tx.subscribe()
    }

    /// Unsubscribe from room updates
    pub fn unsubscribe(&self) {
        self.connection_count
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// Get the number of active connections
    pub fn connection_count(&self) -> usize {
        self.connection_count
            .load(std::sync::atomic::Ordering::SeqCst)
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
                    // Client is initiating sync, respond with our diff
                    let workspace = self.workspace.read().await;
                    match workspace.encode_diff(&state_vector) {
                        Ok(diff) => {
                            let response = SyncMessage::SyncStep2(diff).encode();
                            responses.extend(response);

                            // Also send our state vector so client can send us their diff
                            let our_sv = workspace.encode_state_vector();
                            let sv_msg = SyncMessage::SyncStep1(our_sv).encode();
                            responses.extend(sv_msg);
                        }
                        Err(e) => {
                            warn!("Failed to encode diff: {}", e);
                        }
                    }
                }
                SyncMessage::SyncStep2(diff) => {
                    // Client sent us their diff, apply it
                    let workspace = self.workspace.write().await;
                    if let Err(e) = workspace.apply_update(&diff, UpdateOrigin::Remote) {
                        warn!("Failed to apply sync step 2: {}", e);
                    }
                }
                SyncMessage::Update(update) => {
                    // Apply the update
                    {
                        let workspace = self.workspace.write().await;
                        if let Err(e) = workspace.apply_update(&update, UpdateOrigin::Remote) {
                            warn!("Failed to apply update: {}", e);
                            continue;
                        }
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
}
