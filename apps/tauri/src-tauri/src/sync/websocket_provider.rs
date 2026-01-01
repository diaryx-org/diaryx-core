//! WebSocket-based sync provider for Tauri.
//!
//! Connects to a WebSocket relay server to exchange automerge sync messages
//! with other peers in the same workspace.

use diaryx_core::sync_crdt::{LiveSyncProvider, PeerId, SyncError};
use futures::{SinkExt, StreamExt};
use std::sync::{Arc, RwLock};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// WebSocket-based sync provider.
///
/// Connects to a relay server that routes messages between peers in the same workspace.
pub struct WebSocketSyncProvider {
    /// URL of the WebSocket server (e.g., "ws://localhost:8080")
    server_url: String,

    /// Workspace ID to join (peers with same workspace ID sync together)
    workspace_id: String,

    /// Tokio runtime for async operations
    runtime: Arc<Runtime>,

    /// Connection state
    state: Arc<RwLock<ConnectionState>>,

    /// Channel for receiving messages from WebSocket
    rx: Arc<RwLock<mpsc::UnboundedReceiver<Vec<u8>>>>,

    /// Channel for sending messages to WebSocket
    tx: Arc<RwLock<Option<mpsc::UnboundedSender<Vec<u8>>>>>,
}

struct ConnectionState {
    connected: bool,
    peer_id: Option<PeerId>,
}

impl WebSocketSyncProvider {
    /// Create a new WebSocket sync provider.
    ///
    /// # Arguments
    ///
    /// * `server_url` - WebSocket server URL (e.g., "ws://localhost:8080")
    /// * `workspace_id` - Workspace identifier for grouping peers
    pub fn new(server_url: String, workspace_id: String) -> Result<Self, SyncError> {
        let runtime = Arc::new(
            Runtime::new().map_err(|e| SyncError::Transport(format!("Failed to create runtime: {}", e)))?
        );

        let (tx_send, _rx_recv) = mpsc::unbounded_channel();
        let (_tx_send2, rx_recv2) = mpsc::unbounded_channel();

        Ok(Self {
            server_url,
            workspace_id,
            runtime,
            state: Arc::new(RwLock::new(ConnectionState {
                connected: false,
                peer_id: None,
            })),
            rx: Arc::new(RwLock::new(rx_recv2)),
            tx: Arc::new(RwLock::new(Some(tx_send))),
        })
    }

    /// Background task to handle WebSocket connection.
    async fn connection_task(
        url: String,
        workspace_id: String,
        state: Arc<RwLock<ConnectionState>>,
        mut rx_send: mpsc::UnboundedReceiver<Vec<u8>>,
        tx_recv: mpsc::UnboundedSender<Vec<u8>>,
    ) -> Result<(), SyncError> {
        // Connect to WebSocket server with workspace ID as query param
        // Ensure URL has a path before adding query params
        let base_url = if url.ends_with('/') {
            url.trim_end_matches('/').to_string()
        } else {
            url.to_string()
        };
        let ws_url = format!("{}/?workspace={}", base_url, workspace_id);
        let (ws_stream, _) = connect_async(&ws_url)
            .await
            .map_err(|e| SyncError::Transport(format!("WebSocket connection failed: {}", e)))?;

        log::info!("[WebSocket] Connected to {}", ws_url);

        // Generate peer ID (in production, server should assign this)
        let peer_id = format!("peer-{}", uuid::Uuid::new_v4());

        {
            let mut state = state.write().unwrap();
            state.connected = true;
            state.peer_id = Some(peer_id.clone());
        }

        let (mut write, mut read) = ws_stream.split();

        // Spawn task to forward outgoing messages
        let send_task = tokio::spawn(async move {
            while let Some(message) = rx_send.recv().await {
                if let Err(e) = write.send(Message::Binary(message)).await {
                    log::error!("[WebSocket] Send error: {}", e);
                    break;
                }
            }
        });

        // Handle incoming messages
        while let Some(msg_result) = read.next().await {
            match msg_result {
                Ok(Message::Binary(data)) => {
                    // Forward to sync manager
                    if tx_recv.send(data).is_err() {
                        log::warn!("[WebSocket] Receiver dropped, closing connection");
                        break;
                    }
                }
                Ok(Message::Close(_)) => {
                    log::info!("[WebSocket] Connection closed by server");
                    break;
                }
                Ok(Message::Ping(data)) => {
                    // Respond to ping (tungstenite should handle this automatically)
                    log::debug!("[WebSocket] Received ping: {} bytes", data.len());
                }
                Ok(_) => {
                    // Ignore text messages, pong, etc.
                }
                Err(e) => {
                    log::error!("[WebSocket] Read error: {}", e);
                    break;
                }
            }
        }

        // Update connection state
        {
            let mut state = state.write().unwrap();
            state.connected = false;
            state.peer_id = None;
        }

        // Wait for send task to finish
        let _ = send_task.await;

        log::info!("[WebSocket] Connection closed");
        Ok(())
    }
}

