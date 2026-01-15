//! Workspace link validation and fixing.
//!
//! This module provides functionality to validate `part_of` and `contents` references
//! within a workspace, detecting broken links and other structural issues, and
//! optionally fixing them.
//!
//! # Async-first Design
//!
//! This module uses `AsyncFileSystem` for all filesystem operations.
//! For synchronous contexts (CLI, tests), wrap a sync filesystem with
//! `SyncToAsyncFs` and use `futures_lite::future::block_on()`.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::Result;
use crate::fs::AsyncFileSystem;
use crate::utils::path::relative_path_from_file_to_target;
use crate::workspace::Workspace;

/// Normalize a path by removing `.` and `..` components without filesystem access.
/// This is more reliable than `canonicalize()` which can fail on WASM or with symlinks.
fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                // Pop the last component if possible
                normalized.pop();
            }
            Component::CurDir => {
                // Skip `.` components
            }
            _ => {
                // Keep everything else (Normal, RootDir, Prefix)
                normalized.push(component);
            }
        }
    }
    normalized
}

/// A validation error indicating a broken reference.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
#[serde(tag = "type")]
pub enum ValidationError {
    /// A file's `part_of` points to a non-existent file.
    BrokenPartOf {
        /// The file containing the broken reference
        file: PathBuf,
        /// The target path that doesn't exist
        target: String,
    },
    /// An index's `contents` references a non-existent file.
    BrokenContentsRef {
        /// The index file containing the broken reference
        index: PathBuf,
        /// The target path that doesn't exist
        target: String,
    },
    /// A file's `attachments` references a non-existent file.
    BrokenAttachment {
        /// The file containing the broken reference
        file: PathBuf,
        /// The attachment path that doesn't exist
        attachment: String,
    },
}

/// A validation warning indicating a potential issue.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
#[serde(tag = "type")]
pub enum ValidationWarning {
    /// A file exists but is not referenced by any index's contents.
    OrphanFile {
        /// The orphan file path
        file: PathBuf,
        /// Suggested index to add this to (nearest parent index in hierarchy)
        suggested_index: Option<PathBuf>,
    },
    /// A file or directory exists but is not in the contents hierarchy.
    /// Used for "List All Files" mode to show all filesystem entries.
    UnlinkedEntry {
        /// The entry path
        path: PathBuf,
        /// Whether this is a directory
        is_dir: bool,
        /// Suggested index to add this to (nearest parent index in hierarchy)
        /// For directories, this points to the index file inside the directory if one exists
        suggested_index: Option<PathBuf>,
        /// For directories with an index file, this is the path to that index file
        /// (which should be added to contents instead of the directory path)
        index_file: Option<PathBuf>,
    },
    /// A markdown file exists in the directory but is not listed in the index's contents.
    UnlistedFile {
        /// The index file that should contain this file
        index: PathBuf,
        /// The unlisted file path
        file: PathBuf,
    },
    /// Circular reference detected in workspace hierarchy.
    CircularReference {
        /// The files involved in the cycle
        files: Vec<PathBuf>,
        /// Suggested file to edit to break the cycle (the one that would break it most cleanly)
        suggested_file: Option<PathBuf>,
        /// The part_of value to remove from the suggested file
        suggested_remove_part_of: Option<String>,
    },
    /// A path in frontmatter is not portable (absolute, contains `.`, etc.)
    NonPortablePath {
        /// The file containing the non-portable path
        file: PathBuf,
        /// The property containing the path ("part_of" or "contents")
        property: String,
        /// The problematic path value
        value: String,
        /// The suggested normalized path
        suggested: String,
    },
    /// Multiple index files found in the same directory.
    MultipleIndexes {
        /// The directory containing multiple indexes
        directory: PathBuf,
        /// The index files found
        indexes: Vec<PathBuf>,
    },
    /// A binary file exists but is not referenced by any file's attachments.
    OrphanBinaryFile {
        /// The orphan binary file path
        file: PathBuf,
        /// Suggested index to add this to (if exactly one index in same directory)
        suggested_index: Option<PathBuf>,
    },
    /// A file has no `part_of` property and is not the root index (orphan/disconnected).
    MissingPartOf {
        /// The file missing the part_of property
        file: PathBuf,
        /// Suggested index to connect to (if exactly one index in same directory)
        suggested_index: Option<PathBuf>,
    },
    /// A non-markdown file is referenced in `contents` (should be in `attachments` instead).
    InvalidContentsRef {
        /// The index file containing the invalid reference
        index: PathBuf,
        /// The non-markdown file that was referenced
        target: String,
    },
}

