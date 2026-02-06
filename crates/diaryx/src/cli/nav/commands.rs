//! Command execution for TUI workspace operations.
//!
//! Each function wraps a core workspace API call via block_on().

use std::path::{Path, PathBuf};

use diaryx_core::entry::slugify_title;

use crate::cli::{CliWorkspace, block_on};

/// Create a new child entry under the selected parent.
/// Returns the path to the new child.
pub fn exec_create(ws: &CliWorkspace, parent_path: &Path, title: &str) -> Result<PathBuf, String> {
    block_on(ws.create_child_entry(parent_path, Some(title)))
        .map_err(|e| format!("Create failed: {}", e))
}

/// Delete the selected entry.
pub fn exec_delete(ws: &CliWorkspace, path: &Path) -> Result<(), String> {
    block_on(ws.delete_entry(path)).map_err(|e| format!("Delete failed: {}", e))
}

/// Duplicate the selected entry.
/// Returns the path to the duplicate.
pub fn exec_duplicate(ws: &CliWorkspace, path: &Path) -> Result<PathBuf, String> {
    block_on(ws.duplicate_entry(path)).map_err(|e| format!("Duplicate failed: {}", e))
}

/// Rename the selected entry by moving it to a new filename derived from the title.
/// Also updates the frontmatter title.
/// Returns the new path.
pub fn exec_rename(ws: &CliWorkspace, path: &Path, new_title: &str) -> Result<PathBuf, String> {
    let parent_dir = path
        .parent()
        .ok_or_else(|| "Cannot rename: no parent directory".to_string())?;
    let new_filename = slugify_title(new_title);
    let new_path = parent_dir.join(&new_filename);

    if new_path == path {
        // Just update the title in frontmatter
        block_on(ws.set_frontmatter_property(
            path,
            "title",
            serde_yaml::Value::String(new_title.to_string()),
        ))
        .map_err(|e| format!("Rename failed: {}", e))?;
        return Ok(path.to_path_buf());
    }

    if new_path.exists() {
        return Err(format!("Cannot rename: '{}' already exists", new_filename));
    }

    // Move the file (updates parent/child links)
    block_on(ws.move_entry(path, &new_path)).map_err(|e| format!("Rename failed: {}", e))?;

    // Update the title in frontmatter
    block_on(ws.set_frontmatter_property(
        &new_path,
        "title",
        serde_yaml::Value::String(new_title.to_string()),
    ))
    .map_err(|e| format!("Title update failed: {}", e))?;

    Ok(new_path)
}

/// Move an entry under a new parent.
/// Returns the new path of the moved entry.
pub fn exec_move(
    ws: &CliWorkspace,
    source: &Path,
    target_parent: &Path,
) -> Result<PathBuf, String> {
    let target_dir = target_parent
        .parent()
        .ok_or_else(|| "Cannot move: target has no parent directory".to_string())?;
    let filename = source
        .file_name()
        .ok_or_else(|| "Cannot move: source has no filename".to_string())?;
    let new_path = target_dir.join(filename);

    if new_path == source {
        return Err("Source and destination are the same".to_string());
    }

    block_on(ws.move_entry(source, &new_path)).map_err(|e| format!("Move failed: {}", e))?;

    Ok(new_path)
}

/// Combine/merge two index files. Source is merged into target.
pub fn exec_merge(ws: &CliWorkspace, source: &Path, target: &Path) -> Result<(), String> {
    block_on(ws.combine_indices(source, target)).map_err(|e| format!("Merge failed: {}", e))
}
