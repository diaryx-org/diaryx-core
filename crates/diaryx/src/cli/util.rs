//! Shared utilities for CLI commands

use diaryx_core::config::Config;
use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use diaryx_core::workspace::Workspace;
use glob::glob;
use serde_yaml::Value;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::cli::{block_on, CliDiaryxAppSync};

/// Result of a workspace-aware file rename operation
pub struct RenameResult {
    /// Whether the file was successfully moved/renamed
    pub success: bool,
    /// Parent index that was updated (if any)
    pub parent_updated: Option<PathBuf>,
    /// Children whose part_of was updated
    pub children_updated: Vec<PathBuf>,
}

/// Rename/move a file while updating all workspace references (contents and part_of)
/// This is the canonical way to rename files in the workspace - use this instead of std::fs::rename
///
/// Updates:
/// - Parent's `contents` list (if file has `part_of`)
/// - Children's `part_of` property (if file has `contents`)
/// - The file's own `part_of` (if moving to different directory)
pub fn rename_file_with_refs(
    app: &CliDiaryxAppSync,
    source_path: &Path,
    dest_path: &Path,
    dry_run: bool,
) -> RenameResult {
    let mut result = RenameResult {
        success: false,
        parent_updated: None,
        children_updated: Vec::new(),
    };

    let source_str = source_path.to_string_lossy();

    // Read source file's frontmatter to find its part_of (parent)
    let parent_path = match app.get_frontmatter_property(&source_str, "part_of") {
        Ok(Some(Value::String(part_of))) => source_path.parent().map(|dir| dir.join(&part_of)),
        _ => None,
    };

    // Canonicalize paths for accurate relative path calculations
    let source_canonical = source_path
        .canonicalize()
        .unwrap_or_else(|_| source_path.to_path_buf());

    // Calculate old and new relative paths from parent's perspective
    let (old_relative, new_relative) = if let Some(ref parent) = parent_path {
        let parent_canonical = parent.canonicalize().unwrap_or_else(|_| parent.clone());
        let parent_dir = parent_canonical.parent().unwrap_or(&parent_canonical);

        let old_rel = pathdiff::diff_paths(&source_canonical, parent_dir)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| {
                source_path
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default()
            });

        // For new path, we need to handle the case where dest doesn't exist yet
        // Use dest's parent (which should exist) to calculate relative path
        let new_rel = if let Some(dest_parent) = dest_path.parent() {
            let dest_parent_canonical = dest_parent
                .canonicalize()
                .unwrap_or_else(|_| dest_parent.to_path_buf());
            let dest_canonical =
                dest_parent_canonical.join(dest_path.file_name().unwrap_or_default());
            pathdiff::diff_paths(&dest_canonical, parent_dir)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| {
                    dest_path
                        .file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_default()
                })
        } else {
            dest_path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default()
        };

        (old_rel, new_rel)
    } else {
        // No parent, just use filenames
        let old_rel = source_path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        let new_rel = dest_path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        (old_rel, new_rel)
    };

    // Get children before moving (need to update their part_of after move)
    let children: Vec<PathBuf> = match app.get_frontmatter_property(&source_str, "contents") {
        Ok(Some(Value::Sequence(contents))) => contents
            .iter()
            .filter_map(|v| {
                if let Value::String(s) = v {
                    source_path.parent().map(|dir| dir.join(s))
                } else {
                    None
                }
            })
            .filter(|p| p.exists())
            .collect(),
        _ => Vec::new(),
    };

    if dry_run {
        println!(
            "Would move '{}' to '{}'",
            source_path.display(),
            dest_path.display()
        );
        if let Some(ref parent) = parent_path
            && parent.exists()
        {
            println!(
                "Would update contents in '{}': '{}' -> '{}'",
                parent.display(),
                old_relative,
                new_relative
            );
        }
        if !children.is_empty() {
            println!("Would update part_of in {} child file(s)", children.len());
        }
        result.success = true;
        return result;
    }

    // 1. Update parent's contents (if parent exists)
    if let Some(ref parent) = parent_path
        && parent.exists()
    {
        let parent_str = parent.to_string_lossy();
        if let Ok(Some(Value::Sequence(mut items))) =
            app.get_frontmatter_property(&parent_str, "contents")
        {
            let mut updated = false;
            for item in &mut items {
                if let Value::String(s) = item
                    && *s == old_relative
                {
                    *s = new_relative.clone();
                    updated = true;
                }
            }
            if updated {
                if let Err(e) =
                    app.set_frontmatter_property(&parent_str, "contents", Value::Sequence(items))
                {
                    eprintln!("⚠ Error updating parent contents: {}", e);
                } else {
                    println!(
                        "✓ Updated contents in '{}': '{}' -> '{}'",
                        parent.display(),
                        old_relative,
                        new_relative
                    );
                    result.parent_updated = Some(parent.clone());
                }
            }
        }
    }

    // 2. Create destination directory if needed
    if let Some(parent_dir) = dest_path.parent()
        && !parent_dir.exists()
        && let Err(e) = std::fs::create_dir_all(parent_dir)
    {
        eprintln!("✗ Error creating directory: {}", e);
        return result;
    }

    // 3. Move/rename the file
    if let Err(e) = std::fs::rename(source_path, dest_path) {
        eprintln!("✗ Error moving file: {}", e);
        return result;
    }
    println!(
        "✓ Moved '{}' to '{}'",
        source_path.display(),
        dest_path.display()
    );
    result.success = true;

    // 4. Update part_of in the moved file if parent exists and relative path changed
    if let Some(ref parent) = parent_path {
        let dest_str = dest_path.to_string_lossy();
        let new_part_of = calculate_relative_path(dest_path, parent);
        if let Err(e) =
            app.set_frontmatter_property(&dest_str, "part_of", Value::String(new_part_of))
        {
            eprintln!("⚠ Error updating part_of in moved file: {}", e);
        }
    }

    // 5. Update children's part_of to point to new location
    for child in &children {
        let child_str = child.to_string_lossy();
        let new_part_of = calculate_relative_path(child, dest_path);
        if let Err(e) =
            app.set_frontmatter_property(&child_str, "part_of", Value::String(new_part_of))
        {
            eprintln!("⚠ Error updating part_of in '{}': {}", child.display(), e);
        } else {
            println!("✓ Updated part_of in '{}'", child.display());
            result.children_updated.push(child.clone());
        }
    }

    result
}

