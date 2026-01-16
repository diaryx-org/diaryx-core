//! # Diaryx Tauri Library
//!
//! This is the library file for the Tauri backend.
//!

/// Where all the Tauri `invoke` functions are defined.
mod commands;

use commands::CrdtState;

/// Cloud backup targets (S3, Google Drive, etc.)
mod cloud;

/// Run function used by Tauri clients. Builds Tauri plugins and invokable commands.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    log::info!("Starting Diaryx application...");

    tauri::Builder::default()
        // Stronghold plugin for secure credential storage
        .plugin(
            tauri_plugin_stronghold::Builder::new(|password| {
                // Use argon2 for password hashing
                use argon2::{Config, Variant, Version, hash_raw};
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
        // CRDT state for version history and sync
        .manage(CrdtState::new())
        .invoke_handler(tauri::generate_handler![
            // ============================================================
            // UNIFIED COMMAND API - All operations go through execute()
            // ============================================================
            commands::execute,
            // ============================================================
            // PLATFORM-SPECIFIC COMMANDS
            // These cannot be moved to execute() as they require platform
            // features (file dialogs, cloud auth, app paths, etc.)
            // ============================================================

            // App initialization (iOS-compatible)
            commands::initialize_app,
            commands::get_app_paths,
            commands::pick_workspace_folder,
            // Backup (local filesystem)
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
            // Cloud Sync (bidirectional)
            commands::sync_to_s3,
            commands::sync_to_google_drive,
            commands::get_sync_status,
            commands::resolve_sync_conflict,
            // Import (file picker dialogs)
            commands::import_from_zip,
            commands::pick_and_import_zip,
            commands::import_from_zip_data,
            // Chunked Import (for large files)
            commands::start_import_upload,
            commands::append_import_chunk,
            commands::finish_import_upload,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
