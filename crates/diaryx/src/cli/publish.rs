//! CLI handler for publish command

use std::path::{Path, PathBuf};

use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use diaryx_core::pandoc;
use diaryx_core::publish::{PublishOptions, Publisher};
use diaryx_core::workspace::Workspace;

/// Helper to run async operations in sync context
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    futures_lite::future::block_on(f)
}

/// Handle the publish command
pub fn handle_publish(
    destination: PathBuf,
    workspace_override: Option<PathBuf>,
    audience: Option<String>,
    format: &str,
    single_file: bool,
    title: Option<String>,
    force: bool,
    dry_run: bool,
) {
    // Validate format (publish doesn't support "markdown" — it always starts from HTML)
    let valid_publish_formats = ["html", "docx", "epub", "pdf", "latex", "odt", "rst"];
    if !valid_publish_formats.contains(&format) {
        eprintln!(
            "✗ Unsupported publish format: '{}'. Supported: {}",
            format,
            valid_publish_formats.join(", ")
        );
        return;
    }

    // Check pandoc availability for non-HTML formats
    if pandoc::requires_pandoc(format) && !pandoc::is_pandoc_available() {
        pandoc::print_install_instructions();
        return;
    }
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
    if format != "html" {
        println!("Format: {} (via pandoc)", format);
    }
    println!(
        "Output mode: {}",
        if single_file {
            "single file"
        } else {
            "multiple files"
        }
    );
    println!();

    if dry_run {
        println!("(dry run - no changes made)");
        return;
    }

    // Execute publish
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let publisher = Publisher::new(fs);

    match block_on(publisher.publish(&workspace_root, &destination, &options)) {
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

            // Post-process with pandoc if a non-HTML format was requested
            if pandoc::requires_pandoc(format) {
                println!("Converting to {}...", format);
                let ext = pandoc::format_extension(format);
                let mut converted = 0;
                let mut failed = 0;

                let html_files = if single_file {
                    vec![destination.clone()]
                } else {
                    walkdir_html(&destination)
                };

                for html_path in &html_files {
                    let out_path = html_path.with_extension(ext);
                    match pandoc::convert_file(html_path, &out_path, "html", format, true) {
                        Ok(()) => {
                            let _ = std::fs::remove_file(html_path);
                            converted += 1;
                        }
                        Err(e) => {
                            eprintln!("  ✗ Failed to convert {}: {}", html_path.display(), e);
                            failed += 1;
                        }
                    }
                }

                if failed == 0 {
                    println!("✓ Converted {} files to {}", converted, format);
                } else {
                    eprintln!("⚠ Converted {} files, {} failed", converted, failed);
                }
            } else if single_file {
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

/// Collect all `.html` files under a directory recursively.
fn walkdir_html(dir: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    fn visit(dir: &Path, results: &mut Vec<PathBuf>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, results);
            } else if path.extension().map_or(false, |ext| ext == "html") {
                results.push(path);
            }
        }
    }
    visit(dir, &mut results);
    results
}

/// Resolve the workspace root for publishing
fn resolve_workspace_for_publish(workspace_override: Option<PathBuf>) -> Result<PathBuf, String> {
    let ws = Workspace::new(SyncToAsyncFs::new(RealFileSystem));

    // If workspace is explicitly provided, use it
    if let Some(workspace_path) = workspace_override {
        if workspace_path.is_file() {
            return Ok(workspace_path);
        }
        // If it's a directory, find the root index in it
        if let Ok(Some(root)) = block_on(ws.find_root_index_in_dir(&workspace_path)) {
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

    if let Ok(Some(root)) = block_on(ws.detect_workspace(&current_dir)) {
        return Ok(root);
    }

    // Fall back to config default
    let config =
        diaryx_core::config::Config::load().map_err(|e| format!("Failed to load config: {}", e))?;

    if let Ok(Some(root)) = block_on(ws.find_root_index_in_dir(&config.default_workspace)) {
        return Ok(root);
    }

    Err("No workspace found. Run 'diaryx init' first or specify --workspace".to_string())
}