/// Calculate the relative path from one file to another
pub fn calculate_relative_path(from: &Path, to: &Path) -> String {
    // Try to canonicalize both paths for accurate comparison
    let from_canonical = from.canonicalize().ok();
    let to_canonical = to.canonicalize().ok();

    // If they're in the same directory, just use the filename
    let same_dir = match (&from_canonical, &to_canonical) {
        (Some(fc), Some(tc)) => fc.parent() == tc.parent(),
        _ => from.parent() == to.parent(),
    };

    if same_dir {
        return to
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| to.to_string_lossy().to_string());
    }

    // Use canonical paths for pathdiff calculation
    if let (Some(from_canon), Some(to_canon)) = (&from_canonical, &to_canonical)
        && let Some(from_dir) = from_canon.parent()
        && let Some(rel) = pathdiff::diff_paths(to_canon, from_dir)
    {
        return rel.to_string_lossy().to_string();
    }

    // Fall back to non-canonical paths
    if let Some(from_dir) = from.parent()
        && let Some(rel) = pathdiff::diff_paths(to, from_dir)
    {
        let rel_str = rel.to_string_lossy().to_string();
        // Ensure we don't return an absolute path
        if !rel.is_absolute() {
            return rel_str;
        }
    }

    // Fallback: just use the filename
    to.file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| to.to_string_lossy().to_string())
}

/// Result of a confirmation prompt
pub enum ConfirmResult {
    Yes,
    No,
    All,
    Quit,
}

/// Prompt user for confirmation
pub fn prompt_confirm(message: &str) -> ConfirmResult {
    print!("{} [y/n/a/q] ", message);
    io::stdout().flush().unwrap();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return ConfirmResult::Quit;
    }

    match input.trim().to_lowercase().as_str() {
        "y" | "yes" => ConfirmResult::Yes,
        "n" | "no" => ConfirmResult::No,
        "a" | "all" => ConfirmResult::All,
        "q" | "quit" => ConfirmResult::Quit,
        _ => ConfirmResult::No,
    }
}

