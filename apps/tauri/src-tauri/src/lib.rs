//! # Diaryx Tauri Library
//!
//! This is the library file for the Tauri backend.
//!

/// Where all the Tauri `invoke` functions are defined.
mod commands;
mod commands_sync;

/// Cloud backup targets (S3, Google Drive, etc.)
mod cloud;

/// Live sync (WebSocket-based collaboration)
pub mod sync;

/// Run function used by Tauri clients. Builds Tauri plugins and invokable commands.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    log::info!("Starting Diaryx application...");

    // Initialize sync state
    let sync_state = std::sync::Arc::new(
        std::sync::Mutex::new(commands_sync::SyncState::new())
    );

    tauri::Builder::default()
        .manage(sync_state)
        // Stronghold plugin for secure credential storage
        .plugin(
            tauri_plugin_stronghold::Builder::new(|password| {
                // Use argon2 for password hashing
                use argon2::{hash_raw, Config, Variant, Version};
                let config = Config {
                    lanes: 4,
                    mem_cost: 10_000,
                    time_cost: 10,
                    variant: Variant::Argon2id,
                    version: Version::Version13,
                    ..Default::default()
                };
                let salt = "diaryx-stronghold-salt";
                let key = hash_raw(password.as_bytes(), salt.as_bytes(), &config)
                    .expect("Failed to hash password");
                key.try_into().expect("Hash should be 32 bytes")
            })
            .build(),
        )
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_google_auth::init())
        .invoke_handler(tauri::generate_handler![
            // App initialization (iOS-compatible)
            commands::initialize_app,
            commands::get_app_paths,
            commands::create_workspace,
            // Configuration
            commands::get_config,
            commands::save_config,
            // Workspace
            commands::get_workspace_tree,
            commands::get_filesystem_tree,
            commands::validate_workspace,
            // Entries
            commands::get_entry,
            commands::save_entry,
            commands::create_entry,
            commands::delete_entry,
            commands::move_entry,
            commands::attach_entry_to_parent,
            commands::convert_to_index,
            commands::convert_to_leaf,
            commands::create_child_entry,
            commands::rename_entry,
            commands::ensure_daily_entry,
            // Search
            commands::search_workspace,
            // Frontmatter
            commands::get_frontmatter,
            commands::set_frontmatter_property,
            commands::remove_frontmatter_property,
            // Attachments
            commands::get_attachments,
            commands::upload_attachment,
            commands::delete_attachment,
            commands::get_attachment_data,
            commands::get_storage_usage,
            // Export
            commands::get_available_audiences,
            commands::plan_export,
            commands::export_to_memory,
            commands::export_to_html,
            commands::export_binary_attachments,
            // Backup
            commands::backup_workspace,
            commands::restore_workspace,
            commands::list_backup_targets,
            // Cloud Backup (S3)
            commands::test_s3_connection,
            commands::backup_to_s3,
            commands::restore_from_s3,
            // Cloud Backup (Google Drive)
            commands::get_google_auth_config,
            commands::backup_to_google_drive,
            // Import
            commands::import_from_zip,
            commands::pick_and_import_zip,
            commands::import_from_zip_data,
            // Chunked Import (for large files)
            commands::start_import_upload,
            commands::append_import_chunk,
            commands::finish_import_upload,
            // Live Sync
            commands_sync::start_live_sync,
            commands_sync::stop_live_sync,
            commands_sync::get_sync_status,
            commands_sync::sync_round,
            commands_sync::update_synced_document,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