impl ValidationWarning {
    /// Get a human-readable description of this warning.
    pub fn description(&self) -> &'static str {
        match self {
            Self::OrphanFile { .. } => "Not in any index contents",
            Self::UnlinkedEntry { is_dir: true, .. } => "Unlinked directory",
            Self::UnlinkedEntry { is_dir: false, .. } => "Unlinked file",
            Self::UnlistedFile { .. } => "Not listed in index",
            Self::CircularReference { .. } => "Circular reference detected",
            Self::NonPortablePath { .. } => "Non-portable path",
            Self::MultipleIndexes { .. } => "Multiple indexes in directory",
            Self::OrphanBinaryFile { .. } => "Binary file not attached",
            Self::MissingPartOf { .. } => "Missing part_of reference",
            Self::InvalidContentsRef { .. } => "Non-markdown file in contents",
        }
    }

    /// Check if this warning can be automatically fixed.
    pub fn can_auto_fix(&self) -> bool {
        match self {
            Self::OrphanFile {
                suggested_index, ..
            } => suggested_index.is_some(),
            Self::OrphanBinaryFile {
                suggested_index, ..
            } => suggested_index.is_some(),
            Self::MissingPartOf {
                suggested_index, ..
            } => suggested_index.is_some(),
            Self::UnlinkedEntry {
                suggested_index,
                is_dir,
                index_file,
                ..
            } => {
                if suggested_index.is_none() {
                    return false;
                }
                // Directories need an index file inside to be linkable
                if *is_dir { index_file.is_some() } else { true }
            }
            Self::UnlistedFile { .. } => true,
            Self::NonPortablePath { .. } => true,
            Self::CircularReference {
                suggested_file,
                suggested_remove_part_of,
                ..
            } => suggested_file.is_some() && suggested_remove_part_of.is_some(),
            Self::MultipleIndexes { .. } => false,
            Self::InvalidContentsRef { .. } => false,
        }
    }

    /// Get the primary file path associated with this warning.
    pub fn file_path(&self) -> Option<&Path> {
        match self {
            Self::OrphanFile { file, .. } => Some(file),
            Self::OrphanBinaryFile { file, .. } => Some(file),
            Self::MissingPartOf { file, .. } => Some(file),
            Self::UnlinkedEntry { path, .. } => Some(path),
            Self::UnlistedFile { file, .. } => Some(file),
            Self::CircularReference { files, .. } => files.first().map(|p| p.as_path()),
            Self::NonPortablePath { file, .. } => Some(file),
            Self::MultipleIndexes { directory, .. } => Some(directory),
            Self::InvalidContentsRef { index, .. } => Some(index),
        }
    }

    /// Check if the associated file can be viewed/edited (i.e., is a markdown file).
    pub fn is_viewable(&self) -> bool {
        match self {
            Self::OrphanBinaryFile { .. } => false,
            Self::UnlinkedEntry { is_dir: true, .. } => false,
            Self::MultipleIndexes { .. } => false,
            _ => self
                .file_path()
                .and_then(|p| p.extension())
                .is_some_and(|ext| ext == "md"),
        }
    }

    /// Check if this warning supports choosing a different parent index.
    pub fn supports_parent_picker(&self) -> bool {
        matches!(
            self,
            Self::OrphanFile { .. }
                | Self::OrphanBinaryFile { .. }
                | Self::MissingPartOf { .. }
                | Self::UnlinkedEntry { .. }
                | Self::UnlistedFile { .. }
        )
    }
}

impl ValidationError {
    /// Get a human-readable description of this error.
    pub fn description(&self) -> &'static str {
        match self {
            Self::BrokenPartOf { .. } => "Broken part_of reference",
            Self::BrokenContentsRef { .. } => "Broken contents reference",
            Self::BrokenAttachment { .. } => "Broken attachment reference",
        }
    }

    /// Get the primary file path associated with this error.
    pub fn file_path(&self) -> &Path {
        match self {
            Self::BrokenPartOf { file, .. } => file,
            Self::BrokenContentsRef { index, .. } => index,
            Self::BrokenAttachment { file, .. } => file,
        }
    }
}

/// Result of validating a workspace.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct ValidationResult {
    /// Validation errors (broken references)
    pub errors: Vec<ValidationError>,
    /// Validation warnings (orphans, cycles)
    pub warnings: Vec<ValidationWarning>,
    /// Number of files checked
    pub files_checked: usize,
}

impl ValidationResult {
    /// Returns true if validation passed with no errors.
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// Returns true if there are any errors or warnings.
    pub fn has_issues(&self) -> bool {
        !self.errors.is_empty() || !self.warnings.is_empty()
    }

    /// Convert to a result with computed metadata fields for frontend use.
    pub fn with_metadata(self) -> ValidationResultWithMeta {
        ValidationResultWithMeta {
            errors: self
                .errors
                .into_iter()
                .map(ValidationErrorWithMeta::from)
                .collect(),
            warnings: self
                .warnings
                .into_iter()
                .map(ValidationWarningWithMeta::from)
                .collect(),
            files_checked: self.files_checked,
        }
    }
}

/// A validation warning with computed metadata for frontend display.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct ValidationWarningWithMeta {
    /// The warning data
    #[serde(flatten)]
    pub warning: ValidationWarning,
    /// Human-readable description
    pub description: String,
    /// Whether this warning can be auto-fixed
    pub can_auto_fix: bool,
    /// Whether the associated file can be viewed in editor
    pub is_viewable: bool,
    /// Whether this warning supports choosing a different parent
    pub supports_parent_picker: bool,
}

impl From<ValidationWarning> for ValidationWarningWithMeta {
    fn from(warning: ValidationWarning) -> Self {
        Self {
            description: warning.description().to_string(),
            can_auto_fix: warning.can_auto_fix(),
            is_viewable: warning.is_viewable(),
            supports_parent_picker: warning.supports_parent_picker(),
            warning,
        }
    }
}

/// A validation error with computed metadata for frontend display.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct ValidationErrorWithMeta {
    /// The error data
    #[serde(flatten)]
    pub error: ValidationError,
    /// Human-readable description
    pub description: String,
}

impl From<ValidationError> for ValidationErrorWithMeta {
    fn from(error: ValidationError) -> Self {
        Self {
            description: error.description().to_string(),
            error,
        }
    }
}

/// Validation result with computed metadata for frontend display.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct ValidationResultWithMeta {
    /// Validation errors with metadata
    pub errors: Vec<ValidationErrorWithMeta>,
    /// Validation warnings with metadata
    pub warnings: Vec<ValidationWarningWithMeta>,
    /// Number of files checked
    pub files_checked: usize,
}

/// Validator for checking workspace link integrity (async-first).
pub struct Validator<FS: AsyncFileSystem> {
    ws: Workspace<FS>,
}

impl<FS: AsyncFileSystem> Validator<FS> {
    /// Create a new validator.
    pub fn new(fs: FS) -> Self {
        Self {
            ws: Workspace::new(fs),
        }
    }

