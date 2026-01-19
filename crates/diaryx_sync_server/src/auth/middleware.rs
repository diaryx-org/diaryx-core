use crate::db::{AuthRepo, SessionInfo, UserInfo};
use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
};
use std::sync::Arc;

/// Authenticated user extracted from request
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub session: SessionInfo,
    pub user: UserInfo,
}

/// Extension trait for extracting auth from requests
#[derive(Clone)]
pub struct AuthExtractor {
    pub repo: Arc<AuthRepo>,
}

/// Extractor for optional authentication
///
/// Use this when auth is optional (e.g., public endpoints that behave differently for authenticated users)
#[derive(Debug, Clone)]
pub struct OptionalAuth(pub Option<AuthUser>);

/// Extractor for required authentication
///
/// Use this for protected endpoints - returns 401 if not authenticated
#[derive(Debug, Clone)]
pub struct RequireAuth(pub AuthUser);

impl AuthExtractor {
    pub fn new(repo: Arc<AuthRepo>) -> Self {
        Self { repo }
    }

    /// Extract authentication from request headers or query parameters
    pub fn extract_auth(&self, parts: &Parts) -> Option<AuthUser> {
        // Try Authorization header first
        let token = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|s| s.to_string());

        // Fall back to query parameter
        let token = token.or_else(|| {
            parts.uri.query().and_then(|q| {
                q.split('&')
                    .find(|p| p.starts_with("token="))
                    .map(|p| p.strip_prefix("token=").unwrap_or("").to_string())
            })
        });

        let token = token?;

        // Validate session
        let session = self.repo.validate_session(&token).ok()??;

        // Update device last seen
        let _ = self.repo.update_device_last_seen(&session.device_id);

        // Get user info
        let user = self.repo.get_user(&session.user_id).ok()??;

        Some(AuthUser { session, user })
    }
}

impl<S> FromRequestParts<S> for OptionalAuth
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Get the AuthExtractor from extensions
        let extractor = parts
            .extensions
            .get::<AuthExtractor>()
            .cloned()
            .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "Auth not configured"))?;

        Ok(OptionalAuth(extractor.extract_auth(parts)))
    }
}

impl<S> FromRequestParts<S> for RequireAuth
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let OptionalAuth(auth) = OptionalAuth::from_request_parts(parts, state).await?;

        match auth {
            Some(user) => Ok(RequireAuth(user)),
            None => Err((StatusCode::UNAUTHORIZED, "Authentication required")),
        }
    }
}

/// Extract token from WebSocket upgrade request query parameters
pub fn extract_token_from_query(query: Option<&str>) -> Option<String> {
    query.and_then(|q| {
        q.split('&')
            .find(|p| p.starts_with("token="))
            .map(|p| p.strip_prefix("token=").unwrap_or("").to_string())
    })
}

/// Validate a token and return the auth user
pub fn validate_token(repo: &AuthRepo, token: &str) -> Option<AuthUser> {
    let session = repo.validate_session(token).ok()??;
    let _ = repo.update_device_last_seen(&session.device_id);
    let user = repo.get_user(&session.user_id).ok()??;
    Some(AuthUser { session, user })
}
