use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::config::Config;
use crate::error::{DiaryxError, Result};
use crate::fs::FileSystem;

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

    /// Additional frontmatter properties
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, Value>,
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
}

/// Represents a parsed index file
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub struct TreeNode {
    pub name: String,
    pub description: Option<String>,
    pub path: PathBuf,
    pub children: Vec<TreeNode>,
}

/// Workspace operations
pub struct Workspace<FS: FileSystem> {
    fs: FS,
}

impl<FS: FileSystem> Workspace<FS> {
    pub fn new(fs: FS) -> Self {
        Self { fs }
    }

    /// Parse a markdown file and extract index frontmatter
    pub fn parse_index(&self, path: &Path) -> Result<IndexFile> {
        let content = self
            .fs
            .read_to_string(path)
            .map_err(|e| DiaryxError::FileRead {
                path: path.to_path_buf(),
                source: e,
            })?;

        // Check if content starts with frontmatter delimiter
        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            return Err(DiaryxError::NoFrontmatter(path.to_path_buf()));
        }

        // Find the closing delimiter
        let rest = &content[4..]; // Skip first "---\n"
        let end_idx = rest
            .find("\n---\n")
            .or_else(|| rest.find("\n---\r\n"))
            .ok_or_else(|| DiaryxError::NoFrontmatter(path.to_path_buf()))?;

        let frontmatter_str = &rest[..end_idx];
        let body = &rest[end_idx + 5..]; // Skip "\n---\n"

        let frontmatter: IndexFrontmatter = serde_yaml::from_str(frontmatter_str)?;

        Ok(IndexFile {
            path: path.to_path_buf(),
            frontmatter,
            body: body.to_string(),
        })
    }

    /// Check if a file is an index file (has contents property)
    pub fn is_index_file(&self, path: &Path) -> bool {
        if !path.extension().is_some_and(|ext| ext == "md") {
            return false;
        }

        self.parse_index(path)
            .map(|idx| idx.frontmatter.is_index())
            .unwrap_or(false)
    }

    /// Check if a file is a root index (has contents but no part_of)
    pub fn is_root_index(&self, path: &Path) -> bool {
        self.parse_index(path)
            .map(|idx| idx.frontmatter.is_root())
            .unwrap_or(false)
    }

    /// Find a root index in the given directory
    pub fn find_root_index_in_dir(&self, dir: &Path) -> Result<Option<PathBuf>> {
        let md_files = self
            .fs
            .list_md_files(dir)
            .map_err(|e| DiaryxError::FileRead {
                path: dir.to_path_buf(),
                source: e,
            })?;

        for file in md_files {
            if self.is_root_index(&file) {
                return Ok(Some(file));
            }
        }

        Ok(None)
    }

    /// Detect the workspace root from the current directory
    /// Searches current directory for a root index file
    pub fn detect_workspace(&self, start_dir: &Path) -> Result<Option<PathBuf>> {
        // Look for root index in start directory
        if let Some(root) = self.find_root_index_in_dir(start_dir)? {
            return Ok(Some(root));
        }

        Ok(None)
    }

    /// Resolve workspace: check current dir, then fall back to config default
    pub fn resolve_workspace(&self, current_dir: &Path, config: &Config) -> Result<PathBuf> {
        // First, try to detect workspace in current directory
        if let Some(root) = self.detect_workspace(current_dir)? {
            return Ok(root);
        }

        // Fall back to config's base_dir and look for root index there
        if let Some(root) = self.find_root_index_in_dir(&config.base_dir)? {
            return Ok(root);
        }

        // If no root index exists in base_dir, return the expected README.md path
        // (it may need to be created)
        Ok(config.base_dir.join("README.md"))
    }

    /// Initialize a new workspace with a root index file
    pub fn init_workspace(
        &self,
        dir: &Path,
        title: Option<&str>,
        description: Option<&str>,
    ) -> Result<PathBuf> {
        let readme_path = dir.join("README.md");

        if self.fs.exists(&readme_path) {
            // Check if it's already a root index
            if self.is_root_index(&readme_path) {
                return Err(DiaryxError::WorkspaceAlreadyExists(dir.to_path_buf()));
            }
        }

        // Create directory if needed
        std::fs::create_dir_all(dir)?;

        let display_title = title.unwrap_or_else(|| {
            dir.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Workspace")
        });

        let desc = description.unwrap_or("A diaryx workspace");

        let content = format!(
            "---\ntitle: {}\ndescription: {}\ncontents: []\n---\n\n# {}\n\n{}\n",
            display_title, desc, display_title, desc
        );

        self.fs
            .create_new(&readme_path, &content)
            .map_err(|e| DiaryxError::FileWrite {
                path: readme_path.clone(),
                source: e,
            })?;

        Ok(readme_path)
    }

    /// Build a tree structure from the workspace hierarchy
    pub fn build_tree(&self, root_path: &Path) -> Result<TreeNode> {
        let index = self.parse_index(root_path)?;

        let name = index
            .frontmatter
            .display_name()
            .map(String::from)
            .unwrap_or_else(|| {
                // Fall back to filename without extension
                root_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(String::from)
                    .unwrap_or_else(|| root_path.display().to_string())
            });

        let mut children = Vec::new();

        for child_path_str in index.frontmatter.contents_list() {
            let child_path = index.resolve_path(child_path_str);

            // Only include if the file exists
            if self.fs.exists(&child_path) {
                match self.build_tree(&child_path) {
                    Ok(child_node) => children.push(child_node),
                    Err(_) => {
                        // If we can't parse a child, include it as a leaf with error indication
                        children.push(TreeNode {
                            name: format!("{} (error)", child_path_str),
                            description: None,
                            path: child_path,
                            children: Vec::new(),
                        });
                    }
                }
            }
            // Ignore non-existent paths (as per spec: "ignore by default")
        }

        Ok(TreeNode {
            name,
            description: index.frontmatter.description,
            path: root_path.to_path_buf(),
            children,
        })
    }

    /// Format tree for display (like the `tree` command)
    pub fn format_tree(&self, node: &TreeNode, prefix: &str) -> String {
        let mut result = String::new();

        // Add the current node (root has no connector)
        result.push_str(prefix);
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

    /// Get workspace info as formatted string
    pub fn workspace_info(&self, root_path: &Path) -> Result<String> {
        let tree = self.build_tree(root_path)?;
        Ok(self.format_tree(&tree, "").trim_end().to_string())
    }
}