    /// Validate all links starting from a workspace root index.
    ///
    /// Checks:
    /// - All `contents` references point to existing files
    /// - All `part_of` references point to existing files
    /// - Detects unlinked files/directories (not reachable via contents references)
    ///
    /// # Arguments
    /// * `root_path` - Path to the root index file
    /// * `max_depth` - Maximum depth for orphan detection (None = unlimited, Some(2) matches tree view)
    pub async fn validate_workspace(
        &self,
        root_path: &Path,
        max_depth: Option<usize>,
    ) -> Result<ValidationResult> {
        let mut result = ValidationResult::default();
        let mut visited = HashSet::new();

        self.validate_recursive(root_path, &mut result, &mut visited, None, None)
            .await?;

        // Find unlinked entries: files/dirs in workspace not visited during traversal
        // Scan with depth limit to match tree view behavior and improve performance
        let workspace_root = root_path.parent().unwrap_or(Path::new("."));
        let all_entries = self
            .list_files_with_depth(workspace_root, 0, max_depth)
            .await;

        if !all_entries.is_empty() {
            // Normalize visited paths for comparison using path normalization
            // This is more reliable than canonicalize() which can fail on WASM
            let visited_normalized: HashSet<PathBuf> =
                visited.iter().map(|p| normalize_path(p)).collect();

            // Build a map of directory -> index file path from visited files
            // This allows us to find the nearest parent index for orphan files
            let mut dir_to_index: std::collections::HashMap<PathBuf, PathBuf> =
                std::collections::HashMap::new();
            for visited_path in &visited {
                if visited_path.extension().is_some_and(|ext| ext == "md")
                    && let Some(parent) = visited_path.parent()
                {
                    // Only add if this is likely an index file (has contents or is named index/README)
                    // For simplicity, we add all visited markdown files and let the first one win
                    dir_to_index
                        .entry(parent.to_path_buf())
                        .or_insert_with(|| visited_path.clone());
                }
            }

            // Helper to find nearest parent index for a given path
            let find_nearest_index = |path: &Path| -> Option<PathBuf> {
                let mut current = path.parent();
                while let Some(dir) = current {
                    if let Some(index) = dir_to_index.get(dir) {
                        return Some(index.clone());
                    }
                    current = dir.parent();
                }
                None
            };

            // Directories to skip (common build/dependency directories)
            let skip_dirs = [
                "node_modules",
                "target",
                ".git",
                ".svn",
                "dist",
                "build",
                "__pycache__",
                ".next",
                ".nuxt",
                "vendor",
                ".cargo",
            ];

            for entry in all_entries {
                // Skip entries that are in hidden directories or are hidden files
                // Check all path components, not just the filename
                let in_hidden_dir = entry.components().any(|c| {
                    if let std::path::Component::Normal(name) = c {
                        name.to_str().is_some_and(|s| s.starts_with('.'))
                    } else {
                        false
                    }
                });

                if in_hidden_dir {
                    continue;
                }

                // Skip entries in common non-workspace directories
                let should_skip = entry.components().any(|c| {
                    if let std::path::Component::Normal(name) = c {
                        skip_dirs.iter().any(|&d| name == std::ffi::OsStr::new(d))
                    } else {
                        false
                    }
                });

                if should_skip {
                    continue;
                }

                let entry_normalized = normalize_path(&entry);
                if !visited_normalized.contains(&entry_normalized) {
                    // Skip directories - we don't emit warnings for them.
                    // If a directory has an unlinked index file, OrphanFile covers it.
                    // If a directory has no index file, it's just a regular folder.
                    if self.ws.fs_ref().is_dir(&entry).await {
                        continue;
                    }

                    let suggested_index = find_nearest_index(&entry);
                    let extension = entry.extension().and_then(|e| e.to_str());

                    if extension == Some("md") {
                        // Markdown file not in hierarchy
                        result.warnings.push(ValidationWarning::OrphanFile {
                            file: entry.clone(),
                            suggested_index,
                        });
                    } else if extension.is_some() {
                        // Binary file not referenced by any attachments
                        result.warnings.push(ValidationWarning::OrphanBinaryFile {
                            file: entry.clone(),
                            suggested_index,
                        });
                    }
                }
            }
        }

        Ok(result)
    }

    /// List files and directories with depth limiting.
    /// Returns all entries up to the specified depth from the starting directory.
    async fn list_files_with_depth(
        &self,
        dir: &Path,
        current_depth: usize,
        max_depth: Option<usize>,
    ) -> Vec<PathBuf> {
        // Check if we've exceeded max depth
        if let Some(max) = max_depth {
            if current_depth >= max {
                return Vec::new();
            }
        }

        let mut all_entries = Vec::new();

        if let Ok(entries) = self.ws.fs_ref().list_files(dir).await {
            for entry in entries {
                all_entries.push(entry.clone());

                // Recurse into subdirectories
                if self.ws.fs_ref().is_dir(&entry).await {
                    let sub_entries =
                        Box::pin(self.list_files_with_depth(&entry, current_depth + 1, max_depth))
                            .await;
                    all_entries.extend(sub_entries);
                }
            }
        }

        all_entries
    }

