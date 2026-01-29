//! Sync client command handlers.
//!
//! Handles start, push, and pull commands using WebSocket connections.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use diaryx_core::config::Config;
use diaryx_core::crdt::{
    BodyDocManager, CrdtStorage, RustSyncManager, SqliteStorage, SyncHandler, SyncMessage,
    WorkspaceCrdt, frame_body_message, unframe_body_message,
};
use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use super::progress;

const DEFAULT_SYNC_SERVER: &str = "https://sync.diaryx.org";

/// Scan the workspace and import existing files into the CRDT.
///
/// This is needed for first-time sync when local files exist but the CRDT is empty.
fn import_existing_files(
    workspace_root: &Path,
    workspace_crdt: &WorkspaceCrdt,
    body_manager: &BodyDocManager,
) -> usize {
    use diaryx_core::crdt::FileMetadata;
    use std::fs;

    let mut imported = 0;

    // Walk the workspace directory
    fn walk_dir(
        dir: &Path,
        workspace_root: &Path,
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

            // Skip hidden files/directories and .diaryx
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

                // Skip if already in CRDT
                if workspace_crdt.get_file(&rel_path).is_some() {
                    continue;
                }

                // Read and parse the file
                let content = match fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                let parsed = match diaryx_core::frontmatter::parse_or_empty(&content) {
                    Ok(p) => p,
                    Err(_) => continue,
                };

                // Extract metadata from frontmatter
                let fm = &parsed.frontmatter;
                let filename = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();

                let metadata = FileMetadata {
                    filename,
                    title: fm.get("title").and_then(|v| v.as_str()).map(String::from),
                    part_of: fm.get("part_of").and_then(|v| v.as_str()).map(String::from),
                    contents: fm.get("contents").and_then(|v| {
                        v.as_sequence().map(|seq| {
                            seq.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                    }),
                    attachments: vec![],
                    deleted: false,
                    audience: fm.get("audience").and_then(|v| {
                        v.as_sequence().map(|seq| {
                            seq.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                    }),
                    description: fm
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    extra: std::collections::HashMap::new(),
                    modified_at: chrono::Utc::now().timestamp_millis(),
                };

                // Add to workspace CRDT
                if workspace_crdt.set_file(&rel_path, metadata).is_ok() {
                    // Also initialize body doc with content
                    let body_doc = body_manager.get_or_create(&rel_path);
                    let _ = body_doc.set_body(&parsed.body);
                    *imported += 1;

                    if *imported % 10 == 0 {
                        print!("\r\x1b[K  Importing local files... {}", imported);
                        use std::io::Write;
                        let _ = std::io::stdout().flush();
                    }
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
        println!("\r\x1b[K  Imported {} local files into CRDT", imported);
    }

    imported
}

/// Control message from the sync server (JSON over WebSocket text frames).
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ControlMessage {
    /// Sync progress update from server
    SyncProgress { completed: usize, total: usize },
    /// Initial sync has completed
    SyncComplete { files_synced: usize },
    /// A peer joined the sync session
    PeerJoined {
        #[serde(default)]
        peer_count: usize,
    },
    /// A peer left the sync session
    PeerLeft {
        #[serde(default)]
        peer_count: usize,
    },
    /// Catch-all for other message types
    #[serde(other)]
    Other,
}

/// Handle the start command - start continuous sync.
pub fn handle_start(config: &Config, workspace_root: &Path) {
    // Validate configuration
    let Some(session_token) = &config.sync_session_token else {
        eprintln!("Not logged in. Please log in first:");
        eprintln!("  diaryx sync login <your-email>");
        return;
    };

    let server_url = config
        .sync_server_url
        .as_deref()
        .unwrap_or(DEFAULT_SYNC_SERVER);

    let workspace_id = config.sync_workspace_id.as_deref().unwrap_or_else(|| {
        // Generate a new workspace ID if not set
        // In a real implementation, this would be assigned by the server
        "default"
    });

    println!("Starting sync...");
    println!("  Server: {}", server_url);
    println!("  Workspace: {}", workspace_id);
    println!("  Local path: {}", workspace_root.display());
    println!();

    // Initialize CRDT storage
    let crdt_dir = workspace_root.join(".diaryx");
    if !crdt_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&crdt_dir) {
            eprintln!("Failed to create .diaryx directory: {}", e);
            return;
        }
    }

    let crdt_db = crdt_dir.join("crdt.db");
    let storage: Arc<dyn CrdtStorage> = match SqliteStorage::open(&crdt_db) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("Failed to open CRDT database: {}", e);
            return;
        }
    };

    // Create CRDT components
    let workspace_crdt = Arc::new(
        WorkspaceCrdt::load(Arc::clone(&storage))
            .unwrap_or_else(|_| WorkspaceCrdt::new(storage.clone())),
    );
    let body_manager = Arc::new(BodyDocManager::new(Arc::clone(&storage)));

    // Check if CRDT is empty and import existing files
    let existing_files = workspace_crdt.list_files();
    if existing_files.is_empty() {
        println!("  Scanning local files...");
        progress::show_indeterminate();
        let imported = import_existing_files(workspace_root, &workspace_crdt, &body_manager);
        if imported > 0 {
            println!("  Ready to sync {} files", imported);
        }
    } else {
        println!("  CRDT has {} files tracked", existing_files.len());
    }

    let fs = SyncToAsyncFs::new(RealFileSystem);
    let sync_handler = Arc::new(SyncHandler::new(fs));
    sync_handler.set_workspace_root(workspace_root.to_path_buf());

    let sync_manager = Arc::new(RustSyncManager::new(
        Arc::clone(&workspace_crdt),
        Arc::clone(&body_manager),
        Arc::clone(&sync_handler),
    ));

    // Build WebSocket URLs
    let ws_server = server_url
        .replace("https://", "wss://")
        .replace("http://", "ws://");

    let metadata_url = format!(
        "{}/sync?doc={}&token={}",
        ws_server, workspace_id, session_token
    );

    let body_url = format!(
        "{}/sync?doc={}&multiplexed=true&token={}",
        ws_server, workspace_id, session_token
    );

    // Set up shutdown flag
    let running = Arc::new(AtomicBool::new(true));

    // Ensure progress bar is cleared on exit
    let _progress_guard = progress::ProgressGuard::new();

    // Show connecting state
    progress::show_indeterminate();

    // Run the sync loop
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

    runtime.block_on(async {
        let running_clone = running.clone();

        // Set up Ctrl+C handler inside the async context
        tokio::spawn(async move {
            match tokio::signal::ctrl_c().await {
                Ok(()) => {
                    println!("\nShutting down sync...");
                    progress::hide();
                    running_clone.store(false, Ordering::SeqCst);
                }
                Err(e) => {
                    eprintln!("Failed to listen for Ctrl+C: {}", e);
                }
            }
        });

        run_sync_loop(
            &metadata_url,
            &body_url,
            sync_manager,
            workspace_crdt,
            running,
        )
        .await;
    });

    println!("Sync stopped.");
}

/// Handle the push command - one-shot push of local changes.
pub fn handle_push(config: &Config, workspace_root: &Path) {
    let Some(session_token) = &config.sync_session_token else {
        eprintln!("Not logged in. Please log in first:");
        eprintln!("  diaryx sync login <your-email>");
        return;
    };

    let server_url = config
        .sync_server_url
        .as_deref()
        .unwrap_or(DEFAULT_SYNC_SERVER);

    let workspace_id = config.sync_workspace_id.as_deref().unwrap_or("default");

    println!("Pushing local changes...");

    // Initialize CRDT storage (create directory if needed)
    let crdt_dir = workspace_root.join(".diaryx");
    if !crdt_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&crdt_dir) {
            eprintln!("Failed to create .diaryx directory: {}", e);
            return;
        }
    }

    let crdt_db = crdt_dir.join("crdt.db");
    let storage: Arc<dyn CrdtStorage> = match SqliteStorage::open(&crdt_db) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("Failed to open CRDT database: {}", e);
            return;
        }
    };

    let workspace_crdt = Arc::new(
        WorkspaceCrdt::load(Arc::clone(&storage))
            .unwrap_or_else(|_| WorkspaceCrdt::new(storage.clone())),
    );
    let body_manager = Arc::new(BodyDocManager::new(Arc::clone(&storage)));

    // Import existing files if CRDT is empty
    let existing_files = workspace_crdt.list_files();
    if existing_files.is_empty() {
        println!("  Scanning local files...");
        let imported = import_existing_files(workspace_root, &workspace_crdt, &body_manager);
        if imported == 0 {
            println!("No files found to push.");
            return;
        }
        println!("  Found {} files to push", imported);
    } else {
        println!("  {} files in local CRDT", existing_files.len());
    }

    let ws_server = server_url
        .replace("https://", "wss://")
        .replace("http://", "ws://");

    let metadata_url = format!(
        "{}/sync?doc={}&token={}",
        ws_server, workspace_id, session_token
    );

    let body_url = format!(
        "{}/sync?doc={}&multiplexed=true&token={}",
        ws_server, workspace_id, session_token
    );

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

    runtime.block_on(async {
        // Connect and push metadata
        match do_one_shot_sync(&metadata_url, &workspace_crdt, false).await {
            Ok(0) => println!("  Metadata already up to date"),
            Ok(_) => println!("  Pushed metadata"),
            Err(e) => eprintln!("  Failed to push metadata: {}", e),
        }

        // Connect and push bodies
        match do_one_shot_body_sync(&body_url, &workspace_crdt, &body_manager, false).await {
            Ok(0) => println!("  Body content already up to date"),
            Ok(count) => println!("  Pushed {} file bodies", count),
            Err(e) => eprintln!("  Failed to push bodies: {}", e),
        }
    });

    println!("Push complete.");
}

