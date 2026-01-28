//! Portable path link parsing and formatting for `part_of` and `contents` properties.
//!
//! This module provides utilities for working with file references in frontmatter that are:
//! - **Portable**: Work across Obsidian, Diaryx, and other markdown editors
//! - **Unambiguous**: Clear distinction between relative and workspace-root paths
//! - **Clickable**: Rendered as links in supporting editors
//! - **Self-documenting**: Include human-readable titles
//!
//! # Link Formats Supported (Read)
//!
//! | Format | Example | Interpretation |
//! |--------|---------|----------------|
//! | Markdown link (root) | `"[Title](/path/file.md)"` | Workspace-root absolute |
//! | Markdown link (relative) | `"[Title](../file.md)"` | Relative to current file |
//! | Plain root path | `/path/file.md` | Workspace-root absolute |
//! | Plain relative | `../file.md` | Relative to current file |
//! | Plain ambiguous | `path/file.md` | Assume relative (legacy) |
//!
//! # Link Format (Write)
//!
//! The write format is configurable via [`LinkFormat`]:
//! - `MarkdownRoot` (default): `"[Title](/workspace/root/path.md)"`
//! - `MarkdownRelative`: `"[Title](../relative/path.md)"`
//! - `PlainRelative`: `../relative/path.md`
//! - `PlainCanonical`: `workspace/root/path.md`
//!
//! # Internal CRDT Storage
//!
//! The CRDT layer stores canonical paths WITHOUT the `/` prefix:
//! ```text
//! Utility/utility_index.md
//! ```
//!
//! The `/` prefix and markdown link syntax are purely for frontmatter serialization.

use serde::{Deserialize, Serialize};
use std::path::Path;
use ts_rs::TS;

/// The format to use when writing links to frontmatter.
///
/// This controls how `part_of` and `contents` paths are serialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
#[serde(rename_all = "snake_case")]
pub enum LinkFormat {
    /// Markdown link with workspace-root path: `[Title](/path/to/file.md)`
    ///
    /// This is the most portable and unambiguous format:
    /// - Clickable in Obsidian and other markdown editors
    /// - Unambiguous (always relative to workspace root)
    /// - Self-documenting with human-readable titles
    #[default]
    MarkdownRoot,

    /// Markdown link with relative path: `[Title](../relative/path.md)`
    ///
    /// Useful for compatibility with tools that don't understand root paths.
    MarkdownRelative,

    /// Plain relative path without markdown link syntax: `../relative/path.md`
    ///
    /// Legacy format for backwards compatibility.
    PlainRelative,

    /// Plain canonical path (workspace-relative): `path/to/file.md`
    ///
    /// Simple format without markdown link syntax or leading slash.
    PlainCanonical,
}

/// The type of path in a parsed link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathType {
    /// Path starts with `/` - workspace-root absolute path
    WorkspaceRoot,
    /// Path contains `../` or `./` - relative to current file
    Relative,
    /// Plain path like `folder/file.md` - ambiguous, assume relative (legacy)
    Ambiguous,
}

/// A parsed link from frontmatter.
///
/// This represents either a markdown link `[Title](path)` or a plain path string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLink {
    /// The display title, if present (from markdown link syntax)
    pub title: Option<String>,
    /// The path portion of the link
    pub path: String,
    /// The type of path (root, relative, or ambiguous)
    pub path_type: PathType,
}

impl ParsedLink {
    /// Create a new parsed link with just a path (no title).
    pub fn new(path: String, path_type: PathType) -> Self {
        Self {
            title: None,
            path,
            path_type,
        }
    }

    /// Create a new parsed link with a title.
    pub fn with_title(title: String, path: String, path_type: PathType) -> Self {
        Self {
            title: Some(title),
            path,
            path_type,
        }
    }
}

/// Parse a link value from frontmatter.
///
/// Handles multiple formats:
/// - Markdown links: `[Title](/path/file.md)` or `[Title](../file.md)`
/// - Plain paths with `/` prefix: `/path/file.md`
/// - Plain relative paths: `../file.md` or `./file.md`
/// - Plain ambiguous paths: `path/file.md`
///
/// # Examples
///
/// ```
/// use diaryx_core::link_parser::{parse_link, PathType};
///
/// // Markdown link with root path
/// let link = parse_link("[Utility Index](/Utility/utility_index.md)");
/// assert_eq!(link.title, Some("Utility Index".to_string()));
/// assert_eq!(link.path, "Utility/utility_index.md");
/// assert_eq!(link.path_type, PathType::WorkspaceRoot);
///
/// // Markdown link with relative path
/// let link = parse_link("[Parent](../index.md)");
/// assert_eq!(link.title, Some("Parent".to_string()));
/// assert_eq!(link.path, "../index.md");
/// assert_eq!(link.path_type, PathType::Relative);
///
/// // Plain root path
/// let link = parse_link("/Utility/file.md");
/// assert_eq!(link.title, None);
/// assert_eq!(link.path, "Utility/file.md");
/// assert_eq!(link.path_type, PathType::WorkspaceRoot);
///
/// // Plain relative path
/// let link = parse_link("../parent.md");
/// assert_eq!(link.path_type, PathType::Relative);
///
/// // Plain ambiguous path (legacy)
/// let link = parse_link("child.md");
/// assert_eq!(link.path_type, PathType::Ambiguous);
/// ```
pub fn parse_link(value: &str) -> ParsedLink {
    let value = value.trim();

    // Try to parse as markdown link: [Title](path)
    if let Some(parsed) = try_parse_markdown_link(value) {
        return parsed;
    }

    // Plain path - determine type
    let path_type = determine_path_type(value);

    // Strip leading `/` for workspace-root paths (CRDT stores without prefix)
    let path = if path_type == PathType::WorkspaceRoot {
        value.strip_prefix('/').unwrap_or(value).to_string()
    } else {
        value.to_string()
    };

    ParsedLink::new(path, path_type)
}

