//! WebSocket sync transport for Tauri.
//!
//! This module provides real-time sync capabilities via WebSocket,
//! connecting to a sync server and routing messages to the Rust CRDT backend.
//!
//! ## Architecture
//!
//! The sync transport handles:
//! - WebSocket lifecycle (connect, disconnect, reconnect)
//! - Y-sync protocol message routing
//! - Exponential backoff for reconnection
//!
//! All sync logic is delegated to `diaryx_core` via the command handler.

use diaryx_core::Response;
use diaryx_core::command::Command;
use diaryx_core::diaryx::Diaryx;
use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::async_runtime::Mutex;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

/// Sync transport configuration.
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Server URL (e.g., wss://sync.diaryx.org/sync)
    pub server_url: String,
    /// Document name (e.g., workspace ID)
    pub doc_name: String,
    /// Auth token for authentication
    pub auth_token: Option<String>,
    /// Whether to write changes to disk
    pub write_to_disk: bool,
}

/// Status of the sync connection.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SyncStatus {
    Disconnected,
    Connecting,
    Connected,
    Syncing { completed: usize, total: usize },
    Synced,
    Reconnecting { attempt: u32 },
    Error { message: String },
}

/// Control messages received from the sync server (JSON text messages).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ControlMessage {
    SyncProgress {
        completed: usize,
        total: usize,
    },
    SyncComplete {
        files_synced: usize,
    },
    PeerJoined {
        guest_id: String,
        peer_count: usize,
    },
    PeerLeft {
        guest_id: String,
        peer_count: usize,
    },
    #[serde(other)]
    Other,
}

/// WebSocket sync transport.
pub struct SyncTransport {
    config: SyncConfig,
    status: Arc<Mutex<SyncStatus>>,
    running: Arc<AtomicBool>,
    /// Channel to send messages to the WebSocket task.
    tx: Option<mpsc::UnboundedSender<Vec<u8>>>,
    /// Handle to the background task.
    task_handle: Option<tauri::async_runtime::JoinHandle<()>>,
}

impl SyncTransport {
    /// Create a new sync transport.
    pub fn new(config: SyncConfig) -> Self {
        Self {
            config,
            status: Arc::new(Mutex::new(SyncStatus::Disconnected)),
            running: Arc::new(AtomicBool::new(false)),
            tx: None,
            task_handle: None,
        }
    }

    /// Get the current sync status.
    pub async fn status(&self) -> SyncStatus {
        self.status.lock().await.clone()
    }

    /// Check if connected.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Connect to the sync server.
    pub async fn connect(&mut self) -> Result<(), String> {
        if self.is_running() {
            return Ok(());
        }

        log::info!(
            "[SyncTransport] Connecting to {} with doc {}",
            self.config.server_url,
            self.config.doc_name
        );

        self.running.store(true, Ordering::Relaxed);
        *self.status.lock().await = SyncStatus::Connecting;

        // Create channel for sending messages to WebSocket
        let (tx, rx) = mpsc::unbounded_channel::<Vec<u8>>();
        self.tx = Some(tx);

        let config = self.config.clone();
        let status = Arc::clone(&self.status);
        let running = Arc::clone(&self.running);

        // Spawn the WebSocket task
        let handle = tauri::async_runtime::spawn(async move {
            run_sync_loop(config, status, running, rx).await;
        });

        self.task_handle = Some(handle);
        Ok(())
    }

    /// Disconnect from the sync server.
    pub async fn disconnect(&mut self) {
        log::info!("[SyncTransport] Disconnecting");

        self.running.store(false, Ordering::Relaxed);
        self.tx = None;

        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }

        *self.status.lock().await = SyncStatus::Disconnected;
    }

    /// Send a message to the server.
    pub fn send(&self, message: Vec<u8>) -> Result<(), String> {
        if let Some(ref tx) = self.tx {
            tx.send(message).map_err(|e| format!("Send failed: {}", e))
        } else {
            Err("Not connected".to_string())
        }
    }
}

