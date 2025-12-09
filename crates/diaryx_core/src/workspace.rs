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

    /// Audience groups that can see this file and its contents
    /// If absent, inherits from parent; if at root with no audience, treated as private
    /// Special value "private" means never export regardless of other values
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audience: Option<Vec<String>>,

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
#[derive(Debug, Clone, Serialize)]
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

    /// Get a reference to the underlying filesystem
    pub fn fs_ref(&self) -> &FS {
        &self.fs
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
        if path.extension().is_none_or(|ext| ext != "md") {
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

    /// Find any index file in the given directory (has `contents` property)
    /// Prefers root indexes over non-root indexes
    pub fn find_any_index_in_dir(&self, dir: &Path) -> Result<Option<PathBuf>> {
        let md_files = self
            .fs
            .list_md_files(dir)
            .map_err(|e| DiaryxError::FileRead {
                path: dir.to_path_buf(),
                source: e,
            })?;

        let mut found_index: Option<PathBuf> = None;

        for file in md_files {
            if let Ok(index) = self.parse_index(&file) {
                if index.frontmatter.is_index() {
                    // Prefer root index if found
                    if index.frontmatter.is_root() {
                        return Ok(Some(file));
                    }
                    // Otherwise remember the first index we find
                    if found_index.is_none() {
                        found_index = Some(file);
                    }
                }
            }
        }

        Ok(found_index)
    }

    /// Collect all files reachable from an index via `contents` traversal
    /// Returns a list of all files including the index itself and all nested contents
    pub fn collect_workspace_files(&self, index_path: &Path) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        let mut visited = std::collections::HashSet::new();
        self.collect_workspace_files_recursive(index_path, &mut files, &mut visited)?;
        files.sort();
        Ok(files)
    }

    /// Recursive helper for collecting workspace files
    fn collect_workspace_files_recursive(
        &self,
        path: &Path,
        files: &mut Vec<PathBuf>,
        visited: &mut std::collections::HashSet<PathBuf>,
    ) -> Result<()> {
        // Canonicalize to handle relative paths consistently
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        // Avoid cycles
        if visited.contains(&canonical) {
            return Ok(());
        }
        visited.insert(canonical.clone());

        // Add this file to the list
        files.push(path.to_path_buf());

        // If this is an index file, recurse into its contents
        if let Ok(index) = self.parse_index(path) {
            if index.frontmatter.is_index() {
                for child_path_str in index.frontmatter.contents_list() {
                    let child_path = index.resolve_path(child_path_str);

                    // Only include if the file exists
                    if self.fs.exists(&child_path) {
                        self.collect_workspace_files_recursive(&child_path, files, visited)?;
                    }
                }
            }
        }

        Ok(())
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

        // Fall back to config's default_workspace and look for root index there
        if let Some(root) = self.find_root_index_in_dir(&config.default_workspace)? {
            return Ok(root);
        }

        // If no root index exists in default_workspace, return the expected README.md path
        // (it may need to be created)
        Ok(config.default_workspace.join("README.md"))
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
        self.build_tree_with_depth(root_path, None, &mut std::collections::HashSet::new())
    }

    /// Build a tree structure with depth limit and cycle detection
    /// `max_depth` of None means unlimited, Some(0) means just the root node
    pub fn build_tree_with_depth(
        &self,
        root_path: &Path,
        max_depth: Option<usize>,
        visited: &mut std::collections::HashSet<PathBuf>,
    ) -> Result<TreeNode> {
        let index = self.parse_index(root_path)?;

        // Canonicalize path for cycle detection
        let canonical = root_path
            .canonicalize()
            .unwrap_or_else(|_| root_path.to_path_buf());

        // Check for cycles
        if visited.contains(&canonical) {
            return Ok(TreeNode {
                name: format!(
                    "{} (cycle)",
                    root_path.file_name().unwrap_or_default().to_string_lossy()
                ),
                description: None,
                path: root_path.to_path_buf(),
                children: Vec::new(),
            });
        }
        visited.insert(canonical);

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
        let contents = index.frontmatter.contents_list();
        let child_count = contents.len();

        // Check if we've hit depth limit
        let at_depth_limit = max_depth.map(|d| d == 0).unwrap_or(false);

        if at_depth_limit && child_count > 0 {
            // Show truncation indicator
            children.push(TreeNode {
                name: format!("... ({} more)", child_count),
                description: None,
                path: root_path.to_path_buf(),
                children: Vec::new(),
            });
        } else {
            let next_depth = max_depth.map(|d| d.saturating_sub(1));

            for child_path_str in contents {
                let child_path = index.resolve_path(child_path_str);

                // Only include if the file exists
                if self.fs.exists(&child_path) {
                    match self.build_tree_with_depth(&child_path, next_depth, visited) {
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
        self.workspace_info_with_depth(root_path, None)
    }

    /// Get workspace info as formatted string with depth limit
    /// `max_depth` of None means unlimited
    pub fn workspace_info_with_depth(
        &self,
        root_path: &Path,
        max_depth: Option<usize>,
    ) -> Result<String> {
        let mut visited = std::collections::HashSet::new();
        let tree = self.build_tree_with_depth(root_path, max_depth, &mut visited)?;
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
            audience: None,
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
            audience: None,
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
            audience: None,
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
            audience: None,
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
            audience: None,
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
