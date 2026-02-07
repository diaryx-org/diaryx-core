//! Custom hooks for siphonophore integration.
//!
//! These hooks implement the `siphonophore::Hook` trait to provide:
//! - JWT authentication and session validation
//! - SQLite-based document persistence
//! - Change event handling

use async_trait::async_trait;
use diaryx_core::crdt::{CrdtStorage, UpdateOrigin};
use siphonophore::Handle;
use siphonophore::{
    BeforeCloseDirtyPayload, BeforeSyncAction, ControlMessageResponse, Hook, HookResult,
    OnAuthenticatePayload, OnBeforeSyncPayload, OnChangePayload, OnConnectPayload,
    OnControlMessagePayload, OnDisconnectPayload, OnLoadDocumentPayload, OnPeerJoinedPayload,
    OnPeerLeftPayload, OnSavePayload,
};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::auth::validate_token;
use crate::db::AuthRepo;

use super::store::StorageCache;

/// User information stored in the connection context after authentication.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct AuthenticatedUser {
    pub user_id: String,
    pub workspace_id: String,
    pub device_id: Option<String>,
    pub is_guest: bool,
    pub read_only: bool,
}

/// Document type determined from doc_id prefix.
#[derive(Debug, Clone, PartialEq)]
pub enum DocType {
    /// Workspace metadata CRDT (workspace:<id>)
    Workspace(String),
    /// Body document CRDT (body:<workspace_id>/<path>)
    Body { workspace_id: String, path: String },
}

impl DocType {
    /// Parse a doc_id into a DocType.
    pub fn parse(doc_id: &str) -> Option<Self> {
        if let Some(workspace_id) = doc_id.strip_prefix("workspace:") {
            Some(DocType::Workspace(workspace_id.to_string()))
        } else if let Some(rest) = doc_id.strip_prefix("body:") {
            // Format: body:<workspace_id>/<path>
            let (workspace_id, path) = rest.split_once('/')?;
            Some(DocType::Body {
                workspace_id: workspace_id.to_string(),
                path: path.to_string(),
            })
        } else {
            // Legacy format: just workspace_id (treat as workspace doc)
            Some(DocType::Workspace(doc_id.to_string()))
        }
    }

    /// Get the workspace_id for this document.
    pub fn workspace_id(&self) -> &str {
        match self {
            DocType::Workspace(id) => id,
            DocType::Body { workspace_id, .. } => workspace_id,
        }
    }

    /// Get the storage key for this document.
    pub fn storage_key(&self) -> String {
        match self {
            DocType::Workspace(id) => format!("workspace:{}", id),
            DocType::Body { workspace_id, path } => format!("body:{}/{}", workspace_id, path),
        }
    }
}

/// Diaryx hook implementation for siphonophore.
///
/// This hook provides:
/// - JWT authentication for authenticated users
/// - Session code validation for guests
/// - SQLite persistence for CRDT documents
pub struct DiaryxHook {
    /// Auth repository for token validation.
    repo: Arc<AuthRepo>,
    /// Shared storage cache (also used by WorkspaceStore for HTTP API operations).
    storage_cache: Arc<StorageCache>,
    /// Handle for broadcasting messages to connected clients.
    /// Set via `OnceLock` after server construction (hook is created before server).
    handle: Arc<OnceLock<Handle>>,
    /// Shared session-to-workspace mapping (also used by SyncV2State for peer counts).
    session_to_workspace: Arc<RwLock<HashMap<String, String>>>,
}

impl DiaryxHook {
    /// Create a new DiaryxHook.
    ///
    /// Returns the hook and a shared `OnceLock` that must be populated with the
    /// server `Handle` after `Server::with_hooks()` is called.
    pub fn new(
        repo: Arc<AuthRepo>,
        storage_cache: Arc<StorageCache>,
        session_to_workspace: Arc<RwLock<HashMap<String, String>>>,
    ) -> (Self, Arc<OnceLock<Handle>>) {
        let handle = Arc::new(OnceLock::new());
        let hook = Self {
            repo,
            storage_cache,
            handle: handle.clone(),
            session_to_workspace,
        };
        (hook, handle)
    }

