// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

fn main() {
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
