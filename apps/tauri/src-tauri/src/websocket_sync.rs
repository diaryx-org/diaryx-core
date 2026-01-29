//! WebSocket sync transport for Tauri.
//!
//! This module provides real-time sync capabilities via WebSocket,
//! connecting to a sync server and routing messages to the Rust CRDT backend.
//!
//! ## Architecture
//!
//! The sync transport handles:
//! - WebSocket lifecycle (connect, disconnect, reconnect)
//! - Y-sync protocol message routing for both workspace metadata AND body content
//! - Exponential backoff for reconnection
//!
//! ## Two-Connection Sync
//!
//! Sync uses two WebSocket connections:
//! 1. **Metadata connection**: Syncs file metadata (title, part_of, contents, etc.)
//! 2. **Body connection**: Syncs file content (markdown body) via multiplexed protocol
//!
//! All sync logic is delegated to `diaryx_core` via the RustSyncManager.

use diaryx_core::crdt::{
    BodyDocManager, CrdtStorage, RustSyncManager, SyncHandler, WorkspaceCrdt, frame_body_message,
    unframe_body_message,
};
use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use futures_util::{SinkExt, StreamExt};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::async_runtime::Mutex;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

/// Message to send over the WebSocket (from event callback).
#[derive(Debug, Clone)]
pub struct OutgoingSyncMessage {
    /// Document name ("workspace" for metadata, file path for body)
    pub doc_name: String,
    /// Encoded sync message bytes
    pub message: Vec<u8>,
    /// Whether this is a body doc (true) or workspace (false)
    pub is_body: bool,
}

/// Sync transport configuration.
#[derive(Clone)]
pub struct SyncConfig {
    /// Server URL (e.g., wss://sync.diaryx.org/sync)
    pub server_url: String,
    /// Document name (e.g., workspace ID)
    pub doc_name: String,
    /// Auth token for authentication
    pub auth_token: Option<String>,
    /// Whether to write changes to disk
    pub write_to_disk: bool,
    /// CRDT storage backend (shared with main app)
    pub storage: Arc<dyn CrdtStorage>,
    /// Workspace root path for file operations
    pub workspace_root: PathBuf,
    /// Pre-built sync manager (optional).
    /// When provided, the sync transport will use this instead of creating its own.
    /// This ensures the same CRDT instances are used for both command execution
    /// and WebSocket sync, preventing state divergence.
    pub sync_manager: Option<Arc<RustSyncManager<SyncToAsyncFs<RealFileSystem>>>>,
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
///
/// Manages two WebSocket connections:
/// 1. Metadata connection for workspace CRDT
/// 2. Body connection for file content CRDTs (multiplexed)
pub struct SyncTransport {
    config: SyncConfig,
    status: Arc<Mutex<SyncStatus>>,
    running: Arc<AtomicBool>,
    /// Handle to the background task.
    task_handle: Option<tauri::async_runtime::JoinHandle<()>>,
    /// Channel sender for outgoing sync messages from local edits.
    /// This allows the main Diaryx instance to send messages to the WebSocket.
    outgoing_tx: Option<mpsc::UnboundedSender<OutgoingSyncMessage>>,
}

impl SyncTransport {
    /// Create a new sync transport.
    pub fn new(config: SyncConfig) -> Self {
        Self {
            config,
            status: Arc::new(Mutex::new(SyncStatus::Disconnected)),
            running: Arc::new(AtomicBool::new(false)),
            task_handle: None,
            outgoing_tx: None,
        }
    }

    /// Get a clone of the outgoing message sender.
    ///
    /// Use this to send local edit messages to the WebSocket.
    /// Returns None if not connected.
    pub fn get_outgoing_sender(&self) -> Option<mpsc::UnboundedSender<OutgoingSyncMessage>> {
        self.outgoing_tx.clone()
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

        let config = self.config.clone();
        let status = Arc::clone(&self.status);
        let running = Arc::clone(&self.running);

        // Create channel for outgoing messages from local edits
        let (outgoing_tx, outgoing_rx) = mpsc::unbounded_channel();
        self.outgoing_tx = Some(outgoing_tx);

        // Spawn the WebSocket task
        let handle = tauri::async_runtime::spawn(async move {
            run_sync_loop(config, status, running, outgoing_rx).await;
        });

        self.task_handle = Some(handle);
        Ok(())
    }

