//! Authentication command handlers for sync.
//!
//! Handles login, verify, and logout commands.

use diaryx_core::config::Config;

const DEFAULT_SYNC_SERVER: &str = "https://sync.diaryx.org";

/// Handle the login command - initiate magic link authentication.
pub fn handle_login(config: &Config, email: &str, server: Option<&str>) {
    let server_url = server
        .or(config.sync_server_url.as_deref())
        .unwrap_or(DEFAULT_SYNC_SERVER);

    println!("Logging in to sync server...");
    println!("  Server: {}", server_url);
    println!("  Email: {}", email);
    println!();

    // Build the request URL
    let url = format!("{}/auth/magic-link", server_url);

    // Use blocking reqwest for simplicity in CLI context
    let client = reqwest::blocking::Client::new();
    let response = client
        .post(&url)
        .json(&serde_json::json!({ "email": email }))
        .send();

    match response {
        Ok(resp) => {
            if resp.status().is_success() {
                // Save server URL to config for future use
                let mut new_config = config.clone();
                new_config.sync_server_url = Some(server_url.to_string());
                new_config.sync_email = Some(email.to_string());

                if let Err(e) = new_config.save() {
                    eprintln!("Warning: Could not save config: {}", e);
                }

                println!("Check your email for a magic link!");
                println!();
                println!("Once you receive the email, run:");
                println!("  diaryx sync verify <TOKEN>");
                println!();
                println!("The token is in the magic link URL (the part after ?token=)");
            } else {
                let status = resp.status();
                let body = resp.text().unwrap_or_default();
                eprintln!("Login request failed: {} - {}", status, body);
            }
        }
        Err(e) => {
            eprintln!("Failed to connect to sync server: {}", e);
            eprintln!();
            eprintln!("Please check:");
            eprintln!("  - Your internet connection");
            eprintln!("  - The server URL is correct: {}", server_url);
        }
    }
}