/// Try to parse a markdown link `[Title](path)`.
fn try_parse_markdown_link(value: &str) -> Option<ParsedLink> {
    // Must start with `[` and contain `](`
    if !value.starts_with('[') {
        return None;
    }

    // Find the closing bracket and opening paren
    let close_bracket = value.find(']')?;
    if !value[close_bracket..].starts_with("](") {
        return None;
    }

    // Find the closing paren
    let path_start = close_bracket + 2;
    let close_paren = value[path_start..].find(')')? + path_start;

    let title = value[1..close_bracket].to_string();
    let raw_path = value[path_start..close_paren].to_string();

    let path_type = determine_path_type(&raw_path);

    // Strip leading `/` for workspace-root paths
    let path = if path_type == PathType::WorkspaceRoot {
        raw_path.strip_prefix('/').unwrap_or(&raw_path).to_string()
    } else {
        raw_path
    };

    Some(ParsedLink::with_title(title, path, path_type))
}

/// Determine the path type from a raw path string.
fn determine_path_type(path: &str) -> PathType {
    if path.starts_with('/') {
        PathType::WorkspaceRoot
    } else if path.starts_with("../") || path.starts_with("./") || path == ".." || path == "." {
        PathType::Relative
    } else {
        PathType::Ambiguous
    }
}

/// Convert a parsed link to a canonical (workspace-relative) path.
///
/// - Workspace-root paths are already canonical (just stripped the `/`)
/// - Relative paths are resolved against the current file's directory
/// - Ambiguous paths are treated as relative (legacy behavior)
///
/// # Arguments
///
/// * `parsed` - The parsed link to convert
/// * `current_file_path` - The canonical path of the file containing this link
///
/// # Examples
///
/// ```
/// use diaryx_core::link_parser::{parse_link, to_canonical};
/// use std::path::Path;
///
/// // Workspace-root path - already canonical
/// let link = parse_link("[Title](/Utility/file.md)");
/// let canonical = to_canonical(&link, Path::new("Other/entry.md"));
/// assert_eq!(canonical, "Utility/file.md");
///
/// // Relative path - resolve against current file
/// let link = parse_link("../index.md");
/// let canonical = to_canonical(&link, Path::new("Folder/Sub/entry.md"));
/// assert_eq!(canonical, "Folder/index.md");
///
/// // Ambiguous path - treat as relative to current file's directory
/// let link = parse_link("child.md");
/// let canonical = to_canonical(&link, Path::new("Folder/index.md"));
/// assert_eq!(canonical, "Folder/child.md");
/// ```
pub fn to_canonical(parsed: &ParsedLink, current_file_path: &Path) -> String {
    match parsed.path_type {
        PathType::WorkspaceRoot => {
            // Already canonical (we stripped the `/` during parsing)
            parsed.path.clone()
        }
        PathType::Relative | PathType::Ambiguous => {
            // Resolve relative to current file's directory
            let file_dir = current_file_path.parent().unwrap_or(Path::new(""));
            let resolved = file_dir.join(&parsed.path);
            normalize_path(&resolved)
        }
    }
}

/// Normalize a path by resolving `.` and `..` components.
fn normalize_path(path: &Path) -> String {
    use std::path::Component;

    let mut normalized: Vec<&str> = Vec::new();

    for component in path.components() {
        match component {
            Component::ParentDir => {
                // Pop the last component if possible
                if !normalized.is_empty() && normalized.last() != Some(&"..") {
                    normalized.pop();
                } else {
                    // Can't go up further - this shouldn't happen for valid workspace paths
                    // but keep it for robustness
                    normalized.push("..");
                }
            }
            Component::CurDir => {
                // Skip `.` components
            }
            Component::Normal(s) => {
                if let Some(s) = s.to_str() {
                    normalized.push(s);
                }
            }
            _ => {}
        }
    }

    if normalized.is_empty() {
        String::new()
    } else {
        normalized.join("/")
    }
}

/// Format a canonical path as a markdown link for frontmatter (default format).
///
/// Creates a link in the format: `[Title](/canonical/path.md)`
///
/// This is a convenience function that uses `LinkFormat::MarkdownRoot`.
/// For other formats, use [`format_link_with_format`].
///
/// # Arguments
///
/// * `canonical_path` - The canonical (workspace-relative) path, without leading `/`
/// * `title` - The display title for the link
///
/// # Examples
///
/// ```
/// use diaryx_core::link_parser::format_link;
///
/// let link = format_link("Utility/utility_index.md", "Utility Index");
/// assert_eq!(link, "[Utility Index](/Utility/utility_index.md)");
/// ```
pub fn format_link(canonical_path: &str, title: &str) -> String {
    format!("[{}](/{})", title, canonical_path)
}

