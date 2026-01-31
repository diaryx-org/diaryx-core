//! Workspace data types.
//!
//! This module contains the core data types for workspace operations:
//! - `IndexFrontmatter` - Parsed frontmatter for workspace files
//! - `IndexFile` - A parsed file with frontmatter and body
//! - `TreeNode` - A node in the workspace tree for display

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};
use serde_yaml::Value;
use ts_rs::TS;

use crate::link_parser::{self, LinkFormat};

/// Normalize a path by resolving `.` and `..` components without filesystem access.
/// This is necessary for web/WASM where the virtual filesystem doesn't handle `..` in paths.
fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = Vec::new();

    for component in path.components() {
        match component {
            Component::ParentDir => {
                // Pop the last component if possible (handle ..)
                if !normalized.is_empty()
                    && !matches!(normalized.last(), Some(Component::ParentDir))
                {
                    normalized.pop();
                } else {
                    // Can't go up further, keep the ..
                    normalized.push(component);
                }
            }
            Component::CurDir => {
                // Skip . components
            }
            _ => {
                normalized.push(component);
            }
        }
    }

    normalized.iter().collect()
}

/// Deserializes a value that should be a string, but may be an array or other type.
/// - String: returned as-is
/// - Array: takes the first element if it's a string
/// - Number/Bool: converted to string
/// - Null/None: returns None
fn deserialize_string_lenient<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: Option<Value> = Option::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(Value::Number(n)) => Ok(Some(n.to_string())),
        Some(Value::Bool(b)) => Ok(Some(b.to_string())),
        Some(Value::Null) => Ok(None),
        Some(Value::Sequence(seq)) => {
            // Take the first string element from the array
            for item in seq {
                if let Value::String(s) = item {
                    return Ok(Some(s));
                }
            }
            Ok(None)
        }
        Some(Value::Mapping(_)) => Ok(None), // Can't convert a mapping to string
        Some(Value::Tagged(_)) => Ok(None),  // Tagged YAML values are rare, skip them
    }
}

