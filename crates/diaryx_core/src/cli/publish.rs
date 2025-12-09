//! CLI handler for publish command

use std::path::PathBuf;

use diaryx_core::fs::RealFileSystem;
use diaryx_core::publish::{PublishOptions, Publisher};
use diaryx_core::workspace::Workspace;

/// Handle the publish command
pub fn handle_publish(
    destination: PathBuf,
    workspace_override: Option<PathBuf>,
    audience: Option<String>,
    single_file: bool,
    title: Option<String>,
    force: bool,
    dry_run: bool,
) {
    // Resolve workspace root
    let workspace_root = match resolve_workspace_for_publish(workspace_override) {
        Ok(root) => root,
        Err(e) => {
            eprintln!("✗ {}", e);
            return;
        }
    };

    // Check destination
    if destination.exists() && !force {
        if single_file {
            eprintln!(
                "✗ Destination file '{}' already exists (use --force to overwrite)",
                destination.display()
            );
        } else {
            eprintln!(
                "✗ Destination directory '{}' already exists (use --force to overwrite)",
                destination.display()
            );
        }
        return;
    }

    // Build options
    let options = PublishOptions {
        single_file,
        title,
        audience: audience.clone(),
        force,
    };

    // Show plan
    println!("Publish Plan");
    println!("============");
    println!("Source: {}", workspace_root.display());
    println!("Destination: {}", destination.display());
    if let Some(ref aud) = audience {
        println!("Audience: {}", aud);
    }
    println!(
        "Output mode: {}",
        if single_file {
            "single HTML file"
        } else {
            "multiple HTML files"
        }
    );
    println!();

    if dry_run {
        println!("(dry run - no changes made)");
        return;
    }

    // Execute publish
    let fs = RealFileSystem;
    let publisher = Publisher::new(fs);

    match publisher.publish(&workspace_root, &destination, &options) {
        Ok(result) => {
            if result.files_processed == 0 {
                println!("⚠ No files to publish");
                if audience.is_some() {
                    println!("  (no files match the specified audience)");
                }
                return;
            }

            println!(
                "✓ Published {} page{} to {}",
                result.files_processed,
                if result.files_processed == 1 { "" } else { "s" },
                destination.display()
            );

            if single_file {
                println!("  Open {} in a browser to view", destination.display());
            } else {
                let index_path = destination.join("index.html");
                println!("  Open {} in a browser to view", index_path.display());
            }
        }
        Err(e) => {
            eprintln!("✗ Publish failed: {}", e);
        }
    }
}

/// Resolve the workspace root for publishing
fn resolve_workspace_for_publish(workspace_override: Option<PathBuf>) -> Result<PathBuf, String> {
    let ws = Workspace::new(RealFileSystem);

    // If workspace is explicitly provided, use it
    if let Some(workspace_path) = workspace_override {
        if workspace_path.is_file() {
            return Ok(workspace_path);
        }
        // If it's a directory, find the root index in it
        if let Ok(Some(root)) = ws.find_root_index_in_dir(&workspace_path) {
            return Ok(root);
        }
        return Err(format!(
            "No workspace found at '{}'",
            workspace_path.display()
        ));
    }

    // Try current directory first
    let current_dir =
        std::env::current_dir().map_err(|e| format!("Cannot get current directory: {}", e))?;

    if let Ok(Some(root)) = ws.detect_workspace(&current_dir) {
        return Ok(root);
    }

    // Fall back to config default
    let config =
        diaryx_core::config::Config::load().map_err(|e| format!("Failed to load config: {}", e))?;

    if let Ok(Some(root)) = ws.find_root_index_in_dir(&config.default_workspace) {
        return Ok(root);
    }

    Err("No workspace found. Run 'diaryx init' first or specify --workspace".to_string())
}