/// Format a child node and its descendants (standalone helper function)
fn format_tree_node(node: &TreeNode, prefix: &str) -> String {
    let mut result = String::new();

    // Add node name and description
    result.push_str(&node.name);
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

    #[test]
    fn test_index_frontmatter_is_root() {
        // Root with children
        let root = IndexFrontmatter {
            title: Some("Test".to_string()),
            description: None,
            contents: Some(vec!["child/README.md".to_string()]),
            part_of: None,
            extra: Default::default(),
        };
        assert!(root.is_root());
        assert!(root.is_index());

        // Root with empty contents (still a root because contents key is present)
        let empty_root = IndexFrontmatter {
            title: Some("Empty Root".to_string()),
            description: None,
            contents: Some(vec![]),
            part_of: None,
            extra: Default::default(),
        };
        assert!(empty_root.is_root());
        assert!(empty_root.is_index());

        // Leaf index (has part_of, has contents key but empty)
        let leaf = IndexFrontmatter {
            title: Some("Leaf".to_string()),
            description: None,
            contents: Some(vec![]),
            part_of: Some("../README.md".to_string()),
            extra: Default::default(),
        };
        assert!(!leaf.is_root());
        assert!(leaf.is_index());

        // Regular entry (no contents key at all)
        let entry = IndexFrontmatter {
            title: Some("Entry".to_string()),
            description: None,
            contents: None,
            part_of: Some("../README.md".to_string()),
            extra: Default::default(),
        };
        assert!(!entry.is_root());
        assert!(!entry.is_index());

        // Middle index (has part_of and has children)
        let middle = IndexFrontmatter {
            title: Some("Middle".to_string()),
            description: None,
            contents: Some(vec!["sub/README.md".to_string()]),
            part_of: Some("../README.md".to_string()),
            extra: Default::default(),
        };
        assert!(!middle.is_root()); // has part_of, so not root
        assert!(middle.is_index());
    }

    #[test]
    fn test_tree_node_formatting() {
        use crate::fs::RealFileSystem;

        let ws = Workspace::new(RealFileSystem);

        let tree = TreeNode {
            name: "Root".to_string(),
            description: Some("The root".to_string()),
            path: PathBuf::from("/test"),
            children: vec![
                TreeNode {
                    name: "Child 1".to_string(),
                    description: Some("First child".to_string()),
                    path: PathBuf::from("/test/child1"),
                    children: vec![],
                },
                TreeNode {
                    name: "Child 2".to_string(),
                    description: None,
                    path: PathBuf::from("/test/child2"),
                    children: vec![TreeNode {
                        name: "Grandchild".to_string(),
                        description: Some("Nested".to_string()),
                        path: PathBuf::from("/test/child2/grand"),
                        children: vec![],
                    }],
                },
            ],
        };

        let output = ws.format_tree(&tree, "");
        assert!(output.contains("Root - The root"));
        assert!(output.contains("├── Child 1 - First child"));
        assert!(output.contains("└── Child 2"));
        assert!(output.contains("    └── Grandchild - Nested"));
    }
}