/// Represents an index file's frontmatter
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexFrontmatter {
    /// Display name for this index
    #[serde(default, deserialize_with = "deserialize_string_lenient")]
    pub title: Option<String>,

    /// Description of this area
    #[serde(default, deserialize_with = "deserialize_string_lenient")]
    pub description: Option<String>,

    /// List of paths to child index files (relative to this file)
    /// None means the key was absent; Some(vec) means it was present (even if empty)
    /// List of paths to child index files (relative to this file)
    /// None means the key was absent; Some(vec) means it was present (even if empty)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contents: Option<Vec<String>>,

    /// Path to parent index file (relative to this file)
    /// If absent, this is a root index (workspace root)
    #[serde(default, deserialize_with = "deserialize_string_lenient")]
    pub part_of: Option<String>,

    /// Audience groups that can see this file and its contents
    /// If absent, inherits from parent; if at root with no audience, treated as private
    /// Special value "private" means never export regardless of other values
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audience: Option<Vec<String>>,

    /// List of paths to attachment files (images, documents, etc.) relative to this file
    /// Attachments declared here are available to this entry and all children
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<String>>,

    /// Additional frontmatter properties
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

impl IndexFrontmatter {
    /// Returns true if this is a root index (has contents property but no part_of)
    pub fn is_root(&self) -> bool {
        self.contents.is_some() && self.part_of.is_none()
    }

    /// Returns true if this is an index file (has contents property, even if empty)
    pub fn is_index(&self) -> bool {
        self.contents.is_some()
    }

    /// Get contents as a slice, or empty slice if absent
    pub fn contents_list(&self) -> &[String] {
        self.contents.as_deref().unwrap_or(&[])
    }

    /// Get display name
    pub fn display_name(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Get attachments as a slice, or empty slice if absent
    pub fn attachments_list(&self) -> &[String] {
        self.attachments.as_deref().unwrap_or(&[])
    }

    /// Returns true if this file has attachments
    pub fn has_attachments(&self) -> bool {
        self.attachments.as_ref().is_some_and(|a| !a.is_empty())
    }

    /// Returns true if this file is marked as private (has "private" in audience)
    pub fn is_private(&self) -> bool {
        self.audience
            .as_ref()
            .is_some_and(|a| a.iter().any(|s| s.eq_ignore_ascii_case("private")))
    }

    /// Check if this file is visible to a given audience group
    /// Returns None if audience should be inherited from parent
    pub fn is_visible_to(&self, audience_group: &str) -> Option<bool> {
        // If marked private, never visible
        if self.is_private() {
            return Some(false);
        }

        // If no audience specified, inherit from parent
        let audience = self.audience.as_ref()?;

        // Check if the requested audience is in the list
        Some(
            audience
                .iter()
                .any(|a| a.eq_ignore_ascii_case(audience_group)),
        )
    }
}

/// Represents a parsed index file
#[derive(Debug, Clone, Serialize)]
pub struct IndexFile {
    /// Path to the index file
    pub path: PathBuf,

    /// Parsed frontmatter
    pub frontmatter: IndexFrontmatter,

    /// Body content (after frontmatter)
    pub body: String,

    /// Link format hint for resolving ambiguous paths.
    /// When set to Some(LinkFormat::PlainCanonical), ambiguous paths like "Folder/file.md"
    /// are resolved relative to workspace root instead of relative to current file.
    #[serde(skip)]
    pub link_format_hint: Option<LinkFormat>,
}

impl IndexFile {
    /// Returns the directory containing this index file
    pub fn directory(&self) -> Option<&Path> {
        self.path.parent()
    }

    /// Resolve a path reference from this index's location.
    ///
    /// Handles multiple formats:
    /// - Markdown links: `[Title](/path/file.md)` or `[Title](../file.md)`
    /// - Plain paths with `/` prefix (workspace-root): `/path/file.md`
    /// - Plain relative paths: `../file.md` or `./file.md`
    /// - Plain ambiguous paths: `path/file.md` (treated based on link_format_hint)
    ///
    /// For ambiguous paths (no `/` prefix or `../`):
    /// - If `link_format_hint` is `Some(PlainCanonical)`, resolves as workspace-root
    /// - Otherwise, resolves relative to current file's directory (legacy behavior)
    ///
    /// Returns an absolute path resolved against this index file's location.
    /// The path is normalized to handle `..` and `.` components,
    /// which is necessary for web/WASM where the virtual filesystem
    /// doesn't automatically resolve these.
    pub fn resolve_path(&self, path_ref: &str) -> PathBuf {
        // Parse the link to extract the actual path and determine type
        let parsed = link_parser::parse_link(path_ref);

        match parsed.path_type {
            link_parser::PathType::WorkspaceRoot => {
                // Workspace-root paths are already canonical (workspace-relative).
                // Return as PathBuf directly - callers operate relative to workspace root.
                PathBuf::from(&parsed.path)
            }
            link_parser::PathType::Relative => {
                // Explicit relative paths always resolve relative to current file
                let dir = self.directory().unwrap_or_else(|| std::path::Path::new(""));
                normalize_path(&dir.join(&parsed.path))
            }
            link_parser::PathType::Ambiguous => {
                // Ambiguous paths (like "folder/file.md") are always resolved relative
                // to the current file for backwards compatibility. This handles legacy data
                // that was written before proper link formats were enforced.
                //
                // The link_format_hint only affects how NEW links are WRITTEN:
                // - MarkdownRoot writes: [Title](/workspace/path.md) - explicit workspace-root
                // - PlainCanonical writes: workspace/path.md - also explicit workspace-root
                //
                // When READING, explicit workspace-root indicators (leading `/` or markdown
                // link syntax) are respected, but ambiguous paths default to file-relative.
                let dir = self.directory().unwrap_or_else(|| std::path::Path::new(""));
                normalize_path(&dir.join(&parsed.path))
            }
        }
    }
}

/// Node in the workspace tree (for display purposes)
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct TreeNode {
    /// Title of index/root file (or filename if no title)
    pub name: String,
    /// Description attribute (if given)
    pub description: Option<String>,
    /// Path to index/root file
    pub path: PathBuf,
    /// `contents` property list
    pub children: Vec<TreeNode>,
}

/// Helper function to format a tree node for display
pub fn format_tree_node(node: &TreeNode, prefix: &str) -> String {
    let mut result = String::new();

    // Add the current node name
    result.push_str(&node.name);

    // Add description if present
    if let Some(ref desc) = node.description {
        result.push_str(" - ");
        result.push_str(desc);
    }
    result.push('\n');

    // Add children
    let child_count = node.children.len();
    for (i, child) in node.children.iter().enumerate() {
        let is_last_child = i == child_count - 1;
        let connector = if is_last_child {
            "└── "
        } else {
            "├── "
        };
        let child_prefix = if is_last_child { "    " } else { "│   " };

        result.push_str(prefix);
        result.push_str(connector);
        result.push_str(&format_tree_node(
            child,
            &format!("{}{}", prefix, child_prefix),
        ));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_index_file(path: &str, link_format_hint: Option<LinkFormat>) -> IndexFile {
        IndexFile {
            path: PathBuf::from(path),
            frontmatter: IndexFrontmatter::default(),
            body: String::new(),
            link_format_hint,
        }
    }

    #[test]
    fn test_resolve_path_workspace_root() {
        // Workspace-root paths (with /) should always resolve as-is
        let index = make_index_file("A/B/index.md", None);
        let resolved = index.resolve_path("/Folder/file.md");
        assert_eq!(resolved, PathBuf::from("Folder/file.md"));
    }

    #[test]
    fn test_resolve_path_relative() {
        // Relative paths (../) should always resolve relative to current file
        let index = make_index_file("A/B/index.md", None);
        let resolved = index.resolve_path("../sibling.md");
        assert_eq!(resolved, PathBuf::from("A/sibling.md"));
    }

    #[test]
    fn test_resolve_path_ambiguous_no_hint() {
        // Without hint, ambiguous paths resolve relative to current file
        let index = make_index_file("A/B/index.md", None);
        let resolved = index.resolve_path("Folder/file.md");
        assert_eq!(resolved, PathBuf::from("A/B/Folder/file.md"));
    }

    #[test]
    fn test_resolve_path_ambiguous_with_plain_canonical_hint() {
        // Ambiguous paths always resolve relative to current file for backwards compatibility.
        // The link format hint only affects how NEW links are WRITTEN.
        let index = make_index_file("A/B/index.md", Some(LinkFormat::PlainCanonical));
        let resolved = index.resolve_path("Folder/file.md");
        assert_eq!(resolved, PathBuf::from("A/B/Folder/file.md"));
    }

    #[test]
    fn test_resolve_path_ambiguous_with_markdown_root_hint() {
        // Ambiguous paths always resolve relative to current file for backwards compatibility.
        // The link format hint only affects how NEW links are WRITTEN.
        let index = make_index_file("A/B/index.md", Some(LinkFormat::MarkdownRoot));
        let resolved = index.resolve_path("Folder/file.md");
        assert_eq!(resolved, PathBuf::from("A/B/Folder/file.md"));
    }

    #[test]
    fn test_resolve_path_markdown_link_root() {
        // Markdown links with root path
        let index = make_index_file("A/B/index.md", None);
        let resolved = index.resolve_path("[Title](/Folder/file.md)");
        assert_eq!(resolved, PathBuf::from("Folder/file.md"));
    }

    #[test]
    fn test_resolve_path_markdown_link_relative() {
        // Markdown links with relative path
        let index = make_index_file("A/B/index.md", None);
        let resolved = index.resolve_path("[Title](../sibling.md)");
        assert_eq!(resolved, PathBuf::from("A/sibling.md"));
    }

    #[test]
    fn test_resolve_path_markdown_link_ambiguous_with_hint() {
        // Markdown links with ambiguous path (no leading /) resolve as file-relative
        // for backwards compatibility. Only explicit workspace-root links use the root.
        let index = make_index_file("A/B/index.md", Some(LinkFormat::PlainCanonical));
        let resolved = index.resolve_path("[Title](Folder/file.md)");
        assert_eq!(resolved, PathBuf::from("A/B/Folder/file.md"));
    }

    #[test]
    fn test_resolve_path_plain_canonical_real_world_case() {
        // Ambiguous paths resolve relative to the current file for backwards compatibility.
        // When writing links, PlainCanonical format would produce workspace-root paths,
        // but when reading, we support legacy file-relative paths.
        let index = make_index_file("Projects/Ideas/index.md", Some(LinkFormat::PlainCanonical));

        // Contents ref - resolves relative to current file
        let resolved = index.resolve_path("Daily/2025/01/01.md");
        assert_eq!(
            resolved,
            PathBuf::from("Projects/Ideas/Daily/2025/01/01.md")
        );

        // Part_of ref - resolves relative to current file
        let resolved = index.resolve_path("Projects/index.md");
        assert_eq!(resolved, PathBuf::from("Projects/Ideas/Projects/index.md"));
    }

    #[test]
    fn test_resolve_path_ambiguous_with_markdown_relative_hint() {
        // With MarkdownRelative hint, ambiguous paths resolve relative (legacy behavior)
        let index = make_index_file("A/B/index.md", Some(LinkFormat::MarkdownRelative));
        let resolved = index.resolve_path("Folder/file.md");
        assert_eq!(resolved, PathBuf::from("A/B/Folder/file.md"));
    }

    #[test]
    fn test_resolve_path_ambiguous_with_plain_relative_hint() {
        // With PlainRelative hint, ambiguous paths resolve relative (legacy behavior)
        let index = make_index_file("A/B/index.md", Some(LinkFormat::PlainRelative));
        let resolved = index.resolve_path("Folder/file.md");
        assert_eq!(resolved, PathBuf::from("A/B/Folder/file.md"));
    }

    #[test]
    fn test_resolve_path_explicit_relative_ignores_hint() {
        // Explicit relative paths (with ./ or ../) should always resolve relative,
        // even with PlainCanonical hint
        let index = make_index_file("A/B/index.md", Some(LinkFormat::PlainCanonical));

        let resolved = index.resolve_path("./sibling.md");
        assert_eq!(resolved, PathBuf::from("A/B/sibling.md"));

        let resolved = index.resolve_path("../parent.md");
        assert_eq!(resolved, PathBuf::from("A/parent.md"));
    }
}
