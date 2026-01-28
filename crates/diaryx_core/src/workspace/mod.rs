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
//!
//! # Async-first Design
//!
//! This module uses `AsyncFileSystem` for all filesystem operations.
//! For synchronous contexts (CLI, tests), wrap a sync filesystem with
//! `SyncToAsyncFs` and use `futures_lite::future::block_on()`.

mod types;

// Re-export types for backwards compatibility
pub use types::{IndexFile, IndexFrontmatter, TreeNode, format_tree_node};

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use ts_rs::TS;

use crate::config::Config;
use crate::error::{DiaryxError, Result};
use crate::fs::AsyncFileSystem;
use crate::link_parser::{self, LinkFormat};

/// Workspace-level configuration stored in the root index file's frontmatter.
///
/// This allows workspace settings to live with the data (local-first philosophy)
/// rather than in separate config files.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct WorkspaceConfig {
    /// Format for `part_of` and `contents` links.
    /// Defaults to MarkdownRoot if not specified.
    #[serde(default)]
    pub link_format: LinkFormat,

    /// Subfolder for daily entries (e.g., "Daily" or "Journal/Daily").
    /// If not specified, daily entries are created at workspace root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_entry_folder: Option<String>,
}

/// Workspace operations (async-first).
///
/// All methods are async and use `AsyncFileSystem` for filesystem access.
pub struct Workspace<FS: AsyncFileSystem> {
    fs: FS,
    /// The workspace root directory path (for computing canonical paths)
    root_path: Option<PathBuf>,
    /// Link format for part_of and contents properties
    link_format: LinkFormat,
}

impl<FS: AsyncFileSystem> Workspace<FS> {
    /// Create a new workspace without link formatting (legacy mode).
    /// Links will be written as relative paths.
    pub fn new(fs: FS) -> Self {
        Self {
            fs,
            root_path: None,
            link_format: LinkFormat::PlainRelative,
        }
    }

    /// Create a new workspace with link formatting enabled.
    ///
    /// # Arguments
    /// * `fs` - The filesystem to use
    /// * `root_path` - The workspace root directory (for computing canonical paths)
    /// * `link_format` - How to format part_of and contents links
    pub fn with_link_format(fs: FS, root_path: PathBuf, link_format: LinkFormat) -> Self {
        Self {
            fs,
            root_path: Some(root_path),
            link_format,
        }
    }

    /// Get a reference to the underlying filesystem
    pub fn fs_ref(&self) -> &FS {
        &self.fs
    }

    /// Get the canonical path (workspace-relative) for a filesystem path.
    /// Returns the path as-is if no root_path is configured.
    ///
    /// The canonical path:
    /// - Has no leading `/` or `./`
    /// - Uses forward slashes
    /// - Is relative to the workspace root
    fn get_canonical_path(&self, path: &Path) -> String {
        let raw = if let Some(ref root) = self.root_path {
            // Strip the root path prefix to get workspace-relative path
            path.strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/") // Normalize to forward slashes
        } else {
            path.to_string_lossy().replace('\\', "/")
        };

        // Normalize: strip leading "./" or "/" to get clean canonical path
        raw.trim_start_matches("./")
            .trim_start_matches('/')
            .to_string()
    }

    /// Resolve a title for a canonical path by reading the file's frontmatter.
    /// Falls back to a formatted filename if the file can't be read.
    async fn resolve_title(&self, canonical_path: &str) -> String {
        if let Some(ref root) = self.root_path {
            let full_path = root.join(canonical_path);
            if let Ok(content) = self.fs.read_to_string(&full_path).await {
                // Try to extract title from frontmatter
                if let Some(title) = Self::extract_title_from_content(&content) {
                    return title;
                }
            }
        }
        // Fallback: convert filename to title
        link_parser::path_to_title(canonical_path)
    }

    /// Extract title from file content's frontmatter
    fn extract_title_from_content(content: &str) -> Option<String> {
        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            return None;
        }
        let rest = &content[4..];
        let end_idx = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n"))?;
        let frontmatter_str = &rest[..end_idx];

