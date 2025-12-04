//! Entry command handlers (today, yesterday, open, create)

use diaryx_core::config::Config;
use diaryx_core::date::parse_date;
use diaryx_core::editor::launch_editor;
use diaryx_core::entry::DiaryxApp;
use diaryx_core::fs::RealFileSystem;

use crate::cli::util::load_config;

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
pub fn handle_open(app: &DiaryxApp<RealFileSystem>, date: &str) {
    let config = match load_config() {
        Some(c) => c,
        None => return,
    };

    match parse_date(date) {
        Ok(parsed_date) => match app.ensure_dated_entry(&parsed_date, &config) {
            Ok(path) => {
                println!("Opening: {}", path.display());
                if let Err(e) = launch_editor(&path, &config) {
                    eprintln!("✗ Error launching editor: {}", e);
                }
            }
            Err(e) => eprintln!("✗ Error creating entry: {}", e),
        },
        Err(e) => eprintln!("✗ {}", e),
    }
}

/// Handle the 'create' command
pub fn handle_create(app: &DiaryxApp<RealFileSystem>, path: &str) {
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
