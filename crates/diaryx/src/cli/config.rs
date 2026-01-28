//! Config command handlers

use diaryx_core::config::Config;
use diaryx_core::link_parser::LinkFormat;
use std::path::PathBuf;

use crate::cli::args::ConfigCommands;
use crate::cli::{CliWorkspace, block_on};

pub fn handle_config_command(
    command: Option<ConfigCommands>,
    workspace_override: Option<PathBuf>,
    ws: &CliWorkspace,
) {
    let config = Config::load().ok();
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    match command {
        None => {
            // Show all config (backward compatibility)
            show_config(&config);
        }
        Some(ConfigCommands::Show) => {
            // Show workspace config from root index
            show_workspace_config(workspace_override, ws, &config, &current_dir);
        }
        Some(ConfigCommands::LinkFormat { format }) => match format {
            Some(fmt) => set_link_format(workspace_override, ws, &config, &current_dir, &fmt),
            None => show_link_format(workspace_override, ws, &config, &current_dir),
        },
    }
}

/// Show basic diaryx configuration
fn show_config(config: &Option<Config>) {
    match config {
        Some(cfg) => {
            println!("Diaryx Configuration");
            println!("====================");
            println!("Default workspace: {}", cfg.default_workspace.display());
            if let Some(ref daily) = cfg.daily_entry_folder {
                println!("Daily entry folder: {}", daily);
            }
            if let Some(config_path) = Config::config_path() {
                println!("Config file: {}", config_path.display());
            }
        }
        None => {
            eprintln!("No configuration found. Run 'diaryx init' first.");
        }
    }
}

/// Show workspace configuration from root index frontmatter
fn show_workspace_config(
    workspace_override: Option<PathBuf>,
    ws: &CliWorkspace,
    config: &Option<Config>,
    current_dir: &std::path::Path,
) {
    let root_index = match find_workspace_root(workspace_override, ws, config, current_dir) {
        Some(path) => path,
        None => {
            eprintln!("No workspace found. Run 'diaryx init' or 'diaryx workspace init' first.");
            return;
        }
    };

    match block_on(ws.get_workspace_config(&root_index)) {
        Ok(ws_config) => {
            println!("Workspace Configuration");
            println!("=======================");
            println!("Root index: {}", root_index.display());
            println!(
                "Link format: {}",
                format_link_format_display(ws_config.link_format)
            );
            if let Some(daily) = ws_config.daily_entry_folder {
                println!("Daily entry folder: {}", daily);
            }
        }
        Err(e) => {
            eprintln!("Error reading workspace config: {}", e);
        }
    }
}

/// Show current link format
fn show_link_format(
    workspace_override: Option<PathBuf>,
    ws: &CliWorkspace,
    config: &Option<Config>,
    current_dir: &std::path::Path,
) {
    let root_index = match find_workspace_root(workspace_override, ws, config, current_dir) {
        Some(path) => path,
        None => {
            eprintln!("No workspace found. Run 'diaryx init' or 'diaryx workspace init' first.");
            return;
        }
    };

    match block_on(ws.get_link_format(&root_index)) {
        Ok(format) => {
            println!("{}", format_link_format_display(format));
            println!();
            println!("Available formats:");
            println!(
                "  markdown_root     - [Title](/path/to/file.md) (default, clickable in editors)"
            );
            println!("  markdown_relative - [Title](../relative/path.md)");
            println!("  plain_relative    - ../relative/path.md");
            println!("  plain_canonical   - path/to/file.md");
        }
        Err(e) => {
            eprintln!("Error reading link format: {}", e);
        }
    }
}

/// Set link format
fn set_link_format(
    workspace_override: Option<PathBuf>,
    ws: &CliWorkspace,
    config: &Option<Config>,
    current_dir: &std::path::Path,
    format_str: &str,
) {
    let root_index = match find_workspace_root(workspace_override, ws, config, current_dir) {
        Some(path) => path,
        None => {
            eprintln!("No workspace found. Run 'diaryx init' or 'diaryx workspace init' first.");
            return;
        }
    };

    let format = match parse_link_format(format_str) {
        Some(f) => f,
        None => {
            eprintln!("Invalid link format: {}", format_str);
            eprintln!(
                "Valid formats: markdown_root, markdown_relative, plain_relative, plain_canonical"
            );
            return;
        }
    };

    match block_on(ws.set_link_format(&root_index, format)) {
        Ok(()) => {
            println!("Link format set to: {}", format_link_format_display(format));
            println!();
            println!("Note: Existing links are not automatically converted.");
            println!("To convert existing links, run:");
            println!("  diaryx workspace convert-links --format {}", format_str);
        }
        Err(e) => {
            eprintln!("Error setting link format: {}", e);
        }
    }
}

/// Parse a link format string into the enum
pub fn parse_link_format(s: &str) -> Option<LinkFormat> {
    match s.to_lowercase().as_str() {
        "markdown_root" | "markdownroot" => Some(LinkFormat::MarkdownRoot),
        "markdown_relative" | "markdownrelative" => Some(LinkFormat::MarkdownRelative),
        "plain_relative" | "plainrelative" => Some(LinkFormat::PlainRelative),
        "plain_canonical" | "plaincanonical" => Some(LinkFormat::PlainCanonical),
        _ => None,
    }
}

/// Format a link format enum for display
pub fn format_link_format_display(format: LinkFormat) -> &'static str {
    match format {
        LinkFormat::MarkdownRoot => "markdown_root",
        LinkFormat::MarkdownRelative => "markdown_relative",
        LinkFormat::PlainRelative => "plain_relative",
        LinkFormat::PlainCanonical => "plain_canonical",
    }
}

/// Find the workspace root index file
fn find_workspace_root(
    workspace_override: Option<PathBuf>,
    ws: &CliWorkspace,
    config: &Option<Config>,
    current_dir: &std::path::Path,
) -> Option<PathBuf> {
    // If override is provided and is a file, use it directly
    if let Some(ref override_path) = workspace_override {
        if override_path.extension().is_some_and(|ext| ext == "md") {
            return Some(override_path.clone());
        }
        // If it's a directory, find root index in it
        if let Ok(Some(root)) = block_on(ws.find_root_index_in_dir(override_path)) {
            return Some(root);
        }
    }

    // Try to detect workspace from current directory
    if let Ok(Some(detected)) = block_on(ws.detect_workspace(current_dir)) {
        return Some(detected);
    }

    // Fall back to config's default workspace
    if let Some(cfg) = config {
        if let Ok(Some(root)) = block_on(ws.find_root_index_in_dir(&cfg.default_workspace)) {
            return Some(root);
        }
    }

    None
}