    /// Recursively validate from a given path.
    /// `from_parent` tracks which file led us here (for cycle detection).
    /// `contents_ref` is the reference string used in the parent's contents (for cycle fix suggestions).
    async fn validate_recursive(
        &self,
        path: &Path,
        result: &mut ValidationResult,
        visited: &mut HashSet<PathBuf>,
        from_parent: Option<&Path>,
        contents_ref: Option<&str>,
    ) -> Result<()> {
        // Avoid cycles - use normalize_path for consistent path comparison
        let normalized = normalize_path(path);
        if visited.contains(&normalized) {
            // Cycle detected! Suggest removing the contents reference from the parent
            result.warnings.push(ValidationWarning::CircularReference {
                files: vec![path.to_path_buf()],
                // Suggest editing the parent file that led us here
                suggested_file: from_parent.map(|p| p.to_path_buf()),
                // The contents ref to remove would be in the parent, but we suggest
                // removing part_of from the target file as that's often cleaner
                suggested_remove_part_of: contents_ref.map(|s| s.to_string()),
            });
            return Ok(());
        }
        visited.insert(normalized);
        result.files_checked += 1;

        // Try to parse as index
        if let Ok(index) = self.ws.parse_index(path).await {
            let dir = index.directory().unwrap_or_else(|| Path::new(""));

            // Check all contents references
            for child_ref in index.frontmatter.contents_list() {
                let child_path = normalize_path(&dir.join(child_ref));

                if !self.ws.fs_ref().exists(&child_path).await {
                    result.errors.push(ValidationError::BrokenContentsRef {
                        index: path.to_path_buf(),
                        target: child_ref.clone(),
                    });
                } else if child_path.extension().is_none_or(|ext| ext != "md") {
                    // Non-markdown file in contents - should be in attachments instead
                    result.warnings.push(ValidationWarning::InvalidContentsRef {
                        index: path.to_path_buf(),
                        target: child_ref.clone(),
                    });
                } else {
                    // Recurse into child, tracking parent info for cycle detection
                    Box::pin(self.validate_recursive(
                        &child_path,
                        result,
                        visited,
                        Some(path),
                        Some(child_ref),
                    ))
                    .await?;
                }
            }

            // Check part_of if present
            if let Some(ref part_of) = index.frontmatter.part_of {
                let parent_path = normalize_path(&dir.join(part_of));
                if !self.ws.fs_ref().exists(&parent_path).await {
                    result.errors.push(ValidationError::BrokenPartOf {
                        file: path.to_path_buf(),
                        target: part_of.clone(),
                    });
                }
            }

            // Add attachments to visited set so they're not reported as orphans
            for attachment in index.frontmatter.attachments_list() {
                let attachment_path = normalize_path(&dir.join(attachment));
                if self.ws.fs_ref().exists(&attachment_path).await {
                    visited.insert(attachment_path);
                }
            }
        }

        Ok(())
    }

