// src-tauri/src/lib.rs

// 1. Declare the commands module here so the library owns it
mod commands;

// 2. Create the run function that mobile and desktop will both use
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::get_workspace_tree,
            commands::get_entry,
            commands::save_entry,
            commands::search_workspace,
            commands::create_entry,
            commands::get_frontmatter,
            commands::set_frontmatter_property,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
