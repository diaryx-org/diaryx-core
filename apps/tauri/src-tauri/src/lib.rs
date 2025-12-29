//! # Diaryx Tauri Library
//!
//! This is the library file for the Tauri backend.
//!

/// Where all the Tauri `invoke` functions are defined.
mod commands;

/// Run function used by Tauri clients. Builds Tauri plugins and invokable commands.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    log::info!("Starting Diaryx application...");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