    /// Disconnect from the sync server.
    pub async fn disconnect(&mut self) {
        log::info!("[SyncTransport] Disconnecting");

        self.running.store(false, Ordering::Relaxed);

        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }

        *self.status.lock().await = SyncStatus::Disconnected;
    }
}

/// Create the RustSyncManager from config.
fn create_sync_manager(config: &SyncConfig) -> Arc<RustSyncManager<SyncToAsyncFs<RealFileSystem>>> {
    let workspace_crdt = Arc::new(
        WorkspaceCrdt::load(Arc::clone(&config.storage))
            .unwrap_or_else(|_| WorkspaceCrdt::new(Arc::clone(&config.storage))),
    );
    let body_manager = Arc::new(BodyDocManager::new(Arc::clone(&config.storage)));
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let sync_handler = Arc::new(SyncHandler::new(fs));
    sync_handler.set_workspace_root(config.workspace_root.clone());

    Arc::new(RustSyncManager::new(
        workspace_crdt,
        body_manager,
        sync_handler,
    ))
}

/// Import existing local files into the body CRDTs.
///
/// This is needed for first-time sync when local files exist but body CRDTs are empty.
fn import_local_bodies(
    workspace_root: &PathBuf,
    workspace_crdt: &WorkspaceCrdt,
    body_manager: &BodyDocManager,
) -> usize {
    use std::fs;

    let mut imported = 0;

    // Walk the workspace directory
    fn walk_dir(
        dir: &std::path::Path,
        workspace_root: &std::path::Path,
        workspace_crdt: &WorkspaceCrdt,
        body_manager: &BodyDocManager,
        imported: &mut usize,
    ) {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();

            // Skip hidden files/directories
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') {
                    continue;
                }
            }

            if path.is_dir() {
                walk_dir(
                    &path,
                    workspace_root,
                    workspace_crdt,
                    body_manager,
                    imported,
                );
            } else if path.extension().map(|e| e == "md").unwrap_or(false) {
                // Get relative path from workspace root
                let rel_path = match path.strip_prefix(workspace_root) {
                    Ok(p) => p.to_string_lossy().to_string(),
                    Err(_) => continue,
                };

                // Only process files that exist in workspace CRDT (metadata was already synced)
                if workspace_crdt.get_file(&rel_path).is_none() {
                    continue;
                }

                // Check if body doc already has content
                let body_doc = body_manager.get_or_create(&rel_path);
                if !body_doc.get_body().is_empty() {
                    continue;
                }

                // Read file content and populate body CRDT
                let content = match fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                let parsed = match diaryx_core::frontmatter::parse_or_empty(&content) {
                    Ok(p) => p,
                    Err(_) => continue,
                };

                // Set body content in CRDT
                if body_doc.set_body(&parsed.body).is_ok() {
                    *imported += 1;
                }
            }
        }
    }

    walk_dir(
        workspace_root,
        workspace_root,
        workspace_crdt,
        body_manager,
        &mut imported,
    );

    if imported > 0 {
        log::info!(
            "[SyncTransport] Imported {} local file bodies into CRDT",
            imported
        );
    }

    imported
}

