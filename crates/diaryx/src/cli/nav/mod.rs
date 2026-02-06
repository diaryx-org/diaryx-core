//! Navigate workspace hierarchy with interactive TUI
//!
//! Provides a tree view with preview pane for browsing the workspace's
//! `contents`/`part_of` hierarchy.

mod app;
mod commands;
mod keys;
mod state;
mod tree;
mod ui;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use diaryx_core::config::Config;

use crate::cli::{CliWorkspace, block_on};

pub use state::NavState;

/// Handle the 'nav' command
pub fn handle_nav(
    workspace_override: Option<PathBuf>,
    ws: &CliWorkspace,
    config: &Option<Config>,
    current_dir: &Path,
    path: Option<String>,
    max_depth: usize,
) -> bool {
    // Resolve starting path
    let root_path = match resolve_nav_root(workspace_override, ws, config, current_dir, path) {
        Some(p) => p,
        None => {
            eprintln!("✗ No workspace found");
            eprintln!("  Run 'diaryx init' or 'diaryx workspace init' first");
            return false;
        }
    };

    let workspace_root = root_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let depth_limit = if max_depth == 0 {
        None
    } else {
        Some(max_depth)
    };

    // Build the tree
    let mut visited = HashSet::new();
    let tree = match block_on(ws.build_tree_with_depth(&root_path, depth_limit, &mut visited)) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("✗ Error building tree: {}", e);
            return false;
        }
    };

    // Create application state
    let mut state = NavState::new(tree.clone(), workspace_root, root_path, depth_limit);

    // Get config for editor
    let cfg = config.clone().unwrap_or_default();

    // Initialize terminal
    let mut terminal = ratatui::init();

    // Run TUI (editor is launched inside the loop, TUI resumes after)
    let result = app::run(&mut terminal, &mut state, &cfg, ws);

    // Restore terminal (always, even on error)
    ratatui::restore();

    if let Err(e) = result {
        eprintln!("✗ TUI error: {}", e);
        return false;
    }

    true
}

/// Resolve the navigation root path.
///
/// Uses the same resolution logic as `workspace info`:
/// 1. If path is ".", find index in current directory
/// 2. If path is a file, use it directly
/// 3. If path is a directory, find index in that directory
/// 4. If no path, use workspace override or detect workspace
fn resolve_nav_root(
    workspace_override: Option<PathBuf>,
    ws: &CliWorkspace,
    config: &Option<Config>,
    current_dir: &Path,
    path: Option<String>,
) -> Option<PathBuf> {
    if let Some(ref p) = path {
        if p == "." {
            // Resolve to local index in current directory
            match block_on(ws.find_any_index_in_dir(current_dir)) {
                Ok(Some(index)) => return Some(index),
                Ok(None) => {
                    eprintln!("✗ No index found in current directory");
                    return None;
                }
                Err(e) => {
                    eprintln!("✗ Error finding index: {}", e);
                    return None;
                }
            }
        } else {
            // Treat as a path to resolve
            let path_buf = PathBuf::from(p);
            let resolved = if path_buf.is_absolute() {
                path_buf
            } else {
                current_dir.join(&path_buf)
            };

            if resolved.is_file() {
                return Some(resolved);
            } else if resolved.is_dir() {
                // Find index in that directory
                match block_on(ws.find_any_index_in_dir(&resolved)) {
                    Ok(Some(index)) => return Some(index),
                    Ok(None) => {
                        eprintln!("✗ No index found in directory: {}", p);
                        return None;
                    }
                    Err(e) => {
                        eprintln!("✗ Error finding index: {}", e);
                        return None;
                    }
                }
            } else {
                eprintln!("✗ Path not found: {}", p);
                return None;
            }
        }
    }

    // No path specified - use workspace detection
    if let Some(override_path) = workspace_override {
        return Some(override_path);
    }

    if let Ok(Some(detected)) = block_on(ws.detect_workspace(current_dir)) {
        return Some(detected);
    }

    if let Some(cfg) = config {
        if let Ok(Some(root)) = block_on(ws.find_root_index_in_dir(&cfg.default_workspace)) {
            return Some(root);
        }
    }

    None
}