/// Run the sync loop (background task).
async fn run_sync_loop(
    config: SyncConfig,
    status: Arc<Mutex<SyncStatus>>,
    running: Arc<AtomicBool>,
    mut rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    let mut reconnect_attempts: u32 = 0;
    let max_reconnect_attempts: u32 = 10;

    while running.load(Ordering::Relaxed) {
        // Build WebSocket URL
        let ws_url = match build_websocket_url(&config) {
            Ok(url) => url,
            Err(e) => {
                log::error!("[SyncTransport] Invalid URL: {}", e);
                *status.lock().await = SyncStatus::Error { message: e };
                break;
            }
        };

        log::info!("[SyncTransport] Connecting to {}", ws_url);

        // Connect to WebSocket
        match tokio_tungstenite::connect_async(&ws_url).await {
            Ok((ws_stream, _response)) => {
                log::info!("[SyncTransport] Connected");
                reconnect_attempts = 0;
                *status.lock().await = SyncStatus::Connected;

                let (mut write, mut read) = ws_stream.split();

                // Create Diaryx instance for handling sync messages
                let diaryx = create_diaryx();

                // Send sync step 1 to initiate handshake
                if let Some(ref diaryx) = diaryx {
                    match send_sync_step1(diaryx, &mut write).await {
                        Ok(_) => log::info!("[SyncTransport] Sent sync step 1"),
                        Err(e) => log::error!("[SyncTransport] Failed to send sync step 1: {}", e),
                    }
                }

                // Main message loop
                loop {
                    if !running.load(Ordering::Relaxed) {
                        log::info!("[SyncTransport] Stopping (running = false)");
                        break;
                    }

                    tokio::select! {
                        // Receive message from server
                        msg = read.next() => {
                            match msg {
                                Some(Ok(Message::Binary(data))) => {
                                    if let Some(ref diaryx) = diaryx {
                                        handle_server_message(diaryx, &data, &config, &mut write, &status).await;
                                    }
                                }
                                Some(Ok(Message::Text(text))) => {
                                    // Handle JSON control messages from server
                                    if let Ok(ctrl_msg) = serde_json::from_str::<ControlMessage>(&text) {
                                        match ctrl_msg {
                                            ControlMessage::SyncProgress { completed, total } => {
                                                *status.lock().await = SyncStatus::Syncing { completed, total };
                                                log::debug!("[SyncTransport] Sync progress: {}/{}", completed, total);
                                            }
                                            ControlMessage::SyncComplete { files_synced } => {
                                                *status.lock().await = SyncStatus::Synced;
                                                log::info!("[SyncTransport] Sync complete: {} files synced", files_synced);
                                            }
                                            ControlMessage::PeerJoined { guest_id, peer_count } => {
                                                log::info!("[SyncTransport] Peer joined: {} (total: {})", guest_id, peer_count);
                                            }
                                            ControlMessage::PeerLeft { guest_id, peer_count } => {
                                                log::info!("[SyncTransport] Peer left: {} (total: {})", guest_id, peer_count);
                                            }
                                            ControlMessage::Other => {
                                                log::debug!("[SyncTransport] Received unknown control message");
                                            }
                                        }
                                    } else {
                                        log::warn!("[SyncTransport] Failed to parse control message: {}", text);
                                    }
                                }
                                Some(Ok(Message::Close(_))) => {
                                    log::info!("[SyncTransport] Server closed connection");
                                    break;
                                }
                                Some(Err(e)) => {
                                    log::error!("[SyncTransport] WebSocket error: {}", e);
                                    break;
                                }
                                None => {
                                    log::info!("[SyncTransport] Stream ended");
                                    break;
                                }
                                _ => {}
                            }
                        }

                        // Send message from queue
                        outgoing = rx.recv() => {
                            if let Some(data) = outgoing {
                                if let Err(e) = write.send(Message::Binary(data.into())).await {
                                    log::error!("[SyncTransport] Failed to send: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                log::error!("[SyncTransport] Connection failed: {}", e);
            }
        }

        // Reconnect logic
        if !running.load(Ordering::Relaxed) {
            break;
        }

        reconnect_attempts += 1;
        if reconnect_attempts > max_reconnect_attempts {
            log::error!("[SyncTransport] Max reconnect attempts reached");
            *status.lock().await = SyncStatus::Error {
                message: "Max reconnect attempts reached".to_string(),
            };
            break;
        }

        // Exponential backoff: 1s, 2s, 4s, 8s, 16s, 32s (max)
        let delay = std::cmp::min(1000 * 2u64.pow(reconnect_attempts - 1), 32000);
        log::info!(
            "[SyncTransport] Reconnecting in {}ms (attempt {})",
            delay,
            reconnect_attempts
        );
        *status.lock().await = SyncStatus::Reconnecting {
            attempt: reconnect_attempts,
        };

        tokio::time::sleep(Duration::from_millis(delay)).await;
    }

    log::info!("[SyncTransport] Sync loop ended");
}

/// Build WebSocket URL with query parameters.
fn build_websocket_url(config: &SyncConfig) -> Result<String, String> {
    let mut url = Url::parse(&config.server_url).map_err(|e| format!("Invalid URL: {}", e))?;

    // Add doc parameter
    url.query_pairs_mut().append_pair("doc", &config.doc_name);

    // Add auth token if provided
    if let Some(ref token) = config.auth_token {
        url.query_pairs_mut().append_pair("token", token);
    }

    Ok(url.to_string())
}

/// Create a Diaryx instance with CRDT enabled.
fn create_diaryx() -> Option<Diaryx<SyncToAsyncFs<RealFileSystem>>> {
    // For now, create a basic instance without CRDT storage
    // The calling code should provide the proper instance with CRDT state
    Some(Diaryx::new(SyncToAsyncFs::new(RealFileSystem)))
}

/// Send sync step 1 to initiate handshake.
async fn send_sync_step1<S>(
    diaryx: &Diaryx<SyncToAsyncFs<RealFileSystem>>,
    write: &mut futures_util::stream::SplitSink<S, Message>,
) -> Result<(), String>
where
    S: futures_util::Sink<Message> + Unpin,
    <S as futures_util::Sink<Message>>::Error: std::fmt::Display,
{
    let cmd = Command::CreateWorkspaceSyncStep1;
    let response = diaryx
        .execute(cmd)
        .await
        .map_err(|e| format!("Execute failed: {:?}", e))?;

    if let Response::Binary(data) = response {
        if !data.is_empty() {
            write
                .send(Message::Binary(data.into()))
                .await
                .map_err(|e| format!("Send failed: {}", e))?;
        }
    }

    Ok(())
}

/// Handle incoming message from server.
async fn handle_server_message<S>(
    diaryx: &Diaryx<SyncToAsyncFs<RealFileSystem>>,
    data: &[u8],
    config: &SyncConfig,
    write: &mut futures_util::stream::SplitSink<S, Message>,
    _status: &Arc<Mutex<SyncStatus>>,
) where
    S: futures_util::Sink<Message> + Unpin,
    <S as futures_util::Sink<Message>>::Error: std::fmt::Display,
{
    let cmd = Command::HandleWorkspaceSyncMessage {
        message: data.to_vec(),
        write_to_disk: config.write_to_disk,
    };

    match diaryx.execute(cmd).await {
        Ok(Response::WorkspaceSyncResult {
            response,
            changed_files,
            sync_complete,
        }) => {
            // Send response if any
            if let Some(response_data) = response {
                if !response_data.is_empty() {
                    if let Err(e) = write.send(Message::Binary(response_data.into())).await {
                        log::error!("[SyncTransport] Failed to send response: {}", e);
                    }
                }
            }

            // NOTE: We intentionally do NOT set status to Synced based on Rust's sync_complete flag.
            // The Rust flag fires after receiving just one message (initial handshake), but that's
            // BEFORE the client has finished sending its local data to the server.
            // Instead, we rely on the server's sync_complete JSON message (handled in Message::Text)
            // which indicates the server has received all our data.
            if sync_complete {
                log::debug!(
                    "[SyncTransport] Rust sync_complete flag set (waiting for server confirmation)"
                );
            }

            // Log changed files
            if !changed_files.is_empty() {
                log::info!(
                    "[SyncTransport] {} files changed: {:?}",
                    changed_files.len(),
                    changed_files
                );
            }
        }
        Ok(_) => {
            log::warn!("[SyncTransport] Unexpected response type");
        }
        Err(e) => {
            log::error!("[SyncTransport] Failed to handle message: {:?}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_websocket_url() {
        let config = SyncConfig {
            server_url: "wss://sync.diaryx.org/sync".to_string(),
            doc_name: "workspace123".to_string(),
            auth_token: Some("token123".to_string()),
            write_to_disk: true,
        };

        let url = build_websocket_url(&config).unwrap();
        assert!(url.contains("doc=workspace123"));
        assert!(url.contains("token=token123"));
    }

    #[test]
    fn test_control_message_deserialization() {
        // Test sync_progress - must match server's ControlMessage format
        let json = r#"{"type":"sync_progress","completed":5,"total":42}"#;
        let msg: ControlMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(
            msg,
            ControlMessage::SyncProgress {
                completed: 5,
                total: 42
            }
        ));

        // Test sync_complete
        let json = r#"{"type":"sync_complete","files_synced":100}"#;
        let msg: ControlMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(
            msg,
            ControlMessage::SyncComplete { files_synced: 100 }
        ));

        // Test peer_joined
        let json = r#"{"type":"peer_joined","guest_id":"abc123","peer_count":3}"#;
        let msg: ControlMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(
            msg,
            ControlMessage::PeerJoined { peer_count: 3, .. }
        ));

        // Test peer_left
        let json = r#"{"type":"peer_left","guest_id":"abc123","peer_count":2}"#;
        let msg: ControlMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(
            msg,
            ControlMessage::PeerLeft { peer_count: 2, .. }
        ));

        // Test unknown message type falls back to Other
        let json = r#"{"type":"unknown_future_message","foo":"bar"}"#;
        let msg: ControlMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, ControlMessage::Other));
    }

    #[test]
    fn test_sync_status_serialization() {
        // Test Syncing variant - frontend needs to parse this
        let status = SyncStatus::Syncing {
            completed: 10,
            total: 50,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains(r#""type":"syncing""#));
        assert!(json.contains(r#""completed":10"#));
        assert!(json.contains(r#""total":50"#));

        // Test Synced variant
        let status = SyncStatus::Synced;
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains(r#""type":"synced""#));

        // Test Reconnecting variant
        let status = SyncStatus::Reconnecting { attempt: 3 };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains(r#""type":"reconnecting""#));
        assert!(json.contains(r#""attempt":3"#));

        // Test Error variant
        let status = SyncStatus::Error {
            message: "connection failed".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains(r#""type":"error""#));
        assert!(json.contains("connection failed"));
    }
}
