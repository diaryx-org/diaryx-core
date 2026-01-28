//! Export module - filter and export workspace files by audience
//!
//! # Async-first Design
//!
//! This module uses `AsyncFileSystem` for all filesystem operations.
//! For synchronous contexts (CLI, tests), wrap a sync filesystem with
//! `SyncToAsyncFs` and use `futures_lite::future::block_on()`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{DiaryxError, Result};
use crate::fs::AsyncFileSystem;
use crate::workspace::{IndexFrontmatter, Workspace};

/// Result of planning an export operation
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct ExportPlan {
    /// Files that will be exported
    pub included: Vec<ExportFile>,
    /// Files that were filtered out (with reason)
    pub excluded: Vec<ExcludedFile>,
    /// The audience being exported for
    pub audience: String,
    /// Source workspace root
    pub source_root: PathBuf,
    /// Destination directory
    pub destination: PathBuf,
}

/// A file to be exported
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct ExportFile {
    /// Original path in the workspace
    pub source_path: PathBuf,
    /// Path relative to workspace root
    pub relative_path: PathBuf,
    /// Destination path
    pub dest_path: PathBuf,
    /// Contents entries that will be filtered out (if any)
    pub filtered_contents: Vec<String>,
}

/// A file that was excluded from export
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct ExcludedFile {
    /// Path to the excluded file
    pub path: PathBuf,
    /// Reason for exclusion
    pub reason: ExclusionReason,
}

/// Why a file was excluded
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub enum ExclusionReason {
    /// File is marked as private
    Private,
    /// File's audience doesn't include the target audience
    AudienceMismatch {
        /// What audiences are intended to view the document
        file_audience: Vec<String>,
        /// What audiences were requested for the export
        requested: String,
    },
    /// File inherits private from parent
    InheritedPrivate {
        /// Path to the parent that was marked as `private`
        from: PathBuf,
    },
    /// File has no audience and inherits to root which has no audience (default private)
    NoAudienceDefined,
}

impl std::fmt::Display for ExclusionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExclusionReason::Private => write!(f, "marked as private"),
            ExclusionReason::AudienceMismatch {
                file_audience,
                requested,
            } => {
                write!(
                    f,
                    "audience {:?} doesn't include '{}'",
                    file_audience, requested
                )
            }
            ExclusionReason::InheritedPrivate { from } => {
                write!(f, "inherits private from {}", from.display())
            }
            ExclusionReason::NoAudienceDefined => {
                write!(f, "no audience defined (defaults to private)")
            }
        }
    }
}

/// Options for export operation
#[derive(Debug, Clone, Default, Serialize)]
pub struct ExportOptions {
    /// Whether to overwrite existing destination
    pub force: bool,
    /// Whether to preserve the audience property in exported files
    pub keep_audience: bool,
}

/// Export operations (async-first)
pub struct Exporter<FS: AsyncFileSystem> {
    workspace: Workspace<FS>,
}

impl<FS: AsyncFileSystem> Exporter<FS> {
    /// Create a new exporter
    pub fn new(fs: FS) -> Self {
        Self {
            workspace: Workspace::new(fs),
        }
    }

    /// Plan an export operation without executing it
    /// This traverses the workspace and determines which files would be included/excluded
    pub async fn plan_export(
        &self,
        workspace_root: &Path,
        audience: &str,
        destination: &Path,
    ) -> Result<ExportPlan> {
        let mut included = Vec::new();
        let mut excluded = Vec::new();
        let mut visited = HashSet::new();

        // Get the workspace root directory
        let root_dir = workspace_root
            .parent()
            .unwrap_or(workspace_root)
            .to_path_buf();

        // Start traversal from the root index
        self.plan_file_recursive(
            workspace_root,
            &root_dir,
            destination,
            audience,
            None, // No inherited audience at root
            &mut included,
            &mut excluded,
            &mut visited,
        )
        .await?;

        Ok(ExportPlan {
            included,
            excluded,
            audience: audience.to_string(),
            source_root: root_dir,
            destination: destination.to_path_buf(),
        })
    }