/// Format a link based on the specified format.
///
/// # Arguments
///
/// * `canonical_path` - The canonical (workspace-relative) path of the target file
/// * `title` - The display title for the link
/// * `format` - The link format to use
/// * `from_canonical_path` - The canonical path of the file containing this link
///   (required for relative formats)
///
/// # Examples
///
/// ```
/// use diaryx_core::link_parser::{format_link_with_format, LinkFormat};
///
/// // Markdown with root path (default)
/// let link = format_link_with_format(
///     "Folder/target.md",
///     "Target",
///     LinkFormat::MarkdownRoot,
///     "Other/source.md"
/// );
/// assert_eq!(link, "[Target](/Folder/target.md)");
///
/// // Markdown with relative path
/// let link = format_link_with_format(
///     "Folder/target.md",
///     "Target",
///     LinkFormat::MarkdownRelative,
///     "Folder/source.md"
/// );
/// assert_eq!(link, "[Target](target.md)");
///
/// // Plain relative path
/// let link = format_link_with_format(
///     "Folder/target.md",
///     "Target",
///     LinkFormat::PlainRelative,
///     "Folder/source.md"
/// );
/// assert_eq!(link, "target.md");
///
/// // Plain canonical path
/// let link = format_link_with_format(
///     "Folder/target.md",
///     "Target",
///     LinkFormat::PlainCanonical,
///     "Other/source.md"
/// );
/// assert_eq!(link, "Folder/target.md");
/// ```
pub fn format_link_with_format(
    canonical_path: &str,
    title: &str,
    format: LinkFormat,
    from_canonical_path: &str,
) -> String {
    match format {
        LinkFormat::MarkdownRoot => {
            format!("[{}](/{})", title, canonical_path)
        }
        LinkFormat::MarkdownRelative => {
            let relative = compute_relative_path(from_canonical_path, canonical_path);
            format!("[{}]({})", title, relative)
        }
        LinkFormat::PlainRelative => compute_relative_path(from_canonical_path, canonical_path),
        LinkFormat::PlainCanonical => canonical_path.to_string(),
    }
}

/// Compute a relative path from one file to another.
///
/// Both paths should be canonical (workspace-relative) paths.
///
/// # Examples
///
/// ```
/// use diaryx_core::link_parser::compute_relative_path;
///
/// // Same directory
/// assert_eq!(compute_relative_path("Folder/a.md", "Folder/b.md"), "b.md");
///
/// // Child directory
/// assert_eq!(compute_relative_path("Folder/index.md", "Folder/Sub/child.md"), "Sub/child.md");
///
/// // Parent directory
/// assert_eq!(compute_relative_path("Folder/Sub/child.md", "Folder/index.md"), "../index.md");
///
/// // Sibling directory
/// assert_eq!(compute_relative_path("A/file.md", "B/file.md"), "../B/file.md");
///
/// // Root file from subdirectory
/// assert_eq!(compute_relative_path("Folder/file.md", "README.md"), "../README.md");
/// ```
pub fn compute_relative_path(from_path: &str, to_path: &str) -> String {
    let from_dir = Path::new(from_path).parent().unwrap_or(Path::new(""));
    let to_path = Path::new(to_path);

    // Split both paths into components
    let from_components: Vec<&str> = from_dir
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    let to_components: Vec<&str> = to_path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    // Find common prefix length
    let common_len = from_components
        .iter()
        .zip(to_components.iter())
        .take_while(|(a, b)| a == b)
        .count();

    // Build relative path: go up for each remaining from_component, then down to target
    let ups = from_components.len().saturating_sub(common_len);
    let downs = &to_components[common_len..];

    let mut result_parts: Vec<&str> = vec![".."; ups];
    for part in downs {
        result_parts.push(part);
    }

    if result_parts.is_empty() {
        // Same directory - just return the filename
        to_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(to_path.to_str().unwrap_or(""))
            .to_string()
    } else {
        result_parts.join("/")
    }
}

/// Convert a link from its current format to a target format.
///
/// This parses the input link, extracts its canonical path, and reformats
/// it according to the target format. The title is preserved if present,
/// or generated from the path if not.
///
/// # Arguments
///
/// * `link` - The link string to convert (can be any supported format)
/// * `target_format` - The desired output format
/// * `current_file_path` - The canonical path of the file containing this link
///   (used to compute relative paths)
/// * `title_resolver` - Optional function to resolve the title for a canonical path.
///   If None, title will be extracted from existing link or generated from filename.
///
/// # Examples
///
/// ```
/// use diaryx_core::link_parser::{convert_link, LinkFormat};
///
/// // Convert a relative path to markdown root format
/// let result = convert_link("../parent.md", LinkFormat::MarkdownRoot, "Folder/child.md", None);
/// assert_eq!(result, "[Parent](/parent.md)");
///
/// // Convert a markdown link to plain relative
/// let result = convert_link("[Title](/Folder/file.md)", LinkFormat::PlainRelative, "Folder/other.md", None);
/// assert_eq!(result, "file.md");
/// ```
pub fn convert_link(
    link: &str,
    target_format: LinkFormat,
    current_file_path: &str,
    title_resolver: Option<&dyn Fn(&str) -> String>,
) -> String {
    let parsed = parse_link(link);
    let file_path = Path::new(current_file_path);

    // Get canonical path of the target
    let canonical = to_canonical(&parsed, file_path);

    // Resolve title: use existing title, or resolve via callback, or generate from path
    let title = parsed.title.unwrap_or_else(|| {
        title_resolver
            .map(|r| r(&canonical))
            .unwrap_or_else(|| path_to_title(&canonical))
    });

    format_link_with_format(&canonical, &title, target_format, current_file_path)
}

/// Convert all links in a contents array to a target format.
///
/// # Arguments
///
/// * `contents` - The list of content link strings
/// * `target_format` - The desired output format
/// * `current_file_path` - The canonical path of the file containing these links
/// * `title_resolver` - Optional function to resolve titles for canonical paths
///
/// # Returns
///
/// A vector of converted link strings in the target format.
pub fn convert_links(
    contents: &[String],
    target_format: LinkFormat,
    current_file_path: &str,
    title_resolver: Option<&dyn Fn(&str) -> String>,
) -> Vec<String> {
    contents
        .iter()
        .map(|link| convert_link(link, target_format, current_file_path, title_resolver))
        .collect()
}

