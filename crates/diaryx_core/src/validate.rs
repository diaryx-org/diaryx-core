//! Workspace link validation and fixing.
//!
//! This module provides functionality to validate `part_of` and `contents` references
//! within a workspace, detecting broken links and other structural issues, and
//! optionally fixing them.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::entry::DiaryxApp;
use crate::error::Result;
use crate::fs::FileSystem;
use crate::utils::path::relative_path_from_file_to_target;
use crate::workspace::Workspace;

/// A validation error indicating a broken reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ValidationWarning {
    /// A file exists but is not referenced by any index's contents.
    OrphanFile {
        /// The orphan file path
        file: PathBuf,
    },
    /// A file or directory exists but is not in the contents hierarchy.
    /// Used for "List All Files" mode to show all filesystem entries.
    UnlinkedEntry {
        /// The entry path
        path: PathBuf,
        /// Whether this is a directory
        is_dir: bool,
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
}

/// Result of validating a workspace.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
}

/// Validator for checking workspace link integrity.
pub struct Validator<FS: FileSystem> {
    ws: Workspace<FS>,
}

impl<FS: FileSystem> Validator<FS> {
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
    pub fn validate_workspace(&self, root_path: &Path) -> Result<ValidationResult> {
        let mut result = ValidationResult::default();
        let mut visited = HashSet::new();

        self.validate_recursive(root_path, &mut result, &mut visited)?;

        // Find unlinked entries: files/dirs in workspace not visited during traversal
        // Only scan immediate directory (non-recursive) for performance
        let workspace_root = root_path.parent().unwrap_or(Path::new("."));
        if let Ok(all_entries) = self.ws.fs_ref().list_files(workspace_root) {
            // Normalize visited paths for comparison
            let visited_normalized: HashSet<PathBuf> = visited
                .iter()
                .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
                .collect();

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

                let entry_canonical = entry.canonicalize().unwrap_or_else(|_| entry.clone());
                if !visited_normalized.contains(&entry_canonical) {
                    let is_dir = self.ws.fs_ref().is_dir(&entry);

                    // Report as OrphanFile if it's an .md file (for backwards compat)
                    if entry.extension().is_some_and(|ext| ext == "md") {
                        result.warnings.push(ValidationWarning::OrphanFile {
                            file: entry.clone(),
                        });
                    }

                    // Always report as UnlinkedEntry for "List All Files" mode
                    result.warnings.push(ValidationWarning::UnlinkedEntry {
                        path: entry,
                        is_dir,
                    });
                }
            }
        }

