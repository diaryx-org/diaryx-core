use super::SyncRoom;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, warn};

/// Represents a connected client
pub struct ClientConnection {
    pub user_id: String,
    pub device_id: String,
    pub workspace_id: String,
    room: Arc<SyncRoom>,
    broadcast_rx: broadcast::Receiver<Vec<u8>>,
}

impl ClientConnection {
    /// Create a new client connection
    pub fn new(
        user_id: String,
        device_id: String,
        workspace_id: String,
        room: Arc<SyncRoom>,
    ) -> Self {
        let broadcast_rx = room.subscribe();

        Self {
            user_id,
            device_id,
            workspace_id,
            room,
            broadcast_rx,
        }
    }

    /// Get the initial sync message (full state)
    pub async fn get_initial_sync(&self) -> Vec<u8> {
        self.room.get_full_state().await
    }

    /// Handle an incoming message from the client
    pub async fn handle_message(&self, msg: &[u8]) -> Option<Vec<u8>> {
        self.room.handle_message(msg).await
    }

    /// Receive the next broadcast message (from other clients)
    pub async fn recv_broadcast(&mut self) -> Option<Vec<u8>> {
        match self.broadcast_rx.recv().await {
            Ok(msg) => Some(msg),
            Err(broadcast::error::RecvError::Lagged(n)) => {
                warn!(
                    "Client {} lagged {} messages, requesting full sync",
                    self.user_id, n
                );
                // Return full state when client lags
                Some(self.room.get_full_state().await)
            }
            Err(broadcast::error::RecvError::Closed) => None,
        }
    }
}

impl Drop for ClientConnection {
    fn drop(&mut self) {
        self.room.unsubscribe();
        debug!(
            "Client disconnected: user={}, workspace={}",
            self.user_id, self.workspace_id
        );
    }
}