    /// Validate a single file's links.
    ///
    /// Checks:
    /// - The file's `part_of` reference points to an existing file
    /// - All `contents` references (if any) point to existing files
    /// - Markdown files in the same directory that aren't listed in `contents`
    ///
    /// Does not recursively validate the entire workspace, just the specified file.
    pub async fn validate_file(&self, file_path: &Path) -> Result<ValidationResult> {
        let mut result = ValidationResult::default();

        // Normalize path
        let path = if file_path.is_absolute() {
            file_path.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_default().join(file_path)
        };

        // Canonicalize to remove . and .. components if possible
        let path = path.canonicalize().unwrap_or(path);

        if !self.ws.fs_ref().exists(&path).await {
            return Err(crate::error::DiaryxError::InvalidPath {
                path: path.clone(),
                message: "File not found".to_string(),
            });
        }

        result.files_checked = 1;

        // Try to parse and validate
        if let Ok(index) = self.ws.parse_index(&path).await {
            let dir = index.directory().unwrap_or_else(|| Path::new(""));

            // Collect listed files (normalized to just filenames for comparison)
            let contents_list = index.frontmatter.contents_list();
            let listed_files: HashSet<String> = contents_list
                .iter()
                .filter_map(|p| {
                    Path::new(p)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.to_string())
                })
                .collect();

            // Check all contents references
            for child_ref in contents_list {
                let child_path = normalize_path(&dir.join(child_ref));

                if !self.ws.fs_ref().exists(&child_path).await {
                    result.errors.push(ValidationError::BrokenContentsRef {
                        index: path.clone(),
                        target: child_ref.clone(),
                    });
                }
            }

            // Check part_of if present
            if let Some(ref part_of) = index.frontmatter.part_of {
                let parent_path = normalize_path(&dir.join(part_of));
                if !self.ws.fs_ref().exists(&parent_path).await {
                    result.errors.push(ValidationError::BrokenPartOf {
                        file: path.clone(),
                        target: part_of.clone(),
                    });
                }

                // Check if part_of is a non-portable path
                if let Some(warning) = check_non_portable_path(&path, "part_of", part_of, dir) {
                    result.warnings.push(warning);
                }
            } else {
                // File has no part_of - check if it's a root index
                // Non-index files (files without contents) should have part_of
                // Index files without part_of are potential root indexes, which is allowed
                // But if it has no contents AND no part_of, it's definitely orphaned
                let is_index = index.frontmatter.contents.is_some()
                    || !index.frontmatter.contents_list().is_empty();

                if !is_index {
                    // Regular file with no part_of = orphan
                    // Try to find an index in the same directory to suggest
                    let suggested_index = find_index_in_directory(&self.ws, dir, Some(&path)).await;
                    result.warnings.push(ValidationWarning::MissingPartOf {
                        file: path.clone(),
                        suggested_index,
                    });
                }
            }

            // Check contents entries for non-portable paths
            for child_ref in index.frontmatter.contents_list() {
                if let Some(warning) = check_non_portable_path(&path, "contents", child_ref, dir) {
                    result.warnings.push(warning);
                }
            }

            // Check attachments if present
            for attachment in index.frontmatter.attachments_list() {
                let attachment_path = normalize_path(&dir.join(attachment));

                // Check if attachment exists
                if !self.ws.fs_ref().exists(&attachment_path).await {
                    result.errors.push(ValidationError::BrokenAttachment {
                        file: path.clone(),
                        attachment: attachment.clone(),
                    });
                }

                // Check if attachment path is non-portable
                if let Some(warning) =
                    check_non_portable_path(&path, "attachments", attachment, dir)
                {
                    result.warnings.push(warning);
                }
            }

            // Check for unlisted .md files in the same directory
            // Only if this file has contents (is an index)
            if (!contents_list.is_empty() || index.frontmatter.contents.is_some())
                && let Ok(entries) = self.ws.fs_ref().list_files(dir).await
            {
                let this_filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

                // Collect all attachments referenced by this index
                let referenced_attachments: HashSet<String> = index
                    .frontmatter
                    .attachments_list()
                    .iter()
                    .filter_map(|p| {
                        Path::new(p)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .map(|s| s.to_string())
                    })
                    .collect();

                // Collect other index files in this directory
                let mut other_indexes: Vec<PathBuf> = Vec::new();

                for entry_path in entries {
                    if !self.ws.fs_ref().is_dir(&entry_path).await {
                        let extension = entry_path.extension().and_then(|e| e.to_str());
                        let filename = entry_path.file_name().and_then(|n| n.to_str());

                        match extension {
                            Some("md") => {
                                if let Some(fname) = filename {
                                    // Skip the current file
                                    if fname == this_filename {
                                        continue;
                                    }
                                    // Check if it's an index file (README.md, index.md, or *.index.md)
                                    let lower = fname.to_lowercase();
                                    if lower == "readme.md"
                                        || lower == "index.md"
                                        || lower.ends_with(".index.md")
                                    {
                                        other_indexes.push(entry_path.clone());
                                    }
                                    // Check if this markdown file is in contents
                                    if !listed_files.contains(fname) {
                                        result.warnings.push(ValidationWarning::UnlistedFile {
                                            index: path.clone(),
                                            file: entry_path,
                                        });
                                    }
                                }
                            }
                            Some(ext) if !ext.eq_ignore_ascii_case("md") => {
                                // Binary file - check if it's referenced by attachments
                                if let Some(fname) = filename
                                    && !referenced_attachments.contains(fname)
                                {
                                    result.warnings.push(ValidationWarning::OrphanBinaryFile {
                                        file: entry_path,
                                        // We can suggest connecting to the current index
                                        suggested_index: Some(path.clone()),
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                }

                // Report multiple indexes if found
                if !other_indexes.is_empty() {
                    let mut all_indexes = other_indexes;
                    all_indexes.push(path.clone());
                    all_indexes.sort();
                    result.warnings.push(ValidationWarning::MultipleIndexes {
                        directory: dir.to_path_buf(),
                        indexes: all_indexes,
                    });
                }
            }
        }

        Ok(result)
    }
}

/// Check if a path reference is non-portable (absolute, contains `.` or `..`)
fn check_non_portable_path(
    file: &Path,
    property: &str,
    value: &str,
    base_dir: &Path,
) -> Option<ValidationWarning> {
    let path = Path::new(value);

    // Check for absolute paths
    if path.is_absolute() {
        // Try to compute a relative path
        let target = Path::new(value);
        let suggested = if let Ok(target_canonical) = target.canonicalize() {
            if let Ok(base_canonical) = base_dir.canonicalize() {
                pathdiff::diff_paths(&target_canonical, &base_canonical)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| value.to_string())
            } else {
                value.to_string()
            }
        } else {
            value.to_string()
        };

        return Some(ValidationWarning::NonPortablePath {
            file: file.to_path_buf(),
            property: property.to_string(),
            value: value.to_string(),
            suggested,
        });
    }

    // Check for `.` or `..` components
    let has_dot_component = path.components().any(|c| {
        matches!(
            c,
            std::path::Component::CurDir | std::path::Component::ParentDir
        )
    });

    if has_dot_component {
        // Normalize the path by resolving it and computing relative path back
        let target_path = base_dir.join(value);
        let suggested = if let Ok(target_canonical) = target_path.canonicalize() {
            if let Ok(base_canonical) = base_dir.canonicalize() {
                pathdiff::diff_paths(&target_canonical, &base_canonical)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| value.to_string())
            } else {
                value.to_string()
            }
        } else {
            value.to_string()
        };

        // Only warn if the suggested path is actually different
        if suggested != value {
            return Some(ValidationWarning::NonPortablePath {
                file: file.to_path_buf(),
                property: property.to_string(),
                value: value.to_string(),
                suggested,
            });
        }
    }

    None
}

/// Find a single index file in a directory. Returns Some if exactly one index found, None otherwise.
/// Excludes the file specified in `exclude` from the search.
async fn find_index_in_directory<FS: AsyncFileSystem>(
    ws: &Workspace<FS>,
    dir: &Path,
    exclude: Option<&Path>,
) -> Option<PathBuf> {
    let mut indexes = Vec::new();

    if let Ok(entries) = ws.fs_ref().list_files(dir).await {
        for entry_path in entries {
            // Skip the excluded file
            if let Some(excl) = exclude
                && entry_path == excl
            {
                continue;
            }

            // Only check markdown files
            if entry_path.extension().is_some_and(|ext| ext == "md") {
                // If it's a file (not a dir), try to parse it as an index
                if !ws.fs_ref().is_dir(&entry_path).await
                    && let Ok(index) = ws.parse_index(&entry_path).await
                {
                    // Check if it has contents (is an index)
                    let is_index = index.frontmatter.contents.is_some()
                        || !index.frontmatter.contents_list().is_empty();

                    // Also consider typical index filenames if they could be empty
                    // but 2025_12.md implies it likely has content.
                    // For auto-fix, we prefer files that are clearly indexes.
                    if is_index {
                        indexes.push(entry_path);
                    } else {
                        // Fallback: check typical filenames if contents are empty/missing
                        // to support newly created index files
                        if let Some(fname) = entry_path.file_name().and_then(|n| n.to_str()) {
                            let lower = fname.to_lowercase();
                            if lower == "readme.md"
                                || lower == "index.md"
                                || lower.ends_with(".index.md")
                            {
                                indexes.push(entry_path);
                            }
                        }
                    }
                }
            }
        }
    }

    // Only return if exactly one index found
    if indexes.len() == 1 {
        indexes.into_iter().next()
    } else {
        None
    }
}

// ============================================================================
// ValidationFixer - Fix validation issues
// ============================================================================

/// Result of attempting to fix a validation issue.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct FixResult {
    /// Whether the fix was successful.
    pub success: bool,
    /// Description of what was done (or why it failed).
    pub message: String,
}

impl FixResult {
    /// Create a successful fix result.
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
        }
    }

    /// Create a failed fix result.
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
        }
    }
}

/// Fixer for validation issues (async-first).
///
/// This struct provides methods to automatically fix validation errors and warnings.
pub struct ValidationFixer<FS: AsyncFileSystem> {
    fs: FS,
}

impl<FS: AsyncFileSystem> ValidationFixer<FS> {
    /// Create a new fixer.
    pub fn new(fs: FS) -> Self {
        Self { fs }
    }

    // ==================== Internal Frontmatter Helpers ====================

