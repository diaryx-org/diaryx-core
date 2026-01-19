use crate::auth::RequireAuth;
use crate::db::AuthRepo;
use crate::sync::SyncState;
use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
};
use serde::Serialize;
use std::sync::Arc;

/// Shared state for API handlers
#[derive(Clone)]
pub struct ApiState {
    pub repo: Arc<AuthRepo>,
    pub sync_state: Arc<SyncState>,
}

/// Server status response
#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub status: String,
    pub version: String,
    pub active_connections: usize,
    pub active_rooms: usize,
}

/// Workspace info response
#[derive(Debug, Serialize)]
pub struct WorkspaceResponse {
    pub id: String,
    pub name: String,
}

/// Create API routes
pub fn api_routes(state: ApiState) -> Router {
    Router::new()
        .route("/status", get(get_status))
        .route("/workspaces", get(list_workspaces))
        .route("/workspaces/{workspace_id}", get(get_workspace))
        .with_state(state)
}

/// GET /api/status - Get server status (public endpoint)
async fn get_status(State(state): State<ApiState>) -> impl IntoResponse {
    let stats = state.sync_state.get_stats();

    Json(StatusResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        active_connections: stats.active_connections,
        active_rooms: stats.active_rooms,
    })
}

/// GET /api/workspaces - List user's workspaces
async fn list_workspaces(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
) -> impl IntoResponse {
    let workspaces = state
        .repo
        .get_user_workspaces(&auth.user.id)
        .unwrap_or_default()
        .into_iter()
        .map(|w| WorkspaceResponse {
            id: w.id,
            name: w.name,
        })
        .collect::<Vec<_>>();

    Json(workspaces)
}

/// GET /api/workspaces/:workspace_id - Get workspace info
async fn get_workspace(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
    axum::extract::Path(workspace_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let workspace = match state.repo.get_workspace(&workspace_id) {
        Ok(Some(w)) => w,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    // Verify ownership
    if workspace.user_id != auth.user.id {
        return StatusCode::NOT_FOUND.into_response();
    }

    Json(WorkspaceResponse {
        id: workspace.id,
        name: workspace.name,
    })
    .into_response()
}
