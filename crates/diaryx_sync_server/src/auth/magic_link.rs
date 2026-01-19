use crate::config::Config;
use crate::db::AuthRepo;
use chrono::{Duration, Utc};
use std::sync::Arc;

/// Magic link authentication service
pub struct MagicLinkService {
    repo: Arc<AuthRepo>,
    config: Arc<Config>,
}

/// Result of magic link verification
#[derive(Debug)]
pub struct VerifyResult {
    pub session_token: String,
    pub user_id: String,
    pub device_id: String,
    pub email: String,
}

/// Error types for magic link operations
#[derive(Debug)]
pub enum MagicLinkError {
    /// Token not found or expired
    InvalidToken,
    /// Too many magic link requests (rate limited)
    RateLimited,
    /// Database error
    DatabaseError(String),
    /// Email sending error
    EmailError(String),
}

impl std::fmt::Display for MagicLinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MagicLinkError::InvalidToken => write!(f, "Invalid or expired magic link"),
            MagicLinkError::RateLimited => {
                write!(f, "Too many requests. Please try again later.")
            }
            MagicLinkError::DatabaseError(e) => write!(f, "Database error: {}", e),
            MagicLinkError::EmailError(e) => write!(f, "Email error: {}", e),
        }
    }
}

impl std::error::Error for MagicLinkError {}

impl MagicLinkService {
    /// Create a new MagicLinkService
    pub fn new(repo: Arc<AuthRepo>, config: Arc<Config>) -> Self {
        Self { repo, config }
    }

    /// Request a magic link for the given email
    ///
    /// Returns the magic link token (which should be sent via email)
    pub fn request_magic_link(&self, email: &str) -> Result<String, MagicLinkError> {
        // Normalize email
        let email = email.trim().to_lowercase();

        // Rate limiting: max 3 tokens per hour per email
        let one_hour_ago = Utc::now() - Duration::hours(1);
        let recent_count = self
            .repo
            .count_recent_magic_tokens(&email, one_hour_ago)
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;

        if recent_count >= 3 {
            return Err(MagicLinkError::RateLimited);
        }

        // Create token with configured expiration
        let expires_at = Utc::now() + Duration::minutes(self.config.magic_link_expiry_minutes);
        let token = self
            .repo
            .create_magic_token(&email, expires_at)
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;

        Ok(token)
    }

    /// Verify a magic link token and create a session
    ///
    /// Returns the session token and user info on success
    pub fn verify_magic_link(
        &self,
        token: &str,
        device_name: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<VerifyResult, MagicLinkError> {
        // Verify and consume the magic token
        let email = self
            .repo
            .verify_magic_token(token)
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?
            .ok_or(MagicLinkError::InvalidToken)?;

        // Get or create user
        let user_id = self
            .repo
            .get_or_create_user(&email)
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;

        // Update last login
        self.repo
            .update_last_login(&user_id)
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;

        // Create device
        let device_id = self
            .repo
            .create_device(&user_id, device_name, user_agent)
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;

        // Create session
        let expires_at = Utc::now() + Duration::days(self.config.session_expiry_days);
        let session_token = self
            .repo
            .create_session(&user_id, &device_id, expires_at)
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;

        // Create default workspace if needed
        self.repo
            .get_or_create_workspace(&user_id, "default")
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;

        Ok(VerifyResult {
            session_token,
            user_id,
            device_id,
            email,
        })
    }

    /// Build the magic link URL for a token
    pub fn build_magic_link_url(&self, token: &str) -> String {
        format!("{}/auth/verify?token={}", self.config.app_base_url, token)
    }

    /// Clean up expired tokens (should be called periodically)
    pub fn cleanup_expired(&self) -> Result<(usize, usize), MagicLinkError> {
        let tokens_deleted = self
            .repo
            .cleanup_expired_magic_tokens()
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;
        let sessions_deleted = self
            .repo
            .cleanup_expired_sessions()
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;
        Ok((tokens_deleted, sessions_deleted))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_database;
    use rusqlite::Connection;

    fn setup_test_service() -> MagicLinkService {
        let conn = Connection::open_in_memory().unwrap();
        init_database(&conn).unwrap();
        let repo = Arc::new(AuthRepo::new(conn));
        let config = Arc::new(Config::from_env().unwrap());
        MagicLinkService::new(repo, config)
    }

    #[test]
    fn test_magic_link_flow() {
        let service = setup_test_service();

        // Request magic link
        let token = service.request_magic_link("test@example.com").unwrap();
        assert!(!token.is_empty());

        // Verify magic link
        let result = service
            .verify_magic_link(&token, Some("Test"), None)
            .unwrap();
        assert_eq!(result.email, "test@example.com");
        assert!(!result.session_token.is_empty());
        assert!(!result.user_id.is_empty());
        assert!(!result.device_id.is_empty());

        // Token should be consumed
        let second_try = service.verify_magic_link(&token, None, None);
        assert!(matches!(second_try, Err(MagicLinkError::InvalidToken)));
    }

    #[test]
    fn test_rate_limiting() {
        let service = setup_test_service();
        let email = "ratelimit@example.com";

        // First 3 should succeed
        for _ in 0..3 {
            service.request_magic_link(email).unwrap();
        }

        // 4th should be rate limited
        let result = service.request_magic_link(email);
        assert!(matches!(result, Err(MagicLinkError::RateLimited)));
    }
}
