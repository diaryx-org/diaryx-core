//! Attachment command handlers

use diaryx_core::config::Config;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::args::AttachmentCommands;
use crate::cli::util::resolve_paths;
use crate::cli::{CliDiaryxAppSync, CliWorkspace, block_on};

/// Handle attachment commands
pub fn handle_attachment_command(
    command: AttachmentCommands,
    ws: &CliWorkspace,
    app: &CliDiaryxAppSync,
    current_dir: &Path,
) {
    // Load config for path resolution
    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "Error: Failed to load config: {}. Run 'diaryx init' first.",
                e
            );
            return;
        }
    };

    match command {
        AttachmentCommands::Add {
            entry,
            attachment,
            copy,
            dry_run,
        } => {
            handle_add(
                ws,
                app,
                &config,
                current_dir,
                &entry,
                &attachment,
                copy,
                dry_run,
            );
        }
        AttachmentCommands::Remove {
            entry,
            attachment,
            delete,
            dry_run,
        } => {
            handle_remove(
                ws,
                app,
                &config,
                current_dir,
                &entry,
                &attachment,
                delete,
                dry_run,
            );
        }
        AttachmentCommands::List { entry } => {
            handle_list(ws, app, &config, &entry);
        }
    }
}

/// Handle 'attachment add' command
fn handle_add(
    _ws: &CliWorkspace,
    app: &CliDiaryxAppSync,
    config: &Config,
    current_dir: &Path,
    entry_arg: &str,
    attachment_arg: &str,
    copy: bool,
    dry_run: bool,
) {
    // Resolve entry path
    let entry_paths = resolve_paths(entry_arg, config, app);
    if entry_paths.is_empty() {
        eprintln!("Error: No entry found matching '{}'", entry_arg);
        return;
    }
    if entry_paths.len() > 1 {
        eprintln!(
            "Error: Multiple entries match '{}'. Please be more specific.",
            entry_arg
        );
        for p in &entry_paths {
            eprintln!("  - {}", p.display());
        }
        return;
    }
    let entry_path = &entry_paths[0];

    // Resolve attachment path
    let attachment_source = if Path::new(attachment_arg).is_absolute() {
        PathBuf::from(attachment_arg)
    } else {
        current_dir.join(attachment_arg)
    };

    if !attachment_source.exists() {
        eprintln!(
            "Error: Attachment file not found: {}",
            attachment_source.display()
        );
        return;
    }

    let attachment_path: String;

    if copy {
        // Get _attachments folder for this entry
        let entry_dir = entry_path.parent().unwrap_or(Path::new("."));
        let attachments_dir = entry_dir.join("_attachments");
        let filename = attachment_source
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "attachment".to_string());
        let dest_path = attachments_dir.join(&filename);

        // Calculate relative path from entry to attachment
        attachment_path = format!("_attachments/{}", filename);

        if dry_run {
            println!(
                "[dry-run] Would create directory: {}",
                attachments_dir.display()
            );
            println!(
                "[dry-run] Would copy {} to {}",
                attachment_source.display(),
                dest_path.display()
            );
            println!(
                "[dry-run] Would add '{}' to attachments in {}",
                attachment_path,
                entry_path.display()
            );
            return;
        }

        // Create _attachments directory if needed
        if let Err(e) = fs::create_dir_all(&attachments_dir) {
            eprintln!("Error: Failed to create attachments directory: {}", e);
            return;
        }

        // Copy the file
        if let Err(e) = fs::copy(&attachment_source, &dest_path) {
            eprintln!("Error: Failed to copy attachment: {}", e);
            return;
        }
        println!(
            "Copied {} -> {}",
            attachment_source.display(),
            dest_path.display()
        );
    } else {
        // Use the literal path provided
        attachment_path = attachment_arg.to_string();

        if dry_run {
            println!(
                "[dry-run] Would add '{}' to attachments in {}",
                attachment_path,
                entry_path.display()
            );
            return;
        }
    }

    // Add to attachments property (uses &str, not Path)
    let entry_path_str = entry_path.to_string_lossy();
    match app.add_attachment(&entry_path_str, &attachment_path) {
        Ok(_) => {
            println!(
                "Added attachment '{}' to {}",
                attachment_path,
                entry_path.display()
            );
        }
        Err(e) => {
            eprintln!("Error: Failed to add attachment: {}", e);
        }
    }
}

/// Handle 'attachment remove' command
fn handle_remove(
    _ws: &CliWorkspace,
    app: &CliDiaryxAppSync,
    config: &Config,
    current_dir: &Path,
    entry_arg: &str,
    attachment_arg: &str,
    delete: bool,
    dry_run: bool,
) {
    // Resolve entry path
    let entry_paths = resolve_paths(entry_arg, config, app);
    if entry_paths.is_empty() {
        eprintln!("Error: No entry found matching '{}'", entry_arg);
        return;
    }
    if entry_paths.len() > 1 {
        eprintln!(
            "Error: Multiple entries match '{}'. Please be more specific.",
            entry_arg
        );
        return;
    }
    let entry_path = &entry_paths[0];

    if dry_run {
        println!(
            "[dry-run] Would remove '{}' from attachments in {}",
            attachment_arg,
            entry_path.display()
        );
        if delete {
            let entry_dir = entry_path.parent().unwrap_or(current_dir);
            let full_path = entry_dir.join(attachment_arg);
            println!("[dry-run] Would delete file: {}", full_path.display());
        }
        return;
    }

    // Remove from attachments property (uses &str, not Path)
    let entry_path_str = entry_path.to_string_lossy();
    match app.remove_attachment(&entry_path_str, attachment_arg) {
        Ok(_) => {
            println!(
                "Removed attachment '{}' from {}",
                attachment_arg,
                entry_path.display()
            );
        }
        Err(e) => {
            eprintln!("Error: Failed to remove attachment: {}", e);
            return;
        }
    }

    // Optionally delete the file
    if delete {
        let entry_dir = entry_path.parent().unwrap_or(current_dir);
        let full_path = entry_dir.join(attachment_arg);
        if full_path.exists() {
            if let Err(e) = fs::remove_file(&full_path) {
                eprintln!("Warning: Failed to delete file: {}", e);
            } else {
                println!("Deleted file: {}", full_path.display());
            }
        }
    }
}

/// Handle 'attachment list' command
fn handle_list(ws: &CliWorkspace, app: &CliDiaryxAppSync, config: &Config, entry_arg: &str) {
    // Resolve entry path
    let entry_paths = resolve_paths(entry_arg, config, app);
    if entry_paths.is_empty() {
        eprintln!("Error: No entry found matching '{}'", entry_arg);
        return;
    }
    if entry_paths.len() > 1 {
        eprintln!(
            "Error: Multiple entries match '{}'. Please be more specific.",
            entry_arg
        );
        return;
    }
    let entry_path = &entry_paths[0];

    // Parse the entry to get attachments
    match block_on(ws.parse_index(entry_path)) {
        Ok(index) => {
            let attachments = index.frontmatter.attachments_list();
            if attachments.is_empty() {
                println!("No attachments for {}", entry_path.display());
            } else {
                println!("Attachments for {}:", entry_path.display());
                for (i, att) in attachments.iter().enumerate() {
                    println!("  {}. {}", i + 1, att);
                }
            }
        }
        Err(e) => {
            eprintln!("Error: Failed to parse entry: {}", e);
        }
    }
}
