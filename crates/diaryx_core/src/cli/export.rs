//! CLI handler for export command

use std::path::PathBuf;

use diaryx_core::export::{ExportOptions, ExportPlan, Exporter};
use diaryx_core::fs::RealFileSystem;
use diaryx_core::workspace::Workspace;
use std::path::Path;

/// Handle the export command
pub fn handle_export(
    workspace_root: PathBuf,
    audience: &str,
    destination: &Path,
    force: bool,
    keep_audience: bool,
    verbose: bool,
    dry_run: bool,
) {
    let fs = RealFileSystem;
    let exporter = Exporter::new(fs);

    // Plan the export
    let plan = match exporter.plan_export(&workspace_root, audience, destination) {
        Ok(plan) => plan,
        Err(e) => {
            eprintln!("✗ Failed to plan export: {}", e);
            return;
        }
    };

    // Show plan summary
    println!("Export Plan");
    println!("===========");
    println!("Audience: {}", plan.audience);
    println!("Source: {}", plan.source_root.display());
    println!("Destination: {}", destination.display());
    println!();

    if plan.included.is_empty() {
        println!("⚠ No files to export for audience '{}'", audience);
        println!();
        if !plan.excluded.is_empty() {
            println!("All {} files were excluded:", plan.excluded.len());
            for excluded in &plan.excluded {
                println!("  - {} ({})", excluded.path.display(), excluded.reason);
            }
        }
        return;
    }

    println!(
        "Files to export: {} | Files excluded: {}",
        plan.included.len(),
        plan.excluded.len()
    );
    println!();

    if verbose {
        print_verbose_plan(&plan);
    }

    if dry_run {
        println!("(dry run - no changes made)");
        return;
    }

    // Execute the export
    let options = ExportOptions {
        force,
        keep_audience,
    };

    match exporter.execute_export(&plan, &options) {
        Ok(stats) => {
            println!("✓ {}", stats);
            println!("  Exported to: {}", destination.display());
        }
        Err(e) => {
            eprintln!("✗ Export failed: {}", e);
            if !force && destination.exists() {
                eprintln!("  (use --force to overwrite existing destination)");
            }
        }
    }
}

/// Print detailed information about the export plan
fn print_verbose_plan(plan: &ExportPlan) {
    println!("Included files:");
    for file in &plan.included {
        println!("  ✓ {}", file.relative_path.display());
        if !file.filtered_contents.is_empty() {
            println!(
                "    (contents filtered: {})",
                file.filtered_contents.join(", ")
            );
        }
    }
    println!();

    if !plan.excluded.is_empty() {
        println!("Excluded files:");
        for excluded in &plan.excluded {
            println!("  ✗ {} - {}", excluded.path.display(), excluded.reason);
        }
        println!();
    }
}

/// Resolve the workspace root for export
pub fn resolve_workspace_for_export(
    workspace_override: Option<PathBuf>,
) -> Result<PathBuf, String> {
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
