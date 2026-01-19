use crate::auth::{MagicLinkService, RequireAuth};
use crate::db::AuthRepo;
use crate::email::EmailService;
use axum::{
    Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info, warn};

/// Shared state for auth handlers
#[derive(Clone)]
pub struct AuthState {
    pub magic_link_service: Arc<MagicLinkService>,
    pub email_service: Arc<EmailService>,
    pub repo: Arc<AuthRepo>,
}

/// Request body for magic link request
#[derive(Debug, Deserialize)]
pub struct MagicLinkRequest {
    pub email: String,
}

/// Response for magic link request
#[derive(Debug, Serialize)]
pub struct MagicLinkResponse {
    pub success: bool,
    pub message: String,
    /// Only included in dev mode when email is not configured
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_link: Option<String>,
}

/// Query params for magic link verification
#[derive(Debug, Deserialize)]
pub struct VerifyQuery {
    pub token: String,
    pub device_name: Option<String>,
}

/// Response for successful verification
#[derive(Debug, Serialize)]
pub struct VerifyResponse {
    pub success: bool,
    pub token: String,
    pub user: UserResponse,
}

/// User info in responses
#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: String,
    pub email: String,
}

/// Error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Response for user info
#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub user: UserResponse,
    pub workspaces: Vec<WorkspaceResponse>,
    pub devices: Vec<DeviceResponse>,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceResponse {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct DeviceResponse {
    pub id: String,
    pub name: Option<String>,
    pub last_seen_at: String,
}

/// Create auth routes
pub fn auth_routes(state: AuthState) -> Router {
    Router::new()
        .route("/magic-link", post(request_magic_link))
        .route("/verify", get(verify_magic_link))
        .route("/me", get(get_current_user))
        .route("/logout", post(logout))
        .route("/devices", get(list_devices))
        .route("/devices/{device_id}", axum::routing::delete(delete_device))
        .with_state(state)
}

/// POST /auth/magic-link - Request a magic link
async fn request_magic_link(
    State(state): State<AuthState>,
    Json(body): Json<MagicLinkRequest>,
) -> impl IntoResponse {
    let email = body.email.trim().to_lowercase();

    // Validate email format
    if !email.contains('@') || email.len() < 5 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid email address".to_string(),
            }),
        )
            .into_response();
    }

    // Request magic link
    let token = match state.magic_link_service.request_magic_link(&email) {
        Ok(token) => token,
        Err(crate::auth::MagicLinkError::RateLimited) => {
            warn!("Rate limited magic link request for {}", email);
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(ErrorResponse {
                    error: "Too many requests. Please try again later.".to_string(),
                }),
            )
                .into_response();
        }
        Err(e) => {
            error!("Failed to create magic link: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to create magic link".to_string(),
                }),
            )
                .into_response();
        }
    };

    let magic_link_url = state.magic_link_service.build_magic_link_url(&token);

    // Try to send email
    if state.email_service.is_configured() {
        if let Err(e) = state
            .email_service
            .send_magic_link(&email, &magic_link_url)
            .await
        {
            error!("Failed to send magic link email: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to send email".to_string(),
                }),
            )
                .into_response();
        }

        info!("Magic link sent to {}", email);
        (
            StatusCode::OK,
            Json(MagicLinkResponse {
                success: true,
                message: "Check your email for a sign-in link.".to_string(),
                dev_link: None,
            }),
        )
            .into_response()
    } else {
        // Dev mode: return the link directly
        warn!(
            "Email not configured, returning magic link directly (dev mode only!): {}",
            magic_link_url
        );
        (
            StatusCode::OK,
            Json(MagicLinkResponse {
                success: true,
                message: "Email not configured. Use the dev link below.".to_string(),
                dev_link: Some(magic_link_url),
            }),
        )
            .into_response()
    }
}

/// GET /auth/verify - Verify a magic link and return session token
async fn verify_magic_link(
    State(state): State<AuthState>,
    Query(query): Query<VerifyQuery>,
) -> impl IntoResponse {
    let result = state.magic_link_service.verify_magic_link(
        &query.token,
        query.device_name.as_deref(),
        None, // Could extract user-agent from headers
    );

    match result {
        Ok(verify_result) => {
            info!("User {} logged in successfully", verify_result.email);
            (
                StatusCode::OK,
                Json(VerifyResponse {
                    success: true,
                    token: verify_result.session_token,
                    user: UserResponse {
                        id: verify_result.user_id,
                        email: verify_result.email,
                    },
                }),
            )
                .into_response()
        }
        Err(crate::auth::MagicLinkError::InvalidToken) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid or expired link. Please request a new one.".to_string(),
            }),
        )
            .into_response(),
        Err(e) => {
            error!("Failed to verify magic link: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Verification failed".to_string(),
                }),
            )
                .into_response()
        }
    }
}

/// GET /auth/me - Get current user info
async fn get_current_user(
    State(state): State<AuthState>,
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
        .collect();

    let devices = state
        .repo
        .get_user_devices(&auth.user.id)
        .unwrap_or_default()
        .into_iter()
        .map(|d| DeviceResponse {
            id: d.id,
            name: d.name,
            last_seen_at: d.last_seen_at.to_rfc3339(),
        })
        .collect();

    Json(MeResponse {
        user: UserResponse {
            id: auth.user.id,
            email: auth.user.email,
        },
        workspaces,
        devices,
    })
}

/// POST /auth/logout - Log out (delete session)
async fn logout(
    State(state): State<AuthState>,
    RequireAuth(auth): RequireAuth,
) -> impl IntoResponse {
    if let Err(e) = state.repo.delete_session(&auth.session.token) {
        error!("Failed to delete session: {}", e);
    }

    StatusCode::NO_CONTENT
}

/// GET /auth/devices - List user's devices
async fn list_devices(
    State(state): State<AuthState>,
    RequireAuth(auth): RequireAuth,
) -> impl IntoResponse {
    let devices = state
        .repo
        .get_user_devices(&auth.user.id)
        .unwrap_or_default()
        .into_iter()
        .map(|d| DeviceResponse {
            id: d.id,
            name: d.name,
            last_seen_at: d.last_seen_at.to_rfc3339(),
        })
        .collect::<Vec<_>>();

    Json(devices)
}

/// DELETE /auth/devices/:device_id - Delete a device
async fn delete_device(
    State(state): State<AuthState>,
    RequireAuth(auth): RequireAuth,
    axum::extract::Path(device_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    // Verify the device belongs to the user
    let devices = state
        .repo
        .get_user_devices(&auth.user.id)
        .unwrap_or_default();

    let owns_device = devices.iter().any(|d| d.id == device_id);

    if !owns_device {
        return StatusCode::NOT_FOUND;
    }

    // Don't allow deleting the current device
    if device_id == auth.session.device_id {
        return StatusCode::BAD_REQUEST;
    }

    if let Err(e) = state.repo.delete_device(&device_id) {
        error!("Failed to delete device: {}", e);
        return StatusCode::INTERNAL_SERVER_ERROR;
    }

    StatusCode::NO_CONTENT
}
