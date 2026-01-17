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
        Some(Value::Tagged(_)) => Ok(None), // Tagged YAML values are rare, skip them
    }
}

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
}

impl IndexFile {
    /// Returns the directory containing this index file
    pub fn directory(&self) -> Option<&Path> {
        self.path.parent()
    }

    /// Resolve a relative path from this index's location.
    /// The path is normalized to handle `..` and `.` components,
    /// which is necessary for web/WASM where the virtual filesystem
    /// doesn't automatically resolve these.
    pub fn resolve_path(&self, relative: &str) -> PathBuf {
        let joined = self
            .directory()
            .map(|dir| dir.join(relative))
            .unwrap_or_else(|| PathBuf::from(relative));

        normalize_path(&joined)
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