/// Handle the verify command - complete magic link authentication.
pub fn handle_verify(config: &Config, token: &str, device_name: Option<&str>) {
    let server_url = config
        .sync_server_url
        .as_deref()
        .unwrap_or(DEFAULT_SYNC_SERVER);

    let device = device_name.unwrap_or("CLI");

    println!("Verifying authentication...");

    // Build the request URL with query parameters
    let url = format!(
        "{}/auth/verify?token={}&device_name={}",
        server_url,
        urlencoding::encode(token),
        urlencoding::encode(device)
    );

    let client = reqwest::blocking::Client::new();
    let response = client.get(&url).send();

    match response {
        Ok(resp) => {
            if resp.status().is_success() {
                // Parse response to get session token
                match resp.json::<serde_json::Value>() {
                    Ok(json) => {
                        // Server returns "token" not "session_token"
                        let session_token = json
                            .get("token")
                            .or_else(|| json.get("session_token"))
                            .and_then(|v| v.as_str());

                        if let Some(session_token) = session_token {
                            // Get email from response - may be nested under "user"
                            let email = json
                                .get("user")
                                .and_then(|u| u.get("email"))
                                .and_then(|v| v.as_str())
                                .or_else(|| json.get("email").and_then(|v| v.as_str()))
                                .map(String::from)
                                .or_else(|| config.sync_email.clone());

                            // Get user_id from response - may be nested under "user"
                            // This can be used as a workspace_id fallback
                            let user_id = json
                                .get("user")
                                .and_then(|u| u.get("id"))
                                .and_then(|v| v.as_str())
                                .map(String::from);

                            // Get workspace_id from response if present
                            let workspace_id = json
                                .get("workspace_id")
                                .and_then(|v| v.as_str())
                                .map(String::from)
                                .or(user_id);

                            // Save credentials to config
                            let mut new_config = config.clone();
                            new_config.sync_session_token = Some(session_token.to_string());
                            if let Some(e) = email.clone() {
                                new_config.sync_email = Some(e);
                            }
                            if let Some(wid) = workspace_id.clone() {
                                new_config.sync_workspace_id = Some(wid);
                            }

                            if let Err(e) = new_config.save() {
                                eprintln!("Warning: Could not save config: {}", e);
                            }

                            println!();
                            println!("Successfully logged in!");
                            if let Some(e) = email {
                                println!("  Email: {}", e);
                            }
                            if let Some(wid) = workspace_id {
                                println!("  Workspace ID: {}", wid);
                            }
                            println!();
                            println!("You can now start syncing with:");
                            println!("  diaryx sync start");
                        } else {
                            eprintln!("Verification succeeded but no session token in response");
                            eprintln!("Response: {:?}", json);
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to parse verification response: {}", e);
                    }
                }
            } else {
                let status = resp.status();
                let body = resp.text().unwrap_or_default();

                if status.as_u16() == 401 || status.as_u16() == 400 {
                    eprintln!("Invalid or expired token.");
                    eprintln!();
                    eprintln!("Please request a new magic link with:");
                    eprintln!("  diaryx sync login <your-email>");
                } else {
                    eprintln!("Verification failed: {} - {}", status, body);
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to connect to sync server: {}", e);
        }
    }
}

/// Handle the logout command - clear stored credentials.
pub fn handle_logout(config: &Config) {
    let server_url = config.sync_server_url.as_deref();
    let session_token = config.sync_session_token.as_deref();

    // Try to notify server about logout if we have credentials
    if let (Some(server), Some(token)) = (server_url, session_token) {
        let url = format!("{}/auth/logout", server);
        let client = reqwest::blocking::Client::new();

        // Best-effort logout notification - don't fail if server is unavailable
        let _ = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send();
    }

    // Clear local credentials
    let mut new_config = config.clone();
    new_config.sync_session_token = None;
    // Keep email and server URL for convenience on re-login
    // new_config.sync_email = None;
    // new_config.sync_server_url = None;

    if let Err(e) = new_config.save() {
        eprintln!("Warning: Could not save config: {}", e);
    }

    println!("Logged out successfully.");
    if let Some(email) = &config.sync_email {
        println!();
        println!("To log back in:");
        println!("  diaryx sync login {}", email);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // URL Construction Tests
    // =========================================================================

    #[test]
    fn test_login_url_construction() {
        let server_url = "https://sync.diaryx.org";
        let url = format!("{}/auth/magic-link", server_url);
        assert_eq!(url, "https://sync.diaryx.org/auth/magic-link");
    }

    #[test]
    fn test_login_url_with_trailing_slash() {
        // If server URL had trailing slash, we'd get double slash
        // This test documents current behavior
        let server_url = "https://sync.diaryx.org/";
        let url = format!("{}/auth/magic-link", server_url);
        // Note: Current code doesn't strip trailing slash
        assert_eq!(url, "https://sync.diaryx.org//auth/magic-link");
    }

    #[test]
    fn test_verify_url_construction() {
        let server_url = "https://sync.diaryx.org";
        let token = "abc123";
        let device = "CLI";

        let url = format!(
            "{}/auth/verify?token={}&device_name={}",
            server_url,
            urlencoding::encode(token),
            urlencoding::encode(device)
        );

        assert_eq!(
            url,
            "https://sync.diaryx.org/auth/verify?token=abc123&device_name=CLI"
        );
    }

    #[test]
    fn test_verify_url_encoding_special_chars() {
        let server_url = "https://sync.diaryx.org";
        let token = "token+with/special=chars";
        let device = "My Device Name";

        let url = format!(
            "{}/auth/verify?token={}&device_name={}",
            server_url,
            urlencoding::encode(token),
            urlencoding::encode(device)
        );

        assert!(url.contains("token%2Bwith%2Fspecial%3Dchars"));
        assert!(url.contains("My%20Device%20Name"));
    }

    #[test]
    fn test_logout_url_construction() {
        let server = "https://sync.diaryx.org";
        let url = format!("{}/auth/logout", server);
        assert_eq!(url, "https://sync.diaryx.org/auth/logout");
    }

    // =========================================================================
    // Response Parsing Tests
    // =========================================================================

    #[test]
    fn test_verify_response_parsing_token_field() {
        let json = r#"{"token": "session-token-123"}"#;
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();

        let session_token = parsed
            .get("token")
            .or_else(|| parsed.get("session_token"))
            .and_then(|v| v.as_str());

        assert_eq!(session_token, Some("session-token-123"));
    }

    #[test]
    fn test_verify_response_parsing_session_token_fallback() {
        // Test fallback to session_token field
        let json = r#"{"session_token": "session-token-456"}"#;
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();

        let session_token = parsed
            .get("token")
            .or_else(|| parsed.get("session_token"))
            .and_then(|v| v.as_str());

        assert_eq!(session_token, Some("session-token-456"));
    }

    #[test]
    fn test_verify_response_parsing_nested_user() {
        let json = r#"{
            "token": "token123",
            "user": {
                "email": "user@example.com",
                "id": "user-id-abc"
            }
        }"#;
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();

        // Extract email from nested user object
        let email = parsed
            .get("user")
            .and_then(|u| u.get("email"))
            .and_then(|v| v.as_str())
            .or_else(|| parsed.get("email").and_then(|v| v.as_str()));

        assert_eq!(email, Some("user@example.com"));

        // Extract user_id from nested user object
        let user_id = parsed
            .get("user")
            .and_then(|u| u.get("id"))
            .and_then(|v| v.as_str());

        assert_eq!(user_id, Some("user-id-abc"));
    }

    #[test]
    fn test_verify_response_parsing_flat_email() {
        // Test email at root level (fallback)
        let json = r#"{"token": "token", "email": "flat@example.com"}"#;
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();

        let email = parsed
            .get("user")
            .and_then(|u| u.get("email"))
            .and_then(|v| v.as_str())
            .or_else(|| parsed.get("email").and_then(|v| v.as_str()));

        assert_eq!(email, Some("flat@example.com"));
    }

    #[test]
    fn test_verify_response_parsing_workspace_id() {
        let json = r#"{
            "token": "token",
            "workspace_id": "ws-123"
        }"#;
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();

        let workspace_id = parsed
            .get("workspace_id")
            .and_then(|v| v.as_str())
            .map(String::from);

        assert_eq!(workspace_id, Some("ws-123".to_string()));
    }

    #[test]
    fn test_verify_response_fallback_to_user_id() {
        // When workspace_id is absent, use user.id as fallback
        let json = r#"{
            "token": "token",
            "user": {"id": "user-123"}
        }"#;
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();

        let user_id = parsed
            .get("user")
            .and_then(|u| u.get("id"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let workspace_id = parsed
            .get("workspace_id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or(user_id);

        assert_eq!(workspace_id, Some("user-123".to_string()));
    }

    #[test]
    fn test_verify_response_no_token_returns_none() {
        let json = r#"{"email": "test@example.com"}"#;
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();

        let session_token = parsed
            .get("token")
            .or_else(|| parsed.get("session_token"))
            .and_then(|v| v.as_str());

        assert!(session_token.is_none());
    }

    // =========================================================================
    // Default Server URL Tests
    // =========================================================================

    #[test]
    fn test_default_sync_server_constant() {
        assert_eq!(DEFAULT_SYNC_SERVER, "https://sync.diaryx.org");
    }

    #[test]
    fn test_server_url_fallback_logic() {
        let config = Config::default();
        let explicit_server: Option<&str> = None;

        let server_url = explicit_server
            .or(config.sync_server_url.as_deref())
            .unwrap_or(DEFAULT_SYNC_SERVER);

        assert_eq!(server_url, "https://sync.diaryx.org");
    }

    #[test]
    fn test_server_url_uses_explicit() {
        let config = Config::default();
        let explicit_server = Some("https://custom.server.com");

        let server_url = explicit_server
            .or(config.sync_server_url.as_deref())
            .unwrap_or(DEFAULT_SYNC_SERVER);

        assert_eq!(server_url, "https://custom.server.com");
    }

    #[test]
    fn test_server_url_uses_config() {
        let mut config = Config::default();
        config.sync_server_url = Some("https://config.server.com".to_string());
        let explicit_server: Option<&str> = None;

        let server_url = explicit_server
            .or(config.sync_server_url.as_deref())
            .unwrap_or(DEFAULT_SYNC_SERVER);

        assert_eq!(server_url, "https://config.server.com");
    }
}
