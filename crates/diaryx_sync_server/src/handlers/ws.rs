use crate::auth::validate_token;
use crate::db::AuthRepo;
use crate::sync::{ClientConnection, SyncState};
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
    /// Document/workspace name
    pub doc: String,
    /// Auth token (optional in query, can also use header)
    pub token: Option<String>,
}

/// Shared state for WebSocket handler
#[derive(Clone)]
pub struct WsState {
    pub repo: Arc<AuthRepo>,
    pub sync_state: Arc<SyncState>,
}

/// WebSocket upgrade handler
pub async fn ws_handler(
    State(state): State<WsState>,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    // Extract and validate token
    let token = query.token.as_deref();

    let auth = match token {
        Some(t) => validate_token(&state.repo, t),
        None => None,
    };

    let auth = match auth {
        Some(a) => a,
        None => {
            warn!("WebSocket connection rejected: invalid or missing token");
            return StatusCode::UNAUTHORIZED.into_response();
        }
    };

    // Parse document name to get workspace ID
    // Format: "workspace:{workspace_id}" or just use the doc name as workspace ID
    let workspace_id = if query.doc.starts_with("workspace:") {
        query.doc.strip_prefix("workspace:").unwrap().to_string()
    } else {
        query.doc.clone()
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
        // Use or create user's default workspace
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

    // Upgrade to WebSocket
    let user_id = auth.user.id.clone();
    let device_id = auth.session.device_id.clone();

    ws.on_upgrade(move |socket| handle_socket(socket, state, user_id, device_id, workspace_id))
        .into_response()
}

/// Handle an established WebSocket connection
async fn handle_socket(
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