impl LiveSyncProvider for WebSocketSyncProvider {
    fn connect(&mut self) -> Result<(), SyncError> {
        // Check if already connected
        {
            let state = self.state.read().unwrap();
            if state.connected {
                return Ok(());
            }
        }

        // Create channels for bidirectional communication
        let (tx_send, rx_send) = mpsc::unbounded_channel();
        let (tx_recv, rx_recv) = mpsc::unbounded_channel();

        // Store sender for outgoing messages
        {
            let mut tx_lock = self.tx.write().unwrap();
            *tx_lock = Some(tx_send);
        }

        // Store receiver for incoming messages
        {
            let mut rx_lock = self.rx.write().unwrap();
            *rx_lock = rx_recv;
        }

        // Spawn connection task
        let url = self.server_url.clone();
        let workspace_id = self.workspace_id.clone();
        let state = Arc::clone(&self.state);

        self.runtime.spawn(async move {
            if let Err(e) = Self::connection_task(url, workspace_id, state, rx_send, tx_recv).await {
                log::error!("[WebSocket] Connection task failed: {}", e);
            }
        });

        // Wait a bit for connection to establish
        std::thread::sleep(std::time::Duration::from_millis(100));

        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), SyncError> {
        // Drop the sender to signal the connection task to close
        {
            let mut tx_lock = self.tx.write().unwrap();
            *tx_lock = None;
        }

        // Update state
        {
            let mut state = self.state.write().unwrap();
            state.connected = false;
            state.peer_id = None;
        }

        Ok(())
    }

    fn send_sync_message(&self, message: Vec<u8>) -> Result<(), SyncError> {
        log::info!("[WebSocket] send_sync_message called with {} bytes", message.len());
        let tx_lock = self.tx.read().unwrap();
        if let Some(ref tx) = *tx_lock {
            tx.send(message)
                .map_err(|_| SyncError::Transport("Send channel closed".to_string()))?;
            log::info!("[WebSocket] Message queued for sending");
            Ok(())
        } else {
            log::warn!("[WebSocket] Cannot send - not connected");
            Err(SyncError::Transport("Not connected".to_string()))
        }
    }

    fn receive_sync_messages(&self) -> Result<Vec<Vec<u8>>, SyncError> {
        let mut messages = Vec::new();
        let mut rx_lock = self.rx.write().unwrap();

        // Drain all pending messages (non-blocking)
        while let Ok(msg) = rx_lock.try_recv() {
            messages.push(msg);
        }

        Ok(messages)
    }

    fn is_connected(&self) -> bool {
        let state = self.state.read().unwrap();
        state.connected
    }

    fn peer_id(&self) -> Option<PeerId> {
        let state = self.state.read().unwrap();
        state.peer_id.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_provider() {
        let provider = WebSocketSyncProvider::new(
            "ws://localhost:8080".to_string(),
            "test-workspace".to_string(),
        )
        .unwrap();

        assert!(!provider.is_connected());
        assert_eq!(provider.peer_id(), None);
    }
}
