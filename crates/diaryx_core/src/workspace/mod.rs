//! Workspace operations module.
//!
//! This module provides functionality for working with Diaryx workspaces:
//! - Tree traversal and building
//! - File operations (move, rename, delete)
//! - Index management (contents, part_of relationships)
//!
//! # Module Structure
//!
//! - `types` - Core data types (IndexFrontmatter, IndexFile, TreeNode)

mod types;

// Re-export types for backwards compatibility
pub use types::{format_tree_node, IndexFile, IndexFrontmatter, TreeNode};

use std::path::{Path, PathBuf};

use serde_yaml::Value;

use crate::config::Config;
use crate::error::{DiaryxError, Result};
use crate::fs::FileSystem;

/// Workspace operations
pub struct Workspace<FS: FileSystem> {
    fs: FS,
}

impl<FS: FileSystem> Workspace<FS> {
    /// Create a new workspace
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
            if let Ok(index) = self.parse_index(&file)
                && index.frontmatter.is_index()
            {
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
        if let Ok(index) = self.parse_index(path)
            && index.frontmatter.is_index()
        {
            for child_path_str in index.frontmatter.contents_list() {
                let child_path = index.resolve_path(child_path_str);

                // Only include if the file exists
                if self.fs.exists(&child_path) {
                    self.collect_workspace_files_recursive(&child_path, files, visited)?;
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
        self.fs.create_dir_all(dir)?;

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

    /// Build a tree structure from the actual filesystem (for "Show All Files" mode)
    /// Unlike build_tree, this scans directories for actual files rather than following contents references
    pub fn build_filesystem_tree(
        &self,
        root_dir: &Path,
        show_hidden: bool,
    ) -> Result<TreeNode> {
        self.build_filesystem_tree_recursive(root_dir, show_hidden)
    }

    fn build_filesystem_tree_recursive(
        &self,
        dir: &Path,
        show_hidden: bool,
    ) -> Result<TreeNode> {
        // Get directory name for display
        let dir_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| dir.to_string_lossy().to_string());

        // Try to find an index file in this directory to get title/description
        let (name, description, index_path) = if let Ok(Some(index)) = self.find_any_index_in_dir(dir) {
            if let Ok(parsed) = self.parse_index(&index) {
                let title = parsed.frontmatter.title.unwrap_or_else(|| dir_name.clone());
                (title, parsed.frontmatter.description, Some(index))
            } else {
                (dir_name.clone(), None, Some(index))
            }
        } else {
            (dir_name.clone(), None, None)
        };

        // The path to use - if there's an index, use it; otherwise use the directory
        let node_path = index_path.unwrap_or_else(|| dir.to_path_buf());

        // List all entries in this directory
        let mut children = Vec::new();
        if let Ok(entries) = self.fs.list_files(dir) {
            let mut entries: Vec<_> = entries.into_iter().collect();
            entries.sort(); // Sort alphabetically

            for entry in entries {
                let file_name = entry
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                // Skip hidden files unless show_hidden is true
                if !show_hidden && file_name.starts_with('.') {
                    continue;
                }

                if self.fs.is_dir(&entry) {
                    // Recurse into subdirectory
                    if let Ok(child_tree) = self.build_filesystem_tree_recursive(&entry, show_hidden) {
                        children.push(child_tree);
                    }
                } else {
                    // It's a file - skip index files (already represented by parent dir)
                    if self.is_index_file(&entry) {
                        continue;
                    }

                    // Get title from frontmatter if it's a markdown file
                    let (file_title, file_desc) = if entry.extension().is_some_and(|e| e == "md") {
                        if let Ok(parsed) = self.parse_index(&entry) {
                            (
                                parsed.frontmatter.title.unwrap_or(file_name.clone()),
                                parsed.frontmatter.description,
                            )
                        } else {
                            (file_name.clone(), None)
                        }
                    } else {
                        (file_name.clone(), None)
                    };

                    children.push(TreeNode {
                        name: file_title,
                        description: file_desc,
                        path: entry,
                        children: Vec::new(),
                    });
                }
            }
        }

        Ok(TreeNode {
            name,
            description,
            path: node_path,
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

    /// Attach an entry to a parent index, creating bidirectional links.
    ///
    /// This method:
    /// - Adds the entry to the parent index's `contents` list (relative to parent's directory)
    /// - Sets the entry's `part_of` property to point to the parent index (relative to entry)
    ///
    /// Both paths must exist. Uses `DiaryxApp` for frontmatter manipulation.
    pub fn attach_entry_to_parent(
        &self,
        entry_path: &Path,
        parent_index_path: &Path,
    ) -> Result<()> {
        use crate::entry::DiaryxApp;
        use crate::path_utils::{
            relative_path_from_dir_to_target, relative_path_from_file_to_target,
        };

        // Validate both paths exist
        if !self.fs.exists(entry_path) {
            return Err(DiaryxError::FileRead {
                path: entry_path.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "Entry does not exist"),
            });
        }
        if !self.fs.exists(parent_index_path) {
            return Err(DiaryxError::FileRead {
                path: parent_index_path.to_path_buf(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Parent index does not exist",
                ),
            });
        }

        let app = DiaryxApp::new(&self.fs);

        // Calculate relative path from parent's directory to entry
        let parent_dir = parent_index_path.parent().unwrap_or_else(|| Path::new(""));
        let child_rel = relative_path_from_dir_to_target(parent_dir, entry_path);

        // Add entry to parent's contents
        app.add_to_index_contents(parent_index_path, &child_rel)?;

        // Calculate relative path from entry to parent index
        let parent_rel = relative_path_from_file_to_target(entry_path, parent_index_path);

        // Set entry's part_of
        let entry_str = entry_path.to_string_lossy();
        app.set_frontmatter_property(&entry_str, "part_of", Value::String(parent_rel))?;

        Ok(())
    }

    /// Move/rename an entry while updating workspace index references.
    ///
    /// This method:
    /// - Moves the file from `from_path` to `to_path`
    /// - Removes the entry from old parent's `contents` (if parent index exists)
    /// - Adds the entry to new parent's `contents` (if parent index exists)
    /// - Updates the moved file's `part_of` to point to new parent index
    ///
    /// Returns `Ok(())` if successful. Does nothing if source equals destination.
    pub fn move_entry(&self, from_path: &Path, to_path: &Path) -> Result<()> {
        use crate::entry::DiaryxApp;
        use crate::path_utils::relative_path_from_file_to_target;

        // No-op if same path
        if from_path == to_path {
            return Ok(());
        }

        // Get filenames and parent directories before moving
        let old_parent = from_path.parent().ok_or_else(|| DiaryxError::InvalidPath {
            path: from_path.to_path_buf(),
            message: "No parent directory for source path".to_string(),
        })?;
        let old_file_name = from_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: from_path.to_path_buf(),
                message: "Invalid source file name".to_string(),
            })?
            .to_string();

        let new_parent = to_path.parent().ok_or_else(|| DiaryxError::InvalidPath {
            path: to_path.to_path_buf(),
            message: "No parent directory for destination path".to_string(),
        })?;
        let new_file_name = to_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: to_path.to_path_buf(),
                message: "Invalid destination file name".to_string(),
            })?
            .to_string();

        // Move the file
        self.fs
            .move_file(from_path, to_path)
            .map_err(|e| DiaryxError::FileWrite {
                path: to_path.to_path_buf(),
                source: e,
            })?;

        let app = DiaryxApp::new(&self.fs);

        // Remove from old parent's contents (if old parent has an index)
        if let Ok(Some(old_index_path)) = self.find_any_index_in_dir(old_parent) {
            let _ = app.remove_from_index_contents(&old_index_path, &old_file_name);
        }

        // Add to new parent's contents and update part_of (if new parent has an index)
        if let Ok(Some(new_index_path)) = self.find_any_index_in_dir(new_parent) {
            let _ = app.add_to_index_contents(&new_index_path, &new_file_name);

            // Update moved entry's part_of
            let rel_part_of = relative_path_from_file_to_target(to_path, &new_index_path);
            let to_str = to_path.to_string_lossy();
            let _ = app.set_frontmatter_property(&to_str, "part_of", Value::String(rel_part_of));
        }

        Ok(())
    }

    /// Delete an entry while updating workspace index references.
    ///
    /// This method:
    /// - Fails if the entry is an index with non-empty `contents` (has children)
    /// - Removes the entry from parent's `contents` (if parent index exists)
    /// - Deletes the file
    ///
    /// For index files with directories, only the file is deleted (not the directory).
    pub fn delete_entry(&self, path: &Path) -> Result<()> {
        use crate::entry::DiaryxApp;

        // Check if this is an index file with children
        if let Ok(index) = self.parse_index(path) {
            let contents = index.frontmatter.contents_list();
            if !contents.is_empty() {
                return Err(DiaryxError::InvalidPath {
                    path: path.to_path_buf(),
                    message: format!(
                        "Cannot delete index with {} children. Delete children first.",
                        contents.len()
                    ),
                });
            }
        }

        // Get the filename and parent directory
        let parent = path.parent().ok_or_else(|| DiaryxError::InvalidPath {
            path: path.to_path_buf(),
            message: "No parent directory".to_string(),
        })?;
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "Invalid file name".to_string(),
            })?
            .to_string();