/// Generate a display title from a canonical path.
///
/// Uses the filename without extension, converting underscores/hyphens to spaces
/// and applying title case.
///
/// # Examples
///
/// ```
/// use diaryx_core::link_parser::path_to_title;
///
/// assert_eq!(path_to_title("utility_index.md"), "Utility Index");
/// assert_eq!(path_to_title("Folder/my-file.md"), "My File");
/// assert_eq!(path_to_title("2025.md"), "2025");
/// assert_eq!(path_to_title("README.md"), "README");
/// ```
pub fn path_to_title(path: &str) -> String {
    // Extract filename without extension
    let filename = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path);

    // Replace underscores and hyphens with spaces
    let spaced: String = filename
        .chars()
        .map(|c| if c == '_' || c == '-' { ' ' } else { c })
        .collect();

    // Apply title case (capitalize first letter of each word)
    spaced
        .split_whitespace()
        .map(|word| {
            let mut chars: Vec<char> = word.chars().collect();
            if let Some(first) = chars.first_mut() {
                *first = first.to_ascii_uppercase();
            }
            chars.into_iter().collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_markdown_link_with_root_path() {
        let link = parse_link("[Utility Index](/Utility/utility_index.md)");
        assert_eq!(link.title, Some("Utility Index".to_string()));
        assert_eq!(link.path, "Utility/utility_index.md");
        assert_eq!(link.path_type, PathType::WorkspaceRoot);
    }

    #[test]
    fn test_parse_markdown_link_with_relative_path() {
        let link = parse_link("[Parent](../index.md)");
        assert_eq!(link.title, Some("Parent".to_string()));
        assert_eq!(link.path, "../index.md");
        assert_eq!(link.path_type, PathType::Relative);
    }

    #[test]
    fn test_parse_markdown_link_with_ambiguous_path() {
        let link = parse_link("[Child](child.md)");
        assert_eq!(link.title, Some("Child".to_string()));
        assert_eq!(link.path, "child.md");
        assert_eq!(link.path_type, PathType::Ambiguous);
    }

    #[test]
    fn test_parse_plain_root_path() {
        let link = parse_link("/Utility/file.md");
        assert_eq!(link.title, None);
        assert_eq!(link.path, "Utility/file.md");
        assert_eq!(link.path_type, PathType::WorkspaceRoot);
    }

    #[test]
    fn test_parse_plain_relative_path() {
        let link = parse_link("../parent.md");
        assert_eq!(link.title, None);
        assert_eq!(link.path, "../parent.md");
        assert_eq!(link.path_type, PathType::Relative);
    }

    #[test]
    fn test_parse_plain_ambiguous_path() {
        let link = parse_link("child.md");
        assert_eq!(link.title, None);
        assert_eq!(link.path, "child.md");
        assert_eq!(link.path_type, PathType::Ambiguous);
    }

    #[test]
    fn test_parse_dotslash_relative() {
        let link = parse_link("./sibling.md");
        assert_eq!(link.path, "./sibling.md");
        assert_eq!(link.path_type, PathType::Relative);
    }

    #[test]
    fn test_to_canonical_workspace_root() {
        let link = parse_link("[Title](/Utility/file.md)");
        let canonical = to_canonical(&link, Path::new("Other/entry.md"));
        assert_eq!(canonical, "Utility/file.md");
    }

    #[test]
    fn test_to_canonical_relative_parent() {
        let link = parse_link("../index.md");
        let canonical = to_canonical(&link, Path::new("Folder/Sub/entry.md"));
        assert_eq!(canonical, "Folder/index.md");
    }

    #[test]
    fn test_to_canonical_relative_sibling() {
        let link = parse_link("./sibling.md");
        let canonical = to_canonical(&link, Path::new("Folder/entry.md"));
        assert_eq!(canonical, "Folder/sibling.md");
    }

    #[test]
    fn test_to_canonical_ambiguous() {
        let link = parse_link("child.md");
        let canonical = to_canonical(&link, Path::new("Folder/index.md"));
        assert_eq!(canonical, "Folder/child.md");
    }

    #[test]
    fn test_to_canonical_deep_relative() {
        let link = parse_link("../../root.md");
        let canonical = to_canonical(&link, Path::new("A/B/C/file.md"));
        assert_eq!(canonical, "A/root.md");
    }

    #[test]
    fn test_format_link() {
        let link = format_link("Utility/utility_index.md", "Utility Index");
        assert_eq!(link, "[Utility Index](/Utility/utility_index.md)");
    }

    #[test]
    fn test_format_link_root_file() {
        let link = format_link("README.md", "README");
        assert_eq!(link, "[README](/README.md)");
    }

    #[test]
    fn test_path_to_title_underscore() {
        assert_eq!(path_to_title("utility_index.md"), "Utility Index");
    }

    #[test]
    fn test_path_to_title_hyphen() {
        assert_eq!(path_to_title("my-file.md"), "My File");
    }

    #[test]
    fn test_path_to_title_with_path() {
        assert_eq!(path_to_title("Folder/sub_file.md"), "Sub File");
    }

    #[test]
    fn test_path_to_title_number() {
        assert_eq!(path_to_title("2025.md"), "2025");
    }

    #[test]
    fn test_path_to_title_uppercase() {
        assert_eq!(path_to_title("README.md"), "README");
    }

    #[test]
    fn test_roundtrip_link() {
        // Parse a markdown link, convert to canonical, format back
        let original = "[Daily Index](/Daily/daily_index.md)";
        let parsed = parse_link(original);
        let canonical = to_canonical(&parsed, Path::new("Other/file.md"));
        let title = parsed.title.unwrap_or_else(|| path_to_title(&canonical));
        let formatted = format_link(&canonical, &title);

        assert_eq!(formatted, "[Daily Index](/Daily/daily_index.md)");
    }

    #[test]
    fn test_roundtrip_relative_to_canonical_to_formatted() {
        // Start with relative path, convert to canonical, format as markdown link
        let relative = "../parent_index.md";
        let parsed = parse_link(relative);
        let canonical = to_canonical(&parsed, Path::new("Folder/child.md"));
        let title = path_to_title(&canonical);
        let formatted = format_link(&canonical, &title);

        assert_eq!(canonical, "parent_index.md");
        assert_eq!(formatted, "[Parent Index](/parent_index.md)");
    }

    // =========================================================================
    // compute_relative_path tests
    // =========================================================================

    #[test]
    fn test_compute_relative_path_same_directory() {
        assert_eq!(compute_relative_path("Folder/a.md", "Folder/b.md"), "b.md");
    }

    #[test]
    fn test_compute_relative_path_child_directory() {
        assert_eq!(
            compute_relative_path("Folder/index.md", "Folder/Sub/child.md"),
            "Sub/child.md"
        );
    }

    #[test]
    fn test_compute_relative_path_parent_directory() {
        assert_eq!(
            compute_relative_path("Folder/Sub/child.md", "Folder/index.md"),
            "../index.md"
        );
    }

    #[test]
    fn test_compute_relative_path_sibling_directory() {
        assert_eq!(
            compute_relative_path("A/file.md", "B/file.md"),
            "../B/file.md"
        );
    }

    #[test]
    fn test_compute_relative_path_root_from_subdir() {
        assert_eq!(
            compute_relative_path("Folder/file.md", "README.md"),
            "../README.md"
        );
    }

    #[test]
    fn test_compute_relative_path_deep_to_root() {
        assert_eq!(
            compute_relative_path("A/B/C/file.md", "README.md"),
            "../../../README.md"
        );
    }

    // =========================================================================
    // format_link_with_format tests
    // =========================================================================

    #[test]
    fn test_format_link_with_format_markdown_root() {
        let link = format_link_with_format(
            "Folder/target.md",
            "Target",
            LinkFormat::MarkdownRoot,
            "Other/source.md",
        );
        assert_eq!(link, "[Target](/Folder/target.md)");
    }

    #[test]
    fn test_format_link_with_format_markdown_relative_same_dir() {
        let link = format_link_with_format(
            "Folder/target.md",
            "Target",
            LinkFormat::MarkdownRelative,
            "Folder/source.md",
        );
        assert_eq!(link, "[Target](target.md)");
    }

    #[test]
    fn test_format_link_with_format_markdown_relative_parent() {
        let link = format_link_with_format(
            "Folder/target.md",
            "Target",
            LinkFormat::MarkdownRelative,
            "Folder/Sub/source.md",
        );
        assert_eq!(link, "[Target](../target.md)");
    }

    #[test]
    fn test_format_link_with_format_plain_relative() {
        let link = format_link_with_format(
            "Folder/target.md",
            "Target",
            LinkFormat::PlainRelative,
            "Folder/source.md",
        );
        assert_eq!(link, "target.md");
    }

    #[test]
    fn test_format_link_with_format_plain_canonical() {
        let link = format_link_with_format(
            "Folder/target.md",
            "Target",
            LinkFormat::PlainCanonical,
            "Other/source.md",
        );
        assert_eq!(link, "Folder/target.md");
    }

    // =========================================================================
    // LinkFormat tests
    // =========================================================================

    #[test]
    fn test_link_format_default() {
        assert_eq!(LinkFormat::default(), LinkFormat::MarkdownRoot);
    }

    #[test]
    fn test_link_format_serialize() {
        assert_eq!(
            serde_json::to_string(&LinkFormat::MarkdownRoot).unwrap(),
            "\"markdown_root\""
        );
        assert_eq!(
            serde_json::to_string(&LinkFormat::MarkdownRelative).unwrap(),
            "\"markdown_relative\""
        );
        assert_eq!(
            serde_json::to_string(&LinkFormat::PlainRelative).unwrap(),
            "\"plain_relative\""
        );
        assert_eq!(
            serde_json::to_string(&LinkFormat::PlainCanonical).unwrap(),
            "\"plain_canonical\""
        );
    }

    #[test]
    fn test_link_format_deserialize() {
        assert_eq!(
            serde_json::from_str::<LinkFormat>("\"markdown_root\"").unwrap(),
            LinkFormat::MarkdownRoot
        );
        assert_eq!(
            serde_json::from_str::<LinkFormat>("\"markdown_relative\"").unwrap(),
            LinkFormat::MarkdownRelative
        );
        assert_eq!(
            serde_json::from_str::<LinkFormat>("\"plain_relative\"").unwrap(),
            LinkFormat::PlainRelative
        );
        assert_eq!(
            serde_json::from_str::<LinkFormat>("\"plain_canonical\"").unwrap(),
            LinkFormat::PlainCanonical
        );
    }

    // =========================================================================
    // convert_link tests
    // =========================================================================

    #[test]
    fn test_convert_relative_to_markdown_root() {
        let result = convert_link(
            "../parent.md",
            LinkFormat::MarkdownRoot,
            "Folder/child.md",
            None,
        );
        assert_eq!(result, "[Parent](/parent.md)");
    }

    #[test]
    fn test_convert_markdown_root_to_plain_relative() {
        let result = convert_link(
            "[Title](/Folder/file.md)",
            LinkFormat::PlainRelative,
            "Folder/other.md",
            None,
        );
        assert_eq!(result, "file.md");
    }

    #[test]
    fn test_convert_preserves_title() {
        let result = convert_link(
            "[Custom Title](/Folder/file.md)",
            LinkFormat::MarkdownRelative,
            "Other/source.md",
            None,
        );
        assert_eq!(result, "[Custom Title](../Folder/file.md)");
    }

    #[test]
    fn test_convert_plain_canonical_to_markdown_root() {
        // Ambiguous paths (no leading / or ..) are treated as relative to current file's directory
        // So "Sub/file.md" from "Folder/source.md" resolves to "Folder/Sub/file.md"
        let result = convert_link(
            "Sub/file.md",
            LinkFormat::MarkdownRoot,
            "Folder/source.md",
            None,
        );
        assert_eq!(result, "[File](/Folder/Sub/file.md)");
    }

    #[test]
    fn test_convert_with_title_resolver() {
        let resolver = |path: &str| format!("Resolved: {}", path);
        let result = convert_link(
            "../file.md",
            LinkFormat::MarkdownRoot,
            "Folder/source.md",
            Some(&resolver),
        );
        assert_eq!(result, "[Resolved: file.md](/file.md)");
    }

    #[test]
    fn test_convert_links_batch() {
        let contents = vec![
            "../parent.md".to_string(),
            "sibling.md".to_string(),
            "child/index.md".to_string(),
        ];
        let result = convert_links(&contents, LinkFormat::MarkdownRoot, "Folder/index.md", None);
        assert_eq!(
            result,
            vec![
                "[Parent](/parent.md)".to_string(),
                "[Sibling](/Folder/sibling.md)".to_string(),
                "[Index](/Folder/child/index.md)".to_string(),
            ]
        );
    }

    // =========================================================================
    // Comprehensive Bidirectional Conversion Tests
    // =========================================================================
    //
    // These tests verify that links can be converted between all format pairs.
    // Test setup: File at "Projects/Work/notes.md" links to "Projects/ideas.md"
    //
    // In each format, the link looks like:
    // - MarkdownRoot:     [Ideas](/Projects/ideas.md)
    // - MarkdownRelative: [Ideas](../ideas.md)
    // - PlainRelative:    ../ideas.md
    // - PlainCanonical:   Projects/ideas.md

    // --- MarkdownRoot as source ---

    #[test]
    fn test_convert_markdown_root_to_markdown_root() {
        // Same format - should be unchanged
        let result = convert_link(
            "[Ideas](/Projects/ideas.md)",
            LinkFormat::MarkdownRoot,
            "Projects/Work/notes.md",
            None,
        );
        assert_eq!(result, "[Ideas](/Projects/ideas.md)");
    }

    #[test]
    fn test_convert_markdown_root_to_markdown_relative() {
        let result = convert_link(
            "[Ideas](/Projects/ideas.md)",
            LinkFormat::MarkdownRelative,
            "Projects/Work/notes.md",
            None,
        );
        assert_eq!(result, "[Ideas](../ideas.md)");
    }

    #[test]
    fn test_convert_markdown_root_to_plain_relative_bidirectional() {
        let result = convert_link(
            "[Ideas](/Projects/ideas.md)",
            LinkFormat::PlainRelative,
            "Projects/Work/notes.md",
            None,
        );
        assert_eq!(result, "../ideas.md");
    }

    #[test]
    fn test_convert_markdown_root_to_plain_canonical() {
        let result = convert_link(
            "[Ideas](/Projects/ideas.md)",
            LinkFormat::PlainCanonical,
            "Projects/Work/notes.md",
            None,
        );
        assert_eq!(result, "Projects/ideas.md");
    }

    // --- MarkdownRelative as source ---

    #[test]
    fn test_convert_markdown_relative_to_markdown_root() {
        let result = convert_link(
            "[Ideas](../ideas.md)",
            LinkFormat::MarkdownRoot,
            "Projects/Work/notes.md",
            None,
        );
        assert_eq!(result, "[Ideas](/Projects/ideas.md)");
    }

    #[test]
    fn test_convert_markdown_relative_to_markdown_relative() {
        // Same format - should be unchanged
        let result = convert_link(
            "[Ideas](../ideas.md)",
            LinkFormat::MarkdownRelative,
            "Projects/Work/notes.md",
            None,
        );
        assert_eq!(result, "[Ideas](../ideas.md)");
    }

    #[test]
    fn test_convert_markdown_relative_to_plain_relative() {
        let result = convert_link(
            "[Ideas](../ideas.md)",
            LinkFormat::PlainRelative,
            "Projects/Work/notes.md",
            None,
        );
        assert_eq!(result, "../ideas.md");
    }

    #[test]
    fn test_convert_markdown_relative_to_plain_canonical() {
        let result = convert_link(
            "[Ideas](../ideas.md)",
            LinkFormat::PlainCanonical,
            "Projects/Work/notes.md",
            None,
        );
        assert_eq!(result, "Projects/ideas.md");
    }

    // --- PlainRelative as source ---

    #[test]
    fn test_convert_plain_relative_to_markdown_root() {
        let result = convert_link(
            "../ideas.md",
            LinkFormat::MarkdownRoot,
            "Projects/Work/notes.md",
            None,
        );
        assert_eq!(result, "[Ideas](/Projects/ideas.md)");
    }

    #[test]
    fn test_convert_plain_relative_to_markdown_relative() {
        let result = convert_link(
            "../ideas.md",
            LinkFormat::MarkdownRelative,
            "Projects/Work/notes.md",
            None,
        );
        assert_eq!(result, "[Ideas](../ideas.md)");
    }

    #[test]
    fn test_convert_plain_relative_to_plain_relative() {
        // Same format - should be unchanged
        let result = convert_link(
            "../ideas.md",
            LinkFormat::PlainRelative,
            "Projects/Work/notes.md",
            None,
        );
        assert_eq!(result, "../ideas.md");
    }

    #[test]
    fn test_convert_plain_relative_to_plain_canonical() {
        let result = convert_link(
            "../ideas.md",
            LinkFormat::PlainCanonical,
            "Projects/Work/notes.md",
            None,
        );
        assert_eq!(result, "Projects/ideas.md");
    }

    // --- PlainCanonical as source ---
    // Note: PlainCanonical paths without leading / are treated as relative to current file's dir

    #[test]
    fn test_convert_plain_canonical_to_markdown_root_from_root() {
        // When current file is at root, ambiguous path is relative to root
        let result = convert_link(
            "Projects/ideas.md",
            LinkFormat::MarkdownRoot,
            "README.md",
            None,
        );
        assert_eq!(result, "[Ideas](/Projects/ideas.md)");
    }

    #[test]
    fn test_convert_plain_canonical_to_markdown_relative_from_root() {
        let result = convert_link(
            "Projects/ideas.md",
            LinkFormat::MarkdownRelative,
            "README.md",
            None,
        );
        assert_eq!(result, "[Ideas](Projects/ideas.md)");
    }

    #[test]
    fn test_convert_plain_canonical_to_plain_relative_from_root() {
        let result = convert_link(
            "Projects/ideas.md",
            LinkFormat::PlainRelative,
            "README.md",
            None,
        );
        assert_eq!(result, "Projects/ideas.md");
    }

    #[test]
    fn test_convert_plain_canonical_to_plain_canonical_from_root() {
        let result = convert_link(
            "Projects/ideas.md",
            LinkFormat::PlainCanonical,
            "README.md",
            None,
        );
        assert_eq!(result, "Projects/ideas.md");
    }

    // --- Round-trip conversion tests ---
    // Verify that converting to a format and back produces equivalent results

    #[test]
    fn test_roundtrip_markdown_root_through_plain_relative() {
        let original = "[My Document](/Folder/Sub/document.md)";
        let current_file = "Folder/index.md";

        // Convert to PlainRelative
        let as_relative = convert_link(original, LinkFormat::PlainRelative, current_file, None);
        assert_eq!(as_relative, "Sub/document.md");

        // Convert back to MarkdownRoot
        let back = convert_link(&as_relative, LinkFormat::MarkdownRoot, current_file, None);
        assert_eq!(back, "[Document](/Folder/Sub/document.md)");
    }

    #[test]
    fn test_roundtrip_markdown_relative_through_plain_canonical() {
        let original = "[Notes](../notes.md)";
        let current_file = "Projects/Work/tasks.md";

        // Convert to PlainCanonical
        let as_canonical = convert_link(original, LinkFormat::PlainCanonical, current_file, None);
        assert_eq!(as_canonical, "Projects/notes.md");

        // Note: Converting back from PlainCanonical is tricky because the path is ambiguous.
        // "Projects/notes.md" without a leading / or .. is treated as relative to current file's directory.
        // So from "Projects/Work/tasks.md", it would resolve to "Projects/Work/Projects/notes.md".
        // This is a known limitation - PlainCanonical round-trips require the file to be at workspace root.
    }

    // --- Same directory tests ---

    #[test]
    fn test_convert_same_directory_markdown_root_to_relative() {
        let result = convert_link(
            "[Sibling](/Folder/sibling.md)",
            LinkFormat::PlainRelative,
            "Folder/current.md",
            None,
        );
        assert_eq!(result, "sibling.md");
    }

    #[test]
    fn test_convert_same_directory_relative_to_markdown_root() {
        let result = convert_link(
            "sibling.md",
            LinkFormat::MarkdownRoot,
            "Folder/current.md",
            None,
        );
        assert_eq!(result, "[Sibling](/Folder/sibling.md)");
    }

    // --- Root file tests ---

    #[test]
    fn test_convert_from_root_to_subdir_markdown_root() {
        let result = convert_link(
            "[Child](/Projects/child.md)",
            LinkFormat::PlainRelative,
            "README.md",
            None,
        );
        assert_eq!(result, "Projects/child.md");
    }

    #[test]
    fn test_convert_from_subdir_to_root_markdown_root() {
        let result = convert_link(
            "[Root](/README.md)",
            LinkFormat::PlainRelative,
            "Projects/Work/deep.md",
            None,
        );
        assert_eq!(result, "../../README.md");
    }

    // --- Deep nesting tests ---

    #[test]
    fn test_convert_deep_nested_markdown_root_to_relative() {
        let result = convert_link(
            "[Target](/A/B/C/target.md)",
            LinkFormat::PlainRelative,
            "X/Y/Z/source.md",
            None,
        );
        assert_eq!(result, "../../../A/B/C/target.md");
    }

    #[test]
    fn test_convert_deep_nested_relative_to_markdown_root() {
        let result = convert_link(
            "../../../A/B/C/target.md",
            LinkFormat::MarkdownRoot,
            "X/Y/Z/source.md",
            None,
        );
        assert_eq!(result, "[Target](/A/B/C/target.md)");
    }

    // --- Contents array with multiple formats ---

    #[test]
    fn test_convert_mixed_format_contents_to_markdown_root() {
        let contents = vec![
            "[Explicit](/Folder/explicit.md)".to_string(), // Already MarkdownRoot
            "../relative.md".to_string(),                  // PlainRelative
            "sibling.md".to_string(),                      // Ambiguous/PlainCanonical
        ];
        let result = convert_links(
            &contents,
            LinkFormat::MarkdownRoot,
            "Folder/Sub/index.md",
            None,
        );
        assert_eq!(
            result,
            vec![
                "[Explicit](/Folder/explicit.md)".to_string(),
                "[Relative](/Folder/relative.md)".to_string(),
                "[Sibling](/Folder/Sub/sibling.md)".to_string(),
            ]
        );
    }

    #[test]
    fn test_convert_markdown_root_contents_to_plain_relative() {
        let contents = vec![
            "[Parent](/Folder/parent.md)".to_string(),
            "[Sibling](/Folder/Sub/sibling.md)".to_string(),
            "[Deep](/Folder/Sub/Deep/file.md)".to_string(),
        ];
        let result = convert_links(
            &contents,
            LinkFormat::PlainRelative,
            "Folder/Sub/index.md",
            None,
        );
        assert_eq!(
            result,
            vec![
                "../parent.md".to_string(),
                "sibling.md".to_string(),
                "Deep/file.md".to_string(),
            ]
        );
    }

    // --- Title preservation tests ---

    #[test]
    fn test_title_preserved_through_all_markdown_formats() {
        let original = "[Custom Title](/path/to/file.md)";
        let current = "other/file.md";

        let to_relative = convert_link(original, LinkFormat::MarkdownRelative, current, None);
        assert!(to_relative.starts_with("[Custom Title]"));

        let back_to_root = convert_link(&to_relative, LinkFormat::MarkdownRoot, current, None);
        assert!(back_to_root.starts_with("[Custom Title]"));
    }

    #[test]
    fn test_title_generated_for_plain_to_markdown() {
        let result = convert_link(
            "../my-important-file.md",
            LinkFormat::MarkdownRoot,
            "Folder/index.md",
            None,
        );
        // Title should be generated from filename
        assert_eq!(result, "[My Important File](/my-important-file.md)");
    }

    // =========================================================================
    // Integration test: Full file conversion workflow
    // =========================================================================

    #[test]
    fn test_full_file_conversion_workflow() {
        use crate::frontmatter;
        use serde_yaml::Value;

        // Simulate a file at "Projects/index.md" with various link formats
        let original_content = r#"---
title: Projects Index
part_of: "[Root](/README.md)"
contents:
  - "[Work](/Projects/Work/index.md)"
  - "[Personal](/Projects/Personal/index.md)"
---
# Projects

This is the projects index.
"#;

        // Parse the file
        let parsed = frontmatter::parse_or_empty(original_content).unwrap();
        let mut fm = parsed.frontmatter.clone();
        let current_file = "Projects/index.md";

        // Convert part_of to PlainRelative
        if let Some(part_of_value) = fm.get("part_of") {
            if let Some(part_of_str) = part_of_value.as_str() {
                let converted =
                    convert_link(part_of_str, LinkFormat::PlainRelative, current_file, None);
                assert_eq!(converted, "../README.md");
                fm.insert("part_of".to_string(), Value::String(converted));
            }
        }

        // Convert contents to PlainRelative
        if let Some(contents_value) = fm.get("contents") {
            if let Some(contents_seq) = contents_value.as_sequence() {
                let mut new_contents = Vec::new();
                for item in contents_seq {
                    if let Some(item_str) = item.as_str() {
                        let converted =
                            convert_link(item_str, LinkFormat::PlainRelative, current_file, None);
                        new_contents.push(Value::String(converted));
                    }
                }
                fm.insert("contents".to_string(), Value::Sequence(new_contents));
            }
        }

        // Serialize back
        let new_content = frontmatter::serialize(&fm, &parsed.body).unwrap();

        // Verify the converted content
        assert!(new_content.contains("part_of: ../README.md"));
        assert!(new_content.contains("Work/index.md"));
        assert!(new_content.contains("Personal/index.md"));

        // Now convert back to MarkdownRoot
        let parsed2 = frontmatter::parse_or_empty(&new_content).unwrap();
        let mut fm2 = parsed2.frontmatter.clone();

        if let Some(part_of_value) = fm2.get("part_of") {
            if let Some(part_of_str) = part_of_value.as_str() {
                let converted =
                    convert_link(part_of_str, LinkFormat::MarkdownRoot, current_file, None);
                assert_eq!(converted, "[README](/README.md)");
                fm2.insert("part_of".to_string(), Value::String(converted));
            }
        }

        if let Some(contents_value) = fm2.get("contents") {
            if let Some(contents_seq) = contents_value.as_sequence() {
                let mut new_contents = Vec::new();
                for item in contents_seq {
                    if let Some(item_str) = item.as_str() {
                        let converted =
                            convert_link(item_str, LinkFormat::MarkdownRoot, current_file, None);
                        new_contents.push(Value::String(converted));
                    }
                }
                fm2.insert("contents".to_string(), Value::Sequence(new_contents));
            }
        }

        let final_content = frontmatter::serialize(&fm2, &parsed2.body).unwrap();

        // Verify round-trip preserves target paths
        assert!(final_content.contains("[README](/README.md)"));
        assert!(final_content.contains("[Index](/Projects/Work/index.md)"));
        assert!(final_content.contains("[Index](/Projects/Personal/index.md)"));
    }

    #[test]
    fn test_detect_link_format_change() {
        // This test verifies that we can detect when a link needs conversion

        let markdown_root_link = "[Title](/Folder/file.md)";
        let current_file = "Other/source.md";

        // Converting MarkdownRoot to MarkdownRoot should not change it
        let same_format = convert_link(
            markdown_root_link,
            LinkFormat::MarkdownRoot,
            current_file,
            None,
        );
        assert_eq!(same_format, markdown_root_link);

        // Converting MarkdownRoot to PlainRelative should change it
        let different_format = convert_link(
            markdown_root_link,
            LinkFormat::PlainRelative,
            current_file,
            None,
        );
        assert_ne!(different_format, markdown_root_link);
        assert_eq!(different_format, "../Folder/file.md");
    }
}
