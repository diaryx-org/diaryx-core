use crate::auth::validate_token;
use crate::db::AuthRepo;
use crate::sync::{ClientConnection, ControlMessage, SessionContext, SyncState};
use axum::{
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::IntoResponse,
};
use diaryx_core::crdt::{frame_body_message, unframe_body_message};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Query parameters for WebSocket connection
#[derive(Debug, Deserialize)]
pub struct WsQuery {
    /// Document/workspace name (for authenticated sync)
    pub doc: Option<String>,
    /// Auth token (for authenticated sync)
    pub token: Option<String>,
    /// Session code (for share session - alternative to doc+token)
    pub session: Option<String>,
    /// Guest ID (for session guests)
    pub guest_id: Option<String>,
    /// File path (for body doc sync - if present, routes to body doc handler)
    pub file: Option<String>,
    /// Multiplexed mode (for body sync - uses single connection for all files)
    pub multiplexed: Option<bool>,
}

/// Shared state for WebSocket handler
#[derive(Clone)]
pub struct WsState {
    pub repo: Arc<AuthRepo>,
    pub sync_state: Arc<SyncState>,
}

/// Connection mode determined from query parameters
enum ConnectionMode {
    /// Authenticated user sync (doc + token) - workspace metadata only
    Authenticated {
        user_id: String,
        device_id: String,
        workspace_id: String,
    },
    /// Authenticated user sync for body doc (doc + token + file)
    AuthenticatedBody {
        user_id: String,
        device_id: String,
        workspace_id: String,
        file_path: String,
    },
    /// Authenticated user sync for multiplexed body docs (doc + token + multiplexed=true)
    /// Uses a single WebSocket for all body syncs, with message framing to identify files.
    AuthenticatedMultiplexedBody {
        user_id: String,
        device_id: String,
        workspace_id: String,
    },
    /// Session guest (session code) - workspace metadata only
    SessionGuest {
        session_code: String,
        guest_id: String,
        workspace_id: String,
        read_only: bool,
    },
    /// Session guest for body doc (session code + file)
    SessionGuestBody {
        session_code: String,
        guest_id: String,
        workspace_id: String,
        file_path: String,
        read_only: bool,
    },
    /// Session guest for multiplexed body docs (session code + multiplexed=true)
    SessionGuestMultiplexedBody {
        session_code: String,
        guest_id: String,
        workspace_id: String,
        read_only: bool,
    },
}