/// Check if a path pattern contains glob characters
pub fn is_glob_pattern(path: &str) -> bool {
    path.contains('*') || path.contains('?') || path.contains('[')
}

/// Resolve a path pattern to a list of files
/// Returns either a single resolved path (for dates/literals) or multiple paths (for globs/workspace)
///
/// Special handling:
/// - `.` resolves to all files in the current workspace (traversing from local index)
/// - `title:...` or `t:...` resolves files by their frontmatter title property (fuzzy matching)
/// - Glob patterns (`*.md`, `**/*.md`) match files by pattern
/// - Date strings (via chrono-english) resolve to dated entry paths
/// - Literal paths are returned as-is
pub fn resolve_paths(path: &str, config: &Config, app: &CliDiaryxAppSync) -> Vec<PathBuf> {
    // Check for title: or t: prefix
    if let Some(title_query) = path
        .strip_prefix("title:")
        .or_else(|| path.strip_prefix("t:"))
    {
        return match_files_by_title(title_query);
    }

    // Handle directories as workspace-aware path resolution
    let path_buf = Path::new(path);
    if path_buf.is_dir() {
        return resolve_workspace_files_in_dir(path_buf);
    }

    // Check if it's a glob pattern
    if is_glob_pattern(path) {
        match glob(path) {
            Ok(paths) => {
                let mut result: Vec<PathBuf> = paths
                    .filter_map(|p| p.ok())
                    .filter(|p| p.extension().is_some_and(|ext| ext == "md"))
                    .collect();
                result.sort();
                result
            }
            Err(e) => {
                eprintln!("✗ Invalid glob pattern: {}", e);
                vec![]
            }
        }
    } else {
        // Try to resolve as date or literal path first
        let resolved = app.resolve_path(path, config);

        // If the resolved path exists, use it
        if resolved.exists() {
            return vec![resolved];
        }

        // If path doesn't exist and doesn't look like a date, try fuzzy matching
        // (date resolution would have returned a path in daily_entry_dir)
        if !resolved.starts_with(config.daily_entry_dir())
            || path.contains('/')
            || path.contains('\\')
        {
            // This was likely meant as a literal path that doesn't exist
            // Try fuzzy matching in current directory
            if let Some(matches) = fuzzy_match_files(path)
                && !matches.is_empty()
            {
                return matches;
            }
        }

        // Fall back to the resolved path (may not exist, but that's the user's intent)
        vec![resolved]
    }
}

/// Resolve a directory to all files in its workspace
/// Finds the local index in the directory and traverses its contents
fn resolve_workspace_files_in_dir(dir: &Path) -> Vec<PathBuf> {
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let workspace = Workspace::new(fs);

    // Canonicalize the directory path
    let dir = match dir.canonicalize() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("✗ Could not resolve directory '{}': {}", dir.display(), e);
            return vec![];
        }
    };

    // Find a local index in the directory
    match block_on(workspace.find_any_index_in_dir(&dir)) {
        Ok(Some(index_path)) => {
            // Collect all files from the index
            match block_on(workspace.collect_workspace_files(&index_path)) {
                Ok(files) => files,
                Err(e) => {
                    eprintln!("✗ Error traversing workspace: {}", e);
                    vec![]
                }
            }
        }
        Ok(None) => {
            // No index found, fall back to all .md files in the directory
            let glob_pattern = format!("{}/*.md", dir.display());
            match glob(&glob_pattern) {
                Ok(paths) => {
                    let mut result: Vec<PathBuf> = paths.filter_map(|p| p.ok()).collect();
                    result.sort();
                    if result.is_empty() {
                        eprintln!(
                            "⚠ No index file found in '{}' and no .md files present",
                            dir.display()
                        );
                    }
                    result
                }
                Err(e) => {
                    eprintln!("✗ Error listing files: {}", e);
                    vec![]
                }
            }
        }
        Err(e) => {
            eprintln!("✗ Error searching for index: {}", e);
            vec![]
        }
    }
}