/// Handle the pull command - one-shot pull of remote changes.
pub fn handle_pull(config: &Config, workspace_root: &Path) {
    let Some(session_token) = &config.sync_session_token else {
        eprintln!("Not logged in. Please log in first:");
        eprintln!("  diaryx sync login <your-email>");
        return;
    };

    let server_url = config
        .sync_server_url
        .as_deref()
        .unwrap_or(DEFAULT_SYNC_SERVER);

    let workspace_id = config.sync_workspace_id.as_deref().unwrap_or("default");

    println!("Pulling remote changes...");

    // Initialize CRDT storage
    let crdt_dir = workspace_root.join(".diaryx");
    if !crdt_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&crdt_dir) {
            eprintln!("Failed to create .diaryx directory: {}", e);
            return;
        }
    }

    let crdt_db = crdt_dir.join("crdt.db");
    let storage: Arc<dyn CrdtStorage> = match SqliteStorage::open(&crdt_db) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("Failed to open CRDT database: {}", e);
            return;
        }
    };

    let workspace_crdt = Arc::new(
        WorkspaceCrdt::load(Arc::clone(&storage))
            .unwrap_or_else(|_| WorkspaceCrdt::new(storage.clone())),
    );
    let body_manager = Arc::new(BodyDocManager::new(Arc::clone(&storage)));
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let sync_handler = Arc::new(SyncHandler::new(fs));
    sync_handler.set_workspace_root(workspace_root.to_path_buf());

    let ws_server = server_url
        .replace("https://", "wss://")
        .replace("http://", "ws://");

    let metadata_url = format!(
        "{}/sync?doc={}&token={}",
        ws_server, workspace_id, session_token
    );

    let body_url = format!(
        "{}/sync?doc={}&multiplexed=true&token={}",
        ws_server, workspace_id, session_token
    );

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

    runtime.block_on(async {
        // Connect and pull metadata
        match do_one_shot_sync(&metadata_url, &workspace_crdt, true).await {
            Ok(count) => {
                if count == 0 {
                    println!("  Metadata already up to date");
                } else {
                    println!("  Received metadata for {} files", count);
                }

                // Write updated files to disk
                // list_files() returns Vec<(String, FileMetadata)>
                let files = workspace_crdt.list_files();

                if !files.is_empty() {
                    let body_mgr_ref = Some(body_manager.as_ref());
                    if let Err(e) = sync_handler
                        .handle_remote_metadata_update(files, vec![], body_mgr_ref, true)
                        .await
                    {
                        eprintln!("  Warning: Failed to write some files: {}", e);
                    }
                }
            }
            Err(e) => eprintln!("  Failed to pull metadata: {}", e),
        }

        // Connect and pull bodies
        match do_one_shot_body_sync(&body_url, &workspace_crdt, &body_manager, true).await {
            Ok(0) => println!("  Body content already up to date"),
            Ok(count) => println!("  Received {} file bodies", count),
            Err(e) => eprintln!("  Failed to pull bodies: {}", e),
        }
    });

    println!("Pull complete.");
}

