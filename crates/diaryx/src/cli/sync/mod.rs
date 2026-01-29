//! Sync command handlers for the CLI.
//!
//! This module provides the `diaryx sync` command family for syncing
//! workspace metadata and file content with a remote sync server.

mod auth;
mod client;
mod progress;
mod status;

use std::path::PathBuf;

use diaryx_core::config::Config;

use crate::cli::args::SyncCommands;

/// Handle sync subcommands.
pub fn handle_sync_command(command: SyncCommands, workspace_override: Option<PathBuf>) {
    // Load config
    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error loading config: {}", e);
            return;
        }
    };

    // Determine workspace root
    let workspace_root = workspace_override.unwrap_or_else(|| config.default_workspace.clone());

    match command {
        SyncCommands::Login { email, server } => {
            auth::handle_login(&config, &email, server.as_deref());
        }
        SyncCommands::Verify { token, device_name } => {
            auth::handle_verify(&config, &token, device_name.as_deref());
        }
        SyncCommands::Logout => {
            auth::handle_logout(&config);
        }
        SyncCommands::Status => {
            status::handle_status(&config, &workspace_root);
        }
        SyncCommands::Start { background } => {
            if background {
                eprintln!("Background mode is not yet implemented.");
                eprintln!("Running in foreground mode instead.");
            }
            client::handle_start(&config, &workspace_root);
        }
        SyncCommands::Push { force: _ } => {
            client::handle_push(&config, &workspace_root);
        }
        SyncCommands::Pull { force: _ } => {
            client::handle_pull(&config, &workspace_root);
        }
        SyncCommands::Config {
            server,
            workspace_id,
            show,
        } => {
            status::handle_config(&config, server, workspace_id, show);
        }
    }
}
