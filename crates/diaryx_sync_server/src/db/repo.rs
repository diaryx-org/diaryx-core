use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use std::sync::{Arc, Mutex};

/// User information
#[derive(Debug, Clone)]
pub struct UserInfo {
    pub id: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
}

/// Device information
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub id: String,
    pub user_id: String,
    pub name: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

/// Session information
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub token: String,
    pub user_id: String,
    pub device_id: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// Workspace information
#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

/// Share session information
#[derive(Debug, Clone)]
pub struct ShareSessionInfo {
    pub code: String,
    pub workspace_id: String,
    pub owner_user_id: String,
    pub read_only: bool,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Authentication repository for database operations
#[derive(Clone)]
pub struct AuthRepo {
    conn: Arc<Mutex<Connection>>,
}

impl AuthRepo {
    /// Create a new AuthRepo with the given connection
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    // ===== User operations =====

    /// Get a user by ID
    pub fn get_user(&self, user_id: &str) -> Result<Option<UserInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, email, created_at, last_login_at FROM users WHERE id = ?",
            [user_id],
            |row| {
                Ok(UserInfo {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    created_at: timestamp_to_datetime(row.get(2)?),
                    last_login_at: row.get::<_, Option<i64>>(3)?.map(timestamp_to_datetime),
                })
            },
        )
        .optional()
    }

    /// Get a user by email
    pub fn get_user_by_email(&self, email: &str) -> Result<Option<UserInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, email, created_at, last_login_at FROM users WHERE email = ?",
            [email],
            |row| {
                Ok(UserInfo {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    created_at: timestamp_to_datetime(row.get(2)?),
                    last_login_at: row.get::<_, Option<i64>>(3)?.map(timestamp_to_datetime),
                })
            },
        )
        .optional()
    }

    /// Create or get a user by email (returns user ID)
    pub fn get_or_create_user(&self, email: &str) -> Result<String, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        // Try to get existing user
        if let Some(user_id) = conn
            .query_row("SELECT id FROM users WHERE email = ?", [email], |row| {
                row.get::<_, String>(0)
            })
            .optional()?
        {
            return Ok(user_id);
        }

        // Create new user
        let user_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        conn.execute(
            "INSERT INTO users (id, email, created_at) VALUES (?, ?, ?)",
            params![user_id, email, now],
        )?;

        Ok(user_id)
    }

    /// Update user's last login time
    pub fn update_last_login(&self, user_id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.execute(
            "UPDATE users SET last_login_at = ? WHERE id = ?",
            params![now, user_id],
        )?;
        Ok(())
    }

    // ===== Device operations =====

    /// Create a new device
    pub fn create_device(
        &self,
        user_id: &str,
        name: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<String, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let device_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        conn.execute(
            "INSERT INTO devices (id, user_id, name, user_agent, created_at, last_seen_at) VALUES (?, ?, ?, ?, ?, ?)",
            params![device_id, user_id, name, user_agent, now, now],
        )?;

        Ok(device_id)
    }

    /// Get devices for a user
    pub fn get_user_devices(&self, user_id: &str) -> Result<Vec<DeviceInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, user_id, name, user_agent, created_at, last_seen_at
             FROM devices WHERE user_id = ? ORDER BY last_seen_at DESC",
        )?;

        let devices = stmt
            .query_map([user_id], |row| {
                Ok(DeviceInfo {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    name: row.get(2)?,
                    user_agent: row.get(3)?,
                    created_at: timestamp_to_datetime(row.get(4)?),
                    last_seen_at: timestamp_to_datetime(row.get(5)?),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(devices)
    }

    /// Update device last seen time
    pub fn update_device_last_seen(&self, device_id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.execute(
            "UPDATE devices SET last_seen_at = ? WHERE id = ?",
            params![now, device_id],
        )?;
        Ok(())
    }

    /// Delete a device (and its sessions)
    pub fn delete_device(&self, device_id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM devices WHERE id = ?", [device_id])?;
        Ok(())
    }

    // ===== Magic link operations =====

    /// Create a magic link token
    pub fn create_magic_token(
        &self,
        email: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<String, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        // Generate secure random token
        let token = generate_secure_token();
        let now = Utc::now().timestamp();

        conn.execute(
            "INSERT INTO magic_tokens (token, email, expires_at, created_at) VALUES (?, ?, ?, ?)",
            params![token, email, expires_at.timestamp(), now],
        )?;

        Ok(token)
    }

    /// Verify and consume a magic token (returns email if valid)
    pub fn verify_magic_token(&self, token: &str) -> Result<Option<String>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();

        // Get token if valid and not used
        let result: Option<String> = conn
            .query_row(
                "SELECT email FROM magic_tokens WHERE token = ? AND used = 0 AND expires_at > ?",
                params![token, now],
                |row| row.get(0),
            )
            .optional()?;

        if result.is_some() {
            // Mark token as used
            conn.execute("UPDATE magic_tokens SET used = 1 WHERE token = ?", [token])?;
        }

        Ok(result)
    }

    /// Clean up expired magic tokens
    pub fn cleanup_expired_magic_tokens(&self) -> Result<usize, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        let deleted = conn.execute("DELETE FROM magic_tokens WHERE expires_at < ?", [now])?;
        Ok(deleted)
    }

    /// Count recent magic tokens for an email (for rate limiting)
    pub fn count_recent_magic_tokens(
        &self,
        email: &str,
        since: DateTime<Utc>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM magic_tokens WHERE email = ? AND created_at > ?",
            params![email, since.timestamp()],
            |row| row.get(0),
        )
    }

    // ===== Session operations =====

    /// Create a new auth session
    pub fn create_session(
        &self,
        user_id: &str,
        device_id: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<String, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let token = generate_secure_token();
        let now = Utc::now().timestamp();

        conn.execute(
            "INSERT INTO auth_sessions (token, user_id, device_id, expires_at, created_at) VALUES (?, ?, ?, ?, ?)",
            params![token, user_id, device_id, expires_at.timestamp(), now],
        )?;

        Ok(token)
    }

    /// Validate a session token (returns session info if valid)
    pub fn validate_session(&self, token: &str) -> Result<Option<SessionInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();

        conn.query_row(
            "SELECT token, user_id, device_id, expires_at, created_at
             FROM auth_sessions WHERE token = ? AND expires_at > ?",
            params![token, now],
            |row| {
                Ok(SessionInfo {
                    token: row.get(0)?,
                    user_id: row.get(1)?,
                    device_id: row.get(2)?,
                    expires_at: timestamp_to_datetime(row.get(3)?),
                    created_at: timestamp_to_datetime(row.get(4)?),
                })
            },
        )
        .optional()
    }

    /// Delete a session
    pub fn delete_session(&self, token: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM auth_sessions WHERE token = ?", [token])?;
        Ok(())
    }

    /// Delete all sessions for a user
    pub fn delete_user_sessions(&self, user_id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM auth_sessions WHERE user_id = ?", [user_id])?;
        Ok(())
    }

    /// Clean up expired sessions
    pub fn cleanup_expired_sessions(&self) -> Result<usize, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        let deleted = conn.execute("DELETE FROM auth_sessions WHERE expires_at < ?", [now])?;
        Ok(deleted)
    }

    // ===== Workspace operations =====

    /// Get or create a workspace for a user
    pub fn get_or_create_workspace(
        &self,
        user_id: &str,
        name: &str,
    ) -> Result<String, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        // Try to get existing workspace
        if let Some(workspace_id) = conn
            .query_row(
                "SELECT id FROM user_workspaces WHERE user_id = ? AND name = ?",
                params![user_id, name],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            return Ok(workspace_id);
        }

        // Create new workspace
        let workspace_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        conn.execute(
            "INSERT INTO user_workspaces (id, user_id, name, created_at) VALUES (?, ?, ?, ?)",
            params![workspace_id, user_id, name, now],
        )?;

        Ok(workspace_id)
    }

    /// Get all workspaces for a user
    pub fn get_user_workspaces(
        &self,
        user_id: &str,
    ) -> Result<Vec<WorkspaceInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, user_id, name, created_at FROM user_workspaces WHERE user_id = ?",
        )?;

        let workspaces = stmt
            .query_map([user_id], |row| {
                Ok(WorkspaceInfo {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    name: row.get(2)?,
                    created_at: timestamp_to_datetime(row.get(3)?),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(workspaces)
    }

    /// Get a workspace by ID
    pub fn get_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<Option<WorkspaceInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, user_id, name, created_at FROM user_workspaces WHERE id = ?",
            [workspace_id],
            |row| {
                Ok(WorkspaceInfo {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    name: row.get(2)?,
                    created_at: timestamp_to_datetime(row.get(3)?),
                })
            },
        )
        .optional()
    }

    // ===== Share session operations =====

    /// Create a new share session
    pub fn create_share_session(
        &self,
        workspace_id: &str,
        owner_user_id: &str,
        read_only: bool,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<String, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let code = generate_session_code();
        let now = Utc::now().timestamp();

        conn.execute(
            "INSERT INTO share_sessions (code, workspace_id, owner_user_id, read_only, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?)",
            params![code, workspace_id, owner_user_id, read_only as i32, now, expires_at.map(|e| e.timestamp())],
        )?;

        Ok(code)
    }

    /// Get a share session by code
    pub fn get_share_session(
        &self,
        code: &str,
    ) -> Result<Option<ShareSessionInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();

        conn.query_row(
            "SELECT code, workspace_id, owner_user_id, read_only, created_at, expires_at
             FROM share_sessions
             WHERE code = ? AND (expires_at IS NULL OR expires_at > ?)",
            params![code, now],
            |row| {
                Ok(ShareSessionInfo {
                    code: row.get(0)?,
                    workspace_id: row.get(1)?,
                    owner_user_id: row.get(2)?,
                    read_only: row.get::<_, i32>(3)? != 0,
                    created_at: timestamp_to_datetime(row.get(4)?),
                    expires_at: row.get::<_, Option<i64>>(5)?.map(timestamp_to_datetime),
                })
            },
        )
        .optional()
    }

    /// Get all share sessions for a user
    pub fn get_user_share_sessions(
        &self,
        user_id: &str,
    ) -> Result<Vec<ShareSessionInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();

        let mut stmt = conn.prepare(
            "SELECT code, workspace_id, owner_user_id, read_only, created_at, expires_at
             FROM share_sessions
             WHERE owner_user_id = ? AND (expires_at IS NULL OR expires_at > ?)
             ORDER BY created_at DESC",
        )?;

        let sessions = stmt
            .query_map(params![user_id, now], |row| {
                Ok(ShareSessionInfo {
                    code: row.get(0)?,
                    workspace_id: row.get(1)?,
                    owner_user_id: row.get(2)?,
                    read_only: row.get::<_, i32>(3)? != 0,
                    created_at: timestamp_to_datetime(row.get(4)?),
                    expires_at: row.get::<_, Option<i64>>(5)?.map(timestamp_to_datetime),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(sessions)
    }

    /// Update read-only status for a share session
    pub fn update_share_session_read_only(
        &self,
        code: &str,
        read_only: bool,
    ) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let updated = conn.execute(
            "UPDATE share_sessions SET read_only = ? WHERE code = ?",
            params![read_only as i32, code],
        )?;
        Ok(updated > 0)
    }

    /// Delete a share session
    pub fn delete_share_session(&self, code: &str) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let deleted = conn.execute("DELETE FROM share_sessions WHERE code = ?", [code])?;
        Ok(deleted > 0)
    }

    /// Clean up expired share sessions
    pub fn cleanup_expired_share_sessions(&self) -> Result<usize, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        let deleted = conn.execute(
            "DELETE FROM share_sessions WHERE expires_at IS NOT NULL AND expires_at < ?",
            [now],
        )?;
        Ok(deleted)
    }
}

