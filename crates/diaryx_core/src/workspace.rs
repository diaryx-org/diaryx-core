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

    /// List of paths to attachment files (images, documents, etc.) relative to this file
    /// Attachments declared here are available to this entry and all children
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<String>>,

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
#[derive(Debug, Clone, Serialize)]
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

            let old_dir_name = current_dir
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| DiaryxError::InvalidPath {
                    path: current_dir.to_path_buf(),
                    message: "Invalid directory name".to_string(),
                })?
                .to_string();

            // Get the old file name
            let old_file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| DiaryxError::InvalidPath {
                    path: path.to_path_buf(),
                    message: "Invalid file name".to_string(),
                })?
                .to_string();

            // Rename the directory (this moves all files within)
            self.fs
                .move_file(current_dir, &new_dir_path)
                .map_err(|e| DiaryxError::FileWrite {
                    path: new_dir_path.clone(),
                    source: e,
                })?;

            // Now rename the file itself from old name to new name (e.g., index.md -> newname.md)
            let old_file_in_new_dir = new_dir_path.join(&old_file_name);
            if old_file_in_new_dir != new_file_path {
                self.fs
                    .move_file(&old_file_in_new_dir, &new_file_path)
                    .map_err(|e| DiaryxError::FileWrite {
                        path: new_file_path.clone(),
                        source: e,
                    })?;
            }

            // Update children's part_of references to point to the new filename
            let app = DiaryxApp::new(&self.fs);
            if let Ok(child_files) = self.fs.list_md_files(&new_dir_path) {
                for child_file in child_files {
                    // Skip the index file itself
                    if child_file == new_file_path {
                        continue;
                    }

                    let child_str = child_file.to_string_lossy();
                    if let Ok(fm) = app.get_all_frontmatter(&child_str)
                        && let Some(Value::String(old_part_of)) = fm.get("part_of")
                    {
                        // Check if it points to the old filename
                        if old_part_of == &old_file_name {
                            let _ = app.set_frontmatter_property(
                                &child_str,
                                "part_of",
                                Value::String(new_filename.to_string()),
                            );
                        }
                    }
                }
            }

            // Update grandparent's contents if there's an index file there
            if let Ok(Some(grandparent_index)) = self.find_any_index_in_dir(parent_of_dir) {
                let app = DiaryxApp::new(&self.fs);
                // Update references: old format could be "oldname/" or "oldname/oldname.md" or "oldname/index.md"
                let old_ref_slash = format!("{}/", old_dir_name);
                let old_ref_dirname = format!("{}/{}.md", old_dir_name, old_dir_name);
                let old_ref_index = format!("{}/index.md", old_dir_name);
                let new_reference = format!("{}/{}", new_dir_name, new_filename);

                // Try to remove any of the possible old reference formats
                let _ = app.remove_from_index_contents(&grandparent_index, &old_ref_slash);
                let _ = app.remove_from_index_contents(&grandparent_index, &old_ref_dirname);
                let _ = app.remove_from_index_contents(&grandparent_index, &old_ref_index);
                let _ = app.remove_from_index_contents(&grandparent_index, &old_file_name);
                let _ = app.add_to_index_contents(&grandparent_index, &new_reference);
            }

            Ok(new_file_path)
        } else {
            // For leaf files, simple rename
            let parent = path.parent().ok_or_else(|| DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "No parent directory".to_string(),
            })?;

            let new_path = parent.join(new_filename);

            // Don't rename if same path
            if new_path == path {
                return Ok(path.to_path_buf());
            }

            // Use move_entry which handles contents updates
            self.move_entry(path, &new_path)?;

            Ok(new_path)
        }
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

        // Check if file exists
        if !self.fs.exists(path) {
            return Err(DiaryxError::FileRead {
                path: path.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "File does not exist"),
            });
        }

        // If it's an index file, check that contents is empty
        if let Ok(index) = self.parse_index(path)
            && index.frontmatter.is_index()
            && !index.frontmatter.contents_list().is_empty()
        {
            return Err(DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "Cannot delete: entry has children. Delete children first.".to_string(),
            });
        }

        // Get filename for updating parent's contents
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "Invalid file name".to_string(),
            })?
            .to_string();

        // Get parent directory
        let parent = path.parent().ok_or_else(|| DiaryxError::InvalidPath {
            path: path.to_path_buf(),
            message: "No parent directory".to_string(),
        })?;

        // Check if this is an index file (we need to handle reference differently)
        let is_index = self.is_index_file(path);

        // Delete the file
        self.fs
            .delete_file(path)
            .map_err(|e| DiaryxError::FileWrite {
                path: path.to_path_buf(),
                source: e,
            })?;

        // Remove from parent's contents if there's a parent index
        if is_index {
            // For index files, the grandparent's contents has the reference
            if let Some(grandparent) = parent.parent()
                && let Ok(Some(grandparent_index)) = self.find_any_index_in_dir(grandparent)
            {
                let app = DiaryxApp::new(&self.fs);
                let dir_name = parent.file_name().and_then(|n| n.to_str()).unwrap_or("");
                // Try to remove various possible reference formats
                let _ =
                    app.remove_from_index_contents(&grandparent_index, &format!("{}/", dir_name));
                let _ = app.remove_from_index_contents(
                    &grandparent_index,
                    &format!("{}/{}", dir_name, filename),
                );
                let _ = app.remove_from_index_contents(
                    &grandparent_index,
                    &format!("{}/index.md", dir_name),
                );
            }
        } else {
            // For leaf files, parent's contents has the reference
            if let Ok(Some(parent_index)) = self.find_any_index_in_dir(parent) {
                let app = DiaryxApp::new(&self.fs);
                let _ = app.remove_from_index_contents(&parent_index, &filename);
            }
        }

        Ok(())
    }

    /// Generate a unique filename for a new child entry in the given directory.
    ///
    /// Returns filenames like "new-entry.md", "new-entry-1.md", "new-entry-2.md", etc.
    pub fn generate_unique_child_name(&self, parent_dir: &Path) -> String {
        let base = "new-entry";
        let ext = ".md";

        // First try without a number
        let first = format!("{}{}", base, ext);
        if !self.fs.exists(&parent_dir.join(&first)) {
            return first;
        }

        // Try with incrementing numbers
        let mut counter = 1u32;
        loop {
            let name = format!("{}-{}{}", base, counter, ext);
            if !self.fs.exists(&parent_dir.join(&name)) {
                return name;
            }
            counter += 1;
            // Safety valve
            if counter > 10000 {
                return format!("{}-{}{}", base, chrono::Utc::now().timestamp(), ext);
            }
        }
    }

    /// Convert a leaf file into an index file with a directory.
    ///
    /// This method:
    /// - Creates a directory with the same name as the file (without .md)
    /// - Moves the file into the directory as `{dirname}.md`
    /// - Adds `contents: []` to the frontmatter
    /// - Adjusts `part_of` path to account for the new nesting level (e.g., `parent.md` → `../parent.md`)
    ///
    /// Example: `journal/my-note.md` → `journal/my-note/my-note.md`
    ///
    /// Returns the new path to the index file.
    pub fn convert_to_index(&self, path: &Path) -> Result<PathBuf> {
        use crate::entry::DiaryxApp;

        // Validate file exists and is a markdown file
        if !self.fs.exists(path) {
            return Err(DiaryxError::FileRead {
                path: path.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "File does not exist"),
            });
        }

        // If already an index file, return error
        if self.is_index_file(path) {
            return Err(DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "File is already an index file (has contents property)".to_string(),
            });
        }

        // Get the file stem (name without .md extension)
        let file_stem =
            path.file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| DiaryxError::InvalidPath {
                    path: path.to_path_buf(),
                    message: "Invalid file name".to_string(),
                })?;

        // Get old filename for updating parent's contents
        let old_filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "Invalid file name".to_string(),
            })?
            .to_string();

        // Calculate new paths - use {dirname}.md instead of index.md
        let parent_dir = path.parent().unwrap_or_else(|| Path::new(""));
        let new_dir = parent_dir.join(file_stem);
        let new_index_filename = format!("{}.md", file_stem);
        let new_index_path = new_dir.join(&new_index_filename);

        // Create the new directory
        self.fs
            .create_dir_all(&new_dir)
            .map_err(|e| DiaryxError::FileWrite {
                path: new_dir.clone(),
                source: e,
            })?;

        // Read the current file content to preserve frontmatter
        let app = DiaryxApp::new(&self.fs);
        let path_str = path.to_string_lossy();

        // Get existing frontmatter
        let frontmatter = app.get_all_frontmatter(&path_str)?;

        // Move the file to the new location
        self.fs
            .move_file(path, &new_index_path)
            .map_err(|e| DiaryxError::FileWrite {
                path: new_index_path.clone(),
                source: e,
            })?;

        let new_index_str = new_index_path.to_string_lossy();

        // Add contents: [] property to make it an index
        app.set_frontmatter_property(&new_index_str, "contents", Value::Sequence(vec![]))?;

        // Adjust part_of path if it exists (add ../ prefix since we're one level deeper)
        if let Some(Value::String(old_part_of)) = frontmatter.get("part_of") {
            let new_part_of = format!("../{}", old_part_of);
            app.set_frontmatter_property(&new_index_str, "part_of", Value::String(new_part_of))?;
        }

        // Update parent index if there is one (change reference from file.md to file/{file}.md)
        if let Ok(Some(parent_index)) = self.find_any_index_in_dir(parent_dir) {
            let new_reference = format!("{}/{}", file_stem, new_index_filename);

            let _ = app.remove_from_index_contents(&parent_index, &old_filename);
            let _ = app.add_to_index_contents(&parent_index, &new_reference);
        }

        Ok(new_index_path)
    }

    /// Convert an empty index file back to a leaf file.
    ///
    /// This method:
    /// - Fails if the index has non-empty `contents`
    /// - Moves `dir/{name}.md` → `parent/dir.md`
    /// - Removes the now-empty directory
    /// - Removes the `contents` property
    /// - Adjusts `part_of` path to account for the reduced nesting level (e.g., `../parent.md` → `parent.md`)
    ///
    /// Example: `journal/my-note/my-note.md` → `journal/my-note.md`
    ///
    /// Returns the new path to the leaf file.
    pub fn convert_to_leaf(&self, path: &Path) -> Result<PathBuf> {
        use crate::entry::DiaryxApp;

        // Validate file exists
        if !self.fs.exists(path) {
            return Err(DiaryxError::FileRead {
                path: path.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "File does not exist"),
            });
        }

        // Parse as index and check if it has empty contents
        let index = self.parse_index(path)?;
        if !index.frontmatter.is_index() {
            return Err(DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "File is not an index file (no contents property)".to_string(),
            });
        }

        if !index.frontmatter.contents_list().is_empty() {
            return Err(DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "Cannot convert to leaf: contents is not empty".to_string(),
            });
        }

        // Get the directory name to use as the new filename
        let index_dir = path.parent().ok_or_else(|| DiaryxError::InvalidPath {
            path: path.to_path_buf(),
            message: "Index file has no parent directory".to_string(),
        })?;

        let dir_name = index_dir
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "Invalid directory name".to_string(),
            })?;

        // Get the current filename for updating parent's contents
        let old_filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "Invalid file name".to_string(),
            })?
            .to_string();

        // Calculate new path
        let grandparent = index_dir.parent().unwrap_or_else(|| Path::new(""));
        let new_leaf_path = grandparent.join(format!("{}.md", dir_name));

        // Move the file
        self.fs
            .move_file(path, &new_leaf_path)
            .map_err(|e| DiaryxError::FileWrite {
                path: new_leaf_path.clone(),
                source: e,
            })?;

        let app = DiaryxApp::new(&self.fs);
        let new_leaf_str = new_leaf_path.to_string_lossy();

        // Remove the contents property
        app.remove_frontmatter_property(&new_leaf_str, "contents")?;

        // Adjust part_of path if it exists (remove ../ prefix since we're one level shallower)
        let frontmatter = app.get_all_frontmatter(&new_leaf_str)?;
        if let Some(Value::String(old_part_of)) = frontmatter.get("part_of")
            && let Some(new_part_of) = old_part_of.strip_prefix("../")
        {
            app.set_frontmatter_property(
                &new_leaf_str,
                "part_of",
                Value::String(new_part_of.to_string()),
            )?;
        }

        // Update parent index if there is one (change reference from dir/{name}.md to dir.md)
        if let Ok(Some(parent_index)) = self.find_any_index_in_dir(grandparent) {
            let old_reference = format!("{}/{}", dir_name, old_filename);
            let new_reference = format!("{}.md", dir_name);

            let _ = app.remove_from_index_contents(&parent_index, &old_reference);
            let _ = app.add_to_index_contents(&parent_index, &new_reference);
        }

        // Note: We don't delete the empty directory here because some filesystems
        // don't support delete_dir and it's not critical. The directory will be empty.

        Ok(new_leaf_path)
    }

    /// Attach an entry to a parent, converting the parent to an index if needed,
    /// and moving the entry file into the parent's directory.
    ///
    /// This is a higher-level operation that combines:
    /// 1. Convert parent to index if it's a leaf file (creates directory)
    /// 2. Move entry into the parent's directory (if not already there)
    /// 3. Create bidirectional links (contents and part_of)
    ///
    /// Returns the new path to the entry after any moves.
    pub fn attach_and_move_entry_to_parent(&self, entry: &Path, parent: &Path) -> Result<PathBuf> {
        // Validate entry exists
        if !self.fs.exists(entry) {
            return Err(DiaryxError::FileRead {
                path: entry.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "Entry does not exist"),
            });
        }

        // Validate parent exists
        if !self.fs.exists(parent) {
            return Err(DiaryxError::FileRead {
                path: parent.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "Parent does not exist"),
            });
        }

        // Convert parent to index if needed (creates directory)
        let parent_index = if !self.is_index_file(parent) {
            self.convert_to_index(parent)?
        } else {
            parent.to_path_buf()
        };

        // Get parent directory (where the index file lives)
        let parent_dir = parent_index.parent().unwrap_or_else(|| Path::new(""));

        // Get entry filename
        let entry_filename = entry
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("entry.md")
            .to_string();

        // Check if entry is already in parent directory
        let entry_in_parent_dir = entry.parent() == Some(parent_dir);

        // Move entry if not already in parent directory
        let final_entry = if !entry_in_parent_dir {
            let new_entry_path = parent_dir.join(&entry_filename);

            // Handle filename collision
            let unique_path = if self.fs.exists(&new_entry_path) {
                self.generate_unique_path(parent_dir, &entry_filename)
            } else {
                new_entry_path
            };

            // Move the file
            self.fs
                .move_file(entry, &unique_path)
                .map_err(|e| DiaryxError::FileWrite {
                    path: unique_path.clone(),
                    source: e,
                })?;

            unique_path
        } else {
            entry.to_path_buf()
        };

        // Attach entry to parent (creates bidirectional links)
        self.attach_entry_to_parent(&final_entry, &parent_index)?;

        Ok(final_entry)
    }

    /// Generate a unique path for a file in a directory, handling collisions.
    fn generate_unique_path(&self, dir: &Path, filename: &str) -> PathBuf {
        let stem = filename.strip_suffix(".md").unwrap_or(filename);
        let mut counter = 2u32;

        loop {
            let numbered_name = format!("{}_{}.md", stem, counter);
            let numbered_path = dir.join(&numbered_name);
            if !self.fs.exists(&numbered_path) {
                return numbered_path;
            }
            counter += 1;
            if counter > 10000 {
                return dir.join(format!("{}_{}.md", stem, chrono::Utc::now().timestamp()));
            }
        }
    }

    /// Create a new child entry under a parent, converting the parent to an index if needed.
    ///
    /// This is a higher-level operation that combines:
    /// 1. Convert parent to index if it's a leaf file (creates directory)
    /// 2. Generate a unique filename in the parent's directory
    /// 3. Create the entry file with title, created, and updated frontmatter
    /// 4. Attach the new entry to the parent
    ///
    /// Returns the path to the newly created entry.
    pub fn create_child_entry(&self, parent: &Path, title: Option<&str>) -> Result<PathBuf> {
        use crate::entry::DiaryxApp;

        // Validate parent exists
        if !self.fs.exists(parent) {
            return Err(DiaryxError::FileRead {
                path: parent.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "Parent does not exist"),
            });
        }

        // Convert parent to index if needed (creates directory)
        let parent_index = if !self.is_index_file(parent) {
            self.convert_to_index(parent)?
        } else {
            parent.to_path_buf()
        };

        // Get parent directory
        let parent_dir = parent_index.parent().unwrap_or_else(|| Path::new(""));

        // Generate filename
        let (filename, entry_title) = if let Some(t) = title {
            let slug = self.slugify(t);
            let unique_name = if self.fs.exists(&parent_dir.join(format!("{}.md", slug))) {
                self.generate_unique_path(parent_dir, &format!("{}.md", slug))
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("entry.md")
                    .to_string()
            } else {
                format!("{}.md", slug)
            };
            (unique_name, t.to_string())
        } else {
            let name = self.generate_unique_child_name(parent_dir);
            let title = self.title_from_filename(&name);
            (name, title)
        };

        let child_path = parent_dir.join(&filename);
        let child_path_str = child_path.to_string_lossy().to_string();

        // Create the entry
        let app = DiaryxApp::new(&self.fs);
        app.create_entry(&child_path_str)?;

        // Set frontmatter properties
        app.set_frontmatter_property(&child_path_str, "title", Value::String(entry_title))?;

        let now = chrono::Utc::now().to_rfc3339();
        app.set_frontmatter_property(&child_path_str, "created", Value::String(now.clone()))?;
        app.set_frontmatter_property(&child_path_str, "updated", Value::String(now))?;

        // Attach to parent
        self.attach_entry_to_parent(&child_path, &parent_index)?;

        Ok(child_path)
    }

    /// Convert a title to a slug suitable for filenames.
    fn slugify(&self, title: &str) -> String {
        title
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-")
    }

    /// Convert a filename to a title.
    fn title_from_filename(&self, filename: &str) -> String {
        filename
            .trim_end_matches(".md")
            .replace(['-', '_'], " ")
            .split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
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
            attachments: None,
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
            attachments: None,
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
            attachments: None,
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
            attachments: None,
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
            attachments: None,
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