/// Run the main sync loop with two WebSocket connections.
async fn run_sync_loop(
    metadata_url: &str,
    body_url: &str,
    sync_manager: Arc<RustSyncManager<SyncToAsyncFs<RealFileSystem>>>,
    workspace_crdt: Arc<WorkspaceCrdt>,
    running: Arc<AtomicBool>,
) {
    println!("Connecting to sync server...");
    progress::show_progress(10); // 10% - starting connection

    // Connect to metadata WebSocket
    let metadata_ws = match connect_async(metadata_url).await {
        Ok((ws, _)) => {
            println!("Connected to metadata sync");
            progress::show_progress(30); // 30% - metadata connected
            Some(ws)
        }
        Err(e) => {
            eprintln!("Failed to connect to metadata sync: {}", e);
            progress::show_error(30);
            None
        }
    };

    // Connect to body WebSocket
    let body_ws = match connect_async(body_url).await {
        Ok((ws, _)) => {
            println!("Connected to body sync");
            progress::show_progress(50); // 50% - both connected
            Some(ws)
        }
        Err(e) => {
            eprintln!("Failed to connect to body sync: {}", e);
            progress::show_error(50);
            None
        }
    };

    if metadata_ws.is_none() && body_ws.is_none() {
        eprintln!("No connections established. Exiting.");
        progress::show_error(100);
        return;
    }

    println!();
    println!("Sync is running. Press Ctrl+C to stop.");
    println!();

    // Track initial sync completion for progress
    let metadata_synced = Arc::new(AtomicBool::new(false));
    let body_synced = Arc::new(AtomicBool::new(false));

    // Track when connections are closed so we can exit
    let metadata_done = Arc::new(AtomicBool::new(metadata_ws.is_none()));
    let body_done = Arc::new(AtomicBool::new(body_ws.is_none()));

    // Spawn metadata WebSocket handler
    let metadata_handle = if let Some(mut ws) = metadata_ws {
        let running_clone = running.clone();
        let sync_manager_clone = Arc::clone(&sync_manager);
        let metadata_synced_clone = Arc::clone(&metadata_synced);
        let body_synced_clone = Arc::clone(&body_synced);
        let metadata_done_clone = Arc::clone(&metadata_done);

        Some(tokio::spawn(async move {
            // Send SyncStep1
            let step1 = sync_manager_clone.create_workspace_sync_step1();
            if let Err(e) = ws.send(Message::Binary(step1.into())).await {
                eprintln!("Failed to send metadata SyncStep1: {}", e);
                metadata_done_clone.store(true, Ordering::SeqCst);
                return;
            }

            while running_clone.load(Ordering::SeqCst) {
                tokio::select! {
                    msg = ws.next() => {
                        match msg {
                            Some(Ok(Message::Binary(data))) => {
                                match sync_manager_clone.handle_workspace_message(&data, true).await {
                                    Ok(result) => {
                                        if let Some(response) = result.response {
                                            if let Err(e) = ws.send(Message::Binary(response.into())).await {
                                                eprintln!("Failed to send metadata response: {}", e);
                                            }
                                        }
                                        // Log synced files (SyncComplete from server will handle progress)
                                        if !result.changed_files.is_empty() {
                                            for file in &result.changed_files {
                                                println!("  Synced: {}", file);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("Error handling metadata message: {}", e);
                                    }
                                }
                            }
                            Some(Ok(Message::Text(text))) => {
                                // Handle JSON control messages from server
                                if let Ok(ctrl_msg) = serde_json::from_str::<ControlMessage>(&text) {
                                    match ctrl_msg {
                                        ControlMessage::SyncProgress { completed, total } => {
                                            if total > 0 {
                                                let percent = ((completed as f64 / total as f64) * 100.0) as u8;
                                                // Scale to 50-100% range (first 50% is connection)
                                                let scaled = 50 + (percent / 2);
                                                progress::show_progress(scaled);
                                                // Clear line and print progress
                                                print!("\r\x1b[K  Metadata progress: {}/{} files ({}%)", completed, total, percent);
                                                use std::io::Write;
                                                let _ = std::io::stdout().flush();
                                            }
                                        }
                                        ControlMessage::SyncComplete { files_synced } => {
                                            metadata_synced_clone.store(true, Ordering::SeqCst);
                                            println!("\r\x1b[K  Metadata sync complete ({} files)", files_synced);
                                            // Update progress based on body sync state
                                            if body_synced_clone.load(Ordering::SeqCst) {
                                                progress::show_progress(100);
                                                println!("Sync complete. Watching for changes...");
                                                progress::show_indeterminate();
                                            } else {
                                                progress::show_progress(75);
                                            }
                                        }
                                        ControlMessage::PeerJoined { peer_count } => {
                                            println!("\r\x1b[K  Peer joined ({} connected)", peer_count);
                                        }
                                        ControlMessage::PeerLeft { peer_count } => {
                                            println!("\r\x1b[K  Peer left ({} connected)", peer_count);
                                        }
                                        ControlMessage::Other => {}
                                    }
                                }
                            }
                            Some(Ok(Message::Close(_))) => {
                                println!("\r\x1b[KMetadata connection closed by server");
                                break;
                            }
                            Some(Ok(Message::Pong(_))) => {
                                // Pong received, connection is alive
                            }
                            Some(Err(e)) => {
                                eprintln!("\r\x1b[KMetadata WebSocket error: {}", e);
                                break;
                            }
                            None => break,
                            _ => {}
                        }
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                        // Send ping to keep connection alive
                        if let Err(e) = ws.send(Message::Ping(vec![].into())).await {
                            eprintln!("Failed to send ping: {}", e);
                            break;
                        }
                    }
                }
            }

            // Close connection gracefully
            let _ = ws.close(None).await;
            metadata_done_clone.store(true, Ordering::SeqCst);
        }))
    } else {
        None
    };

    // Spawn body WebSocket handler
    let body_handle = if let Some(mut ws) = body_ws {
        let running_clone = running.clone();
        let sync_manager_clone = Arc::clone(&sync_manager);
        let workspace_crdt_clone = Arc::clone(&workspace_crdt);
        let metadata_synced_clone = Arc::clone(&metadata_synced);
        let body_synced_clone = Arc::clone(&body_synced);
        let body_done_clone = Arc::clone(&body_done);

        Some(tokio::spawn(async move {
            // Send SyncStep1 for all known files
            // list_files() returns Vec<(String, FileMetadata)>
            let files = workspace_crdt_clone.list_files();
            let file_count = files.len();
            let mut sent = 0;

            // Send in batches to avoid overwhelming the server
            const BATCH_SIZE: usize = 50;
            for (file_path, _metadata) in files {
                if !running_clone.load(Ordering::SeqCst) {
                    break;
                }
                let step1 = sync_manager_clone.create_body_sync_step1(&file_path);
                let framed = frame_body_message(&file_path, &step1);
                if let Err(e) = ws.send(Message::Binary(framed.into())).await {
                    eprintln!("Failed to send body SyncStep1 for {}: {}", file_path, e);
                }
                sent += 1;

                // Rate limit: small delay every BATCH_SIZE messages
                if sent % BATCH_SIZE == 0 {
                    print!(
                        "\r\x1b[K  Sending body state: {}/{} files",
                        sent, file_count
                    );
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                }
            }

            if sent > 0 {
                println!("\r\x1b[K  Sent body state for {} files", sent);
            }

            // If no files to sync, mark as complete immediately
            if file_count == 0 && !body_synced_clone.load(Ordering::SeqCst) {
                body_synced_clone.store(true, Ordering::SeqCst);
                println!("  No body content to sync");
                if metadata_synced_clone.load(Ordering::SeqCst) {
                    progress::show_progress(100);
                    println!("Sync complete. Watching for changes...");
                    progress::show_indeterminate();
                }
            }

            // Track body responses received for progress
            let mut body_responses_received = 0usize;
            let mut last_body_progress_shown = 0usize;

            while running_clone.load(Ordering::SeqCst) {
                tokio::select! {
                    msg = ws.next() => {
                        match msg {
                            Some(Ok(Message::Binary(data))) => {
                                // Unframe the multiplexed message
                                if let Some((file_path, body_msg)) = unframe_body_message(&data) {
                                    match sync_manager_clone.handle_body_message(&file_path, &body_msg, true).await {
                                        Ok(result) => {
                                            if let Some(response) = result.response {
                                                let framed = frame_body_message(&file_path, &response);
                                                if let Err(e) = ws.send(Message::Binary(framed.into())).await {
                                                    eprintln!("Failed to send body response: {}", e);
                                                }
                                            }

                                            // Track and show progress for received responses
                                            body_responses_received += 1;
                                            let progress_interval = (file_count / 20).max(50).min(200);
                                            if body_responses_received - last_body_progress_shown >= progress_interval {
                                                last_body_progress_shown = body_responses_received;
                                                let percent = if file_count > 0 {
                                                    (body_responses_received * 100 / file_count).min(100)
                                                } else {
                                                    100
                                                };
                                                print!("\r\x1b[K  Receiving body data: {}/{} ({}%)", body_responses_received, file_count, percent);
                                                use std::io::Write;
                                                let _ = std::io::stdout().flush();
                                                progress::show_progress((50 + percent / 2) as u8);
                                            }

                                            if result.content.is_some() && !result.is_echo {
                                                println!("\r\x1b[K  Body synced: {}", file_path);
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("Error handling body message for {}: {}", file_path, e);
                                        }
                                    }
                                }
                            }
                            Some(Ok(Message::Text(text))) => {
                                // Handle JSON control messages from server
                                if let Ok(ctrl_msg) = serde_json::from_str::<ControlMessage>(&text) {
                                    match ctrl_msg {
                                        ControlMessage::SyncProgress { completed, total } => {
                                            if total > 0 {
                                                let percent = ((completed as f64 / total as f64) * 100.0) as u8;
                                                // Scale to 50-100% range
                                                let scaled = 50 + (percent / 2);
                                                progress::show_progress(scaled);
                                                print!("\r\x1b[K  Body progress: {}/{} files ({}%)", completed, total, percent);
                                                use std::io::Write;
                                                let _ = std::io::stdout().flush();
                                            }
                                        }
                                        ControlMessage::SyncComplete { files_synced } => {
                                            body_synced_clone.store(true, Ordering::SeqCst);
                                            println!("\r\x1b[K  Body sync complete ({} files)", files_synced);
                                            // Update progress based on metadata sync state
                                            if metadata_synced_clone.load(Ordering::SeqCst) {
                                                progress::show_progress(100);
                                                println!("Sync complete. Watching for changes...");
                                                progress::show_indeterminate();
                                            } else {
                                                progress::show_progress(85);
                                            }
                                        }
                                        ControlMessage::PeerJoined { .. } | ControlMessage::PeerLeft { .. } => {
                                            // Handled by metadata connection
                                        }
                                        ControlMessage::Other => {}
                                    }
                                }
                            }
                            Some(Ok(Message::Close(_))) => {
                                println!("\r\x1b[KBody connection closed by server");
                                break;
                            }
                            Some(Ok(Message::Pong(_))) => {
                                // Pong received, connection is alive
                            }
                            Some(Err(e)) => {
                                eprintln!("\r\x1b[KBody WebSocket error: {}", e);
                                break;
                            }
                            None => break,
                            _ => {}
                        }
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                        // Send ping to keep connection alive
                        if let Err(e) = ws.send(Message::Ping(vec![].into())).await {
                            eprintln!("Failed to send ping: {}", e);
                            break;
                        }
                    }
                }
            }

            // Close connection gracefully
            let _ = ws.close(None).await;
            body_done_clone.store(true, Ordering::SeqCst);
        }))
    } else {
        None
    };

    // Wait for shutdown signal OR both connections to close
    while running.load(Ordering::SeqCst) {
        // Check if both connections have closed
        if metadata_done.load(Ordering::SeqCst) && body_done.load(Ordering::SeqCst) {
            println!("\nBoth connections closed. Exiting sync.");
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Signal tasks to stop
    running.store(false, Ordering::SeqCst);

    // Wait for tasks to complete
    if let Some(handle) = metadata_handle {
        let _ = handle.await;
    }
    if let Some(handle) = body_handle {
        let _ = handle.await;
    }
}

/// Perform one-shot metadata sync.
async fn do_one_shot_sync(
    url: &str,
    workspace_crdt: &WorkspaceCrdt,
    pull: bool,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let (mut ws, _) = connect_async(url).await?;

    // Send our state vector
    let sv = workspace_crdt.encode_state_vector();
    let step1 = SyncMessage::SyncStep1(sv).encode();
    ws.send(Message::Binary(step1.into())).await?;

    let mut push_count = 0;
    let mut pull_count = 0;
    let mut sent_step2 = false;
    let mut received_step2 = false;

    // Receive and process messages until sync is complete
    let timeout = tokio::time::Duration::from_secs(10);
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        tokio::select! {
            biased;

            msg = ws.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        let messages = SyncMessage::decode_all(&data)?;
                        for sync_msg in messages {
                            match sync_msg {
                                SyncMessage::SyncStep1(remote_sv) => {
                                    // Send our diff to the server
                                    let diff = workspace_crdt.encode_diff(&remote_sv)?;
                                    // Y.js empty update is 2 bytes (header only)
                                    if diff.len() > 2 {
                                        // Mark that we pushed data
                                        push_count = 1;
                                    }
                                    let step2 = SyncMessage::SyncStep2(diff).encode();
                                    ws.send(Message::Binary(step2.into())).await?;
                                    sent_step2 = true;
                                }
                                SyncMessage::SyncStep2(update) | SyncMessage::Update(update) => {
                                    received_step2 = true;
                                    // Y.js empty update is 2 bytes (header only)
                                    if update.len() > 2 {
                                        // Always apply incoming updates to stay in sync
                                        let (_, changed_files, _) = workspace_crdt
                                            .apply_update_tracking_changes(&update, diaryx_core::crdt::UpdateOrigin::Sync)?;
                                        pull_count += changed_files.len();
                                    }
                                }
                            }
                        }

                        // Exit after bidirectional sync is complete
                        if sent_step2 && received_step2 {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(e)) => return Err(e.into()),
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                break;
            }
        }
    }

    ws.close(None).await?;
    // Return the relevant count based on mode
    Ok(if pull { pull_count } else { push_count })
}

/// Perform one-shot body sync.
async fn do_one_shot_body_sync(
    url: &str,
    workspace_crdt: &WorkspaceCrdt,
    body_manager: &BodyDocManager,
    pull: bool,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    use std::collections::HashSet;

    let (mut ws, _) = connect_async(url).await?;

    // Get list of files to sync
    let files: Vec<String> = workspace_crdt
        .list_files()
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    let file_count = files.len();

    if file_count == 0 {
        ws.close(None).await?;
        return Ok(0);
    }

    // Send SyncStep1 for all files
    for file_path in &files {
        let sv = body_manager
            .get_sync_state(file_path)
            .unwrap_or_else(Vec::new);
        let step1 = SyncMessage::SyncStep1(sv).encode();
        let framed = frame_body_message(file_path, &step1);
        ws.send(Message::Binary(framed.into())).await?;
    }

    let mut push_count = 0;
    let mut pull_count = 0;
    let mut files_sent_step2: HashSet<String> = HashSet::new();
    let mut files_received_step2: HashSet<String> = HashSet::new();

    // Timeout based on file count - give more time for larger workspaces
    let timeout_secs = (10 + file_count / 100).min(60) as u64;
    let timeout = tokio::time::Duration::from_secs(timeout_secs);
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        tokio::select! {
            biased;

            msg = ws.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        if let Some((file_path, body_msg)) = unframe_body_message(&data) {
                            let messages = SyncMessage::decode_all(&body_msg)?;
                            for sync_msg in messages {
                                match sync_msg {
                                    SyncMessage::SyncStep1(remote_sv) => {
                                        // Server is asking for our diff
                                        let diff = body_manager.get_diff(&file_path, &remote_sv)?;

                                        // Y.js empty update is 2 bytes (header only, no operations)
                                        // Treat ≤2 bytes as "no actual changes"
                                        let has_changes = diff.len() > 2;

                                        if has_changes {
                                            // Count files where we actually have data to push
                                            push_count += 1;
                                        }
                                        let step2 = SyncMessage::SyncStep2(diff).encode();
                                        let framed = frame_body_message(&file_path, &step2);
                                        ws.send(Message::Binary(framed.into())).await?;
                                        files_sent_step2.insert(file_path.clone());
                                    }
                                    SyncMessage::SyncStep2(update) | SyncMessage::Update(update) => {
                                        files_received_step2.insert(file_path.clone());
                                        // Y.js empty update is 2 bytes (header only)
                                        let has_changes = update.len() > 2;
                                        if has_changes {
                                            // Always apply incoming updates to stay in sync
                                            let body_doc = body_manager.get_or_create(&file_path);
                                            body_doc.apply_update(&update, diaryx_core::crdt::UpdateOrigin::Sync)?;
                                            pull_count += 1;
                                        }
                                    }
                                }
                            }
                        }

                        // Exit after bidirectional sync is complete for all files
                        if files_sent_step2.len() >= file_count && files_received_step2.len() >= file_count {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(e)) => return Err(e.into()),
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                break;
            }
        }
    }

    ws.close(None).await?;
    // Return the relevant count based on mode
    Ok(if pull { pull_count } else { push_count })
}