// ===== Helper functions =====

/// Generate a cryptographically secure random token
fn generate_secure_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.r#gen()).collect();
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
}

/// Generate a session code in XXXXXXXX-XXXXXXXX format
fn generate_session_code() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();

    let part1: String = (0..8)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect();

    let part2: String = (0..8)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect();

    format!("{}-{}", part1, part2)
}

/// Convert Unix timestamp to DateTime<Utc>
fn timestamp_to_datetime(timestamp: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(timestamp, 0).unwrap_or_else(|| Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_database;

    fn setup_test_db() -> AuthRepo {
        let conn = Connection::open_in_memory().unwrap();
        init_database(&conn).unwrap();
        AuthRepo::new(conn)
    }

    #[test]
    fn test_user_creation() {
        let repo = setup_test_db();

        let user_id = repo.get_or_create_user("test@example.com").unwrap();
        assert!(!user_id.is_empty());

        // Getting the same user should return the same ID
        let user_id2 = repo.get_or_create_user("test@example.com").unwrap();
        assert_eq!(user_id, user_id2);

        // Verify user exists
        let user = repo.get_user(&user_id).unwrap();
        assert!(user.is_some());
        assert_eq!(user.unwrap().email, "test@example.com");
    }

    #[test]
    fn test_magic_token_flow() {
        let repo = setup_test_db();
        let email = "test@example.com";
        let expires = Utc::now() + chrono::Duration::hours(1);

        // Create token
        let token = repo.create_magic_token(email, expires).unwrap();
        assert!(!token.is_empty());

        // Verify token
        let verified_email = repo.verify_magic_token(&token).unwrap();
        assert_eq!(verified_email, Some(email.to_string()));

        // Token should be consumed (can't verify again)
        let second_verify = repo.verify_magic_token(&token).unwrap();
        assert!(second_verify.is_none());
    }

    #[test]
    fn test_session_flow() {
        let repo = setup_test_db();

        // Create user and device
        let user_id = repo.get_or_create_user("test@example.com").unwrap();
        let device_id = repo
            .create_device(&user_id, Some("Test Device"), None)
            .unwrap();

        // Create session
        let expires = Utc::now() + chrono::Duration::days(30);
        let token = repo.create_session(&user_id, &device_id, expires).unwrap();

        // Validate session
        let session = repo.validate_session(&token).unwrap();
        assert!(session.is_some());
        let session = session.unwrap();
        assert_eq!(session.user_id, user_id);
        assert_eq!(session.device_id, device_id);

        // Delete session
        repo.delete_session(&token).unwrap();
        let deleted = repo.validate_session(&token).unwrap();
        assert!(deleted.is_none());
    }
}