    /// Authenticate from JWT token.
    fn authenticate_token(
        &self,
        token: &str,
        doc_type: &DocType,
    ) -> Result<AuthenticatedUser, String> {
        let auth = validate_token(&self.repo, token).ok_or("Invalid or expired token")?;

        let workspace_id = doc_type.workspace_id();

        // Verify user has access to this workspace
        let workspaces = self
            .repo
            .get_user_workspaces(&auth.user.id)
            .unwrap_or_default();

        let has_access = workspaces
            .iter()
            .any(|w| w.id == workspace_id || w.name == workspace_id);

        // Allow access if user owns the workspace, or get/create default
        let workspace_id = if !has_access {
            self.repo
                .get_or_create_workspace(&auth.user.id, "default")
                .map_err(|e| format!("Failed to get/create workspace: {}", e))?
        } else {
            workspace_id.to_string()
        };

        Ok(AuthenticatedUser {
            user_id: auth.user.id,
            workspace_id,
            device_id: Some(auth.session.device_id),
            is_guest: false,
            read_only: false,
        })
    }

    /// Authenticate from session code (for guests).
    fn authenticate_session(
        &self,
        session_code: &str,
        guest_id: &str,
        doc_type: &DocType,
    ) -> Result<AuthenticatedUser, String> {
        let session_code = session_code.to_uppercase();

        let session = self
            .repo
            .get_share_session(&session_code)
            .map_err(|e| format!("Failed to get session: {}", e))?
            .ok_or("Session not found")?;

        // Verify this document belongs to the session's workspace
        if doc_type.workspace_id() != session.workspace_id {
            return Err("Document does not belong to session workspace".to_string());
        }

        Ok(AuthenticatedUser {
            user_id: format!("guest:{}", guest_id),
            workspace_id: session.workspace_id,
            device_id: None,
            is_guest: true,
            read_only: session.read_only,
        })
    }
}

#[async_trait]
impl Hook for DiaryxHook {
    /// Called when a client first tries to access a document.
    async fn on_connect(&self, payload: OnConnectPayload<'_>) -> HookResult {
        debug!(
            "Client {:?} connecting to document: {}",
            payload.client_id, payload.doc_id
        );
        Ok(())
    }

    /// Called to authenticate/authorize. Use `context.insert()` to store user info.
    async fn on_authenticate(&self, payload: OnAuthenticatePayload<'_>) -> HookResult {
        let doc_id = payload.doc_id;
        let request = payload.request;

        // Parse document type
        let doc_type = DocType::parse(doc_id)
            .ok_or_else(|| format!("Invalid document ID format: {}", doc_id))?;

        // Try JWT token first
        if let Some(token) = &request.token {
            match self.authenticate_token(token, &doc_type) {
                Ok(user) => {
                    info!("Authenticated user {} for doc {}", user.user_id, doc_id);
                    payload.context.insert(user);
                    return Ok(());
                }
                Err(e) => {
                    debug!("Token auth failed: {}", e);
                }
            }
        }

        // Try session code
        if let Some(session_code) = request.query_params.get("session") {
            let guest_id = request
                .query_params
                .get("guest_id")
                .cloned()
                .unwrap_or_else(|| format!("guest-{}", uuid::Uuid::new_v4()));

            match self.authenticate_session(session_code, &guest_id, &doc_type) {
                Ok(user) => {
                    info!(
                        "Authenticated guest {} for session {} doc {}",
                        guest_id, session_code, doc_id
                    );
                    // Register session-to-workspace mapping for peer count lookups
                    self.session_to_workspace
                        .write()
                        .await
                        .insert(session_code.to_uppercase(), user.workspace_id.clone());
                    payload.context.insert(user);
                    return Ok(());
                }
                Err(e) => {
                    warn!("Session auth failed for {}: {}", session_code, e);
                    return Err(e.into());
                }
            }
        }

        // No valid auth method
        warn!("No valid authentication for document: {}", doc_id);
        Err("Authentication required".into())
    }

    /// Called when a document is first loaded. Return `Some(bytes)` for persisted state.
    async fn on_load_document(
        &self,
        payload: OnLoadDocumentPayload<'_>,
    ) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
        let doc_id = payload.doc_id;
        debug!("Loading document: {}", doc_id);

        // Parse document type
        let doc_type = match DocType::parse(doc_id) {
            Some(dt) => dt,
            None => {
                warn!("Invalid document ID format: {}", doc_id);
                return Ok(None);
            }
        };