    /// Get a frontmatter property from a file
    async fn get_frontmatter_property(&self, path: &Path, key: &str) -> Option<serde_yaml::Value> {
        let content = self.fs.read_to_string(path).await.ok()?;

        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            return None;
        }

        let rest = &content[4..];
        let end_idx = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n"))?;

        let frontmatter_str = &rest[..end_idx];
        let frontmatter: indexmap::IndexMap<String, serde_yaml::Value> =
            serde_yaml::from_str(frontmatter_str).ok()?;
        frontmatter.get(key).cloned()
    }

    /// Set a frontmatter property in a file
    async fn set_frontmatter_property(
        &self,
        path: &Path,
        key: &str,
        value: serde_yaml::Value,
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
                    crate::error::DiaryxError::FileWrite {
                        path: path.to_path_buf(),
                        source: e,
                    }
                });
            }
            Err(e) => {
                return Err(crate::error::DiaryxError::FileRead {
                    path: path.to_path_buf(),
                    source: e,
                });
            }
        };

        let (mut frontmatter, body) =
            if content.starts_with("---\n") || content.starts_with("---\r\n") {
                let rest = &content[4..];
                if let Some(idx) = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n")) {
                    let frontmatter_str = &rest[..idx];
                    let body = &rest[idx + 5..];
                    let fm: indexmap::IndexMap<String, serde_yaml::Value> =
                        serde_yaml::from_str(frontmatter_str)?;
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

        self.fs.write_file(path, &new_content).await.map_err(|e| {
            crate::error::DiaryxError::FileWrite {
                path: path.to_path_buf(),
                source: e,
            }
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

        let mut frontmatter: indexmap::IndexMap<String, serde_yaml::Value> =
            serde_yaml::from_str(frontmatter_str)?;
        frontmatter.shift_remove(key);

        let yaml_str = serde_yaml::to_string(&frontmatter)?;
        let new_content = format!("---\n{}---\n{}", yaml_str, body);

        self.fs.write_file(path, &new_content).await.map_err(|e| {
            crate::error::DiaryxError::FileWrite {
                path: path.to_path_buf(),
                source: e,
            }
        })
    }

    // ==================== Fix Methods ====================

    /// Fix a broken `part_of` reference by removing it.
    pub async fn fix_broken_part_of(&self, file: &Path) -> FixResult {
        match self.remove_frontmatter_property(file, "part_of").await {
            Ok(_) => FixResult::success(format!("Removed broken part_of from {}", file.display())),
            Err(e) => FixResult::failure(format!(
                "Failed to remove part_of from {}: {}",
                file.display(),
                e
            )),
        }
    }

    /// Fix a broken `contents` reference by removing it from the index.
    pub async fn fix_broken_contents_ref(&self, index: &Path, target: &str) -> FixResult {
        match self.get_frontmatter_property(index, "contents").await {
            Some(serde_yaml::Value::Sequence(items)) => {
                let filtered: Vec<serde_yaml::Value> = items
                    .into_iter()
                    .filter(|item| {
                        if let serde_yaml::Value::String(s) = item {
                            s != target
                        } else {
                            true
                        }
                    })
                    .collect();

                match self
                    .set_frontmatter_property(
                        index,
                        "contents",
                        serde_yaml::Value::Sequence(filtered),
                    )
                    .await
                {
                    Ok(_) => FixResult::success(format!(
                        "Removed broken contents ref '{}' from {}",
                        target,
                        index.display()
                    )),
                    Err(e) => FixResult::failure(format!(
                        "Failed to update contents in {}: {}",
                        index.display(),
                        e
                    )),
                }
            }
            _ => FixResult::failure(format!("Could not read contents from {}", index.display())),
        }
    }

    /// Fix a broken `attachments` reference by removing it.
    pub async fn fix_broken_attachment(&self, file: &Path, attachment: &str) -> FixResult {
        match self.get_frontmatter_property(file, "attachments").await {
            Some(serde_yaml::Value::Sequence(items)) => {
                let filtered: Vec<serde_yaml::Value> = items
                    .into_iter()
                    .filter(|item| {
                        if let serde_yaml::Value::String(s) = item {
                            s != attachment
                        } else {
                            true
                        }
                    })
                    .collect();

                let result = if filtered.is_empty() {
                    self.remove_frontmatter_property(file, "attachments").await
                } else {
                    self.set_frontmatter_property(
                        file,
                        "attachments",
                        serde_yaml::Value::Sequence(filtered),
                    )
                    .await
                };

                match result {
                    Ok(_) => FixResult::success(format!(
                        "Removed broken attachment '{}' from {}",
                        attachment,
                        file.display()
                    )),
                    Err(e) => FixResult::failure(format!(
                        "Failed to update attachments in {}: {}",
                        file.display(),
                        e
                    )),
                }
            }
            _ => FixResult::failure(format!(
                "Could not read attachments from {}",
                file.display()
            )),
        }
    }

    /// Fix a non-portable path by replacing it with the normalized version.
    pub async fn fix_non_portable_path(
        &self,
        file: &Path,
        property: &str,
        old_value: &str,
        new_value: &str,
    ) -> FixResult {
        match property {
            "part_of" => {
                match self
                    .set_frontmatter_property(
                        file,
                        "part_of",
                        serde_yaml::Value::String(new_value.to_string()),
                    )
                    .await
                {
                    Ok(_) => FixResult::success(format!(
                        "Normalized {} '{}' -> '{}' in {}",
                        property,
                        old_value,
                        new_value,
                        file.display()
                    )),
                    Err(e) => FixResult::failure(format!(
                        "Failed to update {} in {}: {}",
                        property,
                        file.display(),
                        e
                    )),
                }
            }
            "contents" | "attachments" => {
                match self.get_frontmatter_property(file, property).await {
                    Some(serde_yaml::Value::Sequence(items)) => {
                        let updated: Vec<serde_yaml::Value> = items
                            .into_iter()
                            .map(|item| {
                                if let serde_yaml::Value::String(ref s) = item
                                    && s == old_value
                                {
                                    return serde_yaml::Value::String(new_value.to_string());
                                }
                                item
                            })
                            .collect();

                        match self
                            .set_frontmatter_property(
                                file,
                                property,
                                serde_yaml::Value::Sequence(updated),
                            )
                            .await
                        {
                            Ok(_) => FixResult::success(format!(
                                "Normalized {} '{}' -> '{}' in {}",
                                property,
                                old_value,
                                new_value,
                                file.display()
                            )),
                            Err(e) => FixResult::failure(format!(
                                "Failed to update {} in {}: {}",
                                property,
                                file.display(),
                                e
                            )),
                        }
                    }
                    _ => FixResult::failure(format!(
                        "Could not read {} from {}",
                        property,
                        file.display()
                    )),
                }
            }
            _ => FixResult::failure(format!("Unknown property: {}", property)),
        }
    }

    /// Add an unlisted file to an index's contents.
    pub async fn fix_unlisted_file(&self, index: &Path, file: &Path) -> FixResult {
        let file_rel = relative_path_from_file_to_target(index, file);

        match self.get_frontmatter_property(index, "contents").await {
            Some(serde_yaml::Value::Sequence(mut items)) => {
                items.push(serde_yaml::Value::String(file_rel.clone()));
                match self
                    .set_frontmatter_property(index, "contents", serde_yaml::Value::Sequence(items))
                    .await
                {
                    Ok(_) => FixResult::success(format!(
                        "Added '{}' to contents in {}",
                        file_rel,
                        index.display()
                    )),
                    Err(e) => FixResult::failure(format!(
                        "Failed to update contents in {}: {}",
                        index.display(),
                        e
                    )),
                }
            }
            None => {
                // No contents yet, create it
                match self
                    .set_frontmatter_property(
                        index,
                        "contents",
                        serde_yaml::Value::Sequence(vec![serde_yaml::Value::String(
                            file_rel.clone(),
                        )]),
                    )
                    .await
                {
                    Ok(_) => FixResult::success(format!(
                        "Added '{}' to new contents in {}",
                        file_rel,
                        index.display()
                    )),
                    Err(e) => FixResult::failure(format!(
                        "Failed to create contents in {}: {}",
                        index.display(),
                        e
                    )),
                }
            }
            _ => FixResult::failure(format!("Could not read contents from {}", index.display())),
        }
    }

    /// Add an orphan binary file to an index's attachments.
    pub async fn fix_orphan_binary_file(&self, index: &Path, file: &Path) -> FixResult {
        let file_rel = relative_path_from_file_to_target(index, file);

        match self.get_frontmatter_property(index, "attachments").await {
            Some(serde_yaml::Value::Sequence(mut items)) => {
                items.push(serde_yaml::Value::String(file_rel.clone()));
                match self
                    .set_frontmatter_property(
                        index,
                        "attachments",
                        serde_yaml::Value::Sequence(items),
                    )
                    .await
                {
                    Ok(_) => FixResult::success(format!(
                        "Added '{}' to attachments in {}",
                        file_rel,
                        index.display()
                    )),
                    Err(e) => FixResult::failure(format!(
                        "Failed to update attachments in {}: {}",
                        index.display(),
                        e
                    )),
                }
            }
            None => {
                // No attachments yet, create it
                match self
                    .set_frontmatter_property(
                        index,
                        "attachments",
                        serde_yaml::Value::Sequence(vec![serde_yaml::Value::String(
                            file_rel.clone(),
                        )]),
                    )
                    .await
                {
                    Ok(_) => FixResult::success(format!(
                        "Added '{}' to new attachments in {}",
                        file_rel,
                        index.display()
                    )),
                    Err(e) => FixResult::failure(format!(
                        "Failed to create attachments in {}: {}",
                        index.display(),
                        e
                    )),
                }
            }
            _ => FixResult::failure(format!(
                "Could not read attachments from {}",
                index.display()
            )),
        }
    }

    /// Fix a missing `part_of` by setting it to point to the given index.
    pub async fn fix_missing_part_of(&self, file: &Path, index: &Path) -> FixResult {
        let index_rel = relative_path_from_file_to_target(file, index);

        match self
            .set_frontmatter_property(
                file,
                "part_of",
                serde_yaml::Value::String(index_rel.clone()),
            )
            .await
        {
            Ok(_) => FixResult::success(format!(
                "Set part_of to '{}' in {}",
                index_rel,
                file.display()
            )),
            Err(e) => FixResult::failure(format!(
                "Failed to set part_of in {}: {}",
                file.display(),
                e
            )),
        }
    }

    /// Fix a circular reference by removing a contents reference from a file.
    ///
    /// This removes the specified reference from the file's `contents` array,
    /// breaking the cycle.
    pub async fn fix_circular_reference(
        &self,
        file: &Path,
        contents_ref_to_remove: &str,
    ) -> FixResult {
        match self.get_frontmatter_property(file, "contents").await {
            Some(serde_yaml::Value::Sequence(items)) => {
                let filtered: Vec<serde_yaml::Value> = items
                    .into_iter()
                    .filter(|item| {
                        if let serde_yaml::Value::String(s) = item {
                            s != contents_ref_to_remove
                        } else {
                            true
                        }
                    })
                    .collect();

                match self
                    .set_frontmatter_property(
                        file,
                        "contents",
                        serde_yaml::Value::Sequence(filtered),
                    )
                    .await
                {
                    Ok(_) => FixResult::success(format!(
                        "Removed circular reference '{}' from {}",
                        contents_ref_to_remove,
                        file.display()
                    )),
                    Err(e) => FixResult::failure(format!(
                        "Failed to remove circular reference from {}: {}",
                        file.display(),
                        e
                    )),
                }
            }
            _ => FixResult::failure(format!("Could not read contents from {}", file.display())),
        }
    }

    /// Fix a validation error.
    pub async fn fix_error(&self, error: &ValidationError) -> FixResult {
        match error {
            ValidationError::BrokenPartOf { file, target: _ } => {
                self.fix_broken_part_of(file).await
            }
            ValidationError::BrokenContentsRef { index, target } => {
                self.fix_broken_contents_ref(index, target).await
            }
            ValidationError::BrokenAttachment { file, attachment } => {
                self.fix_broken_attachment(file, attachment).await
            }
        }
    }

    /// Fix a validation warning.
    ///
    /// Returns `None` if the warning type cannot be automatically fixed.
    pub async fn fix_warning(&self, warning: &ValidationWarning) -> Option<FixResult> {
        match warning {
            ValidationWarning::UnlistedFile { index, file } => {
                Some(self.fix_unlisted_file(index, file).await)
            }
            ValidationWarning::NonPortablePath {
                file,
                property,
                value,
                suggested,
            } => Some(
                self.fix_non_portable_path(file, property, value, suggested)
                    .await,
            ),
            ValidationWarning::OrphanBinaryFile {
                file,
                suggested_index,
            } => {
                if let Some(index) = suggested_index {
                    Some(self.fix_orphan_binary_file(index, file).await)
                } else {
                    None
                }
            }
            ValidationWarning::MissingPartOf {
                file,
                suggested_index,
            } => {
                if let Some(index) = suggested_index {
                    Some(self.fix_missing_part_of(file, index).await)
                } else {
                    None
                }
            }
            ValidationWarning::OrphanFile {
                file,
                suggested_index,
            } => {
                // Fix by adding the file to the nearest parent index's contents
                if let Some(index) = suggested_index {
                    Some(self.fix_unlisted_file(index, file).await)
                } else {
                    None
                }
            }
            ValidationWarning::UnlinkedEntry {
                path,
                is_dir,
                suggested_index,
                index_file,
            } => {
                if let Some(index) = suggested_index {
                    if *is_dir {
                        // For directories, we need to link the index file inside, not the directory itself
                        if let Some(dir_index) = index_file {
                            Some(self.fix_unlisted_file(index, dir_index).await)
                        } else {
                            // Directory has no index file - can't auto-fix
                            None
                        }
                    } else {
                        // For files, add directly to contents
                        Some(self.fix_unlisted_file(index, path).await)
                    }
                } else {
                    None
                }
            }
            ValidationWarning::CircularReference {
                suggested_file,
                suggested_remove_part_of,
                ..
            } => {
                // Can auto-fix if we have a suggestion
                if let (Some(file), Some(contents_ref)) = (suggested_file, suggested_remove_part_of)
                {
                    Some(self.fix_circular_reference(file, contents_ref).await)
                } else {
                    None
                }
            }
            // These cannot be auto-fixed
            ValidationWarning::MultipleIndexes { .. } => None,
            ValidationWarning::InvalidContentsRef { .. } => None,
        }
    }

    /// Attempt to fix all errors in a validation result.
    ///
    /// Returns a list of fix results for each error.
    pub async fn fix_all_errors(&self, result: &ValidationResult) -> Vec<FixResult> {
        let mut fixes = Vec::new();
        for error in &result.errors {
            fixes.push(self.fix_error(error).await);
        }
        fixes
    }

    /// Attempt to fix all fixable warnings in a validation result.
    ///
    /// Returns a list of fix results for warnings that could be fixed.
    /// Warnings that cannot be auto-fixed are skipped.
    pub async fn fix_all_warnings(&self, result: &ValidationResult) -> Vec<FixResult> {
        let mut fixes = Vec::new();
        for warning in &result.warnings {
            if let Some(fix) = self.fix_warning(warning).await {
                fixes.push(fix);
            }
        }
        fixes
    }

    /// Attempt to fix all errors and fixable warnings in a validation result.
    ///
    /// Returns a tuple of (error fix results, warning fix results).
    pub async fn fix_all(&self, result: &ValidationResult) -> (Vec<FixResult>, Vec<FixResult>) {
        (
            self.fix_all_errors(result).await,
            self.fix_all_warnings(result).await,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{FileSystem, InMemoryFileSystem, SyncToAsyncFs, block_on_test};

    type TestFs = SyncToAsyncFs<InMemoryFileSystem>;

    fn make_test_fs() -> InMemoryFileSystem {
        InMemoryFileSystem::new()
    }

    #[test]
    fn test_valid_workspace() {
        let fs = make_test_fs();
        fs.write_file(
            Path::new("README.md"),
            "---\ntitle: Root\ncontents:\n  - note.md\n---\n",
        )
        .unwrap();
        fs.write_file(
            Path::new("note.md"),
            "---\ntitle: Note\npart_of: README.md\n---\n",
        )
        .unwrap();

        let async_fs: TestFs = SyncToAsyncFs::new(fs);
        let validator = Validator::new(async_fs);
        // Use None for unlimited depth in tests
        let result =
            block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

        assert!(result.is_ok());
        assert_eq!(result.files_checked, 2);
    }

    #[test]
    fn test_broken_contents_ref() {
        let fs = make_test_fs();
        fs.write_file(
            Path::new("README.md"),
            "---\ntitle: Root\ncontents:\n  - missing.md\n---\n",
        )
        .unwrap();

        let async_fs: TestFs = SyncToAsyncFs::new(fs);
        let validator = Validator::new(async_fs);
        let result =
            block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

        assert!(!result.is_ok());
        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::BrokenContentsRef { target, .. } => {
                assert_eq!(target, "missing.md");
            }
            _ => panic!("Expected BrokenContentsRef"),
        }
    }

    #[test]
    fn test_broken_part_of() {
        let fs = make_test_fs();
        fs.write_file(
            Path::new("README.md"),
            "---\ntitle: Root\ncontents:\n  - note.md\n---\n",
        )
        .unwrap();
        fs.write_file(
            Path::new("note.md"),
            "---\ntitle: Note\npart_of: missing_parent.md\n---\n",
        )
        .unwrap();

        let async_fs: TestFs = SyncToAsyncFs::new(fs);
        let validator = Validator::new(async_fs);
        let result =
            block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

        assert!(!result.is_ok());
        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::BrokenPartOf { target, .. } => {
                assert_eq!(target, "missing_parent.md");
            }
            _ => panic!("Expected BrokenPartOf"),
        }
    }
}