/// WebSocket upgrade handler
pub async fn ws_handler(
    State(state): State<WsState>,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    // Determine connection mode based on query parameters
    let mode = if let Some(session_code) = &query.session {
        // Session-based connection (guest joining via code)
        let session_code = session_code.to_uppercase();

        // Validate session exists
        let session = match state.repo.get_share_session(&session_code) {
            Ok(Some(s)) => s,
            Ok(None) => {
                warn!(
                    "WebSocket connection rejected: session not found: {}",
                    session_code
                );
                return StatusCode::NOT_FOUND.into_response();
            }
            Err(e) => {
                error!("Failed to get session: {}", e);
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };

        let guest_id = query
            .guest_id
            .clone()
            .unwrap_or_else(|| format!("guest-{}", uuid::Uuid::new_v4()));

        // Check if this is a multiplexed body connection
        if query.multiplexed == Some(true) {
            ConnectionMode::SessionGuestMultiplexedBody {
                session_code,
                guest_id,
                workspace_id: session.workspace_id,
                read_only: session.read_only,
            }
        }
        // Check if this is a body doc connection
        else if let Some(file_path) = &query.file {
            ConnectionMode::SessionGuestBody {
                session_code,
                guest_id,
                workspace_id: session.workspace_id,
                file_path: file_path.clone(),
                read_only: session.read_only,
            }
        } else {
            ConnectionMode::SessionGuest {
                session_code,
                guest_id,
                workspace_id: session.workspace_id,
                read_only: session.read_only,
            }
        }
    } else if let (Some(doc), Some(token)) = (&query.doc, &query.token) {
        // Authenticated sync
        let auth = match validate_token(&state.repo, token) {
            Some(a) => a,
            None => {
                warn!("WebSocket connection rejected: invalid or missing token");
                return StatusCode::UNAUTHORIZED.into_response();
            }
        };

        // Parse document name to get workspace ID
        let workspace_id = if doc.starts_with("workspace:") {
            doc.strip_prefix("workspace:").unwrap().to_string()
        } else {
            doc.clone()
        };

        // Verify user has access to this workspace
        let workspaces = state
            .repo
            .get_user_workspaces(&auth.user.id)
            .unwrap_or_default();

        let has_access = workspaces
            .iter()
            .any(|w| w.id == workspace_id || w.name == workspace_id);

        // Allow access to user's default workspace
        let workspace_id = if !has_access {
            match state.repo.get_or_create_workspace(&auth.user.id, "default") {
                Ok(id) => id,
                Err(e) => {
                    error!("Failed to get/create workspace: {}", e);
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            }
        } else {
            workspace_id
        };

        // Check if this is a multiplexed body connection
        if query.multiplexed == Some(true) {
            info!(
                "WebSocket upgrade (multiplexed body): user={}, workspace={}",
                auth.user.email, workspace_id
            );

            ConnectionMode::AuthenticatedMultiplexedBody {
                user_id: auth.user.id,
                device_id: auth.session.device_id,
                workspace_id,
            }
        }
        // Check if this is a body doc connection
        else if let Some(file_path) = &query.file {
            info!(
                "WebSocket upgrade (body): user={}, workspace={}, file={}",
                auth.user.email, workspace_id, file_path
            );

            ConnectionMode::AuthenticatedBody {
                user_id: auth.user.id,
                device_id: auth.session.device_id,
                workspace_id,
                file_path: file_path.clone(),
            }
        } else {
            info!(
                "METADATA WebSocket upgrade: user={}, workspace={}",
                auth.user.email, workspace_id
            );

            ConnectionMode::Authenticated {
                user_id: auth.user.id,
                device_id: auth.session.device_id,
                workspace_id,
            }
        }
    } else {
        warn!(
            "WebSocket connection rejected: missing required parameters (need either session or doc+token)"
        );
        return StatusCode::BAD_REQUEST.into_response();
    };

    // Upgrade to WebSocket based on mode
    match mode {
        ConnectionMode::Authenticated {
            user_id,
            device_id,
            workspace_id,
        } => ws
            .on_upgrade(move |socket| {
                handle_authenticated_socket(socket, state, user_id, device_id, workspace_id)
            })
            .into_response(),
        ConnectionMode::AuthenticatedBody {
            user_id,
            device_id,
            workspace_id,
            file_path,
        } => ws
            .on_upgrade(move |socket| {
                handle_body_socket(socket, state, user_id, device_id, workspace_id, file_path)
            })
            .into_response(),
        ConnectionMode::SessionGuest {
            session_code,
            guest_id,
            workspace_id,
            read_only,
        } => {
            info!(
                "WebSocket upgrade: session={}, guest={}, workspace={}",
                session_code, guest_id, workspace_id
            );
            ws.on_upgrade(move |socket| {
                handle_session_socket(
                    socket,
                    state,
                    session_code,
                    guest_id,
                    workspace_id,
                    read_only,
                )
            })
            .into_response()
        }
        ConnectionMode::SessionGuestBody {
            session_code,
            guest_id,
            workspace_id,
            file_path,
            read_only,
        } => {
            info!(
                "WebSocket upgrade (body): session={}, guest={}, workspace={}, file={}",
                session_code, guest_id, workspace_id, file_path
            );
            ws.on_upgrade(move |socket| {
                handle_session_body_socket(
                    socket,
                    state,
                    session_code,
                    guest_id,
                    workspace_id,
                    file_path,
                    read_only,
                )
            })
            .into_response()
        }
        ConnectionMode::AuthenticatedMultiplexedBody {
            user_id,
            device_id,
            workspace_id,
        } => ws
            .on_upgrade(move |socket| {
                handle_multiplexed_body_socket(
                    socket,
                    state,
                    user_id,
                    device_id,
                    workspace_id,
                    false, // not read-only
                )
            })
            .into_response(),
        ConnectionMode::SessionGuestMultiplexedBody {
            session_code,
            guest_id,
            workspace_id,
            read_only,
        } => {
            info!(
                "WebSocket upgrade (multiplexed body): session={}, guest={}, workspace={}",
                session_code, guest_id, workspace_id
            );
            ws.on_upgrade(move |socket| {
                handle_multiplexed_body_socket(
                    socket,
                    state,
                    guest_id.clone(),
                    guest_id,
                    workspace_id,
                    read_only,
                )
            })
            .into_response()
        }
    }
}

/// Handle an authenticated WebSocket connection (existing multi-device sync)
async fn handle_authenticated_socket(
    socket: WebSocket,
    state: WsState,
    user_id: String,
    device_id: String,
    workspace_id: String,
) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Get or create the sync room
    let room = state.sync_state.get_or_create_room(&workspace_id).await;

    // Create client connection
    let mut connection = ClientConnection::new(
        user_id.clone(),
        device_id.clone(),
        workspace_id.clone(),
        room.clone(),
    );

    // Subscribe to control messages for progress updates
    let mut control_rx = room.subscribe_control();

    info!(
        "METADATA WebSocket connected: user={}, workspace={}, connections={}",
        user_id,
        workspace_id,
        room.connection_count()
    );

    // Send initial sync (full state)
    let initial_state = connection.get_initial_sync().await;
    info!(
        "METADATA sending initial state: {} bytes to user={}",
        initial_state.len(),
        user_id
    );
    if let Err(e) = ws_tx.send(Message::Binary(initial_state.into())).await {
        error!("Failed to send initial state: {}", e);
        return;
    }

    // Track initial sync completion for this connection
    let mut initial_sync_complete = false;

    // Handle bidirectional communication
    loop {
        tokio::select! {
            // Handle incoming messages from client
            Some(msg) = ws_rx.next() => {
                match msg {
                    Ok(Message::Binary(data)) => {
                        // Y-sync message format:
                        // Byte 0: msg_type::SYNC (0)
                        // Byte 1: sync_type - STEP1 (0), STEP2 (1), or UPDATE (2)
                        let sync_type = data.get(1).copied();
                        let msg_type = match sync_type {
                            Some(0) => "SyncStep1",
                            Some(1) => "SyncStep2",
                            Some(2) => "Update",
                            _ => "Unknown",
                        };
                        info!(
                            "METADATA message from {}: {} ({} bytes)",
                            user_id, msg_type, data.len()
                        );

                        // Handle Y-sync message
                        if let Some(response) = connection.handle_message(&data).await {
                            if let Err(e) = ws_tx.send(Message::Binary(response.into())).await {
                                error!("Failed to send response: {}", e);
                                break;
                            }
                        }

                        // Send SyncComplete after receiving client's SyncStep2
                        if sync_type == Some(1) && !initial_sync_complete {
                            initial_sync_complete = true;
                            let file_count = room.get_file_count().await;
                            let complete_msg = ControlMessage::SyncComplete { files_synced: file_count };
                            if let Ok(json) = serde_json::to_string(&complete_msg) {
                                let _ = ws_tx.send(Message::Text(json.into())).await;
                                info!("Metadata sync complete for {}: {} files", user_id, file_count);
                            }
                        }
                    }
                    Ok(Message::Ping(data)) => {
                        if let Err(e) = ws_tx.send(Message::Pong(data)).await {
                            error!("Failed to send pong: {}", e);
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => {
                        debug!("Client requested close");
                        break;
                    }
                    Err(e) => {
                        error!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }

            // Handle broadcast messages from other clients
            Some(broadcast_msg) = connection.recv_broadcast() => {
                if let Err(e) = ws_tx.send(Message::Binary(broadcast_msg.into())).await {
                    error!("Failed to send broadcast: {}", e);
                    break;
                }
            }

            // Handle control messages (progress updates, etc.)
            result = control_rx.recv() => {
                match result {
                    Ok(control_msg) => {
                        // Convert to JSON and send as text message
                        match serde_json::to_string(&control_msg) {
                            Ok(json) => {
                                if let Err(e) = ws_tx.send(Message::Text(json.into())).await {
                                    error!("Failed to send control message: {}", e);
                                    break;
                                }
                            }
                            Err(e) => {
                                warn!("Failed to serialize control message: {}", e);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Control message receiver lagged {} messages", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }

            else => break,
        }
    }

    info!(
        "WebSocket disconnected: user={}, workspace={}",
        user_id, workspace_id
    );

    // Connection will be dropped here, which calls unsubscribe

    // Maybe remove the room if no more connections
    state.sync_state.maybe_remove_room(&workspace_id).await;
}

/// Handle a session-based WebSocket connection (share session guest)
async fn handle_session_socket(
    socket: WebSocket,
    state: WsState,
    session_code: String,
    guest_id: String,
    workspace_id: String,
    read_only: bool,
) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Get or create the sync room with session context
    let room = state
        .sync_state
        .get_or_create_session_room(
            &workspace_id,
            SessionContext {
                code: session_code.clone(),
                owner_user_id: String::new(), // Not tracked here, comes from session info
                read_only,
            },
        )
        .await;

    // Create client connection (using guest_id as both user_id and device_id)
    let mut connection = ClientConnection::new(
        guest_id.clone(),
        guest_id.clone(),
        workspace_id.clone(),
        room.clone(),
    );

    // Subscribe to control messages
    let mut control_rx = room.subscribe_control();

    // Add guest to the room
    room.add_guest(&guest_id).await;

    info!(
        "Session WebSocket connected: session={}, guest={}, connections={}",
        session_code,
        guest_id,
        room.connection_count()
    );

    // Send session_joined message first (as JSON text message)
    let joined_msg = serde_json::json!({
        "type": "session_joined",
        "joinCode": session_code,
        "workspaceId": workspace_id,
        "readOnly": read_only,
    });
    if let Err(e) = ws_tx
        .send(Message::Text(joined_msg.to_string().into()))
        .await
    {
        error!("Failed to send session_joined: {}", e);
        room.remove_guest(&guest_id).await;
        return;
    }

    // Send initial sync (full state)
    let initial_state = connection.get_initial_sync().await;
    if let Err(e) = ws_tx.send(Message::Binary(initial_state.into())).await {
        error!("Failed to send initial state: {}", e);
        room.remove_guest(&guest_id).await;
        return;
    }

    // Track if session ended
    let mut session_ended = false;

    // Handle bidirectional communication
    loop {
        tokio::select! {
            // Handle incoming messages from client
            Some(msg) = ws_rx.next() => {
                match msg {
                    Ok(Message::Binary(data)) => {
                        // Check read-only mode for updates
                        if room.is_read_only() {
                            // In read-only mode, only allow sync step 1 (state vector request)
                            // which is safe and doesn't modify data
                            // We still process the message but won't broadcast updates
                            debug!("Processing message in read-only mode for guest {}", guest_id);
                        }

                        // Handle Y-sync message
                        if let Some(response) = connection.handle_message(&data).await {
                            if let Err(e) = ws_tx.send(Message::Binary(response.into())).await {
                                error!("Failed to send response: {}", e);
                                break;
                            }
                        }
                    }
                    Ok(Message::Ping(data)) => {
                        if let Err(e) = ws_tx.send(Message::Pong(data)).await {
                            error!("Failed to send pong: {}", e);
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => {
                        debug!("Client requested close");
                        break;
                    }
                    Err(e) => {
                        error!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }

            // Handle broadcast messages from other clients
            Some(broadcast_msg) = connection.recv_broadcast() => {
                if let Err(e) = ws_tx.send(Message::Binary(broadcast_msg.into())).await {
                    error!("Failed to send broadcast: {}", e);
                    break;
                }
            }

            // Handle control messages
            result = control_rx.recv() => {
                match result {
                    Ok(control_msg) => {
                        // Convert to JSON and send as text message
                        match serde_json::to_string(&control_msg) {
                            Ok(json) => {
                                if let Err(e) = ws_tx.send(Message::Text(json.into())).await {
                                    error!("Failed to send control message: {}", e);
                                    break;
                                }

                                // Check if session ended
                                if matches!(control_msg, ControlMessage::SessionEnded) {
                                    session_ended = true;
                                    break;
                                }
                            }
                            Err(e) => {
                                warn!("Failed to serialize control message: {}", e);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Control message receiver lagged {} messages", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }

            else => break,
        }
    }

    // Remove guest from the room (only if session didn't end - if it ended, guests are already cleared)
    if !session_ended {
        room.remove_guest(&guest_id).await;
    }

    info!(
        "Session WebSocket disconnected: session={}, guest={}",
        session_code, guest_id
    );

    // Connection will be dropped here, which calls unsubscribe

    // Maybe remove the room if no more connections
    state.sync_state.maybe_remove_room(&workspace_id).await;
}

/// Handle an authenticated body document WebSocket connection
async fn handle_body_socket(
    socket: WebSocket,
    state: WsState,
    user_id: String,
    device_id: String,
    workspace_id: String,
    file_path: String,
) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Get or create the sync room
    let room = state.sync_state.get_or_create_room(&workspace_id).await;

    // Generate a unique client ID
    let client_id = format!("{}:{}", user_id, device_id);

    info!(
        "Body sync connected: workspace={}, file={}, user={}",
        workspace_id, file_path, user_id
    );

    // Get initial sync state BEFORE subscribing to avoid race condition
    // This ensures we don't miss updates that arrive between subscribe and getting state
    let initial_sv = room.get_body_state_vector(&file_path).await;

    // Subscribe to body updates for this file AFTER getting initial state
    let mut body_rx = room.subscribe_body(&file_path, &client_id).await;

    // Send initial sync state (our state vector)
    if let Err(e) = ws_tx.send(Message::Binary(initial_sv.into())).await {
        error!("Failed to send initial body state vector: {}", e);
        room.unsubscribe_body(&file_path, &client_id).await;
        return;
    }

    // Handle bidirectional communication
    loop {
        tokio::select! {
            // Handle incoming messages from client
            Some(msg) = ws_rx.next() => {
                match msg {
                    Ok(Message::Binary(data)) => {
                        // Handle body sync message
                        if let Some(response) = room.handle_body_message(&file_path, &data).await {
                            if let Err(e) = ws_tx.send(Message::Binary(response.into())).await {
                                error!("Failed to send body response: {}", e);
                                break;
                            }
                        }
                    }
                    Ok(Message::Ping(data)) => {
                        if let Err(e) = ws_tx.send(Message::Pong(data)).await {
                            error!("Failed to send pong: {}", e);
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => {
                        debug!("Client requested close");
                        break;
                    }
                    Err(e) => {
                        error!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }

            // Handle broadcast messages for this body doc
            result = body_rx.recv() => {
                match result {
                    Ok((broadcast_file, broadcast_msg)) => {
                        // Only forward if it's for our file
                        if broadcast_file == file_path {
                            if let Err(e) = ws_tx.send(Message::Binary(broadcast_msg.into())).await {
                                error!("Failed to send body broadcast: {}", e);
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Body broadcast receiver lagged {} messages", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }

            else => break,
        }
    }

    // Cleanup
    room.unsubscribe_body(&file_path, &client_id).await;

    info!(
        "Body sync disconnected: workspace={}, file={}, user={}",
        workspace_id, file_path, user_id
    );

    // Maybe remove the room if no more connections
    state.sync_state.maybe_remove_room(&workspace_id).await;
}

/// Handle a session guest body document WebSocket connection
async fn handle_session_body_socket(
    socket: WebSocket,
    state: WsState,
    session_code: String,
    guest_id: String,
    workspace_id: String,
    file_path: String,
    _read_only: bool,
) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Get or create the sync room
    let room = state.sync_state.get_or_create_room(&workspace_id).await;

    // Use guest_id as client_id
    let client_id = guest_id.clone();

    info!(
        "Session body sync connected: session={}, file={}, guest={}",
        session_code, file_path, guest_id
    );

    // Get initial sync state BEFORE subscribing to avoid race condition
    // This ensures we don't miss updates that arrive between subscribe and getting state
    let initial_sv = room.get_body_state_vector(&file_path).await;

    // Subscribe to body updates for this file AFTER getting initial state
    let mut body_rx = room.subscribe_body(&file_path, &client_id).await;

    // Send initial sync state (our state vector)
    if let Err(e) = ws_tx.send(Message::Binary(initial_sv.into())).await {
        error!("Failed to send initial body state vector: {}", e);
        room.unsubscribe_body(&file_path, &client_id).await;
        return;
    }

    // Handle bidirectional communication
    loop {
        tokio::select! {
            // Handle incoming messages from client
            Some(msg) = ws_rx.next() => {
                match msg {
                    Ok(Message::Binary(data)) => {
                        // Handle body sync message (TODO: respect read_only in future)
                        if let Some(response) = room.handle_body_message(&file_path, &data).await {
                            if let Err(e) = ws_tx.send(Message::Binary(response.into())).await {
                                error!("Failed to send body response: {}", e);
                                break;
                            }
                        }
                    }
                    Ok(Message::Ping(data)) => {
                        if let Err(e) = ws_tx.send(Message::Pong(data)).await {
                            error!("Failed to send pong: {}", e);
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => {
                        debug!("Client requested close");
                        break;
                    }
                    Err(e) => {
                        error!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }

            // Handle broadcast messages for this body doc
            result = body_rx.recv() => {
                match result {
                    Ok((broadcast_file, broadcast_msg)) => {
                        // Only forward if it's for our file
                        if broadcast_file == file_path {
                            if let Err(e) = ws_tx.send(Message::Binary(broadcast_msg.into())).await {
                                error!("Failed to send body broadcast: {}", e);
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Body broadcast receiver lagged {} messages", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }

            else => break,
        }
    }

    // Cleanup
    room.unsubscribe_body(&file_path, &client_id).await;

    info!(
        "Session body sync disconnected: session={}, file={}, guest={}",
        session_code, file_path, guest_id
    );

    // Maybe remove the room if no more connections
    state.sync_state.maybe_remove_room(&workspace_id).await;
}

/// Handle a multiplexed body document WebSocket connection.
///
/// This handler uses a single WebSocket for all body syncs in a workspace.
/// Messages are framed with the file path prefix to identify which file
/// they belong to.
///
/// Message framing format: `[varUint(pathLen)] [pathBytes (UTF-8)] [message]`
async fn handle_multiplexed_body_socket(
    socket: WebSocket,
    state: WsState,
    user_id: String,
    device_id: String,
    workspace_id: String,
    _read_only: bool,
) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Get or create the sync room
    let room = state.sync_state.get_or_create_room(&workspace_id).await;

    // Generate a unique client ID
    let client_id = format!("{}:{}", user_id, device_id);

    info!(
        "Multiplexed body sync connected: workspace={}, user={}",
        workspace_id, user_id
    );

    // Subscribe to ALL body broadcasts (not filtered by file)
    let mut body_rx = room.subscribe_all_bodies();

    // Track which files this client is subscribed to
    let mut subscribed_files: HashSet<String> = HashSet::new();

    // Track sync state
    let mut last_progress_sent = 0usize;
    let mut messages_processed = 0usize;
    let mut last_new_subscription = std::time::Instant::now();
    let mut initial_sync_complete_sent = false;

    // Handle bidirectional communication
    loop {
        tokio::select! {
            biased;  // Check branches in order - prioritize incoming messages over timeout

            // Handle incoming messages from client
            Some(msg) = ws_rx.next() => {
                match msg {
                    Ok(Message::Binary(data)) => {
                        // Unframe to get file path
                        let Some((file_path, sync_msg)) = unframe_body_message(&data) else {
                            warn!("Invalid multiplexed body message from {}", client_id);
                            continue;
                        };

                        // Auto-subscribe on first message for a file
                        let is_new_subscription = !subscribed_files.contains(&file_path);
                        if is_new_subscription {
                            // Track subscription in the room
                            room.subscribe_body(&file_path, &client_id).await;
                            subscribed_files.insert(file_path.clone());
                            last_new_subscription = std::time::Instant::now();
                            if subscribed_files.len() % 500 == 0 || subscribed_files.len() == 1 {
                                debug!(
                                    "Client {} subscribed to {} body docs",
                                    client_id, subscribed_files.len()
                                );
                            }
                        }

                        // Route to existing handler
                        if let Some(response) = room.handle_body_message(&file_path, &sync_msg).await {
                            // Frame response with file path
                            let framed = frame_body_message(&file_path, &response);
                            if let Err(e) = ws_tx.send(Message::Binary(framed.into())).await {
                                error!("Failed to send multiplexed body response: {}", e);
                                break;
                            }
                            messages_processed += 1;

                            // Send progress every 100 messages or 5% of subscribed files
                            let progress_interval = (subscribed_files.len() / 20).max(100).min(500);
                            if messages_processed - last_progress_sent >= progress_interval {
                                last_progress_sent = messages_processed;
                                let progress_msg = ControlMessage::SyncProgress {
                                    completed: messages_processed,
                                    total: subscribed_files.len(),
                                };
                                if let Ok(json) = serde_json::to_string(&progress_msg) {
                                    let _ = ws_tx.send(Message::Text(json.into())).await;
                                }
                            }
                        }
                    }
                    Ok(Message::Ping(data)) => {
                        if let Err(e) = ws_tx.send(Message::Pong(data)).await {
                            error!("Failed to send pong: {}", e);
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => {
                        debug!("Client requested close");
                        break;
                    }
                    Err(e) => {
                        error!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }

            // Handle broadcast messages from other clients (also prioritized over timeout)
            result = body_rx.recv() => {
                match result {
                    Ok((file_path, msg)) => {
                        // Only forward if client subscribed to this file
                        if subscribed_files.contains(&file_path) {
                            let framed = frame_body_message(&file_path, &msg);
                            if let Err(e) = ws_tx.send(Message::Binary(framed.into())).await {
                                error!("Failed to send multiplexed body broadcast: {}", e);
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Multiplexed body broadcast receiver lagged {} messages", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }

            // Low priority: check for initial sync completion after quiet period
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(3)) => {
                // If we have subscriptions and no new ones for 3+ seconds, send SyncComplete
                if !initial_sync_complete_sent
                    && !subscribed_files.is_empty()
                    && last_new_subscription.elapsed() >= std::time::Duration::from_secs(3)
                {
                    initial_sync_complete_sent = true;
                    let complete_msg = ControlMessage::SyncComplete {
                        files_synced: subscribed_files.len(),
                    };
                    if let Ok(json) = serde_json::to_string(&complete_msg) {
                        let _ = ws_tx.send(Message::Text(json.into())).await;
                        info!(
                            "Body sync complete for {}: {} files, {} messages processed",
                            client_id, subscribed_files.len(), messages_processed
                        );
                    }
                }
            }

            else => break,
        }
    }

    // Cleanup: unsubscribe from all files
    for file_path in &subscribed_files {
        room.unsubscribe_body(file_path, &client_id).await;
    }

    info!(
        "Multiplexed body sync disconnected: workspace={}, user={}, files={}",
        workspace_id,
        user_id,
        subscribed_files.len()
    );

    // Maybe remove the room if no more connections
    state.sync_state.maybe_remove_room(&workspace_id).await;
}
