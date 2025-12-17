//! Path utilities for calculating relative paths between files and directories.
//!
//! This module provides functions to compute relative paths, which is useful for
//! maintaining `part_of` and `contents` references in the workspace.

use std::path::Path;

/// Compute a relative path from a base directory to a target file.
///
/// # Example
/// ```
/// use diaryx_core::path_utils::relative_path_from_dir_to_target;
/// use std::path::Path;
///
/// // From workspace/ to workspace/Daily/daily_index.md => Daily/daily_index.md
/// let base = Path::new("workspace");
/// let target = Path::new("workspace/Daily/daily_index.md");
/// let rel = relative_path_from_dir_to_target(base, target);
/// assert_eq!(rel, "Daily/daily_index.md");
/// ```
pub fn relative_path_from_dir_to_target(base_dir: &Path, target_path: &Path) -> String {
    let base_components: Vec<_> = base_dir.components().collect();
    let target_components: Vec<_> = target_path.components().collect();

    let mut common = 0usize;
    while common < base_components.len()
        && common < target_components.len()
        && base_components[common] == target_components[common]
    {
        common += 1;
    }

    let mut parts: Vec<String> = Vec::new();
    for _ in common..base_components.len() {
        parts.push("..".to_string());
    }

    for comp in target_components.iter().skip(common) {
        parts.push(comp.as_os_str().to_string_lossy().to_string());
    }

    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

/// Compute a relative path from a source file's location to a target file.
///
/// This is useful for computing `part_of` values - the path from an entry to its parent index.
///
/// # Example
/// ```
/// use diaryx_core::path_utils::relative_path_from_file_to_target;
/// use std::path::Path;
///
/// // From a/b/note.md to a/index.md => ../index.md
/// let from = Path::new("a/b/note.md");
/// let to = Path::new("a/index.md");
/// let rel = relative_path_from_file_to_target(from, to);
/// assert_eq!(rel, "../index.md");
/// ```
pub fn relative_path_from_file_to_target(from_file: &Path, to_target: &Path) -> String {
    // We want relative from the file's directory
    let from_dir = from_file.parent().unwrap_or_else(|| Path::new(""));

    relative_path_from_dir_to_target(from_dir, to_target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_relative_path_same_dir() {
        let base = Path::new("workspace");
        let target = Path::new("workspace/file.md");
        assert_eq!(relative_path_from_dir_to_target(base, target), "file.md");
    }

    #[test]
    fn test_relative_path_nested() {
        let base = Path::new("workspace");
        let target = Path::new("workspace/Daily/2025/01/entry.md");
        assert_eq!(
            relative_path_from_dir_to_target(base, target),
            "Daily/2025/01/entry.md"
        );
    }

    #[test]
    fn test_relative_path_parent() {
        let base = Path::new("workspace/Daily");
        let target = Path::new("workspace/README.md");
        assert_eq!(
            relative_path_from_dir_to_target(base, target),
            "../README.md"
        );
    }

    #[test]
    fn test_relative_path_sibling() {
        let base = Path::new("workspace/Daily");
        let target = Path::new("workspace/Projects/index.md");
        assert_eq!(
            relative_path_from_dir_to_target(base, target),
            "../Projects/index.md"
        );
    }

    #[test]
    fn test_file_to_target_same_dir() {
        let from = Path::new("workspace/note.md");
        let to = Path::new("workspace/index.md");
        assert_eq!(relative_path_from_file_to_target(from, to), "index.md");
    }

    #[test]
    fn test_file_to_target_parent() {
        let from = Path::new("workspace/subdir/note.md");
        let to = Path::new("workspace/index.md");
        assert_eq!(relative_path_from_file_to_target(from, to), "../index.md");
    }

    #[test]
    fn test_file_to_target_nested() {
        let from = Path::new("a/b/c/note.md");
        let to = Path::new("a/index.md");
        assert_eq!(
            relative_path_from_file_to_target(from, to),
            "../../index.md"
        );
    }
}
