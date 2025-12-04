//! Entry command handlers (today, yesterday, open, create, config)

use diaryx_core::config::Config;
use diaryx_core::date::parse_date;
use diaryx_core::editor::launch_editor;
use diaryx_core::entry::DiaryxApp;
use diaryx_core::fs::RealFileSystem;

use crate::cli::util::{load_config, resolve_paths};

/// Handle the 'today' command
pub fn handle_today(app: &DiaryxApp<RealFileSystem>) {
    let config = match load_config() {
        Some(c) => c,
        None => return,
    };

    match parse_date("today") {
        Ok(date) => match app.ensure_dated_entry(&date, &config) {
            Ok(path) => {
                println!("Opening: {}", path.display());
                if let Err(e) = launch_editor(&path, &config) {
                    eprintln!("✗ Error launching editor: {}", e);
                }
            }
            Err(e) => eprintln!("✗ Error creating entry: {}", e),
        },
        Err(e) => eprintln!("✗ Error parsing date: {}", e),
    }
}

/// Handle the 'yesterday' command
pub fn handle_yesterday(app: &DiaryxApp<RealFileSystem>) {
    let config = match load_config() {
        Some(c) => c,
        None => return,
    };

    match parse_date("yesterday") {
        Ok(date) => match app.ensure_dated_entry(&date, &config) {
            Ok(path) => {
                println!("Opening: {}", path.display());
                if let Err(e) = launch_editor(&path, &config) {
                    eprintln!("✗ Error launching editor: {}", e);
                }
            }
            Err(e) => eprintln!("✗ Error creating entry: {}", e),
        },
        Err(e) => eprintln!("✗ Error parsing date: {}", e),
    }
}

/// Handle the 'open' command
/// Supports:
/// - Date strings: "today", "yesterday", "last friday", "2024-01-15"
/// - Fuzzy file matching: "README" -> README.md, "dia" -> diary.md
/// - Exact paths: "./notes/todo.md"
/// - Globs open multiple files: "*.md"
/// - Directories open all workspace files: "."
pub fn handle_open(app: &DiaryxApp<RealFileSystem>, path_or_date: &str) {
    let config = match load_config() {
        Some(c) => c,
        None => return,
    };

    // Use shared path resolution (handles directories, globs, fuzzy matching, dates)
    let paths = resolve_paths(path_or_date, &config, app);

    if paths.is_empty() {
        eprintln!("✗ No files matched: {}", path_or_date);
        return;
    }

    // For single files that don't exist, check if this was meant as a date
    if paths.len() == 1 && !paths[0].exists() {
        // Try to parse as a date and create the entry
        if let Ok(date) = parse_date(path_or_date) {
            match app.ensure_dated_entry(&date, &config) {
                Ok(path) => {
                    println!("Opening: {}", path.display());
                    if let Err(e) = launch_editor(&path, &config) {
                        eprintln!("✗ Error launching editor: {}", e);
                    }
                    return;
                }
                Err(e) => {
                    eprintln!("✗ Error creating entry: {}", e);
                    return;
                }
            }
        }
        // Not a date and file doesn't exist
        eprintln!("✗ File not found: {}", paths[0].display());
        return;
    }

    // Open all resolved files
    for path in &paths {
        if !path.exists() {
            eprintln!("✗ File not found: {}", path.display());
            continue;
        }

        if paths.len() > 1 {
            println!("Opening: {}", path.display());
        }

        if let Err(e) = launch_editor(path, &config) {
            eprintln!("✗ Error launching editor for {}: {}", path.display(), e);
        }
    }
}

/// Handle the 'create' command
/// Supports fuzzy path resolution for the parent directory
pub fn handle_create(app: &DiaryxApp<RealFileSystem>, path: &str) {
    // For create, we use the path as-is since we're creating a new file
    // But we could resolve the parent directory
    match app.create_entry(path) {
        Ok(_) => println!("✓ Created entry: {}", path),
        Err(e) => eprintln!("✗ Error creating entry: {}", e),
    }
}

/// Handle the 'config' command
pub fn handle_config() {
    match Config::load() {
        Ok(config) => {
            println!("Current configuration:");
            println!("  Base directory: {}", config.base_dir.display());
            println!(
                "  Editor: {}",
                config.editor.as_deref().unwrap_or("$EDITOR")
            );
            println!(
                "  Default template: {}",
                config.default_template.as_deref().unwrap_or("none")
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