        // Remove from parent's contents (if parent has an index)
        if let Ok(Some(index_path)) = self.find_any_index_in_dir(parent) {
            let app = DiaryxApp::new(&self.fs);
            let _ = app.remove_from_index_contents(&index_path, &file_name);
        }

        // Delete the file
        self.fs
            .delete_file(path)
            .map_err(|e| DiaryxError::FileWrite {
                path: path.to_path_buf(),
                source: e,
            })?;

        Ok(())
    }

    /// Generate a unique filename for a new child entry in the given directory.
    ///
    /// Returns filenames like "new-entry.md", "new-entry-1.md", "new-entry-2.md", etc.
    pub fn generate_unique_child_name(&self, parent_dir: &Path) -> String {
        let base_name = "new-entry";
        let mut candidate = format!("{}.md", base_name);
        let mut counter = 1;

        while self.fs.exists(&parent_dir.join(&candidate)) {
            candidate = format!("{}-{}.md", base_name, counter);
            counter += 1;
        }

        candidate
    }

    /// Create a new child entry under a parent index.
    ///
    /// This method:
    /// - Generates a unique filename if not provided
    /// - Creates the child file with basic frontmatter
    /// - Adds the child to the parent's `contents`
    /// - Sets the child's `part_of` to point to the parent
    ///
    /// Returns the path to the new child entry.
    pub fn create_child_entry(
        &self,
        parent_index_path: &Path,
        title: Option<&str>,
    ) -> Result<PathBuf> {
        use crate::entry::DiaryxApp;
        use crate::path_utils::relative_path_from_file_to_target;

        // Validate parent exists and is an index
        let parent_index = self.parse_index(parent_index_path)?;
        if !parent_index.frontmatter.is_index() {
            return Err(DiaryxError::InvalidPath {
                path: parent_index_path.to_path_buf(),
                message: "Parent is not an index file".to_string(),
            });
        }

        // Determine parent directory
        let parent_dir = parent_index_path
            .parent()
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: parent_index_path.to_path_buf(),
                message: "Parent index has no directory".to_string(),
            })?;

        // Generate unique filename
        let child_filename = self.generate_unique_child_name(parent_dir);
        let child_path = parent_dir.join(&child_filename);

        // Calculate relative path from child to parent
        let parent_rel = relative_path_from_file_to_target(&child_path, parent_index_path);

        // Create child file with frontmatter
        let display_title = title.unwrap_or("New Entry");
        let content = format!(
            "---\ntitle: {}\npart_of: {}\n---\n\n# {}\n\n",
            display_title, parent_rel, display_title
        );

        self.fs
            .create_new(&child_path, &content)
            .map_err(|e| DiaryxError::FileWrite {
                path: child_path.clone(),
                source: e,
            })?;

        // Add to parent's contents
        let app = DiaryxApp::new(&self.fs);
        app.add_to_index_contents(parent_index_path, &child_filename)?;

        Ok(child_path)
    }

    /// Rename an entry file by giving it a new filename.
    ///
    /// This method handles both leaf files and index files:
    /// - Leaf files: renames the file directly and updates parent `contents`
    /// - Index files: renames the containing directory AND the file itself, updates grandparent `contents`
    ///
    /// Returns the new path to the renamed file.
    pub fn rename_entry(&self, path: &Path, new_filename: &str) -> Result<PathBuf> {
        use crate::entry::DiaryxApp;

        let is_index = self.is_index_file(path);

        if is_index {
            // For index files, we rename the containing directory AND the file
            let current_dir = path.parent().ok_or_else(|| DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "Index file has no parent directory".to_string(),
            })?;

            let parent_of_dir = current_dir
                .parent()
                .ok_or_else(|| DiaryxError::InvalidPath {
                    path: path.to_path_buf(),
                    message: "Directory has no parent".to_string(),
                })?;

            // Get new directory name from the filename (strip .md extension)
            let new_dir_name = new_filename.trim_end_matches(".md");
            let new_dir_path = parent_of_dir.join(new_dir_name);
            // New file will be named {dirname}.md
            let new_file_path = new_dir_path.join(new_filename);

            // Don't rename if same path
            if new_dir_path == current_dir {
                return Ok(path.to_path_buf());
            }

            // Check if target directory already exists
            if self.fs.exists(&new_dir_path) {
                return Err(DiaryxError::InvalidPath {
                    path: new_dir_path,
                    message: "Target directory already exists".to_string(),
                });
            }

            // Create new directory
            self.fs.create_dir_all(&new_dir_path)?;

            // Move all files from old directory to new directory
            if let Ok(files) = self.fs.list_files(current_dir) {
                for file in files {
                    let file_name = file.file_name().unwrap_or_default();
                    let new_path = new_dir_path.join(file_name);
                    
                    // If this is the index file itself, use the new filename
                    if file == path {
                        self.fs.move_file(&file, &new_file_path)?;
                    } else {
                        self.fs.move_file(&file, &new_path)?;
                    }
                }
            }

            // Update grandparent's contents if it exists
            if let Ok(Some(grandparent_index)) = self.find_any_index_in_dir(parent_of_dir) {
                let app = DiaryxApp::new(&self.fs);
                let old_dir_name = current_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                
                // Calculate relative paths for old and new entries
                let old_rel = format!("{}/{}.md", old_dir_name, old_dir_name);
                let new_rel = format!("{}/{}", new_dir_name, new_filename);
                
                let _ = app.remove_from_index_contents(&grandparent_index, &old_rel);
                let _ = app.add_to_index_contents(&grandparent_index, &new_rel);
            }

            Ok(new_file_path)
        } else {
            // For leaf files, simple rename within the same directory
            let parent = path.parent().ok_or_else(|| DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "File has no parent directory".to_string(),
            })?;

            let old_filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| DiaryxError::InvalidPath {
                    path: path.to_path_buf(),
                    message: "Invalid file name".to_string(),
                })?
                .to_string();

            let new_path = parent.join(new_filename);

            // Don't rename if same path
            if new_path == path {
                return Ok(path.to_path_buf());
            }

            // Check if target already exists
            if self.fs.exists(&new_path) {
                return Err(DiaryxError::InvalidPath {
                    path: new_path,
                    message: "Target file already exists".to_string(),
                });
            }

            // Move the file
            self.fs.move_file(path, &new_path)?;

            // Update parent's contents if it exists
            if let Ok(Some(parent_index)) = self.find_any_index_in_dir(parent) {
                let app = DiaryxApp::new(&self.fs);
                let _ = app.remove_from_index_contents(&parent_index, &old_filename);
                let _ = app.add_to_index_contents(&parent_index, new_filename);
            }

            Ok(new_path)
        }
    }

    /// Convert a leaf file into an index file with a directory.
    ///
    /// This method:
    /// - Creates a directory with the same name as the file (without .md)
    /// - Moves the file into the directory as `{dirname}.md`
    /// - Adds empty `contents` property to the file
    ///
    /// Example: `journal/my-note.md` → `journal/my-note/my-note.md`
    ///
    /// Returns the new path to the index file.
    pub fn convert_to_index(&self, path: &Path) -> Result<PathBuf> {
        use crate::entry::DiaryxApp;

        // Check if already an index
        if self.is_index_file(path) {
            return Err(DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "File is already an index".to_string(),
            });
        }

        let parent = path.parent().ok_or_else(|| DiaryxError::InvalidPath {
            path: path.to_path_buf(),
            message: "File has no parent directory".to_string(),
        })?;

        let file_stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "Invalid file name".to_string(),
            })?;

        let old_filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "Invalid file name".to_string(),
            })?
            .to_string();

        // Create new directory and file paths
        let new_dir = parent.join(file_stem);
        let new_filename = format!("{}.md", file_stem);
        let new_path = new_dir.join(&new_filename);

        // Create directory
        self.fs.create_dir_all(&new_dir)?;

        // Move file into directory
        self.fs.move_file(path, &new_path)?;

        // Add contents property
        let app = DiaryxApp::new(&self.fs);
        let new_path_str = new_path.to_string_lossy();
        app.set_frontmatter_property(&new_path_str, "contents", Value::Sequence(vec![]))?;

        // Update parent's contents to point to new location
        if let Ok(Some(parent_index)) = self.find_any_index_in_dir(parent) {
            let _ = app.remove_from_index_contents(&parent_index, &old_filename);
            let new_rel = format!("{}/{}", file_stem, new_filename);
            let _ = app.add_to_index_contents(&parent_index, &new_rel);
        }

        Ok(new_path)
    }

    /// Convert an empty index file back to a leaf file.
    ///
    /// This method:
    /// - Fails if the index has non-empty `contents`
    /// - Moves `dir/{name}.md` → `parent/dir.md`
    /// - Removes the now-empty directory
    /// - Removes the `contents` property
    ///
    /// Example: `journal/my-note/my-note.md` → `journal/my-note.md`
    ///
    /// Returns the new path to the leaf file.
    pub fn convert_to_leaf(&self, path: &Path) -> Result<PathBuf> {
        use crate::entry::DiaryxApp;

        // Check if this is an index with empty contents
        let index = self.parse_index(path)?;
        let contents = index.frontmatter.contents_list();
        if !contents.is_empty() {
            return Err(DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: format!(
                    "Cannot convert index with {} children to leaf",
                    contents.len()
                ),
            });
        }

        let current_dir = path.parent().ok_or_else(|| DiaryxError::InvalidPath {
            path: path.to_path_buf(),
            message: "File has no parent directory".to_string(),
        })?;

        let parent_of_dir = current_dir
            .parent()
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "Directory has no parent".to_string(),
            })?;

        let dir_name = current_dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: current_dir.to_path_buf(),
                message: "Invalid directory name".to_string(),
            })?;

        let new_filename = format!("{}.md", dir_name);
        let new_path = parent_of_dir.join(&new_filename);

        // Check if target already exists
        if self.fs.exists(&new_path) {
            return Err(DiaryxError::InvalidPath {
                path: new_path,
                message: "Target file already exists".to_string(),
            });
        }

        // Move file out of directory
        self.fs.move_file(path, &new_path)?;

        // Remove contents property
        let app = DiaryxApp::new(&self.fs);
        let new_path_str = new_path.to_string_lossy();
        let _ = app.remove_frontmatter_property(&new_path_str, "contents");

        // Update grandparent's contents
        if let Ok(Some(grandparent_index)) = self.find_any_index_in_dir(parent_of_dir) {
            let old_rel = format!("{}/{}.md", dir_name, dir_name);
            let _ = app.remove_from_index_contents(&grandparent_index, &old_rel);
            let _ = app.add_to_index_contents(&grandparent_index, &new_filename);
        }

        Ok(new_path)
    }

    /// Attach an entry to a parent, converting the parent to an index if needed,
    /// and moving the entry file into the parent's directory.
    ///
    /// This is a higher-level operation that combines:
    /// 1. Convert parent to index if it's a leaf
    /// 2. Move entry into parent's directory
    /// 3. Create bidirectional links (contents and part_of)
    ///
    /// Returns the new path to the entry after any moves.
    pub fn attach_and_move_entry_to_parent(&self, entry: &Path, parent: &Path) -> Result<PathBuf> {
        // Check if parent needs to be converted to index
        let parent_is_index = self.is_index_file(parent);
        
        let effective_parent = if parent_is_index {
            parent.to_path_buf()
        } else {
            // Convert parent to index first
            self.convert_to_index(parent)?
        };

        // Get parent directory
        let parent_dir = effective_parent
            .parent()
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: effective_parent.clone(),
                message: "Parent index has no directory".to_string(),
            })?;

        // Get entry filename
        let entry_filename = entry
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: entry.to_path_buf(),
                message: "Invalid entry filename".to_string(),
            })?;

        // Calculate new path for entry
        let new_entry_path = parent_dir.join(entry_filename);

        // Move entry if not already in parent directory
        if entry.parent() != Some(parent_dir) {
            self.move_entry(entry, &new_entry_path)?;
        }

        // Attach entry to parent (creates bidirectional links)
        self.attach_entry_to_parent(&new_entry_path, &effective_parent)?;

        Ok(new_entry_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::MockFileSystem;

    #[test]
    fn test_index_frontmatter_is_root() {
        let root_fm = IndexFrontmatter {
            title: Some("Root".to_string()),
            description: None,
            contents: Some(vec![]),
            part_of: None,
            audience: None,
            attachments: None,
            extra: std::collections::HashMap::new(),
        };
        assert!(root_fm.is_root());
        assert!(root_fm.is_index());

        let non_root_fm = IndexFrontmatter {
            title: Some("Non-root".to_string()),
            description: None,
            contents: Some(vec![]),
            part_of: Some("../parent.md".to_string()),
            audience: None,
            attachments: None,
            extra: std::collections::HashMap::new(),
        };
        assert!(!non_root_fm.is_root());
        assert!(non_root_fm.is_index());
    }

    #[test]
    fn test_tree_node_formatting() {
        let tree = TreeNode {
            name: "Root".to_string(),
            description: Some("Root description".to_string()),
            path: PathBuf::from("root.md"),
            children: vec![
                TreeNode {
                    name: "Child 1".to_string(),
                    description: None,
                    path: PathBuf::from("child1.md"),
                    children: vec![],
                },
                TreeNode {
                    name: "Child 2".to_string(),
                    description: Some("Child desc".to_string()),
                    path: PathBuf::from("child2.md"),
                    children: vec![],
                },
            ],
        };

        let fs = MockFileSystem::new();
        let ws = Workspace::new(fs);
        let output = ws.format_tree(&tree, "");

        assert!(output.contains("Root - Root description"));
        assert!(output.contains("Child 1"));
        assert!(output.contains("Child 2 - Child desc"));
    }
}
