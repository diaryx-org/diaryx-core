//! Workspace command handlers

use diaryx_core::config::Config;
use diaryx_core::fs::RealFileSystem;
use diaryx_core::workspace::Workspace;
use std::path::PathBuf;

use crate::cli::args::WorkspaceCommands;

pub fn handle_workspace_command(
    command: WorkspaceCommands,
    workspace_override: Option<PathBuf>,
    ws: &Workspace<RealFileSystem>,
) {
    let config = Config::load().ok();
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    match command {
        WorkspaceCommands::Info => {
            let root_path = if let Some(ref override_path) = workspace_override {
                override_path.clone()
            } else if let Ok(Some(detected)) = ws.detect_workspace(&current_dir) {
                detected
            } else if let Some(ref cfg) = config {
                if let Ok(Some(root)) = ws.find_root_index_in_dir(&cfg.base_dir) {
                    root
                } else {
                    eprintln!("✗ No workspace found");
                    eprintln!("  Run 'diaryx init' or 'diaryx workspace init' first");
                    return;
                }
            } else {
                eprintln!("✗ No workspace found");
                eprintln!("  Run 'diaryx init' or 'diaryx workspace init' first");
                return;
            };

            match ws.workspace_info(&root_path) {
                Ok(tree_output) => {
                    println!("{}", tree_output);
                }
                Err(e) => eprintln!("✗ Error reading workspace: {}", e),
            }
        }

        WorkspaceCommands::Init {
            dir,
            title,
            description,
        } => {
            let target_dir = dir.unwrap_or(current_dir);

            match ws.init_workspace(&target_dir, title.as_deref(), description.as_deref()) {
                Ok(readme_path) => {
                    println!("✓ Initialized workspace");
                    println!("  Index file: {}", readme_path.display());
                }
                Err(e) => eprintln!("✗ Error initializing workspace: {}", e),
            }
        }

        WorkspaceCommands::Path => {
            let root_path = if let Some(ref override_path) = workspace_override {
                Some(override_path.clone())
            } else if let Ok(Some(detected)) = ws.detect_workspace(&current_dir) {
                Some(detected)
            } else if let Some(ref cfg) = config {
                ws.find_root_index_in_dir(&cfg.base_dir).ok().flatten()
            } else {
                None
            };

            match root_path {
                Some(path) => {
                    if let Some(dir) = path.parent() {
                        println!("{}", dir.display());
                    } else {
                        println!("{}", path.display());
                    }
                }
                None => {
                    eprintln!("✗ No workspace found");
                    eprintln!("  Run 'diaryx init' or 'diaryx workspace init' first");
                }
            }
        }
    }
}