        Ok(result)
    }

    /// Recursively validate from a given path.
    fn validate_recursive(
        &self,
        path: &Path,
        result: &mut ValidationResult,
        visited: &mut HashSet<PathBuf>,
    ) -> Result<()> {
        // Avoid cycles
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if visited.contains(&canonical) {
            result.warnings.push(ValidationWarning::CircularReference {
                files: vec![path.to_path_buf()],
            });
            return Ok(());
        }
        visited.insert(canonical);
        result.files_checked += 1;

        // Try to parse as index
        if let Ok(index) = self.ws.parse_index(path) {
            let dir = index.directory().unwrap_or_else(|| Path::new(""));

            // Check all contents references
            for child_ref in index.frontmatter.contents_list() {
                let child_path = dir.join(child_ref);

                if !self.ws.fs_ref().exists(&child_path) {
                    result.errors.push(ValidationError::BrokenContentsRef {
                        index: path.to_path_buf(),
                        target: child_ref.clone(),
                    });
                } else {
                    // Recurse into child
                    self.validate_recursive(&child_path, result, visited)?;
                }
            }

            // Check part_of if present
            if let Some(ref part_of) = index.frontmatter.part_of {
                let parent_path = dir.join(part_of);
                if !self.ws.fs_ref().exists(&parent_path) {
                    result.errors.push(ValidationError::BrokenPartOf {
                        file: path.to_path_buf(),
                        target: part_of.clone(),
                    });
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
    pub fn validate_file(&self, file_path: &Path) -> Result<ValidationResult> {
        let mut result = ValidationResult::default();

        // Normalize path
        let path = if file_path.is_absolute() {
            file_path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_default()
                .join(file_path)
        };

        // Canonicalize to remove . and .. components if possible
        let path = path.canonicalize().unwrap_or(path);

        if !self.ws.fs_ref().exists(&path) {
            return Err(crate::error::DiaryxError::InvalidPath {
                path: path.clone(),
                message: "File not found".to_string(),
            });
        }

        result.files_checked = 1;

        // Try to parse and validate
        if let Ok(index) = self.ws.parse_index(&path) {
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
                let child_path = dir.join(child_ref);

                if !self.ws.fs_ref().exists(&child_path) {
                    result.errors.push(ValidationError::BrokenContentsRef {
                        index: path.clone(),
                        target: child_ref.clone(),
                    });
                }
            }

            // Check part_of if present
            if let Some(ref part_of) = index.frontmatter.part_of {
                let parent_path = dir.join(part_of);
                if !self.ws.fs_ref().exists(&parent_path) {
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
                    let suggested_index = find_index_in_directory(&self.ws, dir, Some(&path));
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
                let attachment_path = dir.join(attachment);

                // Check if attachment exists
                if !self.ws.fs_ref().exists(&attachment_path) {
                    result.errors.push(ValidationError::BrokenAttachment {
                        file: path.clone(),
                        attachment: attachment.clone(),
                    });
                }

                // Check if attachment path is non-portable
                if let Some(warning) = check_non_portable_path(&path, "attachments", attachment, dir) {
                    result.warnings.push(warning);
                }
            }

            // Check for unlisted .md files in the same directory
            // Only if this file has contents (is an index)
            if !contents_list.is_empty() || index.frontmatter.contents.is_some() {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    let this_filename = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("");

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

                    for entry in entries.filter_map(|e| e.ok()) {
                        let entry_path = entry.path();
                        if entry_path.is_file() {
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
                                        if lower == "readme.md" || lower == "index.md" || lower.ends_with(".index.md") {
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
                                    if let Some(fname) = filename {
                                        if !referenced_attachments.contains(fname) {
                                            result.warnings.push(ValidationWarning::OrphanBinaryFile {
                                                file: entry_path,
                                                // We can suggest connecting to the current index
                                                suggested_index: Some(path.clone()),
                                            });
                                        }
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
        matches!(c, std::path::Component::CurDir | std::path::Component::ParentDir)
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
fn find_index_in_directory<FS: FileSystem>(
    ws: &Workspace<FS>,
    dir: &Path,
    exclude: Option<&Path>,
) -> Option<PathBuf> {
    let mut indexes = Vec::new();

    if let Ok(entries) = ws.fs_ref().list_files(dir) {
        for entry_path in entries {
            // Skip the excluded file
            if let Some(excl) = exclude {
                if entry_path == excl {
                    continue;
                }
            }

            // Only check markdown files
            if entry_path.extension().is_some_and(|ext| ext == "md") {
                // If it's a file (not a dir), try to parse it as an index
                if !ws.fs_ref().is_dir(&entry_path) {
                    if let Ok(index) = ws.parse_index(&entry_path) {
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Fixer for validation issues.
///
/// This struct provides methods to automatically fix validation errors and warnings.
pub struct ValidationFixer<FS: FileSystem + Clone> {
    app: DiaryxApp<FS>,
}

impl<FS: FileSystem + Clone> ValidationFixer<FS> {
    /// Create a new validation fixer.
    pub fn new(fs: FS) -> Self {
        Self {
            app: DiaryxApp::new(fs),
        }
    }

    /// Fix a broken `part_of` reference by removing it.
    pub fn fix_broken_part_of(&self, file: &Path) -> FixResult {
        let file_str = file.to_string_lossy();
        match self.app.remove_frontmatter_property(&file_str, "part_of") {
            Ok(_) => FixResult::success(format!(
                "Removed broken part_of from {}",
                file.display()
            )),
            Err(e) => FixResult::failure(format!(
                "Failed to remove part_of from {}: {}",
                file.display(),
                e
            )),
        }
    }

    /// Fix a broken `contents` reference by removing it from the index.
    pub fn fix_broken_contents_ref(&self, index: &Path, target: &str) -> FixResult {
        let index_str = index.to_string_lossy();
        match self.app.get_frontmatter_property(&index_str, "contents") {
            Ok(Some(serde_yaml::Value::Sequence(items))) => {
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

                match self.app.set_frontmatter_property(
                    &index_str,
                    "contents",
                    serde_yaml::Value::Sequence(filtered),
                ) {
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
            _ => FixResult::failure(format!(
                "Could not read contents from {}",
                index.display()
            )),
        }
    }

    /// Fix a broken `attachments` reference by removing it.
    pub fn fix_broken_attachment(&self, file: &Path, attachment: &str) -> FixResult {
        let file_str = file.to_string_lossy();
        match self.app.get_frontmatter_property(&file_str, "attachments") {
            Ok(Some(serde_yaml::Value::Sequence(items))) => {
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
                    self.app.remove_frontmatter_property(&file_str, "attachments")
                } else {
                    self.app.set_frontmatter_property(
                        &file_str,
                        "attachments",
                        serde_yaml::Value::Sequence(filtered),
                    )
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
    pub fn fix_non_portable_path(
        &self,
        file: &Path,
        property: &str,
        old_value: &str,
        new_value: &str,
    ) -> FixResult {
        let file_str = file.to_string_lossy();

        match property {
            "part_of" => {
                match self.app.set_frontmatter_property(
                    &file_str,
                    "part_of",
                    serde_yaml::Value::String(new_value.to_string()),
                ) {
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
                match self.app.get_frontmatter_property(&file_str, property) {
                    Ok(Some(serde_yaml::Value::Sequence(items))) => {
                        let updated: Vec<serde_yaml::Value> = items
                            .into_iter()
                            .map(|item| {
                                if let serde_yaml::Value::String(ref s) = item {
                                    if s == old_value {
                                        return serde_yaml::Value::String(new_value.to_string());
                                    }
                                }
                                item
                            })
                            .collect();

                        match self.app.set_frontmatter_property(
                            &file_str,
                            property,
                            serde_yaml::Value::Sequence(updated),
                        ) {
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
    pub fn fix_unlisted_file(&self, index: &Path, file: &Path) -> FixResult {
        let index_str = index.to_string_lossy();
        let file_rel = relative_path_from_file_to_target(index, file);

        match self.app.get_frontmatter_property(&index_str, "contents") {
            Ok(Some(serde_yaml::Value::Sequence(mut items))) => {
                items.push(serde_yaml::Value::String(file_rel.clone()));
                match self.app.set_frontmatter_property(
                    &index_str,
                    "contents",
                    serde_yaml::Value::Sequence(items),
                ) {
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
            Ok(None) => {
                // No contents yet, create it
                match self.app.set_frontmatter_property(
                    &index_str,
                    "contents",
                    serde_yaml::Value::Sequence(vec![serde_yaml::Value::String(file_rel.clone())]),
                ) {
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
            _ => FixResult::failure(format!(
                "Could not read contents from {}",
                index.display()
            )),
        }
    }

    /// Add an orphan binary file to an index's attachments.
    pub fn fix_orphan_binary_file(&self, index: &Path, file: &Path) -> FixResult {
        let index_str = index.to_string_lossy();
        let file_rel = relative_path_from_file_to_target(index, file);

        match self.app.get_frontmatter_property(&index_str, "attachments") {
            Ok(Some(serde_yaml::Value::Sequence(mut items))) => {
                items.push(serde_yaml::Value::String(file_rel.clone()));
                match self.app.set_frontmatter_property(
                    &index_str,
                    "attachments",
                    serde_yaml::Value::Sequence(items),
                ) {
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
            Ok(None) => {
                // No attachments yet, create it
                match self.app.set_frontmatter_property(
                    &index_str,
                    "attachments",
                    serde_yaml::Value::Sequence(vec![serde_yaml::Value::String(file_rel.clone())]),
                ) {
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
    pub fn fix_missing_part_of(&self, file: &Path, index: &Path) -> FixResult {
        let file_str = file.to_string_lossy();
        let index_rel = relative_path_from_file_to_target(file, index);

        match self.app.set_frontmatter_property(
            &file_str,
            "part_of",
            serde_yaml::Value::String(index_rel.clone()),
        ) {
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

    /// Fix a validation error.
    pub fn fix_error(&self, error: &ValidationError) -> FixResult {
        match error {
            ValidationError::BrokenPartOf { file, target: _ } => {
                self.fix_broken_part_of(file)
            }
            ValidationError::BrokenContentsRef { index, target } => {
                self.fix_broken_contents_ref(index, target)
            }
            ValidationError::BrokenAttachment { file, attachment } => {
                self.fix_broken_attachment(file, attachment)
            }
        }
    }

    /// Fix a validation warning.
    ///
    /// Returns `None` if the warning type cannot be automatically fixed.
    pub fn fix_warning(&self, warning: &ValidationWarning) -> Option<FixResult> {
        match warning {
            ValidationWarning::UnlistedFile { index, file } => {
                Some(self.fix_unlisted_file(index, file))
            }
            ValidationWarning::NonPortablePath {
                file,
                property,
                value,
                suggested,
            } => Some(self.fix_non_portable_path(file, property, value, suggested)),
            ValidationWarning::OrphanBinaryFile {
                file,
                suggested_index,
            } => {
                suggested_index
                    .as_ref()
                    .map(|index| self.fix_orphan_binary_file(index, file))
            }
            ValidationWarning::MissingPartOf {
                file,
                suggested_index,
            } => {
                suggested_index
                    .as_ref()
                    .map(|index| self.fix_missing_part_of(file, index))
            }
            // These cannot be auto-fixed
            ValidationWarning::OrphanFile { .. }
            | ValidationWarning::UnlinkedEntry { .. }
            | ValidationWarning::CircularReference { .. }
            | ValidationWarning::MultipleIndexes { .. } => None,
        }
    }

    /// Attempt to fix all errors in a validation result.
    ///
    /// Returns a list of fix results for each error.
    pub fn fix_all_errors(&self, result: &ValidationResult) -> Vec<FixResult> {
        result.errors.iter().map(|e| self.fix_error(e)).collect()
    }

    /// Attempt to fix all fixable warnings in a validation result.
    ///
    /// Returns a list of fix results for warnings that could be fixed.
    /// Warnings that cannot be auto-fixed are skipped.
    pub fn fix_all_warnings(&self, result: &ValidationResult) -> Vec<FixResult> {
        result
            .warnings
            .iter()
            .filter_map(|w| self.fix_warning(w))
            .collect()
    }

    /// Attempt to fix all errors and fixable warnings in a validation result.
    ///
    /// Returns a tuple of (error fix results, warning fix results).
    pub fn fix_all(&self, result: &ValidationResult) -> (Vec<FixResult>, Vec<FixResult>) {
        (self.fix_all_errors(result), self.fix_all_warnings(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::MockFileSystem;

    #[test]
    fn test_valid_workspace() {
        let fs = MockFileSystem::new()
            .with_file(
                "README.md",
                "---\ntitle: Root\ncontents:\n  - note.md\n---\n",
            )
            .with_file("note.md", "---\ntitle: Note\npart_of: README.md\n---\n");

        let validator = Validator::new(fs);
        let result = validator
            .validate_workspace(Path::new("README.md"))
            .unwrap();

        assert!(result.is_ok());
        assert_eq!(result.files_checked, 2);
    }

    #[test]
    fn test_broken_contents_ref() {
        let fs = MockFileSystem::new().with_file(
            "README.md",
            "---\ntitle: Root\ncontents:\n  - missing.md\n---\n",
        );

        let validator = Validator::new(fs);
        let result = validator
            .validate_workspace(Path::new("README.md"))
            .unwrap();

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
        let fs = MockFileSystem::new()
            .with_file(
                "README.md",
                "---\ntitle: Root\ncontents:\n  - note.md\n---\n",
            )
            .with_file(
                "note.md",
                "---\ntitle: Note\npart_of: missing_parent.md\n---\n",
            );

        let validator = Validator::new(fs);
        let result = validator
            .validate_workspace(Path::new("README.md"))
            .unwrap();

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
