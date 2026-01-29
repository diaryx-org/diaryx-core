//! Status and config command handlers for sync.
//!
//! Handles status display and configuration management.

use std::path::Path;

use diaryx_core::config::Config;
use diaryx_core::crdt::SqliteStorage;

/// Handle the status command - show sync status.
pub fn handle_status(config: &Config, workspace_root: &Path) {
    println!("Sync Status");
    println!("===========");
    println!();

    // Server configuration
    if let Some(server) = &config.sync_server_url {
        println!("Server: {}", server);
    } else {
        println!("Server: (not configured)");
    }

    // Account status
    if let Some(email) = &config.sync_email {
        if config.sync_session_token.is_some() {
            println!("Account: {} (logged in)", email);
        } else {
            println!("Account: {} (not logged in)", email);
        }
    } else {
        println!("Account: (not configured)");
    }

    // Workspace ID
    if let Some(workspace_id) = &config.sync_workspace_id {
        println!("Workspace ID: {}", workspace_id);
    } else {
        println!("Workspace ID: (not configured)");
    }

    // Local workspace
    println!("Workspace root: {}", workspace_root.display());

    // CRDT database status
    let crdt_db = workspace_root.join(".diaryx").join("crdt.db");
    if crdt_db.exists() {
        println!("CRDT database: {}", crdt_db.display());

        // Try to get some stats from the database
        if let Ok(storage) = SqliteStorage::open(&crdt_db) {
            if let Ok(files) = storage.query_active_files() {
                println!("  Files tracked: {}", files.len());
            }
        }
    } else {
        println!("CRDT database: (not initialized)");
    }

    // Quick check if we can sync
    println!();
    if config.sync_session_token.is_none() {
        println!("To start syncing, first log in:");
        println!("  diaryx sync login <your-email>");
    } else if config.sync_workspace_id.is_none() {
        println!("Workspace ID not configured. It will be set automatically when syncing,");
        println!("or you can set it manually:");
        println!("  diaryx sync config --workspace-id <id>");
    } else {
        println!("Ready to sync! Start with:");
        println!("  diaryx sync start");
    }
}

