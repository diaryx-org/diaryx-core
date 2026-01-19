use crate::auth::RequireAuth;
use crate::db::AuthRepo;
use crate::sync::SyncState;
use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{delete, get, patch, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Shared state for session handlers
#[derive(Clone)]
pub struct SessionsState {
    pub repo: Arc<AuthRepo>,
    pub sync_state: Arc<SyncState>,
}

/// Request to create a share session
#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub workspace_id: String,
    #[serde(default)]
    pub read_only: bool,
}

/// Request to update a share session
#[derive(Debug, Deserialize)]
pub struct UpdateSessionRequest {
    pub read_only: bool,
}

/// Response for session creation
#[derive(Debug, Serialize)]
pub struct SessionResponse {
    pub code: String,
    pub workspace_id: String,
    pub read_only: bool,
}

/// Response for session info (includes peer count)
#[derive(Debug, Serialize)]
pub struct SessionInfoResponse {
    pub code: String,
    pub workspace_id: String,
    pub read_only: bool,
    pub peer_count: usize,
}

/// Create session routes
pub fn session_routes(state: SessionsState) -> Router {
    Router::new()
        .route("/", post(create_session))
        .route("/{code}", get(get_session))
        .route("/{code}", patch(update_session))
        .route("/{code}", delete(delete_session))
        .with_state(state)
}

/// POST /api/sessions - Create a new share session
async fn create_session(
    State(state): State<SessionsState>,
    RequireAuth(auth): RequireAuth,
    Json(req): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    // Verify user owns the workspace, or use/create their default workspace
    let workspaces = state
        .repo
        .get_user_workspaces(&auth.user.id)
        .unwrap_or_default();

    let has_access = workspaces.iter().any(|w| w.id == req.workspace_id);

    // If user doesn't have access to the specified workspace, use their default workspace
    let workspace_id = if !has_access {
        match state.repo.get_or_create_workspace(&auth.user.id, "default") {
            Ok(id) => {
                tracing::info!(
                    "Using default workspace {} for user {} (requested: {})",
                    id,
                    auth.user.id,
                    req.workspace_id
                );
                id
            }
            Err(e) => {
                tracing::error!("Failed to get/create default workspace: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": "Failed to access workspace"
                    })),
                )
                    .into_response();
            }
        }
    } else {
        req.workspace_id.clone()
    };

    // Create the share session
    match state.repo.create_share_session(
        &workspace_id,
        &auth.user.id,
        req.read_only,
        None, // No expiry for now
    ) {
        Ok(code) => Json(SessionResponse {
            code,
            workspace_id,
            read_only: req.read_only,
        })
        .into_response(),
        Err(e) => {
            tracing::error!("Failed to create share session: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to create session"
                })),
            )
                .into_response()
        }
    }
}

/// GET /api/sessions/{code} - Get session info
async fn get_session(
    State(state): State<SessionsState>,
    Path(code): Path<String>,
) -> impl IntoResponse {
    // Normalize code to uppercase
    let code = code.to_uppercase();

    match state.repo.get_share_session(&code) {
        Ok(Some(session)) => {
            // Get peer count from sync state
            let peer_count = state
                .sync_state
                .get_session_peer_count(&code)
                .await
                .unwrap_or(0);

            Json(SessionInfoResponse {
                code: session.code,
                workspace_id: session.workspace_id,
                read_only: session.read_only,
                peer_count,
            })
            .into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Session not found or expired"
            })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to get share session: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to get session"
                })),
            )
                .into_response()
        }
    }
}

/// PATCH /api/sessions/{code} - Update session settings (owner only)
async fn update_session(
    State(state): State<SessionsState>,
    RequireAuth(auth): RequireAuth,
    Path(code): Path<String>,
    Json(req): Json<UpdateSessionRequest>,
) -> impl IntoResponse {
    // Normalize code to uppercase
    let code = code.to_uppercase();

    // Verify session exists and user is owner
    match state.repo.get_share_session(&code) {
        Ok(Some(session)) => {
            if session.owner_user_id != auth.user.id {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({
                        "error": "Only the session owner can update the session"
                    })),
                )
                    .into_response();
            }

            // Update read-only status in database
            match state
                .repo
                .update_share_session_read_only(&code, req.read_only)
            {
                Ok(true) => {
                    // Broadcast read-only change to all connected clients
                    if let Some(room) = state.sync_state.get_room_for_session(&code).await {
                        room.set_read_only(req.read_only).await;
                    }

                    Json(serde_json::json!({
                        "code": code,
                        "read_only": req.read_only,
                    }))
                    .into_response()
                }
                Ok(false) => (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({
                        "error": "Session not found"
                    })),
                )
                    .into_response(),
                Err(e) => {
                    tracing::error!("Failed to update share session: {}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "error": "Failed to update session"
                        })),
                    )
                        .into_response()
                }
            }
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Session not found or expired"
            })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to get share session: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to get session"
                })),
            )
                .into_response()
        }
    }
}

/// DELETE /api/sessions/{code} - End a share session (owner only)
async fn delete_session(
    State(state): State<SessionsState>,
    RequireAuth(auth): RequireAuth,
    Path(code): Path<String>,
) -> impl IntoResponse {
    // Normalize code to uppercase
    let code = code.to_uppercase();

    // Verify session exists and user is owner
    match state.repo.get_share_session(&code) {
        Ok(Some(session)) => {
            if session.owner_user_id != auth.user.id {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({
                        "error": "Only the session owner can end the session"
                    })),
                )
                    .into_response();
            }

            // Delete the session
            match state.repo.delete_share_session(&code) {
                Ok(true) => {
                    // Notify connected clients that session ended
                    state.sync_state.end_session(&code).await;

                    (StatusCode::NO_CONTENT, ()).into_response()
                }
                Ok(false) => (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({
                        "error": "Session not found"
                    })),
                )
                    .into_response(),
                Err(e) => {
                    tracing::error!("Failed to delete share session: {}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "error": "Failed to delete session"
                        })),
                    )
                        .into_response()
                }
            }
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Session not found or expired"
            })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to get share session: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to get session"
                })),
            )
                .into_response()
        }
    }
}
