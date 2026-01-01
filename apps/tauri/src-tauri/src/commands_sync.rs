//! Live sync commands for Tauri

use crate::sync::WebSocketSyncProvider;
use diaryx_core::fs::RealFileSystem;
use diaryx_core::sync_crdt::{SyncManager, SyncStats};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Global sync manager state
pub struct SyncState {
    pub manager: Option<SyncManager>,
}

impl SyncState {
    pub fn new() -> Self {
        Self { manager: None }
    }
}

/// Status information for sync
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncStatusResponse {
    pub connected: bool,
    pub document_count: usize,
}

/// Start live sync for a workspace
#[tauri::command]
pub fn start_live_sync(
    server_url: String,
    workspace_path: String,
    workspace_id: String,
    state: tauri::State<'_, Arc<Mutex<SyncState>>>,
) -> Result<String, String> {
    log::info!("[Sync] Starting live sync: server={}, workspace={}, id={}", 
        server_url, workspace_path, workspace_id);

    let mut sync_state = state.lock().map_err(|e| format!("Lock error: {}", e))?;

    // Check if already running
    if sync_state.manager.is_some() {
        return Err("Sync already running. Stop it first.".to_string());
    }

    // Create WebSocket provider
    let provider = WebSocketSyncProvider::new(server_url.clone(), workspace_id.clone())
        .map_err(|e| format!("Failed to create sync provider: {}", e))?;

    // Create sync manager
    let filesystem = Arc::new(RealFileSystem);
    let path = PathBuf::from(&workspace_path);
    
    let mut manager = SyncManager::new(Box::new(provider), filesystem, path);

    // Start syncing
    manager
        .start()
        .map_err(|e| format!("Failed to start sync: {}", e))?;

    log::info!("[Sync] Sync manager started, {} documents loaded", manager.document_count());

    let doc_count = manager.document_count();
    sync_state.manager = Some(manager);

    Ok(format!("Live sync started: {} documents syncing to {}", doc_count, server_url))
}

/// Stop live sync
#[tauri::command]
pub fn stop_live_sync(
    state: tauri::State<'_, Arc<Mutex<SyncState>>>,
) -> Result<String, String> {
    log::info!("[Sync] Stopping live sync");

    let mut sync_state = state.lock().map_err(|e| format!("Lock error: {}", e))?;

    if let Some(mut manager) = sync_state.manager.take() {
        manager
            .stop()
            .map_err(|e| format!("Failed to stop sync: {}", e))?;
        log::info!("[Sync] Sync stopped");
        Ok("Sync stopped".to_string())
    } else {
        Err("Sync not running".to_string())
    }
}

/// Get sync status
#[tauri::command]
pub fn get_sync_status(
    state: tauri::State<'_, Arc<Mutex<SyncState>>>,
) -> Result<SyncStatusResponse, String> {
    let sync_state = state.lock().map_err(|e| format!("Lock error: {}", e))?;

    if let Some(ref manager) = sync_state.manager {
        Ok(SyncStatusResponse {
            connected: true,
            document_count: manager.document_count(),
        })
    } else {
        Ok(SyncStatusResponse {
            connected: false,
            document_count: 0,
        })
    }
}

/// Perform a single sync round (should be called periodically by frontend)
#[tauri::command]
pub fn sync_round(
    state: tauri::State<'_, Arc<Mutex<SyncState>>>,
) -> Result<SyncStats, String> {
    let mut sync_state = state.lock().map_err(|e| format!("Lock error: {}", e))?;

    if let Some(ref mut manager) = sync_state.manager {
        let stats = manager
            .sync_round()
            .map_err(|e| format!("Sync round failed: {}", e))?;

        if stats.messages_received > 0 || stats.messages_sent > 0 {
            log::debug!(
                "[Sync] Round complete: {} sent, {} recv, {} updated",
                stats.messages_sent,
                stats.messages_received,
                stats.documents_updated
            );
        }

        Ok(stats)
    } else {
        Err("Sync not running".to_string())
    }
}

/// Update a document with local changes (call this when user edits a file)
#[tauri::command]
pub fn update_synced_document(
    file_path: String,
    content: String,
    state: tauri::State<'_, Arc<Mutex<SyncState>>>,
) -> Result<(), String> {
    let mut sync_state = state.lock().map_err(|e| format!("Lock error: {}", e))?;

    if let Some(ref mut manager) = sync_state.manager {
        let path = PathBuf::from(file_path);
        manager
            .update_document(&path, content)
            .map_err(|e| format!("Failed to update document: {}", e))?;
        Ok(())
    } else {
        Err("Sync not running".to_string())
    }
}