/// Handle the config command - configure sync settings.
pub fn handle_config(
    config: &Config,
    server: Option<String>,
    workspace_id: Option<String>,
    show: bool,
) {
    // If --show or no options, display current config
    if show || (server.is_none() && workspace_id.is_none()) {
        println!("Sync Configuration");
        println!("==================");
        println!();
        println!(
            "Server URL: {}",
            config.sync_server_url.as_deref().unwrap_or("(not set)")
        );
        println!(
            "Email: {}",
            config.sync_email.as_deref().unwrap_or("(not set)")
        );
        println!(
            "Session: {}",
            if config.sync_session_token.is_some() {
                "active"
            } else {
                "(not logged in)"
            }
        );
        println!(
            "Workspace ID: {}",
            config.sync_workspace_id.as_deref().unwrap_or("(not set)")
        );
        return;
    }

    // Update configuration
    let mut new_config = config.clone();
    let mut changes = Vec::new();

    if let Some(s) = server {
        new_config.sync_server_url = Some(s.clone());
        changes.push(format!("Server URL: {}", s));
    }

    if let Some(wid) = workspace_id {
        new_config.sync_workspace_id = Some(wid.clone());
        changes.push(format!("Workspace ID: {}", wid));
    }

    if changes.is_empty() {
        println!("No changes made.");
        return;
    }

    // Save config
    match new_config.save() {
        Ok(()) => {
            println!("Configuration updated:");
            for change in changes {
                println!("  {}", change);
            }
        }
        Err(e) => {
            eprintln!("Failed to save configuration: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a default config for testing.
    fn create_test_config() -> Config {
        Config::default()
    }

    // =========================================================================
    // Config State Tests
    // =========================================================================

    #[test]
    fn test_config_with_all_fields() {
        let mut config = create_test_config();
        config.sync_server_url = Some("https://custom.server.com".to_string());
        config.sync_email = Some("test@example.com".to_string());
        config.sync_session_token = Some("token123".to_string());
        config.sync_workspace_id = Some("workspace-abc".to_string());

        // Verify all fields are set
        assert_eq!(
            config.sync_server_url.as_deref(),
            Some("https://custom.server.com")
        );
        assert_eq!(config.sync_email.as_deref(), Some("test@example.com"));
        assert!(config.sync_session_token.is_some());
        assert_eq!(config.sync_workspace_id.as_deref(), Some("workspace-abc"));
    }

    #[test]
    fn test_config_default_values() {
        let config = create_test_config();

        assert!(config.sync_server_url.is_none());
        assert!(config.sync_email.is_none());
        assert!(config.sync_session_token.is_none());
        assert!(config.sync_workspace_id.is_none());
    }

    #[test]
    fn test_config_logged_in_status() {
        let mut config = create_test_config();

        // Not logged in by default
        assert!(config.sync_session_token.is_none());

        // Set token to simulate logged in state
        config.sync_session_token = Some("token".to_string());
        assert!(config.sync_session_token.is_some());
    }

    #[test]
    fn test_config_partial_setup() {
        let mut config = create_test_config();

        // User has logged in but workspace ID not yet set
        config.sync_email = Some("user@example.com".to_string());
        config.sync_session_token = Some("token".to_string());
        config.sync_server_url = Some("https://sync.diaryx.org".to_string());
        // workspace_id is None

        assert!(config.sync_session_token.is_some());
        assert!(config.sync_workspace_id.is_none());
    }

    // =========================================================================
    // Config Update Logic Tests
    // =========================================================================

    #[test]
    fn test_config_update_server_url() {
        let mut config = create_test_config();

        // Simulate updating server URL
        let new_server = Some("https://new.server.com".to_string());
        if let Some(s) = new_server {
            config.sync_server_url = Some(s);
        }

        assert_eq!(
            config.sync_server_url.as_deref(),
            Some("https://new.server.com")
        );
    }

    #[test]
    fn test_config_update_workspace_id() {
        let mut config = create_test_config();

        // Simulate updating workspace ID
        let new_workspace = Some("new-workspace-id".to_string());
        if let Some(wid) = new_workspace {
            config.sync_workspace_id = Some(wid);
        }

        assert_eq!(
            config.sync_workspace_id.as_deref(),
            Some("new-workspace-id")
        );
    }

    #[test]
    fn test_config_update_both_fields() {
        let mut config = create_test_config();

        config.sync_server_url = Some("https://server.com".to_string());
        config.sync_workspace_id = Some("workspace-id".to_string());

        assert!(config.sync_server_url.is_some());
        assert!(config.sync_workspace_id.is_some());
    }

    #[test]
    fn test_config_no_changes_when_none() {
        let config = create_test_config();

        // Simulate handle_config with no options
        let server: Option<String> = None;
        let workspace_id: Option<String> = None;

        let mut changes = Vec::new();
        if let Some(s) = server {
            changes.push(format!("Server URL: {}", s));
        }
        if let Some(wid) = workspace_id {
            changes.push(format!("Workspace ID: {}", wid));
        }

        assert!(changes.is_empty(), "No changes should be recorded");
        assert!(config.sync_server_url.is_none()); // Config unchanged
    }

    // =========================================================================
    // Status Display Logic Tests
    // =========================================================================

    #[test]
    fn test_status_session_active_display() {
        let mut config = create_test_config();
        config.sync_session_token = Some("token".to_string());

        let session_display = if config.sync_session_token.is_some() {
            "active"
        } else {
            "(not logged in)"
        };

        assert_eq!(session_display, "active");
    }

    #[test]
    fn test_status_session_inactive_display() {
        let config = create_test_config();

        let session_display = if config.sync_session_token.is_some() {
            "active"
        } else {
            "(not logged in)"
        };

        assert_eq!(session_display, "(not logged in)");
    }

    #[test]
    fn test_status_optional_field_display() {
        let config = create_test_config();

        // Test display fallback for None values
        let server_display = config.sync_server_url.as_deref().unwrap_or("(not set)");
        let email_display = config.sync_email.as_deref().unwrap_or("(not set)");
        let workspace_display = config.sync_workspace_id.as_deref().unwrap_or("(not set)");

        assert_eq!(server_display, "(not set)");
        assert_eq!(email_display, "(not set)");
        assert_eq!(workspace_display, "(not set)");
    }

    #[test]
    fn test_status_with_configured_fields() {
        let mut config = create_test_config();
        config.sync_server_url = Some("https://sync.example.com".to_string());
        config.sync_email = Some("user@example.com".to_string());
        config.sync_workspace_id = Some("ws-123".to_string());

        let server_display = config.sync_server_url.as_deref().unwrap_or("(not set)");
        let email_display = config.sync_email.as_deref().unwrap_or("(not set)");
        let workspace_display = config.sync_workspace_id.as_deref().unwrap_or("(not set)");

        assert_eq!(server_display, "https://sync.example.com");
        assert_eq!(email_display, "user@example.com");
        assert_eq!(workspace_display, "ws-123");
    }
}