        // Get storage for this workspace
        let storage = match self.storage_cache.get_storage(doc_type.workspace_id()) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to get storage: {}", e);
                return Ok(None);
            }
        };

        // Load document state
        let storage_key = doc_type.storage_key();
        match storage.load_doc(&storage_key) {
            Ok(state) => {
                debug!(
                    "[on_load_document] doc={}, storage_key={}, has_state={}, state_len={}",
                    doc_id,
                    storage_key,
                    state.is_some(),
                    state.as_ref().map(|s| s.len()).unwrap_or(0)
                );
                Ok(state)
            }
            Err(e) => {
                error!("Failed to load document {}: {}", doc_id, e);
                Ok(None)
            }
        }
    }

    /// Called on every document change.
    async fn on_change(&self, payload: OnChangePayload<'_>) -> HookResult {
        let doc_id = payload.doc_id;
        let update = payload.update;

        debug!(
            "[on_change] doc={}, update_len={}, client={:?}",
            doc_id,
            update.len(),
            payload.client_id
        );

        // Parse document type
        let doc_type = match DocType::parse(doc_id) {
            Some(dt) => dt,
            None => {
                warn!("Invalid document ID on change: {}", doc_id);
                return Ok(());
            }
        };

        // Get user info from context
        let user = payload.context.get::<AuthenticatedUser>();
        let (device_id, device_name) = match user {
            Some(u) => (u.device_id.as_deref(), None),
            None => (None, None),
        };

        // Check read-only mode
        if let Some(u) = user {
            if u.read_only {
                debug!(
                    "Ignoring change from read-only user {} on {}",
                    u.user_id, doc_id
                );
                return Ok(());
            }
        }

        // Get storage
        let storage = match self.storage_cache.get_storage(doc_type.workspace_id()) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to get storage for change: {}", e);
                return Ok(());
            }
        };

        // Append update
        let storage_key = doc_type.storage_key();
        if let Err(e) = storage.append_update_with_device(
            &storage_key,
            update,
            UpdateOrigin::Remote,
            device_id,
            device_name,
        ) {
            error!("Failed to persist update for {}: {}", doc_id, e);
        } else {
            debug!("Persisted {} byte update for {}", update.len(), doc_id);
        }

        Ok(())
    }

    /// Called when a client disconnects from a document.
    async fn on_disconnect(&self, payload: OnDisconnectPayload<'_>) -> HookResult {
        debug!(
            "Client {:?} disconnected from document: {}",
            payload.client_id, payload.doc_id
        );
        Ok(())
    }

    /// Called on explicit save request.
    async fn on_save(&self, payload: OnSavePayload<'_>) -> HookResult {
        let doc_id = payload.doc_id;
        let state = payload.state;

        debug!("Saving document: {} ({} bytes)", doc_id, state.len());

        // Parse document type
        let doc_type = match DocType::parse(doc_id) {
            Some(dt) => dt,
            None => {
                warn!("Invalid document ID on save: {}", doc_id);
                return Ok(());
            }
        };

        // Get storage
        let storage = match self.storage_cache.get_storage(doc_type.workspace_id()) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to get storage for save: {}", e);
                return Err(e.into());
            }
        };

        // Save document
        let storage_key = doc_type.storage_key();
        storage.save_doc(&storage_key, state).map_err(|e| {
            error!("Failed to save document {}: {}", doc_id, e);
            format!("Save failed: {}", e)
        })?;

        info!("Saved document {} ({} bytes)", doc_id, state.len());
        Ok(())
    }

    /// Called before a dirty document is unloaded.
    async fn before_close_dirty(&self, payload: BeforeCloseDirtyPayload<'_>) -> HookResult {
        let doc_id = payload.doc_id;
        let state = payload.state;

        info!("Auto-saving dirty document before close: {}", doc_id);

        // Parse document type
        let doc_type = match DocType::parse(doc_id) {
            Some(dt) => dt,
            None => {
                warn!("Invalid document ID on close: {}", doc_id);
                return Ok(());
            }
        };

        // Get storage
        let storage = match self.storage_cache.get_storage(doc_type.workspace_id()) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to get storage for auto-save: {}", e);
                return Ok(());
            }
        };

        // Save document
        let storage_key = doc_type.storage_key();
        if let Err(e) = storage.save_doc(&storage_key, state) {
            error!("Failed to auto-save document {}: {}", doc_id, e);
        } else {
            info!(
                "Auto-saved document {} on close ({} bytes)",
                doc_id,
                state.len()
            );
        }

        Ok(())
    }

    /// Called after auth but before y-sync starts - for Files-Ready handshake.
    async fn on_before_sync(
        &self,
        payload: OnBeforeSyncPayload<'_>,
    ) -> Result<BeforeSyncAction, Box<dyn std::error::Error + Send + Sync>> {
        let doc_id = payload.doc_id;

        // Parse document type
        let doc_type = match DocType::parse(doc_id) {
            Some(dt) => dt,
            None => {
                return Ok(BeforeSyncAction::Abort {
                    reason: format!("Invalid document ID: {}", doc_id),
                });
            }
        };

        // Only workspace documents need Files-Ready handshake and session_joined
        if !matches!(doc_type, DocType::Workspace(_)) {
            return Ok(BeforeSyncAction::Continue);
        }

        let mut messages = Vec::new();

        // For session guests, send session_joined confirmation before anything else.
        // This is a unicast to the connecting client (on_before_sync runs per-client).
        if let Some(user) = payload.context.get::<AuthenticatedUser>() {
            if user.is_guest {
                if let Some(session_code) = payload.request.query_params.get("session") {
                    let session_joined = serde_json::json!({
                        "type": "session_joined",
                        "joinCode": session_code.to_uppercase(),
                        "workspaceId": user.workspace_id,
                        "readOnly": user.read_only,
                    });
                    messages.push(session_joined.to_string());
                    info!(
                        "Sending session_joined for guest on workspace {}",
                        user.workspace_id
                    );
                }
            }
        }

        // Get storage to generate file manifest
        let storage = match self.storage_cache.get_storage(doc_type.workspace_id()) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to get storage for before_sync: {}", e);
                if messages.is_empty() {
                    return Ok(BeforeSyncAction::Continue);
                }
                // For session guests with no storage, still send a file_manifest so the
                // client replies with FilesReady and the handshake completes.
                let manifest = serde_json::json!({
                    "type": "file_manifest",
                    "files": [],
                    "client_is_new": false
                });
                messages.push(manifest.to_string());
                return Ok(BeforeSyncAction::SendMessages { messages });
            }
        };

        // Query active files
        let files = match storage.query_active_files() {
            Ok(f) => f,
            Err(e) => {
                warn!("Failed to query files for manifest: {}", e);
                if messages.is_empty() {
                    return Ok(BeforeSyncAction::Continue);
                }
                let manifest = serde_json::json!({
                    "type": "file_manifest",
                    "files": [],
                    "client_is_new": false
                });
                messages.push(manifest.to_string());
                return Ok(BeforeSyncAction::SendMessages { messages });
            }
        };

        // If no files and no session messages, skip handshake
        if files.is_empty() && messages.is_empty() {
            debug!("No files in workspace, skipping Files-Ready handshake");
            return Ok(BeforeSyncAction::Continue);
        }

        // Generate file manifest message.
        // Always include a file_manifest when we have messages (e.g. session_joined)
        // so the client replies with FilesReady and the handshake completes.
        {
            let manifest = serde_json::json!({
                "type": "file_manifest",
                "files": files.iter().map(|(path, title, part_of)| {
                    serde_json::json!({
                        "doc_id": format!("body:{}/{}", doc_type.workspace_id(), path),
                        "filename": path,
                        "title": title,
                        "part_of": part_of,
                        "deleted": false
                    })
                }).collect::<Vec<_>>(),
                "client_is_new": false
            });

            if !files.is_empty() {
                info!(
                    "Sending file manifest with {} files for {}",
                    files.len(),
                    doc_id
                );
            }

            messages.push(manifest.to_string());
        }

        Ok(BeforeSyncAction::SendMessages { messages })
    }

    /// Handle custom control messages (FilesReady, focus, etc.).
    async fn on_control_message(
        &self,
        payload: OnControlMessagePayload<'_>,
    ) -> ControlMessageResponse {
        let message = payload.message;

        // Try to parse as JSON
        let json: serde_json::Value = match serde_json::from_str(message) {
            Ok(v) => v,
            Err(_) => return ControlMessageResponse::NotHandled,
        };

        let msg_type = json.get("type").and_then(|v| v.as_str());

        match msg_type {
            Some("files_ready") | Some("FilesReady") => {
                debug!("Received FilesReady from client");

                // Get workspace state to send as CrdtState
                if let Some(doc_id) = payload.doc_id {
                    if let Some(DocType::Workspace(workspace_id)) = DocType::parse(doc_id) {
                        if let Ok(storage) = self.storage_cache.get_storage(&workspace_id) {
                            let storage_key = format!("workspace:{}", workspace_id);
                            if let Ok(Some(state)) = storage.load_doc(&storage_key) {
                                let state_b64 = base64::Engine::encode(
                                    &base64::engine::general_purpose::STANDARD,
                                    &state,
                                );
                                let crdt_state = serde_json::json!({
                                    "type": "crdt_state",
                                    "state": state_b64
                                });
                                info!(
                                    "Completing handshake with CRDT state ({} bytes)",
                                    state.len()
                                );
                                return ControlMessageResponse::CompleteHandshake {
                                    responses: vec![crdt_state.to_string()],
                                };
                            }
                        }
                    }
                }

                // No state to send, just complete handshake
                ControlMessageResponse::CompleteHandshake { responses: vec![] }
            }
            Some("focus") => {
                // Handle focus message (for now just log it)
                if let Some(files) = json.get("files").and_then(|v| v.as_array()) {
                    debug!("Client focusing on {} files", files.len());
                }
                ControlMessageResponse::Handled { responses: vec![] }
            }
            Some("unfocus") => {
                // Handle unfocus message
                if let Some(files) = json.get("files").and_then(|v| v.as_array()) {
                    debug!("Client unfocusing {} files", files.len());
                }
                ControlMessageResponse::Handled { responses: vec![] }
            }
            _ => ControlMessageResponse::NotHandled,
        }
    }

    /// Called when a peer joins a document.
    async fn on_peer_joined(&self, payload: OnPeerJoinedPayload<'_>) -> HookResult {
        let user = payload.context.get::<AuthenticatedUser>();
        let user_id = user.map(|u| u.user_id.as_str()).unwrap_or("unknown");

        info!(
            "Peer {} joined document {} (total: {})",
            user_id, payload.doc_id, payload.peer_count
        );

        if let Some(handle) = self.handle.get() {
            let msg = serde_json::json!({
                "type": "peer_joined",
                "guestId": user_id,
                "peer_count": payload.peer_count,
            });
            handle
                .broadcast_text(payload.doc_id, msg.to_string(), Some(payload.client_id))
                .await;
        }
        Ok(())
    }

    /// Called when a peer leaves a document.
    async fn on_peer_left(&self, payload: OnPeerLeftPayload<'_>) -> HookResult {
        let user = payload.context.get::<AuthenticatedUser>();
        let user_id = user.map(|u| u.user_id.as_str()).unwrap_or("unknown");

        info!(
            "Peer {} left document {} (remaining: {})",
            user_id, payload.doc_id, payload.peer_count
        );

        if let Some(handle) = self.handle.get() {
            let msg = serde_json::json!({
                "type": "peer_left",
                "guestId": user_id,
                "peer_count": payload.peer_count,
            });
            handle
                .broadcast_text(payload.doc_id, msg.to_string(), Some(payload.client_id))
                .await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_doc_type_parse_workspace() {
        let dt = DocType::parse("workspace:abc123").unwrap();
        assert_eq!(dt, DocType::Workspace("abc123".to_string()));
        assert_eq!(dt.workspace_id(), "abc123");
        assert_eq!(dt.storage_key(), "workspace:abc123");
    }

    #[test]
    fn test_doc_type_parse_body() {
        let dt = DocType::parse("body:abc123/path/to/file.md").unwrap();
        assert_eq!(
            dt,
            DocType::Body {
                workspace_id: "abc123".to_string(),
                path: "path/to/file.md".to_string(),
            }
        );
        assert_eq!(dt.workspace_id(), "abc123");
        assert_eq!(dt.storage_key(), "body:abc123/path/to/file.md");
    }

    #[test]
    fn test_doc_type_parse_legacy() {
        // Legacy format without prefix is treated as workspace
        let dt = DocType::parse("abc123").unwrap();
        assert_eq!(dt, DocType::Workspace("abc123".to_string()));
    }
}
