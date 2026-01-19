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
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
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
}

/// Shared state for WebSocket handler
#[derive(Clone)]
pub struct WsState {
    pub repo: Arc<AuthRepo>,
    pub sync_state: Arc<SyncState>,
}

/// Connection mode determined from query parameters
enum ConnectionMode {
    /// Authenticated user sync (doc + token)
    Authenticated {
        user_id: String,
        device_id: String,
        workspace_id: String,
    },
    /// Session guest (session code)
    SessionGuest {
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

        ConnectionMode::SessionGuest {
            session_code,
            guest_id,
            workspace_id: session.workspace_id,
            read_only: session.read_only,
        }
    } else if let (Some(doc), Some(token)) = (&query.doc, &query.token) {
        // Authenticated sync (existing behavior)
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

        info!(
            "WebSocket upgrade: user={}, workspace={}",
            auth.user.email, workspace_id
        );

        ConnectionMode::Authenticated {
            user_id: auth.user.id,
            device_id: auth.session.device_id,
            workspace_id,
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

    info!(
        "WebSocket connected: user={}, workspace={}, connections={}",
        user_id,
        workspace_id,
        room.connection_count()
    );

    // Send initial sync (full state)
    let initial_state = connection.get_initial_sync().await;
    if let Err(e) = ws_tx.send(Message::Binary(initial_state.into())).await {
        error!("Failed to send initial state: {}", e);
        return;
    }

    // Handle bidirectional communication
    loop {
        tokio::select! {
            // Handle incoming messages from client
            Some(msg) = ws_rx.next() => {
                match msg {
                    Ok(Message::Binary(data)) => {
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