/// Match files by their frontmatter title property
/// Uses fuzzy matching: exact (case-insensitive) → prefix → contains
fn match_files_by_title(query: &str) -> Vec<PathBuf> {
    let current_dir = match std::env::current_dir() {
        Ok(d) => d,
        Err(_) => return vec![],
    };

    let query_lower = query.to_lowercase();
    let mut matches: Vec<(PathBuf, usize)> = Vec::new();

    // Scan all .md files in current directory
    if let Ok(entries) = std::fs::read_dir(&current_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();

            // Only consider .md files
            if path.extension().is_none_or(|ext| ext != "md") {
                continue;
            }

            // Read the file and extract title from frontmatter
            if let Some(title) = extract_title_from_file(&path) {
                let title_lower = title.to_lowercase();

                // Score the match:
                // - Exact match: highest priority (score 0)
                // - Prefix match: high priority (score 1)
                // - Contains match: lower priority (score 2)
                let score = if title_lower == query_lower {
                    Some(0)
                } else if title_lower.starts_with(&query_lower) {
                    Some(1)
                } else if title_lower.contains(&query_lower) {
                    Some(2)
                } else {
                    None
                };

                if let Some(s) = score {
                    matches.push((path, s));
                }
            }
        }
    }

    if matches.is_empty() {
        return vec![];
    }

    // Sort by score (best first), then by path name
    matches.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

    // Return all matches with the best score
    let best_score = matches[0].1;
    matches
        .into_iter()
        .filter(|(_, score)| *score == best_score)
        .map(|(path, _)| path)
        .collect()
}

/// Extract the title property from a file's frontmatter
fn extract_title_from_file(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;

    // Check if file starts with frontmatter
    if !content.starts_with("---") {
        return None;
    }

    // Find the end of frontmatter
    let rest = &content[3..];
    let end_idx = rest.find("---")?;
    let frontmatter_str = &rest[..end_idx];

    // Parse YAML
    let frontmatter: serde_yaml::Value = serde_yaml::from_str(frontmatter_str).ok()?;

    // Extract title
    if let Value::Mapping(map) = frontmatter
        && let Some(Value::String(title)) = map.get(Value::String("title".to_string()))
    {
        return Some(title.clone());
    }

    None
}

/// Fuzzy match a string against .md files in the current directory
/// Returns files where the filename (without extension) contains the query (case-insensitive)
/// or where the query is a prefix of the filename
fn fuzzy_match_files(query: &str) -> Option<Vec<PathBuf>> {
    let current_dir = std::env::current_dir().ok()?;
    let query_lower = query.to_lowercase();

    let mut matches: Vec<(PathBuf, usize)> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&current_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();

            // Only consider .md files
            if path.extension().is_none_or(|ext| ext != "md") {
                continue;
            }

            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                let stem_lower = stem.to_lowercase();

                // Score the match:
                // - Exact match (without extension): highest priority (score 0)
                // - Prefix match: high priority (score 1)
                // - Contains match: lower priority (score 2)
                let score = if stem_lower == query_lower {
                    Some(0)
                } else if stem_lower.starts_with(&query_lower) {
                    Some(1)
                } else if stem_lower.contains(&query_lower) {
                    Some(2)
                } else {
                    None
                };

                if let Some(s) = score {
                    matches.push((path, s));
                }
            }
        }
    }

    if matches.is_empty() {
        return None;
    }

    // Sort by score (best first), then by path name
    matches.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

    // Return all matches with the best score
    let best_score = matches[0].1;
    let best_matches: Vec<PathBuf> = matches
        .into_iter()
        .filter(|(_, score)| *score == best_score)
        .map(|(path, _)| path)
        .collect();

    Some(best_matches)
}

/// Load config or print error message
pub fn load_config() -> Option<Config> {
    match Config::load() {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("✗ Error loading config: {}", e);
            eprintln!("  Run 'diaryx init' first");
            None
        }
    }
}

/// Format a YAML value for display
pub fn format_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Sequence(items) => {
            let items_str: Vec<String> = items.iter().map(format_value).collect();
            format!("[{}]", items_str.join(", "))
        }
        _ => serde_yaml::to_string(value)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}
