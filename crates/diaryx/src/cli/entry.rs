//! Entry command handlers (today, yesterday, open, create, config)

use std::path::Path;

use diaryx_core::config::Config;
use diaryx_core::date::parse_date;

use crate::editor::launch_editor;

use crate::cli::CliDiaryxAppSync;
use crate::cli::util::{load_config, resolve_paths};

/// Handle the 'today' command
/// Returns true on success, false on error
pub fn handle_today(app: &CliDiaryxAppSync, template: Option<String>) -> bool {
    let config = match load_config() {
        Some(c) => c,
        None => return false,
    };

    match parse_date("today") {
        Ok(date) => {
            match app.ensure_dated_entry_with_template(&date, &config, template.as_deref()) {
                Ok(path) => {
                    println!("Opening: {}", path.display());
                    if let Err(e) = launch_editor(&path, &config) {
                        eprintln!("✗ Error launching editor: {}", e);
                        return false;
                    }
                    // Note: touch_updated is on async DiaryxApp, not DiaryxAppSync
                    // TODO: Add touch_updated to DiaryxAppSync or migrate to async
                    true
                }
                Err(e) => {
                    eprintln!("✗ Error creating entry: {}", e);
                    false
                }
            }
        }
        Err(e) => {
            eprintln!("✗ Error parsing date: {}", e);
            false
        }
    }
}

/// Handle the 'yesterday' command
/// Returns true on success, false on error
pub fn handle_yesterday(app: &CliDiaryxAppSync, template: Option<String>) -> bool {
    let config = match load_config() {
        Some(c) => c,
        None => return false,
    };

    match parse_date("yesterday") {
        Ok(date) => {
            match app.ensure_dated_entry_with_template(&date, &config, template.as_deref()) {
                Ok(path) => {
                    println!("Opening: {}", path.display());
                    if let Err(e) = launch_editor(&path, &config) {
                        eprintln!("✗ Error launching editor: {}", e);
                        return false;
                    }
                    // Note: touch_updated is on async DiaryxApp, not DiaryxAppSync
                    // TODO: Add touch_updated to DiaryxAppSync or migrate to async
                    true
                }
                Err(e) => {
                    eprintln!("✗ Error creating entry: {}", e);
                    false
                }
            }
        }
        Err(e) => {
            eprintln!("✗ Error parsing date: {}", e);
            false
        }
    }
}

/// Handle the 'open' command
/// Supports:
/// - Date strings: "today", "yesterday", "last friday", "2024-01-15"
/// - Fuzzy file matching: "README" -> README.md, "dia" -> diary.md
/// - Exact paths: "./notes/todo.md"
/// - Globs open multiple files: "*.md"
/// - Directories open all workspace files: "."
/// Returns true on success, false on error
pub fn handle_open(app: &CliDiaryxAppSync, path_or_date: &str) -> bool {
    let config = match load_config() {
        Some(c) => c,
        None => return false,
    };

    // Use shared path resolution (handles directories, globs, fuzzy matching, dates)
    let paths = resolve_paths(path_or_date, &config, app);

    if paths.is_empty() {
        eprintln!("✗ No files matched: {}", path_or_date);
        return false;
    }

    let mut had_error = false;

    // For single files that don't exist, check if this was meant as a date
    if paths.len() == 1 && !paths[0].exists() {
        // Try to parse as a date and create the entry
        if let Ok(date) = parse_date(path_or_date) {
            match app.ensure_dated_entry(&date, &config) {
                Ok(path) => {
                    println!("Opening: {}", path.display());
                    if let Err(e) = launch_editor(&path, &config) {
                        eprintln!("✗ Error launching editor: {}", e);
                        return false;
                    }
                    // Note: touch_updated is on async DiaryxApp, not DiaryxAppSync
                    // TODO: Add touch_updated to DiaryxAppSync or migrate to async
                    return true;
                }
                Err(e) => {
                    eprintln!("✗ Error creating entry: {}", e);
                    return false;
                }
            }
        }
        // Not a date and file doesn't exist
        eprintln!("✗ File not found: {}", paths[0].display());
        return false;
    }

    // Open all resolved files
    for path in &paths {
        if !path.exists() {
            eprintln!("✗ File not found: {}", path.display());
            had_error = true;
            continue;
        }

        if paths.len() > 1 {
            println!("Opening: {}", path.display());
        }

        if let Err(e) = launch_editor(path, &config) {
            eprintln!("✗ Error launching editor for {}: {}", path.display(), e);
            had_error = true;
        }
        // Note: touch_updated is on async DiaryxApp, not DiaryxAppSync
        // TODO: Add touch_updated to DiaryxAppSync or migrate to async
    }

    !had_error
}

/// Handle the 'create' command
/// Supports fuzzy path resolution for the parent directory
/// Returns true on success, false on error
pub fn handle_create(
    app: &CliDiaryxAppSync,
    path: &str,
    template: Option<String>,
    title: Option<String>,
) -> bool {
    let config = match load_config() {
        Some(c) => c,
        None => return false,
    };

    let path_buf = Path::new(path);

    // Create parent directories if they don't exist
    if let Some(parent) = path_buf.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        eprintln!("✗ Error creating directories: {}", e);
        return false;
    }

    // Use template-based creation
    let workspace_dir = Some(config.default_workspace.as_path());
    match app.create_entry_from_template(
        path_buf,
        template.as_deref().or(config.default_template.as_deref()),
        title.as_deref(),
        workspace_dir,
    ) {
        Ok(_) => {
            println!("✓ Created entry: {}", path);
            true
        }
        Err(e) => {
            eprintln!("✗ Error creating entry: {}", e);
            false
        }
    }
}

/// Handle the 'config' command
pub fn handle_config() {
    match Config::load() {
        Ok(config) => {
            println!("Current configuration:");
            println!(
                "  Default workspace: {}",
                config.default_workspace.display()
            );
            println!(
                "  Daily entry folder: {}",
                config
                    .daily_entry_folder
                    .as_deref()
                    .unwrap_or("(workspace root)")
            );
            println!(
                "  Editor: {}",
                config.editor.as_deref().unwrap_or("$EDITOR")
            );
            println!(
                "  Default template: {}",
                config.default_template.as_deref().unwrap_or("none")
            );
            println!(
                "  Daily template: {}",
                config.daily_template.as_deref().unwrap_or("none")
            );
            if let Some(config_path) = Config::config_path() {
                println!("\nConfig file: {}", config_path.display());
            }
        }
        Err(e) => {
            eprintln!("✗ Error loading config: {}", e);
            eprintln!("  Run 'diaryx init' to create a config file");
        }
    }
}