    /// Recursive helper for planning export
    #[allow(clippy::too_many_arguments)]
    async fn plan_file_recursive(
        &self,
        path: &Path,
        root_dir: &Path,
        dest_dir: &Path,
        audience: &str,
        inherited_audience: Option<&Vec<String>>,
        included: &mut Vec<ExportFile>,
        excluded: &mut Vec<ExcludedFile>,
        visited: &mut HashSet<PathBuf>,
    ) -> Result<bool> {
        // Avoid cycles
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if visited.contains(&canonical) {
            return Ok(false);
        }
        visited.insert(canonical);

        // Parse the file - handle files without frontmatter gracefully
        let parse_result = self.workspace.parse_index(path).await;

        // For files without frontmatter or with parse errors, use default frontmatter
        // and inherit parent's audience
        let (frontmatter, index_for_children) = match parse_result {
            Ok(index) => (index.frontmatter.clone(), Some(index)),
            Err(crate::error::DiaryxError::NoFrontmatter(_)) => {
                // File has no frontmatter - treat as a simple file that inherits parent's visibility
                (IndexFrontmatter::default(), None)
            }
            Err(crate::error::DiaryxError::YamlParse { path, message }) => {
                // Log the YAML parse error but continue with default frontmatter
                log::warn!(
                    "[Export] Skipping file with YAML parse error: {} - {}",
                    path.display(),
                    message
                );
                (IndexFrontmatter::default(), None)
            }
            Err(crate::error::DiaryxError::Yaml(e)) => {
                // Legacy YAML error without path context - log and continue
                log::warn!("[Export] Skipping file with YAML error: {}", e);
                (IndexFrontmatter::default(), None)
            }
            Err(e) => return Err(e),
        };

        // Determine visibility
        let (is_visible, effective_audience) =
            self.check_visibility(&frontmatter, audience, inherited_audience);

        if !is_visible {
            // Record exclusion reason
            let reason = self.get_exclusion_reason(&frontmatter, audience, inherited_audience);
            excluded.push(ExcludedFile {
                path: path.to_path_buf(),
                reason,
            });
            return Ok(false);
        }

        // Calculate relative and destination paths
        let relative_path =
            pathdiff::diff_paths(path, root_dir).unwrap_or_else(|| path.to_path_buf());
        let dest_path = dest_dir.join(&relative_path);

        // If this is an index file, process children and track which will be filtered
        let mut filtered_contents = Vec::new();

        if frontmatter.is_index()
            && let Some(ref index) = index_for_children
        {
            let child_audience = effective_audience.as_ref().or(inherited_audience);

            for child_path_str in frontmatter.contents_list() {
                let child_path = index.resolve_path(child_path_str);

                // Make path absolute if needed by joining with root_dir
                let absolute_child_path = if child_path.is_absolute() {
                    child_path.clone()
                } else {
                    root_dir.join(&child_path)
                };

                if self.workspace.fs_ref().exists(&absolute_child_path).await {
                    let child_included = Box::pin(self.plan_file_recursive(
                        &absolute_child_path,
                        root_dir,
                        dest_dir,
                        audience,
                        child_audience,
                        included,
                        excluded,
                        visited,
                    ))
                    .await?;

                    if !child_included {
                        filtered_contents.push(child_path_str.clone());
                    }
                }
            }
        }

        // Add this file to included list
        included.push(ExportFile {
            source_path: path.to_path_buf(),
            relative_path,
            dest_path,
            filtered_contents,
        });

        Ok(true)
    }

    /// Check if a file is visible to the given audience
    /// Returns (is_visible, effective_audience_for_children)
    fn check_visibility(
        &self,
        frontmatter: &IndexFrontmatter,
        audience: &str,
        inherited: Option<&Vec<String>>,
    ) -> (bool, Option<Vec<String>>) {
        // Check for explicit private - always excluded
        if frontmatter.is_private() {
            return (false, None);
        }

        // Special case: "*" means "all non-private files"
        if audience == "*" {
            // Return the file's explicit audience for inheritance, or None
            let effective_audience = frontmatter.audience.clone();
            return (true, effective_audience);
        }

        // Check explicit audience
        if let Some(file_audience) = &frontmatter.audience {
            let visible = file_audience
                .iter()
                .any(|a| a.eq_ignore_ascii_case(audience));
            return (visible, Some(file_audience.clone()));
        }

        // Inherit from parent
        if let Some(parent_audience) = inherited {
            let visible = parent_audience
                .iter()
                .any(|a| a.eq_ignore_ascii_case(audience));
            return (visible, None); // Don't override inherited audience
        }

        // No audience defined anywhere - default to private (not visible)
        (false, None)
    }