        // Simple extraction - look for "title: " line
        for line in frontmatter_str.lines() {
            let trimmed = line.trim();
            if let Some(title) = trimmed.strip_prefix("title:") {
                let title = title.trim().trim_matches('"').trim_matches('\'');
                if !title.is_empty() {
                    return Some(title.to_string());
                }
            }
        }
        None
    }

    /// Format a link for frontmatter based on configured link format.
    ///
    /// # Arguments
    /// * `target_canonical` - The canonical path of the target file
    /// * `from_canonical` - The canonical path of the file containing the link
    #[allow(dead_code)]
    async fn format_link(&self, target_canonical: &str, from_canonical: &str) -> String {
        let title = self.resolve_title(target_canonical).await;
        link_parser::format_link_with_format(
            target_canonical,
            &title,
            self.link_format,
            from_canonical,
        )
    }

    /// Format a link synchronously (when title is already known).
    fn format_link_sync(
        &self,
        target_canonical: &str,
        title: &str,
        from_canonical: &str,
    ) -> String {
        link_parser::format_link_with_format(
            target_canonical,
            title,
            self.link_format,
            from_canonical,
        )
    }

    /// Parse a markdown file and extract index frontmatter
    pub async fn parse_index(&self, path: &Path) -> Result<IndexFile> {
        let content = self
            .fs
            .read_to_string(path)
            .await
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

        let frontmatter: IndexFrontmatter =
            serde_yaml::from_str(frontmatter_str).map_err(|e| DiaryxError::YamlParse {
                path: path.to_path_buf(),
                message: e.to_string(),
            })?;

        Ok(IndexFile {
            path: path.to_path_buf(),
            frontmatter,
            body: body.to_string(),
            link_format_hint: None,
        })
    }

    /// Parse a markdown file and extract index frontmatter, with a link format hint.
    ///
    /// This variant of `parse_index` allows setting the `link_format_hint` field
    /// which affects how ambiguous paths (like `Folder/file.md`) are resolved.
    /// When `link_format` is `Some(PlainCanonical)`, ambiguous paths are resolved
    /// as workspace-root paths instead of relative paths.
    pub async fn parse_index_with_hint(
        &self,
        path: &Path,
        link_format: Option<LinkFormat>,
    ) -> Result<IndexFile> {
        let mut index = self.parse_index(path).await?;
        index.link_format_hint = link_format;
        Ok(index)
    }

    /// Check if a file is an index file (has contents property)
    pub async fn is_index_file(&self, path: &Path) -> bool {
        if path.extension().is_none_or(|ext| ext != "md") {
            return false;
        }

        self.parse_index(path)
            .await
            .map(|idx| idx.frontmatter.is_index())
            .unwrap_or(false)
    }

    /// Check if a file is a root index (has contents but no part_of)
    pub async fn is_root_index(&self, path: &Path) -> bool {
        self.parse_index(path)
            .await
            .map(|idx| idx.frontmatter.is_root())
            .unwrap_or(false)
    }

    /// Find a root index in the given directory
    pub async fn find_root_index_in_dir(&self, dir: &Path) -> Result<Option<PathBuf>> {
        let md_files = self
            .fs
            .list_md_files(dir)
            .await
            .map_err(|e| DiaryxError::FileRead {
                path: dir.to_path_buf(),
                source: e,
            })?;

        for file in md_files {
            if self.is_root_index(&file).await {
                return Ok(Some(file));
            }
        }

        Ok(None)
    }

    /// Find any index file in the given directory (has `contents` property)
    /// Prefers root indexes over non-root indexes
    pub async fn find_any_index_in_dir(&self, dir: &Path) -> Result<Option<PathBuf>> {
        let md_files = self
            .fs
            .list_md_files(dir)
            .await
            .map_err(|e| DiaryxError::FileRead {
                path: dir.to_path_buf(),
                source: e,
            })?;

        let mut found_index: Option<PathBuf> = None;

        for file in md_files {
            if let Ok(index) = self.parse_index(&file).await
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
    pub async fn collect_workspace_files(&self, index_path: &Path) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        let mut visited = HashSet::new();

        // Get link format from workspace config for proper path resolution
        let link_format = self
            .get_workspace_config(index_path)
            .await
            .map(|c| c.link_format)
            .ok();

        // Get the workspace root directory (parent of root index file)
        let workspace_root = index_path.parent().unwrap_or(Path::new(".")).to_path_buf();

        self.collect_workspace_files_recursive(
            index_path,
            &mut files,
            &mut visited,
            link_format,
            &workspace_root,
        )
        .await?;
        files.sort();
        Ok(files)
    }

    /// Recursive helper for collecting workspace files
    async fn collect_workspace_files_recursive(
        &self,
        path: &Path,
        files: &mut Vec<PathBuf>,
        visited: &mut HashSet<PathBuf>,
        link_format: Option<LinkFormat>,
        workspace_root: &Path,
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
        if let Ok(index) = self.parse_index_with_hint(path, link_format).await
            && index.frontmatter.is_index()
        {
            for child_path_str in index.frontmatter.contents_list() {
                let child_path = index.resolve_path(child_path_str);

                // Make path absolute if needed by joining with workspace root
                let absolute_child_path = if child_path.is_absolute() {
                    child_path
                } else {
                    workspace_root.join(&child_path)
                };

                // Only include if the file exists
                if self.fs.exists(&absolute_child_path).await {
                    Box::pin(self.collect_workspace_files_recursive(
                        &absolute_child_path,
                        files,
                        visited,
                        link_format,
                        workspace_root,
                    ))
                    .await?;
                }
            }
        }

        Ok(())
    }

    /// Detect the workspace root from the current directory
    /// Searches current directory for a root index file
    pub async fn detect_workspace(&self, start_dir: &Path) -> Result<Option<PathBuf>> {
        // Look for root index in start directory
        if let Some(root) = self.find_root_index_in_dir(start_dir).await? {
            return Ok(Some(root));
        }

        Ok(None)
    }

    // ==================== Workspace Config Methods ====================

    /// Get the workspace configuration from the root index file's frontmatter.
    ///
    /// Reads `link_format` and other workspace-level settings from the root index.
    /// Returns default values if the properties aren't present.
    pub async fn get_workspace_config(&self, root_index_path: &Path) -> Result<WorkspaceConfig> {
        let index = self.parse_index(root_index_path).await?;

        // Extract link_format from extra fields
        let link_format = index
            .frontmatter
            .extra
            .get("link_format")
            .and_then(|v| v.as_str())
            .and_then(|s| match s {
                "markdown_root" => Some(LinkFormat::MarkdownRoot),
                "markdown_relative" => Some(LinkFormat::MarkdownRelative),
                "plain_relative" => Some(LinkFormat::PlainRelative),
                "plain_canonical" => Some(LinkFormat::PlainCanonical),
                _ => None,
            })
            .unwrap_or_default();

        // Extract daily_entry_folder from extra fields
        let daily_entry_folder = index
            .frontmatter
            .extra
            .get("daily_entry_folder")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(WorkspaceConfig {
            link_format,
            daily_entry_folder,
        })
    }

    /// Set a workspace configuration field in the root index file's frontmatter.
    ///
    /// # Arguments
    /// * `root_index_path` - Path to the root index file
    /// * `field` - Field name to set (e.g., "link_format", "daily_entry_folder")
    /// * `value` - Value to set (will be stored as a string)
    pub async fn set_workspace_config_field(
        &self,
        root_index_path: &Path,
        field: &str,
        value: &str,
    ) -> Result<()> {
        self.set_frontmatter_property(root_index_path, field, Value::String(value.to_string()))
            .await
    }

    /// Get the link format configuration from a workspace root index.
    pub async fn get_link_format(&self, root_index_path: &Path) -> Result<LinkFormat> {
        let config = self.get_workspace_config(root_index_path).await?;
        Ok(config.link_format)
    }

    /// Set the link format configuration in a workspace root index.
    pub async fn set_link_format(&self, root_index_path: &Path, format: LinkFormat) -> Result<()> {
        let format_str = match format {
            LinkFormat::MarkdownRoot => "markdown_root",
            LinkFormat::MarkdownRelative => "markdown_relative",
            LinkFormat::PlainRelative => "plain_relative",
            LinkFormat::PlainCanonical => "plain_canonical",
        };
        self.set_workspace_config_field(root_index_path, "link_format", format_str)
            .await
    }

    /// Resolve workspace: check current dir, then fall back to config default
    pub async fn resolve_workspace(&self, current_dir: &Path, config: &Config) -> Result<PathBuf> {
        // First, try to detect workspace in current directory
        if let Some(root) = self.detect_workspace(current_dir).await? {
            return Ok(root);
        }

        // Fall back to config's default_workspace and look for root index there
        if let Some(root) = self
            .find_root_index_in_dir(&config.default_workspace)
            .await?
        {
            return Ok(root);
        }

        // If no root index exists in default_workspace, return the expected README.md path
        // (it may need to be created)
        Ok(config.default_workspace.join("README.md"))
    }

    /// Initialize a new workspace with a root index file
    pub async fn init_workspace(
        &self,
        dir: &Path,
        title: Option<&str>,
        description: Option<&str>,
    ) -> Result<PathBuf> {
        // Check if ANY root index already exists in this directory
        // (not just README.md - could be index.md or any other .md file)
        if let Ok(Some(existing_root)) = self.find_root_index_in_dir(dir).await {
            return Err(DiaryxError::WorkspaceAlreadyExists(existing_root));
        }

        let readme_path = dir.join("README.md");

        // Create directory if needed
        self.fs.create_dir_all(dir).await?;

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
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: readme_path.clone(),
                source: e,
            })?;

        Ok(readme_path)
    }

    /// Build a tree structure from the workspace hierarchy
    pub async fn build_tree(&self, root_path: &Path) -> Result<TreeNode> {
        self.build_tree_with_depth(root_path, None, &mut HashSet::new())
            .await
    }

    /// Build a tree structure with depth limit and cycle detection
    /// `max_depth` of None means unlimited, Some(0) means just the root node
    pub async fn build_tree_with_depth(
        &self,
        root_path: &Path,
        max_depth: Option<usize>,
        visited: &mut HashSet<PathBuf>,
    ) -> Result<TreeNode> {
        // Get link format from workspace config for proper path resolution
        let link_format = self
            .get_workspace_config(root_path)
            .await
            .map(|c| c.link_format)
            .ok();

        // Get the workspace root directory (parent of root index file)
        let workspace_root = root_path.parent().unwrap_or(Path::new(".")).to_path_buf();

        self.build_tree_with_depth_and_format(
            root_path,
            max_depth,
            visited,
            link_format,
            &workspace_root,
        )
        .await
    }

    /// Build a tree structure with depth limit, cycle detection, and explicit link format.
    ///
    /// This is the internal implementation that handles tree building.
    /// Use `build_tree_with_depth` for public API which auto-detects the link format.
    async fn build_tree_with_depth_and_format(
        &self,
        root_path: &Path,
        max_depth: Option<usize>,
        visited: &mut HashSet<PathBuf>,
        link_format: Option<LinkFormat>,
        workspace_root: &Path,
    ) -> Result<TreeNode> {
        let index = self.parse_index_with_hint(root_path, link_format).await?;

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

                // Make path absolute if needed by joining with workspace root
                let absolute_child_path = if child_path.is_absolute() {
                    child_path.clone()
                } else {
                    workspace_root.join(&child_path)
                };

                // Only include if the file exists
                if self.fs.exists(&absolute_child_path).await {
                    match Box::pin(self.build_tree_with_depth_and_format(
                        &absolute_child_path,
                        next_depth,
                        visited,
                        link_format,
                        workspace_root,
                    ))
                    .await
                    {
                        Ok(child_node) => children.push(child_node),
                        Err(_) => {
                            // If we can't parse a child, include it as a leaf with error indication
                            children.push(TreeNode {
                                name: format!("{} (error)", child_path_str),
                                description: None,
                                path: absolute_child_path,
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
    pub async fn build_filesystem_tree(
        &self,
        root_dir: &Path,
        show_hidden: bool,
    ) -> Result<TreeNode> {
        self.build_filesystem_tree_with_depth(root_dir, show_hidden, None)
            .await
    }

    /// Build a filesystem tree with optional depth limiting for lazy loading
    pub async fn build_filesystem_tree_with_depth(
        &self,
        root_dir: &Path,
        show_hidden: bool,
        max_depth: Option<usize>,
    ) -> Result<TreeNode> {
        self.build_filesystem_tree_recursive(root_dir, show_hidden, max_depth)
            .await
    }

    async fn build_filesystem_tree_recursive(
        &self,
        dir: &Path,
        show_hidden: bool,
        max_depth: Option<usize>,
    ) -> Result<TreeNode> {
        // Get directory name for display
        let dir_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| dir.to_string_lossy().to_string());

        // Try to find an index file in this directory to get title/description
        let (name, description, index_path) =
            if let Ok(Some(index)) = self.find_any_index_in_dir(dir).await {
                if let Ok(parsed) = self.parse_index(&index).await {
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

        // Check if we've hit depth limit
        let at_depth_limit = max_depth.map(|d| d == 0).unwrap_or(false);

        // List all entries in this directory
        let mut children = Vec::new();
        if let Ok(entries) = self.fs.list_files(dir).await {
            let mut entries: Vec<_> = entries.into_iter().collect();
            entries.sort(); // Sort alphabetically

            // Filter out hidden files first to get accurate count
            let entries: Vec<_> = entries
                .into_iter()
                .filter(|entry| {
                    let file_name = entry
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    show_hidden || !file_name.starts_with('.')
                })
                .collect();

            // If at depth limit, show truncation indicator
            if at_depth_limit && !entries.is_empty() {
                children.push(TreeNode {
                    name: format!("... ({} more)", entries.len()),
                    description: None,
                    path: node_path.clone(),
                    children: Vec::new(),
                });
            } else {
                let next_depth = max_depth.map(|d| d.saturating_sub(1));

                for entry in entries {
                    let file_name = entry
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();

                    if self.fs.is_dir(&entry).await {
                        // Recurse into subdirectory with decremented depth
                        if let Ok(child_tree) = Box::pin(self.build_filesystem_tree_recursive(
                            &entry,
                            show_hidden,
                            next_depth,
                        ))
                        .await
                        {
                            children.push(child_tree);
                        }
                    } else {
                        // It's a file - skip index files (already represented by parent dir)
                        if self.is_index_file(&entry).await {
                            continue;
                        }

                        // Get title from frontmatter if it's a markdown file
                        let (file_title, file_desc) =
                            if entry.extension().is_some_and(|e| e == "md") {
                                if let Ok(parsed) = self.parse_index(&entry).await {
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
    pub async fn workspace_info(&self, root_path: &Path) -> Result<String> {
        self.workspace_info_with_depth(root_path, None).await
    }

    /// Get workspace info as formatted string with depth limit
    /// `max_depth` of None means unlimited
    pub async fn workspace_info_with_depth(
        &self,
        root_path: &Path,
        max_depth: Option<usize>,
    ) -> Result<String> {
        let mut visited = HashSet::new();
        let tree = self
            .build_tree_with_depth(root_path, max_depth, &mut visited)
            .await?;
        Ok(self.format_tree(&tree, "").trim_end().to_string())
    }

    // ==================== Frontmatter Helper Methods ====================
    // These are internal helpers for manipulating frontmatter in workspace operations

    /// Get a frontmatter property from a file
    async fn get_frontmatter_property(&self, path: &Path, key: &str) -> Result<Option<Value>> {
        let content = match self.fs.read_to_string(path).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(DiaryxError::FileRead {
                    path: path.to_path_buf(),
                    source: e,
                });
            }
        };

        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            return Ok(None);
        }

        let rest = &content[4..];
        let end_idx = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n"));

        if let Some(idx) = end_idx {
            let frontmatter_str = &rest[..idx];
            let frontmatter: indexmap::IndexMap<String, Value> =
                serde_yaml::from_str(frontmatter_str)?;
            Ok(frontmatter.get(key).cloned())
        } else {
            Ok(None)
        }
    }

    /// Set a frontmatter property in a file
    pub async fn set_frontmatter_property(
        &self,
        path: &Path,
        key: &str,
        value: Value,
    ) -> Result<()> {
        let content = match self.fs.read_to_string(path).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Create new file with just this property
                let mut frontmatter = indexmap::IndexMap::new();
                frontmatter.insert(key.to_string(), value);
                let yaml_str = serde_yaml::to_string(&frontmatter)?;
                let new_content = format!("---\n{}---\n", yaml_str);
                return self.fs.write_file(path, &new_content).await.map_err(|e| {
                    DiaryxError::FileWrite {
                        path: path.to_path_buf(),
                        source: e,
                    }
                });
            }
            Err(e) => {
                return Err(DiaryxError::FileRead {
                    path: path.to_path_buf(),
                    source: e,
                });
            }
        };

        let (mut frontmatter, body) = if content.starts_with("---\n")
            || content.starts_with("---\r\n")
        {
            let rest = &content[4..];
            if let Some(idx) = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n")) {
                let frontmatter_str = &rest[..idx];
                let body = &rest[idx + 5..];
                let fm: indexmap::IndexMap<String, Value> = serde_yaml::from_str(frontmatter_str)?;
                (fm, body.to_string())
            } else {
                (indexmap::IndexMap::new(), content)
            }
        } else {
            (indexmap::IndexMap::new(), content)
        };

        frontmatter.insert(key.to_string(), value);
        let yaml_str = serde_yaml::to_string(&frontmatter)?;
        let new_content = format!("---\n{}---\n{}", yaml_str, body);

        self.fs
            .write_file(path, &new_content)
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: path.to_path_buf(),
                source: e,
            })
    }

    /// Remove a frontmatter property from a file
    async fn remove_frontmatter_property(&self, path: &Path, key: &str) -> Result<()> {
        let content = match self.fs.read_to_string(path).await {
            Ok(c) => c,
            Err(_) => return Ok(()), // File doesn't exist, nothing to remove
        };

        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            return Ok(()); // No frontmatter
        }

        let rest = &content[4..];
        let end_idx = match rest.find("\n---\n").or_else(|| rest.find("\n---\r\n")) {
            Some(idx) => idx,
            None => return Ok(()), // Malformed frontmatter
        };

        let frontmatter_str = &rest[..end_idx];
        let body = &rest[end_idx + 5..];

        let mut frontmatter: indexmap::IndexMap<String, Value> =
            serde_yaml::from_str(frontmatter_str)?;
        frontmatter.shift_remove(key);

        let yaml_str = serde_yaml::to_string(&frontmatter)?;
        let new_content = format!("---\n{}---\n{}", yaml_str, body);

        self.fs
            .write_file(path, &new_content)
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: path.to_path_buf(),
                source: e,
            })
    }

    /// Normalize a path string by stripping leading "./" prefix
    fn normalize_contents_path(path: &str) -> &str {
        path.strip_prefix("./").unwrap_or(path)
    }

    /// Add an entry to an index's contents list (using raw entry string).
    /// For formatted links, use `add_to_index_contents_canonical` instead.
    pub async fn add_to_index_contents(&self, index_path: &Path, entry: &str) -> Result<bool> {
        // Normalize the entry path (strip leading ./)
        let normalized_entry = Self::normalize_contents_path(entry);

        // Parse the new entry to get its canonical path for comparison
        let parsed_entry = link_parser::parse_link(normalized_entry);
        let entry_canonical = link_parser::to_canonical(&parsed_entry, index_path);

        match self.get_frontmatter_property(index_path, "contents").await {
            Ok(Some(Value::Sequence(mut items))) => {
                // Check if entry already exists (comparing canonical paths)
                let already_exists = items.iter().any(|item| {
                    if let Some(s) = item.as_str() {
                        let parsed = link_parser::parse_link(s);
                        let existing_canonical = link_parser::to_canonical(&parsed, index_path);
                        existing_canonical == entry_canonical
                    } else {
                        false
                    }
                });

                if !already_exists {
                    items.push(Value::String(normalized_entry.to_string()));
                    // Sort contents for consistent ordering
                    items.sort_by(|a, b| {
                        let a_str = a.as_str().unwrap_or("");
                        let b_str = b.as_str().unwrap_or("");
                        a_str.cmp(b_str)
                    });
                    self.set_frontmatter_property(index_path, "contents", Value::Sequence(items))
                        .await?;
                    return Ok(true);
                }
                Ok(false)
            }
            Ok(None) => {
                // Create contents with just this entry (normalized)
                let items = vec![Value::String(normalized_entry.to_string())];
                self.set_frontmatter_property(index_path, "contents", Value::Sequence(items))
                    .await?;
                Ok(true)
            }
            _ => {
                // Contents exists but isn't a sequence, or error reading - skip
                Ok(false)
            }
        }
    }

    /// Add an entry to an index's contents list using a canonical path.
    /// This formats the link according to the configured link_format.
    pub async fn add_to_index_contents_canonical(
        &self,
        index_path: &Path,
        entry_canonical: &str,
        title: &str,
    ) -> Result<bool> {
        let index_canonical = self.get_canonical_path(index_path);

        // Format the entry based on link format
        let formatted_entry = if self.root_path.is_some() {
            self.format_link_sync(entry_canonical, title, &index_canonical)
        } else {
            // Fallback: just use the canonical path
            entry_canonical.to_string()
        };

        // Extract canonical path from existing entries for comparison
        let entry_for_comparison = entry_canonical;

        match self.get_frontmatter_property(index_path, "contents").await {
            Ok(Some(Value::Sequence(mut items))) => {
                // Check if entry already exists (comparing canonical paths)
                let already_exists = items.iter().any(|item| {
                    if let Some(s) = item.as_str() {
                        // Parse the existing item to get its canonical path
                        let parsed = link_parser::parse_link(s);
                        let existing_canonical =
                            link_parser::to_canonical(&parsed, Path::new(&index_canonical));
                        existing_canonical == entry_for_comparison
                    } else {
                        false
                    }
                });

                if !already_exists {
                    items.push(Value::String(formatted_entry));
                    // Sort contents for consistent ordering
                    items.sort_by(|a, b| {
                        let a_str = a.as_str().unwrap_or("");
                        let b_str = b.as_str().unwrap_or("");
                        a_str.cmp(b_str)
                    });
                    self.set_frontmatter_property(index_path, "contents", Value::Sequence(items))
                        .await?;
                    return Ok(true);
                }
                Ok(false)
            }
            Ok(None) => {
                // Create contents with just this entry
                let items = vec![Value::String(formatted_entry)];
                self.set_frontmatter_property(index_path, "contents", Value::Sequence(items))
                    .await?;
                Ok(true)
            }
            _ => {
                // Contents exists but isn't a sequence, or error reading - skip
                Ok(false)
            }
        }
    }

    /// Remove an entry from an index's contents list.
    ///
    /// The `entry` can be:
    /// - A plain filename: `new-entry.md`
    /// - A relative path: `subdir/file.md`
    /// - A markdown link: `[Title](/path/to/file.md)`
    ///
    /// This properly handles markdown links in the contents list by comparing
    /// canonical paths.
    async fn remove_from_index_contents(&self, index_path: &Path, entry: &str) -> Result<bool> {
        // Parse the entry to remove to get its canonical form
        let parsed_entry = link_parser::parse_link(entry);
        let entry_canonical = link_parser::to_canonical(&parsed_entry, index_path);

        match self.get_frontmatter_property(index_path, "contents").await {
            Ok(Some(Value::Sequence(mut items))) => {
                let before_len = items.len();
                // Remove entries that match when comparing canonical paths
                items.retain(|item| {
                    if let Some(s) = item.as_str() {
                        // Parse the existing item to get its canonical path
                        let parsed = link_parser::parse_link(s);
                        let existing_canonical = link_parser::to_canonical(&parsed, index_path);
                        existing_canonical != entry_canonical
                    } else {
                        true
                    }
                });

                if items.len() != before_len {
                    // Sort contents for consistent ordering
                    items.sort_by(|a, b| {
                        let a_str = a.as_str().unwrap_or("");
                        let b_str = b.as_str().unwrap_or("");
                        a_str.cmp(b_str)
                    });
                    self.set_frontmatter_property(index_path, "contents", Value::Sequence(items))
                        .await?;
                    return Ok(true);
                }
                Ok(false)
            }
            Ok(None) | Ok(Some(_)) => {
                // No contents property or not a sequence - nothing to remove
                Ok(false)
            }
            Err(_) => {
                // Error reading - skip
                Ok(false)
            }
        }
    }

    // ==================== Entry Management Methods ====================

    /// Attach an entry to a parent index, creating bidirectional links.
    ///
    /// This method:
    /// - Adds the entry to the parent index's `contents` list (relative to parent's directory)
    /// - Sets the entry's `part_of` property to point to the parent index (relative to entry)
    ///
    /// Both paths must exist.
    pub async fn attach_entry_to_parent(
        &self,
        entry_path: &Path,
        parent_index_path: &Path,
    ) -> Result<()> {
        use crate::path_utils::{
            relative_path_from_dir_to_target, relative_path_from_file_to_target,
        };

        // Validate both paths exist
        if !self.fs.exists(entry_path).await {
            return Err(DiaryxError::FileRead {
                path: entry_path.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "Entry does not exist"),
            });
        }
        if !self.fs.exists(parent_index_path).await {
            return Err(DiaryxError::FileRead {
                path: parent_index_path.to_path_buf(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Parent index does not exist",
                ),
            });
        }

        // Calculate relative path from parent's directory to entry
        let parent_dir = parent_index_path.parent().unwrap_or_else(|| Path::new(""));
        let child_rel = relative_path_from_dir_to_target(parent_dir, entry_path);

        // Add entry to parent's contents
        self.add_to_index_contents(parent_index_path, &child_rel)
            .await?;

        // Calculate relative path from entry to parent index
        let parent_rel = relative_path_from_file_to_target(entry_path, parent_index_path);

        // Set entry's part_of
        self.set_frontmatter_property(entry_path, "part_of", Value::String(parent_rel))
            .await?;

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
    pub async fn move_entry(&self, from_path: &Path, to_path: &Path) -> Result<()> {
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
        let _new_file_name = to_path
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
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: to_path.to_path_buf(),
                source: e,
            })?;

        // Remove from old parent's contents (if old parent has an index)
        if let Ok(Some(old_index_path)) = self.find_any_index_in_dir(old_parent).await {
            let _ = self
                .remove_from_index_contents(&old_index_path, &old_file_name)
                .await;
        }

        // Add to new parent's contents and update part_of (if new parent has an index)
        if let Ok(Some(new_index_path)) = self.find_any_index_in_dir(new_parent).await {
            // Add with proper formatting
            let to_path_canonical = self.get_canonical_path(to_path);
            let title = self.resolve_title(&to_path_canonical).await;
            let _ = self
                .add_to_index_contents_canonical(&new_index_path, &to_path_canonical, &title)
                .await;

            // Update moved entry's part_of with proper formatting
            let part_of_value = if self.root_path.is_some() {
                let new_index_canonical = self.get_canonical_path(&new_index_path);
                let parent_title = self.resolve_title(&new_index_canonical).await;
                self.format_link_sync(&new_index_canonical, &parent_title, &to_path_canonical)
            } else {
                relative_path_from_file_to_target(to_path, &new_index_path)
            };
            let _ = self
                .set_frontmatter_property(to_path, "part_of", Value::String(part_of_value))
                .await;
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
    pub async fn delete_entry(&self, path: &Path) -> Result<()> {
        // Check if this is an index file with children
        if let Ok(index) = self.parse_index(path).await {
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
        if let Ok(Some(index_path)) = self.find_any_index_in_dir(parent).await {
            let _ = self
                .remove_from_index_contents(&index_path, &file_name)
                .await;
        }

        // Delete the file
        self.fs
            .delete_file(path)
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: path.to_path_buf(),
                source: e,
            })?;

        Ok(())
    }

    /// Generate a unique filename for a new child entry in the given directory.
    ///
    /// Returns filenames like "new-entry.md", "new-entry-1.md", "new-entry-2.md", etc.
    pub async fn generate_unique_child_name(&self, parent_dir: &Path) -> String {
        let base_name = "new-entry";
        let mut candidate = format!("{}.md", base_name);
        let mut counter = 1;

        while self.fs.exists(&parent_dir.join(&candidate)).await {
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
    pub async fn create_child_entry(
        &self,
        parent_index_path: &Path,
        title: Option<&str>,
    ) -> Result<PathBuf> {
        use crate::path_utils::relative_path_from_file_to_target;

        // Parse parent - if it's a leaf (not an index), convert it to an index first
        let effective_parent = if let Ok(parent_index) = self.parse_index(parent_index_path).await {
            if parent_index.frontmatter.is_index() {
                parent_index_path.to_path_buf()
            } else {
                // Parent is a leaf file - convert to index first
                self.convert_to_index(parent_index_path).await?
            }
        } else {
            // Parent doesn't exist or couldn't be parsed - try to convert anyway
            // (convert_to_index will fail with a proper error if file doesn't exist)
            return Err(DiaryxError::FileRead {
                path: parent_index_path.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "Parent file not found"),
            });
        };

        // Determine parent directory (from effective parent, which may have moved)
        let parent_dir = effective_parent
            .parent()
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: effective_parent.clone(),
                message: "Parent index has no directory".to_string(),
            })?;

        // Generate unique filename
        let child_filename = self.generate_unique_child_name(parent_dir).await;
        let child_path = parent_dir.join(&child_filename);

        // Format part_of link based on configured format
        let display_title = title.unwrap_or("New Entry");
        let part_of_value = if self.root_path.is_some() {
            // Use link formatting - resolve parent's title for the link display
            let child_canonical = self.get_canonical_path(&child_path);
            let parent_canonical = self.get_canonical_path(&effective_parent);
            let parent_title = self.resolve_title(&parent_canonical).await;
            self.format_link_sync(&parent_canonical, &parent_title, &child_canonical)
        } else {
            // Fallback: use relative path
            relative_path_from_file_to_target(&child_path, &effective_parent)
        };

        // Create child file with frontmatter
        let content = format!(
            "---\ntitle: \"{}\"\npart_of: \"{}\"\n---\n\n# {}\n\n",
            display_title, part_of_value, display_title
        );

        self.fs
            .create_new(&child_path, &content)
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: child_path.clone(),
                source: e,
            })?;

        // Add to parent's contents (using formatted link)
        let child_canonical = self.get_canonical_path(&child_path);
        self.add_to_index_contents_canonical(&effective_parent, &child_canonical, display_title)
            .await?;

        Ok(child_path)
    }

    /// Rename an entry file by giving it a new filename.
    ///
    /// This method handles both leaf files and index files:
    /// - Leaf files: renames the file directly and updates parent `contents`
    /// - Index files: renames the containing directory AND the file itself, updates grandparent `contents`
    ///
    /// Returns the new path to the renamed file.
    pub async fn rename_entry(&self, path: &Path, new_filename: &str) -> Result<PathBuf> {
        let is_index = self.is_index_file(path).await;

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
            if self.fs.exists(&new_dir_path).await {
                return Err(DiaryxError::InvalidPath {
                    path: new_dir_path,
                    message: "Target directory already exists".to_string(),
                });
            }

            // Create new directory
            self.fs.create_dir_all(&new_dir_path).await?;

            // Move all files from old directory to new directory and track children
            let mut children_paths: Vec<PathBuf> = Vec::new();
            if let Ok(files) = self.fs.list_files(current_dir).await {
                for file in files {
                    let file_name = file.file_name().unwrap_or_default();
                    let new_path = new_dir_path.join(file_name);

                    // If this is the index file itself, use the new filename
                    if file == path {
                        self.fs.move_file(&file, &new_file_path).await?;
                    } else {
                        self.fs.move_file(&file, &new_path).await?;
                        children_paths.push(new_path);
                    }
                }
            }

            // Update all children's part_of to point to new index
            // Update children's part_of to point to renamed parent
            let new_file_canonical = self.get_canonical_path(&new_file_path);
            let new_file_title = self.resolve_title(&new_file_canonical).await;
            for child_path in &children_paths {
                let part_of_value = if self.root_path.is_some() {
                    let child_canonical = self.get_canonical_path(child_path);
                    self.format_link_sync(&new_file_canonical, &new_file_title, &child_canonical)
                } else {
                    use crate::path_utils::relative_path_from_file_to_target;
                    relative_path_from_file_to_target(child_path, &new_file_path)
                };
                let _ = self
                    .set_frontmatter_property(child_path, "part_of", Value::String(part_of_value))
                    .await;
            }

            // Update grandparent's contents if it exists
            if let Ok(Some(grandparent_index)) = self.find_any_index_in_dir(parent_of_dir).await {
                let old_dir_name = current_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();

                // Remove old entry
                let old_rel = format!("{}/{}.md", old_dir_name, old_dir_name);
                let _ = self
                    .remove_from_index_contents(&grandparent_index, &old_rel)
                    .await;

                // Add new entry with proper formatting
                let _ = self
                    .add_to_index_contents_canonical(
                        &grandparent_index,
                        &new_file_canonical,
                        &new_file_title,
                    )
                    .await;
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
            if self.fs.exists(&new_path).await {
                return Err(DiaryxError::InvalidPath {
                    path: new_path,
                    message: "Target file already exists".to_string(),
                });
            }

            // Move the file
            self.fs.move_file(path, &new_path).await?;

            // Update parent's contents if it exists
            if let Ok(Some(parent_index)) = self.find_any_index_in_dir(parent).await {
                // Remove old entry
                let _ = self
                    .remove_from_index_contents(&parent_index, &old_filename)
                    .await;

                // Add new entry with proper formatting
                let new_path_canonical = self.get_canonical_path(&new_path);
                let title = self.resolve_title(&new_path_canonical).await;
                let _ = self
                    .add_to_index_contents_canonical(&parent_index, &new_path_canonical, &title)
                    .await;
            }

            Ok(new_path)
        }
    }

    /// Duplicate an entry, creating a copy with a unique name.
    ///
    /// This method:
    /// - For leaf files: copies the file with a "-copy" suffix (or "-copy-N" if exists)
    /// - For index files: copies the entire directory structure recursively
    /// - Updates the copy's `part_of` to point to the same parent
    /// - Adds the copy to the parent's `contents`
    ///
    /// Returns the path to the new duplicated entry.
    pub async fn duplicate_entry(&self, source_path: &Path) -> Result<PathBuf> {
        use crate::path_utils::relative_path_from_file_to_target;

        let is_index = self.is_index_file(source_path).await;

        if is_index {
            // For index files, we duplicate the entire directory
            let source_dir = source_path
                .parent()
                .ok_or_else(|| DiaryxError::InvalidPath {
                    path: source_path.to_path_buf(),
                    message: "Index file has no parent directory".to_string(),
                })?;

            let parent_of_dir = source_dir
                .parent()
                .ok_or_else(|| DiaryxError::InvalidPath {
                    path: source_path.to_path_buf(),
                    message: "Directory has no parent".to_string(),
                })?;

            // Get source directory name and generate unique copy name
            let source_dir_name =
                source_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(|| DiaryxError::InvalidPath {
                        path: source_path.to_path_buf(),
                        message: "Invalid directory name".to_string(),
                    })?;

            let new_dir_name = self
                .generate_unique_copy_name(parent_of_dir, source_dir_name, false)
                .await;
            let new_dir_path = parent_of_dir.join(&new_dir_name);
            let new_index_path = new_dir_path.join(format!("{}.md", new_dir_name));

            // Create new directory
            self.fs.create_dir_all(&new_dir_path).await?;

            // Copy all files from source directory to new directory
            if let Ok(files) = self.fs.list_files(source_dir).await {
                for file in files {
                    let file_name = file
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or_default();

                    // For the index file, use the new directory name
                    let new_path = if file == source_path {
                        new_index_path.clone()
                    } else {
                        new_dir_path.join(file_name)
                    };

                    // Copy file content
                    let content =
                        self.fs
                            .read_to_string(&file)
                            .await
                            .map_err(|e| DiaryxError::FileRead {
                                path: file.clone(),
                                source: e,
                            })?;
                    self.fs.write_file(&new_path, &content).await.map_err(|e| {
                        DiaryxError::FileWrite {
                            path: new_path.clone(),
                            source: e,
                        }
                    })?;

                    // Update part_of for child files to point to new index
                    if new_path != new_index_path {
                        let new_part_of =
                            relative_path_from_file_to_target(&new_path, &new_index_path);
                        let _ = self
                            .set_frontmatter_property(
                                &new_path,
                                "part_of",
                                Value::String(new_part_of),
                            )
                            .await;
                    }
                }
            }

            // Update the copied index's part_of to point to grandparent (same as source)
            if let Ok(Some(grandparent_index)) = self.find_any_index_in_dir(parent_of_dir).await {
                let new_part_of =
                    relative_path_from_file_to_target(&new_index_path, &grandparent_index);
                let _ = self
                    .set_frontmatter_property(
                        &new_index_path,
                        "part_of",
                        Value::String(new_part_of),
                    )
                    .await;

                // Add to grandparent's contents
                let rel_path = format!("{}/{}.md", new_dir_name, new_dir_name);
                let _ = self
                    .add_to_index_contents(&grandparent_index, &rel_path)
                    .await;
            }

            Ok(new_index_path)
        } else {
            // For leaf files, simple copy in same directory
            let parent = source_path
                .parent()
                .ok_or_else(|| DiaryxError::InvalidPath {
                    path: source_path.to_path_buf(),
                    message: "File has no parent directory".to_string(),
                })?;

            let source_filename = source_path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| DiaryxError::InvalidPath {
                    path: source_path.to_path_buf(),
                    message: "Invalid file name".to_string(),
                })?;

            // Generate unique copy name
            let new_filename = self
                .generate_unique_copy_name(parent, source_filename, true)
                .await;
            let new_path = parent.join(&new_filename);

            // Copy file content
            let content =
                self.fs
                    .read_to_string(source_path)
                    .await
                    .map_err(|e| DiaryxError::FileRead {
                        path: source_path.to_path_buf(),
                        source: e,
                    })?;
            self.fs
                .write_file(&new_path, &content)
                .await
                .map_err(|e| DiaryxError::FileWrite {
                    path: new_path.clone(),
                    source: e,
                })?;

            // Update parent's contents if it exists
            if let Ok(Some(parent_index)) = self.find_any_index_in_dir(parent).await {
                // Update part_of to point to parent
                let new_part_of = relative_path_from_file_to_target(&new_path, &parent_index);
                let _ = self
                    .set_frontmatter_property(&new_path, "part_of", Value::String(new_part_of))
                    .await;

                // Add to parent's contents
                let _ = self
                    .add_to_index_contents(&parent_index, &new_filename)
                    .await;
            }

            Ok(new_path)
        }
    }

    /// Generate a unique copy name for a file or directory.
    ///
    /// For files: "name.md" → "name-copy.md", "name-copy-2.md", etc.
    /// For directories: "name" → "name-copy", "name-copy-2", etc.
    async fn generate_unique_copy_name(
        &self,
        parent_dir: &Path,
        original_name: &str,
        is_file: bool,
    ) -> String {
        let (base_name, extension) = if is_file {
            // Strip .md extension for files
            let base = original_name.trim_end_matches(".md");
            (base.to_string(), ".md".to_string())
        } else {
            (original_name.to_string(), String::new())
        };

        // Try "name-copy" first
        let mut candidate = format!("{}-copy{}", base_name, extension);
        let mut counter = 2;

        while self.fs.exists(&parent_dir.join(&candidate)).await {
            candidate = format!("{}-copy-{}{}", base_name, counter, extension);
            counter += 1;
        }

        candidate
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
    pub async fn convert_to_index(&self, path: &Path) -> Result<PathBuf> {
        // Check if already an index
        if self.is_index_file(path).await {
            return Err(DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "File is already an index".to_string(),
            });
        }

        let parent = path.parent().ok_or_else(|| DiaryxError::InvalidPath {
            path: path.to_path_buf(),
            message: "File has no parent directory".to_string(),
        })?;

        let file_stem =
            path.file_stem()
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
        self.fs.create_dir_all(&new_dir).await?;

        // Move file into directory
        self.fs.move_file(path, &new_path).await?;

        // Add contents property
        self.set_frontmatter_property(&new_path, "contents", Value::Sequence(vec![]))
            .await?;

        // Update part_of path since file moved one level deeper
        if let Ok(Some(Value::String(old_part_of))) =
            self.get_frontmatter_property(&new_path, "part_of").await
        {
            use crate::path_utils::{normalize_path, relative_path_from_file_to_target};

            // Parse the markdown link to get the path and type
            let parsed = link_parser::parse_link(&old_part_of);

            // Resolve target path based on path type
            let target_path = match parsed.path_type {
                link_parser::PathType::WorkspaceRoot => {
                    // Workspace-root path: canonical is already workspace-relative
                    if let Some(ref root) = self.root_path {
                        normalize_path(&root.join(&parsed.path))
                    } else {
                        // No workspace root: path is already workspace-relative
                        PathBuf::from(&parsed.path)
                    }
                }
                link_parser::PathType::Relative | link_parser::PathType::Ambiguous => {
                    // Relative path: resolve against old file's parent directory
                    normalize_path(&parent.join(&parsed.path))
                }
            };

            // Format the new part_of link
            let new_part_of = if self.root_path.is_some() {
                // Use markdown link format
                let target_canonical = self.get_canonical_path(&target_path);
                let new_path_canonical = self.get_canonical_path(&new_path);
                let target_title = self.resolve_title(&target_canonical).await;
                self.format_link_sync(&target_canonical, &target_title, &new_path_canonical)
            } else {
                // Fallback: plain relative path
                relative_path_from_file_to_target(&new_path, &target_path)
            };

            let _ = self
                .set_frontmatter_property(&new_path, "part_of", Value::String(new_part_of))
                .await;
        }

        // Update parent's contents to point to new location
        if let Ok(Some(parent_index)) = self.find_any_index_in_dir(parent).await {
            // Remove old entry (works with both plain paths and markdown links)
            let _ = self
                .remove_from_index_contents(&parent_index, &old_filename)
                .await;

            // Add new entry with proper formatting
            let new_path_canonical = self.get_canonical_path(&new_path);
            let title = self.resolve_title(&new_path_canonical).await;
            let _ = self
                .add_to_index_contents_canonical(&parent_index, &new_path_canonical, &title)
                .await;
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
    pub async fn convert_to_leaf(&self, path: &Path) -> Result<PathBuf> {
        // Check if this is an index with empty contents
        let index = self.parse_index(path).await?;
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
        if self.fs.exists(&new_path).await {
            return Err(DiaryxError::InvalidPath {
                path: new_path,
                message: "Target file already exists".to_string(),
            });
        }

        // Move file out of directory
        self.fs.move_file(path, &new_path).await?;

        // Remove contents property
        let _ = self
            .remove_frontmatter_property(&new_path, "contents")
            .await;

        // Update part_of path since file moved one level up
        if let Ok(Some(Value::String(old_part_of))) =
            self.get_frontmatter_property(&new_path, "part_of").await
        {
            use crate::path_utils::{normalize_path, relative_path_from_file_to_target};

            // Parse the markdown link to get the path and type
            let parsed = link_parser::parse_link(&old_part_of);

            // Resolve target path based on path type
            let target_path = match parsed.path_type {
                link_parser::PathType::WorkspaceRoot => {
                    // Workspace-root path: canonical is already workspace-relative
                    if let Some(ref root) = self.root_path {
                        normalize_path(&root.join(&parsed.path))
                    } else {
                        // No workspace root: path is already workspace-relative
                        PathBuf::from(&parsed.path)
                    }
                }
                link_parser::PathType::Relative | link_parser::PathType::Ambiguous => {
                    // Relative path: resolve against old file's directory
                    normalize_path(&current_dir.join(&parsed.path))
                }
            };

            // Format the new part_of link
            let new_part_of = if self.root_path.is_some() {
                // Use markdown link format
                let target_canonical = self.get_canonical_path(&target_path);
                let new_path_canonical = self.get_canonical_path(&new_path);
                let target_title = self.resolve_title(&target_canonical).await;
                self.format_link_sync(&target_canonical, &target_title, &new_path_canonical)
            } else {
                // Fallback: plain relative path
                relative_path_from_file_to_target(&new_path, &target_path)
            };

            let _ = self
                .set_frontmatter_property(&new_path, "part_of", Value::String(new_part_of))
                .await;
        }

        // Update grandparent's contents
        if let Ok(Some(grandparent_index)) = self.find_any_index_in_dir(parent_of_dir).await {
            // Remove old entry (works with both plain paths and markdown links)
            let old_rel = format!("{}/{}.md", dir_name, dir_name);
            let _ = self
                .remove_from_index_contents(&grandparent_index, &old_rel)
                .await;

            // Add new entry with proper formatting
            let new_path_canonical = self.get_canonical_path(&new_path);
            let title = self.resolve_title(&new_path_canonical).await;
            let _ = self
                .add_to_index_contents_canonical(&grandparent_index, &new_path_canonical, &title)
                .await;
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
    pub async fn attach_and_move_entry_to_parent(
        &self,
        entry: &Path,
        parent: &Path,
    ) -> Result<PathBuf> {
        // Check if parent needs to be converted to index
        let parent_is_index = self.is_index_file(parent).await;

        let effective_parent = if parent_is_index {
            parent.to_path_buf()
        } else {
            // Convert parent to index first
            self.convert_to_index(parent).await?
        };

        // Get parent directory
        let parent_dir = effective_parent
            .parent()
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: effective_parent.clone(),
                message: "Parent index has no directory".to_string(),
            })?;

        // Get entry filename
        let entry_filename =
            entry
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
            self.move_entry(entry, &new_entry_path).await?;
        }

        // Attach entry to parent (creates bidirectional links)
        self.attach_entry_to_parent(&new_entry_path, &effective_parent)
            .await?;

        Ok(new_entry_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{FileSystem, InMemoryFileSystem, SyncToAsyncFs, block_on_test};

    type TestFs = SyncToAsyncFs<InMemoryFileSystem>;

    fn make_test_fs() -> TestFs {
        SyncToAsyncFs::new(InMemoryFileSystem::new())
    }

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

        let fs = make_test_fs();
        let ws = Workspace::new(fs);
        let output = ws.format_tree(&tree, "");

        assert!(output.contains("Root - Root description"));
        assert!(output.contains("Child 1"));
        assert!(output.contains("Child 2 - Child desc"));
    }

    #[test]
    fn test_parse_index() {
        let fs = InMemoryFileSystem::new();
        fs.write_file(
            Path::new("test.md"),
            "---\ntitle: Test\ncontents: []\n---\n\nBody content",
        )
        .unwrap();

        let async_fs = SyncToAsyncFs::new(fs);
        let ws = Workspace::new(async_fs);

        let result = block_on_test(ws.parse_index(Path::new("test.md")));
        assert!(result.is_ok());

        let index = result.unwrap();
        assert_eq!(index.frontmatter.title, Some("Test".to_string()));
        assert!(index.frontmatter.is_index());
        assert!(index.body.contains("Body content"));
    }

    #[test]
    fn test_is_index_file() {
        let fs = InMemoryFileSystem::new();
        fs.write_file(
            Path::new("index.md"),
            "---\ntitle: Index\ncontents: []\n---\n",
        )
        .unwrap();
        fs.write_file(Path::new("leaf.md"), "---\ntitle: Leaf\n---\n")
            .unwrap();

        let async_fs = SyncToAsyncFs::new(fs);
        let ws = Workspace::new(async_fs);

        assert!(block_on_test(ws.is_index_file(Path::new("index.md"))));
        assert!(!block_on_test(ws.is_index_file(Path::new("leaf.md"))));
        assert!(!block_on_test(
            ws.is_index_file(Path::new("nonexistent.md"))
        ));
    }

    #[test]
    fn test_is_root_index() {
        let fs = InMemoryFileSystem::new();
        fs.write_file(
            Path::new("root.md"),
            "---\ntitle: Root\ncontents: []\n---\n",
        )
        .unwrap();
        fs.write_file(
            Path::new("child.md"),
            "---\ntitle: Child\ncontents: []\npart_of: root.md\n---\n",
        )
        .unwrap();

        let async_fs = SyncToAsyncFs::new(fs);
        let ws = Workspace::new(async_fs);

        assert!(block_on_test(ws.is_root_index(Path::new("root.md"))));
        assert!(!block_on_test(ws.is_root_index(Path::new("child.md"))));
    }
}