/// Run the sync loop (background task).
async fn run_sync_loop(
    config: SyncConfig,
    status: Arc<Mutex<SyncStatus>>,
    running: Arc<AtomicBool>,
    outgoing_rx: mpsc::UnboundedReceiver<OutgoingSyncMessage>,
) {
    let mut reconnect_attempts: u32 = 0;
    let max_reconnect_attempts: u32 = 10;

    // Wrap receiver in Arc<Mutex> so it survives reconnection attempts
    let outgoing_rx = Arc::new(tokio::sync::Mutex::new(outgoing_rx));

    // Use provided sync manager or create a new one as fallback.
    // When a sync_manager is provided from the cached Diaryx instance,
    // this ensures both command execution and WebSocket sync share
    // the same CRDT state, preventing divergence issues.
    let sync_manager = config.sync_manager.clone().unwrap_or_else(|| {
        log::info!("[SyncTransport] No shared sync_manager provided, creating new one (fallback)");
        create_sync_manager(&config)
    });

    // Import local file bodies into CRDTs before syncing
    {
        let workspace_crdt = Arc::new(
            WorkspaceCrdt::load(Arc::clone(&config.storage))
                .unwrap_or_else(|_| WorkspaceCrdt::new(Arc::clone(&config.storage))),
        );
        let body_manager = Arc::new(BodyDocManager::new(Arc::clone(&config.storage)));
        import_local_bodies(&config.workspace_root, &workspace_crdt, &body_manager);
    }

    // Build WebSocket URLs
    let metadata_url = match build_websocket_url(&config, false) {
        Ok(url) => url,
        Err(e) => {
            log::error!("[SyncTransport] Invalid metadata URL: {}", e);
            *status.lock().await = SyncStatus::Error { message: e };
            return;
        }
    };

    let body_url = match build_websocket_url(&config, true) {
        Ok(url) => url,
        Err(e) => {
            log::error!("[SyncTransport] Invalid body URL: {}", e);
            *status.lock().await = SyncStatus::Error { message: e };
            return;
        }
    };

    while running.load(Ordering::Relaxed) {
        log::info!("[SyncTransport] Connecting to metadata: {}", metadata_url);
        log::info!("[SyncTransport] Connecting to body: {}", body_url);

        // Connect to both WebSockets
        let metadata_ws = tokio_tungstenite::connect_async(&metadata_url).await;
        let body_ws = tokio_tungstenite::connect_async(&body_url).await;

        match (metadata_ws, body_ws) {
            (Ok((metadata_stream, _)), Ok((body_stream, _))) => {
                log::info!("[SyncTransport] Connected to both sync endpoints");
                reconnect_attempts = 0;
                *status.lock().await = SyncStatus::Connected;

                // Step 1: Run metadata initial sync to populate file list
                log::info!("[SyncTransport] Running metadata initial sync...");
                let (metadata_stream, initial_success) = run_metadata_initial_sync(
                    metadata_stream,
                    Arc::clone(&sync_manager),
                    config.clone(),
                    Arc::clone(&status),
                    Arc::clone(&running),
                )
                .await;

                if !initial_success {
                    log::warn!("[SyncTransport] Metadata initial sync failed, will retry");
                    continue;
                }

                log::info!("[SyncTransport] Metadata initial sync complete, starting body sync");

                // Step 2: Now run both loops concurrently with join (not select)
                // This ensures both stay alive until BOTH complete naturally
                let metadata_future = run_metadata_sync_loop(
                    metadata_stream,
                    Arc::clone(&sync_manager),
                    config.clone(),
                    Arc::clone(&status),
                    Arc::clone(&running),
                );

                let body_future = run_body_sync(
                    body_stream,
                    Arc::clone(&sync_manager),
                    config.clone(),
                    Arc::clone(&running),
                    Arc::clone(&outgoing_rx),
                );

                // Run both until BOTH complete (not just one)
                let (metadata_result, body_result) = tokio::join!(metadata_future, body_future);
                log::info!(
                    "[SyncTransport] Both sync loops ended (metadata: {:?}, body: {:?})",
                    metadata_result,
                    body_result
                );
            }
            (Err(e), _) => {
                log::error!("[SyncTransport] Metadata connection failed: {}", e);
            }
            (_, Err(e)) => {
                log::error!("[SyncTransport] Body connection failed: {}", e);
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

/// Run the metadata initial sync handshake.
///
/// This function performs the initial SyncStep1/SyncStep2 handshake to populate
/// the workspace CRDT with the file list from the server. It returns when:
/// - SyncComplete is received (success)
/// - A long timeout occurs (120s) with no activity (failure)
/// - An error occurs (failure)
///
/// Returns the stream (for reuse) and a success flag.
async fn run_metadata_initial_sync<S>(
    ws_stream: S,
    sync_manager: Arc<RustSyncManager<SyncToAsyncFs<RealFileSystem>>>,
    config: SyncConfig,
    status: Arc<Mutex<SyncStatus>>,
    running: Arc<AtomicBool>,
) -> (S, bool)
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + SinkExt<Message>
        + Unpin,
    <S as futures_util::Sink<Message>>::Error: std::fmt::Display,
{
    let (mut write, mut read) = ws_stream.split();

    // Send SyncStep1 to initiate handshake
    let step1 = sync_manager.create_workspace_sync_step1();
    if let Err(e) = write.send(Message::Binary(step1.into())).await {
        log::error!("[SyncTransport] Failed to send metadata SyncStep1: {}", e);
        let stream = read.reunite(write).expect("reunite failed");
        return (stream, false);
    }
    log::info!("[SyncTransport] Sent metadata SyncStep1 (initial)");

    // Use a longer timeout - large workspaces can take minutes to sync
    // This timeout resets on each message received
    let activity_timeout = Duration::from_secs(120);
    let mut total_files_received: usize = 0;

    loop {
        if !running.load(Ordering::Relaxed) {
            let stream = read.reunite(write).expect("reunite failed");
            return (stream, false);
        }

        tokio::select! {
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        match sync_manager.handle_workspace_message(&data, config.write_to_disk).await {
                            Ok(result) => {
                                if let Some(response) = result.response {
                                    if let Err(e) = write.send(Message::Binary(response.into())).await {
                                        log::error!("[SyncTransport] Failed to send metadata response: {}", e);
                                    }
                                }
                                if !result.changed_files.is_empty() {
                                    total_files_received += result.changed_files.len();
                                    log::debug!(
                                        "[SyncTransport] Initial metadata sync: +{} files (total: {})",
                                        result.changed_files.len(),
                                        total_files_received
                                    );
                                }
                            }
                            Err(e) => {
                                log::error!("[SyncTransport] Failed to handle metadata message: {:?}", e);
                            }
                        }
                    }
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(ctrl_msg) = serde_json::from_str::<ControlMessage>(&text) {
                            match ctrl_msg {
                                ControlMessage::SyncProgress { completed, total } => {
                                    *status.lock().await = SyncStatus::Syncing { completed, total };
                                    log::info!("[SyncTransport] Initial sync progress: {}/{}", completed, total);
                                }
                                ControlMessage::SyncComplete { files_synced } => {
                                    *status.lock().await = SyncStatus::Synced;
                                    log::info!(
                                        "[SyncTransport] Initial metadata sync complete: {} files synced",
                                        files_synced
                                    );
                                    // Initial handshake complete - this is the success condition
                                    let stream = read.reunite(write).expect("reunite failed");
                                    return (stream, true);
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        log::info!("[SyncTransport] Metadata connection closed during initial sync");
                        let stream = read.reunite(write).expect("reunite failed");
                        return (stream, false);
                    }
                    Some(Err(e)) => {
                        log::error!("[SyncTransport] Metadata WebSocket error during initial sync: {}", e);
                        let stream = read.reunite(write).expect("reunite failed");
                        return (stream, false);
                    }
                    None => {
                        log::info!("[SyncTransport] Metadata stream ended during initial sync");
                        let stream = read.reunite(write).expect("reunite failed");
                        return (stream, false);
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep(activity_timeout) => {
                // Timeout with no activity - this is a failure
                // We must wait for SyncComplete to ensure body sync has the full file list
                log::error!(
                    "[SyncTransport] Initial metadata sync timed out after {}s with no SyncComplete (received {} files)",
                    activity_timeout.as_secs(),
                    total_files_received
                );
                let stream = read.reunite(write).expect("reunite failed");
                return (stream, false);
            }
        }
    }
}

/// Run the ongoing metadata sync loop (after initial handshake).
///
/// This handles ongoing sync messages (broadcasts, live updates) after the
/// initial handshake has completed.
async fn run_metadata_sync_loop<S>(
    ws_stream: S,
    sync_manager: Arc<RustSyncManager<SyncToAsyncFs<RealFileSystem>>>,
    config: SyncConfig,
    status: Arc<Mutex<SyncStatus>>,
    running: Arc<AtomicBool>,
) -> &'static str
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + SinkExt<Message>
        + Unpin,
    <S as futures_util::Sink<Message>>::Error: std::fmt::Display,
{
    let (mut write, mut read) = ws_stream.split();

    while running.load(Ordering::Relaxed) {
        tokio::select! {
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        match sync_manager.handle_workspace_message(&data, config.write_to_disk).await {
                            Ok(result) => {
                                if let Some(response) = result.response {
                                    if let Err(e) = write.send(Message::Binary(response.into())).await {
                                        log::error!("[SyncTransport] Failed to send metadata response: {}", e);
                                    }
                                }
                                if !result.changed_files.is_empty() {
                                    log::info!("[SyncTransport] Metadata changed: {:?}", result.changed_files);
                                }
                            }
                            Err(e) => {
                                log::error!("[SyncTransport] Failed to handle metadata message: {:?}", e);
                            }
                        }
                    }
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(ctrl_msg) = serde_json::from_str::<ControlMessage>(&text) {
                            match ctrl_msg {
                                ControlMessage::SyncProgress { completed, total } => {
                                    *status.lock().await = SyncStatus::Syncing { completed, total };
                                    log::debug!("[SyncTransport] Sync progress: {}/{}", completed, total);
                                }
                                ControlMessage::SyncComplete { files_synced } => {
                                    *status.lock().await = SyncStatus::Synced;
                                    log::info!("[SyncTransport] Sync complete: {} files", files_synced);
                                }
                                ControlMessage::PeerJoined { guest_id, peer_count } => {
                                    log::info!("[SyncTransport] Peer joined: {} (total: {})", guest_id, peer_count);
                                }
                                ControlMessage::PeerLeft { guest_id, peer_count } => {
                                    log::info!("[SyncTransport] Peer left: {} (total: {})", guest_id, peer_count);
                                }
                                ControlMessage::Other => {}
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        log::info!("[SyncTransport] Metadata connection closed by server");
                        return "closed";
                    }
                    Some(Err(e)) => {
                        log::error!("[SyncTransport] Metadata WebSocket error: {}", e);
                        return "error";
                    }
                    None => {
                        log::info!("[SyncTransport] Metadata stream ended");
                        return "ended";
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(30)) => {
                // Send ping to keep connection alive
                if let Err(e) = write.send(Message::Ping(vec![].into())).await {
                    log::error!("[SyncTransport] Failed to send ping: {}", e);
                    return "ping_failed";
                }
            }
        }
    }

    "stopped"
}

/// Run the body sync loop (multiplexed).
///
/// This loads the file list from the workspace CRDT (which should now be populated
/// after the initial metadata sync) and syncs body content for all files.
///
/// Also handles outgoing messages from local edits via the `outgoing_rx` channel.
async fn run_body_sync<S>(
    ws_stream: S,
    sync_manager: Arc<RustSyncManager<SyncToAsyncFs<RealFileSystem>>>,
    config: SyncConfig,
    running: Arc<AtomicBool>,
    outgoing_rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<OutgoingSyncMessage>>>,
) -> &'static str
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + SinkExt<Message>
        + Unpin,
    <S as futures_util::Sink<Message>>::Error: std::fmt::Display,
{
    let (mut write, mut read) = ws_stream.split();
    let mut outgoing = outgoing_rx.lock().await;

    // Get list of files from workspace CRDT
    // NOTE: This now runs AFTER metadata initial sync, so the file list should be populated
    let workspace_crdt = WorkspaceCrdt::load(Arc::clone(&config.storage))
        .unwrap_or_else(|_| WorkspaceCrdt::new(Arc::clone(&config.storage)));
    let files = workspace_crdt.list_files();

    // Send SyncStep1 for all known files
    log::info!(
        "[SyncTransport] Sending body SyncStep1 for {} files",
        files.len()
    );
    for (file_path, _metadata) in &files {
        if !running.load(Ordering::Relaxed) {
            return "stopped";
        }
        let step1 = sync_manager.create_body_sync_step1(file_path);
        let framed = frame_body_message(file_path, &step1);
        if let Err(e) = write.send(Message::Binary(framed.into())).await {
            log::warn!(
                "[SyncTransport] Failed to send body SyncStep1 for {}: {}",
                file_path,
                e
            );
        }
    }
    log::info!(
        "[SyncTransport] Sent body SyncStep1 for {} files",
        files.len()
    );

    while running.load(Ordering::Relaxed) {
        tokio::select! {
            // Handle incoming messages from server
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        // Unframe the multiplexed message
                        if let Some((file_path, body_msg)) = unframe_body_message(&data) {
                            match sync_manager.handle_body_message(&file_path, &body_msg, config.write_to_disk).await {
                                Ok(result) => {
                                    if let Some(response) = result.response {
                                        let framed = frame_body_message(&file_path, &response);
                                        if let Err(e) = write.send(Message::Binary(framed.into())).await {
                                            log::error!("[SyncTransport] Failed to send body response: {}", e);
                                        }
                                    }
                                    if result.content.is_some() && !result.is_echo {
                                        log::info!("[SyncTransport] Body synced: {}", file_path);
                                    }
                                }
                                Err(e) => {
                                    log::error!("[SyncTransport] Failed to handle body message for {}: {:?}", file_path, e);
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Text(text))) => {
                        // Handle JSON control messages (same format as metadata)
                        if let Ok(ctrl_msg) = serde_json::from_str::<ControlMessage>(&text) {
                            if let ControlMessage::SyncComplete { files_synced } = ctrl_msg {
                                log::info!("[SyncTransport] Body sync complete: {} files", files_synced);
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        log::info!("[SyncTransport] Body connection closed by server");
                        return "closed";
                    }
                    Some(Err(e)) => {
                        log::error!("[SyncTransport] Body WebSocket error: {}", e);
                        return "error";
                    }
                    None => {
                        log::info!("[SyncTransport] Body stream ended");
                        return "ended";
                    }
                    _ => {}
                }
            }
            // Handle outgoing messages from local edits
            outgoing_msg = outgoing.recv() => {
                match outgoing_msg {
                    Some(msg) => {
                        if msg.is_body {
                            // Send body sync message (multiplexed)
                            let framed = frame_body_message(&msg.doc_name, &msg.message);
                            if let Err(e) = write.send(Message::Binary(framed.into())).await {
                                log::error!("[SyncTransport] Failed to send outgoing body message for {}: {}", msg.doc_name, e);
                            } else {
                                log::info!("[SyncTransport] Sent local body update for {}, {} bytes", msg.doc_name, msg.message.len());
                            }
                        } else {
                            // Metadata messages would go to the metadata WebSocket
                            // For now, log a warning since we don't have access to metadata write here
                            log::warn!("[SyncTransport] Received metadata outgoing message for {} but can't send from body loop", msg.doc_name);
                        }
                    }
                    None => {
                        // Channel closed, transport is being destroyed
                        log::info!("[SyncTransport] Outgoing channel closed");
                        return "channel_closed";
                    }
                }
            }
            // Keep connection alive
            _ = tokio::time::sleep(Duration::from_secs(30)) => {
                // Send ping to keep connection alive
                if let Err(e) = write.send(Message::Ping(vec![].into())).await {
                    log::error!("[SyncTransport] Failed to send ping: {}", e);
                    return "ping_failed";
                }
            }
        }
    }

    "stopped"
}

/// Build WebSocket URL with query parameters.
fn build_websocket_url(config: &SyncConfig, multiplexed: bool) -> Result<String, String> {
    let mut url = Url::parse(&config.server_url).map_err(|e| format!("Invalid URL: {}", e))?;

    // Add doc parameter
    url.query_pairs_mut().append_pair("doc", &config.doc_name);

    // Add multiplexed flag for body sync
    if multiplexed {
        url.query_pairs_mut().append_pair("multiplexed", "true");
    }

    // Add auth token if provided
    if let Some(ref token) = config.auth_token {
        url.query_pairs_mut().append_pair("token", token);
    }

    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use diaryx_core::crdt::MemoryStorage;

    fn create_test_config() -> SyncConfig {
        SyncConfig {
            server_url: "wss://sync.diaryx.org/sync".to_string(),
            doc_name: "workspace123".to_string(),
            auth_token: Some("token123".to_string()),
            write_to_disk: true,
            storage: Arc::new(MemoryStorage::new()),
            workspace_root: PathBuf::from("/tmp/test"),
            sync_manager: None,
        }
    }

    #[test]
    fn test_build_websocket_url_metadata() {
        let config = create_test_config();
        let url = build_websocket_url(&config, false).unwrap();
        assert!(url.contains("doc=workspace123"));
        assert!(url.contains("token=token123"));
        assert!(!url.contains("multiplexed"));
    }

    #[test]
    fn test_build_websocket_url_body() {
        let config = create_test_config();
        let url = build_websocket_url(&config, true).unwrap();
        assert!(url.contains("doc=workspace123"));
        assert!(url.contains("token=token123"));
        assert!(url.contains("multiplexed=true"));
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