    /// Determine the reason a file was excluded
    fn get_exclusion_reason(
        &self,
        frontmatter: &IndexFrontmatter,
        audience: &str,
        inherited: Option<&Vec<String>>,
    ) -> ExclusionReason {
        if frontmatter.is_private() {
            return ExclusionReason::Private;
        }

        if let Some(file_audience) = &frontmatter.audience {
            return ExclusionReason::AudienceMismatch {
                file_audience: file_audience.clone(),
                requested: audience.to_string(),
            };
        }

        if inherited.is_some() {
            // Parent had audience but this file wasn't included
            // This shouldn't happen if parent was visible, so it must be inherited private
            return ExclusionReason::InheritedPrivate {
                from: PathBuf::from("parent"),
            };
        }

        ExclusionReason::NoAudienceDefined
    }

    /// Execute an export plan
    /// Only available on native platforms (not WASM) since it writes to the filesystem
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn execute_export(
        &self,
        plan: &ExportPlan,
        options: &ExportOptions,
    ) -> Result<ExportStats> {
        // Check if destination exists
        if self.workspace.fs_ref().exists(&plan.destination).await && !options.force {
            return Err(DiaryxError::WorkspaceAlreadyExists(
                plan.destination.clone(),
            ));
        }

        // Create destination directory
        self.workspace
            .fs_ref()
            .create_dir_all(&plan.destination)
            .await?;

        let mut stats = ExportStats::default();

        for export_file in &plan.included {
            // Create parent directories if needed
            if let Some(parent) = export_file.dest_path.parent() {
                self.workspace.fs_ref().create_dir_all(parent).await?;
            }

            // Read source file
            let content = self
                .workspace
                .fs_ref()
                .read_to_string(&export_file.source_path)
                .await
                .map_err(|e| DiaryxError::FileRead {
                    path: export_file.source_path.clone(),
                    source: e,
                })?;

            // Process content if needed (filter contents array)
            let processed_content = if !export_file.filtered_contents.is_empty() {
                self.filter_contents_in_file(&content, &export_file.filtered_contents, options)?
            } else if !options.keep_audience {
                self.remove_audience_property(&content)?
            } else {
                content
            };

            // Write to destination
            self.workspace
                .fs_ref()
                .write_file(&export_file.dest_path, &processed_content)
                .await?;
            stats.files_exported += 1;
        }

        stats.files_excluded = plan.excluded.len();
        Ok(stats)
    }

    /// Filter out excluded children from a file's contents array
    fn filter_contents_in_file(
        &self,
        content: &str,
        filtered: &[String],
        options: &ExportOptions,
    ) -> Result<String> {
        // Parse frontmatter
        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            return Ok(content.to_string());
        }

        let rest = &content[4..];
        let end_idx = rest
            .find("\n---\n")
            .or_else(|| rest.find("\n---\r\n"))
            .ok_or_else(|| DiaryxError::InvalidFrontmatter(PathBuf::from("export")))?;

        let frontmatter_str = &rest[..end_idx];
        let body = &rest[end_idx + 5..];

        // Parse as YAML
        let mut frontmatter: serde_yaml::Value = serde_yaml::from_str(frontmatter_str)?;

        // Filter contents array
        if let Some(contents) = frontmatter.get_mut("contents")
            && let Some(arr) = contents.as_sequence_mut()
        {
            arr.retain(|item| {
                if let Some(s) = item.as_str() {
                    !filtered.iter().any(|f| f == s)
                } else {
                    true
                }
            });
        }

        // Optionally remove audience property
        if !options.keep_audience
            && let Some(map) = frontmatter.as_mapping_mut()
        {
            map.remove(serde_yaml::Value::String("audience".to_string()));
        }

        // Reconstruct file
        let new_frontmatter = serde_yaml::to_string(&frontmatter)?;
        // Remove trailing newline from YAML output for cleaner formatting
        let new_frontmatter = new_frontmatter.trim_end();

        Ok(format!("---\n{}\n---\n{}", new_frontmatter, body))
    }

    /// Remove audience property from a file
    fn remove_audience_property(&self, content: &str) -> Result<String> {
        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            return Ok(content.to_string());
        }

        let rest = &content[4..];
        let end_idx = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n"));

        let Some(end_idx) = end_idx else {
            return Ok(content.to_string());
        };

        let frontmatter_str = &rest[..end_idx];
        let body = &rest[end_idx + 5..];

        // Parse as YAML
        let mut frontmatter: serde_yaml::Value = serde_yaml::from_str(frontmatter_str)?;

        // Remove audience property
        if let Some(map) = frontmatter.as_mapping_mut() {
            let had_audience = map
                .remove(serde_yaml::Value::String("audience".to_string()))
                .is_some();

            if !had_audience {
                // No audience property, return original
                return Ok(content.to_string());
            }
        }

        // Reconstruct file
        let new_frontmatter = serde_yaml::to_string(&frontmatter)?;
        let new_frontmatter = new_frontmatter.trim_end();

        Ok(format!("---\n{}\n---\n{}", new_frontmatter, body))
    }
}

