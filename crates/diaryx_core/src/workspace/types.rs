//! Workspace data types.
//!
//! This module contains the core data types for workspace operations:
//! - `IndexFrontmatter` - Parsed frontmatter for workspace files
//! - `IndexFile` - A parsed file with frontmatter and body
//! - `TreeNode` - A node in the workspace tree for display

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use ts_rs::TS;

/// Represents an index file's frontmatter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexFrontmatter {
    /// Display name for this index
    pub title: Option<String>,

    /// Description of this area
    pub description: Option<String>,

    /// List of paths to child index files (relative to this file)
    /// None means the key was absent; Some(vec) means it was present (even if empty)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contents: Option<Vec<String>>,

    /// Path to parent index file (relative to this file)
    /// If absent, this is a root index (workspace root)
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

    /// Resolve a relative path from this index's location
    pub fn resolve_path(&self, relative: &str) -> PathBuf {
        self.directory()
            .map(|dir| dir.join(relative))
            .unwrap_or_else(|| PathBuf::from(relative))
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