/// Statistics from an export operation
#[derive(Debug, Clone, Default, Serialize)]
pub struct ExportStats {
    /// Number of files successfully exported
    pub files_exported: usize,
    /// Number of files excluded for some reason
    pub files_excluded: usize,
}

impl std::fmt::Display for ExportStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Exported {} files, excluded {} files",
            self.files_exported, self.files_excluded
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
    fn test_private_file_excluded() {
        let fs = make_test_fs();
        fs.write_file(
            Path::new("/workspace/README.md"),
            "---\ntitle: Root\ncontents:\n  - secret.md\naudience:\n  - family\n---\n\n# Root\n",
        )
        .unwrap();
        fs.write_file(
            Path::new("/workspace/secret.md"),
            "---\ntitle: Secret\npart_of: README.md\naudience:\n  - private\n---\n\n# Secret\n",
        )
        .unwrap();

        let async_fs: TestFs = SyncToAsyncFs::new(fs);
        let exporter = Exporter::new(async_fs);
        let plan = block_on_test(exporter.plan_export(
            Path::new("/workspace/README.md"),
            "family",
            Path::new("/export"),
        ))
        .unwrap();

        assert_eq!(plan.included.len(), 1);
        assert_eq!(plan.excluded.len(), 1);
        assert_eq!(plan.excluded[0].reason, ExclusionReason::Private);
    }

    #[test]
    fn test_audience_inheritance() {
        let fs = make_test_fs();
        fs.write_file(
            Path::new("/workspace/README.md"),
            "---\ntitle: Root\ncontents:\n  - child.md\naudience:\n  - family\n---\n\n# Root\n",
        )
        .unwrap();
        fs.write_file(
            Path::new("/workspace/child.md"),
            "---\ntitle: Child\npart_of: README.md\n---\n\n# Child inherits family audience\n",
        )
        .unwrap();

        let async_fs: TestFs = SyncToAsyncFs::new(fs);
        let exporter = Exporter::new(async_fs);
        let plan = block_on_test(exporter.plan_export(
            Path::new("/workspace/README.md"),
            "family",
            Path::new("/export"),
        ))
        .unwrap();

        // Both should be included - child inherits family audience
        assert_eq!(plan.included.len(), 2);
        assert_eq!(plan.excluded.len(), 0);
    }

    #[test]
    fn test_no_audience_defaults_to_private() {
        let fs = make_test_fs();
        fs.write_file(
            Path::new("/workspace/README.md"),
            "---\ntitle: Root\ncontents: []\n---\n\n# Root with no audience\n",
        )
        .unwrap();

        let async_fs: TestFs = SyncToAsyncFs::new(fs);
        let exporter = Exporter::new(async_fs);
        let plan = block_on_test(exporter.plan_export(
            Path::new("/workspace/README.md"),
            "family",
            Path::new("/export"),
        ))
        .unwrap();

        // Root has no audience, defaults to private
        assert_eq!(plan.included.len(), 0);
        assert_eq!(plan.excluded.len(), 1);
        assert_eq!(plan.excluded[0].reason, ExclusionReason::NoAudienceDefined);
    }

    #[test]
    fn test_filtered_contents_tracked() {
        let fs = make_test_fs();
        fs.write_file(
            Path::new("/workspace/README.md"),
            "---\ntitle: Root\ncontents:\n  - public.md\n  - private.md\naudience:\n  - family\n---\n\n# Root\n",
        )
        .unwrap();
        fs.write_file(
            Path::new("/workspace/public.md"),
            "---\ntitle: Public\npart_of: README.md\n---\n\n# Public\n",
        )
        .unwrap();
        fs.write_file(
            Path::new("/workspace/private.md"),
            "---\ntitle: Private\npart_of: README.md\naudience:\n  - private\n---\n\n# Private\n",
        )
        .unwrap();

        let async_fs: TestFs = SyncToAsyncFs::new(fs);
        let exporter = Exporter::new(async_fs);
        let plan = block_on_test(exporter.plan_export(
            Path::new("/workspace/README.md"),
            "family",
            Path::new("/export"),
        ))
        .unwrap();

        // Find the root in included files
        let root = plan
            .included
            .iter()
            .find(|f| f.source_path == Path::new("/workspace/README.md"))
            .unwrap();

        // Root should track that private.md was filtered
        assert!(root.filtered_contents.contains(&"private.md".to_string()));
    }
}
