//! Command execution handler.
//!
//! This module contains the implementation of the `execute()` method for `Diaryx`.
//! It handles all command types and returns appropriate responses.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde_yaml::Value;

use crate::command::{Command, EntryData, Response};
use crate::diaryx::{Diaryx, json_to_yaml, yaml_to_json};
use crate::error::{DiaryxError, Result};
use crate::frontmatter;
use crate::fs::AsyncFileSystem;

impl<FS: AsyncFileSystem + Clone> Diaryx<FS> {
    /// Execute a command and return the response.
    ///
    /// This is the unified command interface that replaces individual method calls.
    /// All commands are async and return a `Result<Response>`.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use diaryx_core::{Command, Response, Diaryx};
    ///
    /// let cmd = Command::GetEntry { path: "notes/hello.md".to_string() };
    /// let response = diaryx.execute(cmd).await?;
    ///
    /// if let Response::Entry(entry) = response {
    ///     println!("Title: {:?}", entry.title);
    /// }
    /// ```
    pub async fn execute(&self, command: Command) -> Result<Response> {
        match command {
            // === Entry Operations ===
            Command::GetEntry { path } => {
                let content = self.entry().read_raw(&path).await?;
                let parsed = frontmatter::parse_or_empty(&content)?;
                let title = parsed
                    .frontmatter
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                // Convert serde_yaml::Value to serde_json::Value
                let fm: IndexMap<String, serde_json::Value> = parsed
                    .frontmatter
                    .into_iter()
                    .map(|(k, v)| (k, yaml_to_json(v)))
                    .collect();

                Ok(Response::Entry(EntryData {
                    path: PathBuf::from(&path),
                    title,
                    frontmatter: fm,
                    content: parsed.body,
                }))
            }

            Command::SaveEntry { path, content } => {
                self.entry().save_content(&path, &content).await?;
                Ok(Response::Ok)
            }

            Command::GetFrontmatter { path } => {
                let fm = self.entry().get_frontmatter(&path).await?;
                let json_fm: IndexMap<String, serde_json::Value> =
                    fm.into_iter().map(|(k, v)| (k, yaml_to_json(v))).collect();
                Ok(Response::Frontmatter(json_fm))
            }

            Command::SetFrontmatterProperty { path, key, value } => {
                let yaml_value = json_to_yaml(value);
                self.entry()
                    .set_frontmatter_property(&path, &key, yaml_value)
                    .await?;
                Ok(Response::Ok)
            }

            Command::RemoveFrontmatterProperty { path, key } => {
                self.entry()
                    .remove_frontmatter_property(&path, &key)
                    .await?;
                Ok(Response::Ok)
            }

            // === Workspace Operations ===
            Command::FindRootIndex { directory } => {
                let ws = self.workspace().inner();
                match ws.find_root_index_in_dir(Path::new(&directory)).await? {
                    Some(path) => Ok(Response::String(path.to_string_lossy().to_string())),
                    None => Err(DiaryxError::WorkspaceNotFound(PathBuf::from(&directory))),
                }
            }

            Command::GetWorkspaceTree { path, depth } => {
                let root_path = path.unwrap_or_else(|| "workspace/index.md".to_string());
                let tree = self
                    .workspace()
                    .inner()
                    .build_tree_with_depth(
                        Path::new(&root_path),
                        depth.map(|d| d as usize),
                        &mut std::collections::HashSet::new(),
                    )
                    .await?;
                Ok(Response::Tree(tree))
            }

            Command::GetFilesystemTree {
                path,
                show_hidden,
                depth,
            } => {
                let root_path = path.unwrap_or_else(|| "workspace".to_string());
                let tree = self
                    .workspace()
                    .inner()
                    .build_filesystem_tree_with_depth(
                        Path::new(&root_path),
                        show_hidden,
                        depth.map(|d| d as usize),
                    )
                    .await?;
                Ok(Response::Tree(tree))
            }

            // === Validation Operations ===
            Command::ValidateWorkspace { path } => {
                let root_path = path.unwrap_or_else(|| "workspace/index.md".to_string());
                // Use depth limit of 2 to match tree view (TREE_INITIAL_DEPTH in App.svelte)
                // This significantly improves performance for large workspaces
                let result = self
                    .validate()
                    .validate_workspace(Path::new(&root_path), Some(2))
                    .await?;
                // Include computed metadata for frontend display
                Ok(Response::ValidationResult(result.with_metadata()))
            }

            Command::ValidateFile { path } => {
                let result = self.validate().validate_file(Path::new(&path)).await?;
                // Include computed metadata for frontend display
                Ok(Response::ValidationResult(result.with_metadata()))
            }

            Command::FixBrokenPartOf { path } => {
                let result = self
                    .validate()
                    .fixer()
                    .fix_broken_part_of(Path::new(&path))
                    .await;
                Ok(Response::FixResult(result))
            }

            Command::FixBrokenContentsRef { index_path, target } => {
                let result = self
                    .validate()
                    .fixer()
                    .fix_broken_contents_ref(Path::new(&index_path), &target)
                    .await;
                Ok(Response::FixResult(result))
            }

            // === Search Operations ===
            Command::SearchWorkspace { pattern, options } => {
                use crate::search::SearchQuery;

                let query = if options.search_frontmatter {
                    if let Some(prop) = options.property {
                        SearchQuery::property(&pattern, prop)
                    } else {
                        SearchQuery::frontmatter(&pattern)
                    }
                } else {
                    SearchQuery::content(&pattern)
                }
                .case_sensitive(options.case_sensitive);

                let workspace_path = options
                    .workspace_path
                    .unwrap_or_else(|| "workspace/index.md".to_string());
                let results = self
                    .search()
                    .search_workspace(Path::new(&workspace_path), &query)
                    .await?;
                Ok(Response::SearchResults(results))
            }

            // === Export Operations ===
            Command::PlanExport {
                root_path,
                audience,
            } => {
                let plan = self
                    .export()
                    .plan_export(Path::new(&root_path), &audience, Path::new("/tmp/export"))
                    .await?;
                Ok(Response::ExportPlan(plan))
            }

            // === File System Operations ===
            Command::FileExists { path } => {
                let exists = self.fs().exists(Path::new(&path)).await;
                Ok(Response::Bool(exists))
            }

            Command::ReadFile { path } => {
                let content = self.entry().read_raw(&path).await?;
                Ok(Response::String(content))
            }

            Command::WriteFile { path, content } => {
                self.fs()
                    .write_file(Path::new(&path), &content)
                    .await
                    .map_err(|e| DiaryxError::FileWrite {
                        path: PathBuf::from(&path),
                        source: e,
                    })?;
                Ok(Response::Ok)
            }

            Command::DeleteFile { path } => {
                self.fs().delete_file(Path::new(&path)).await.map_err(|e| {
                    DiaryxError::FileWrite {
                        path: PathBuf::from(&path),
                        source: e,
                    }
                })?;
                Ok(Response::Ok)
            }

            Command::WriteFileWithMetadata {
                path,
                metadata,
                body,
            } => {
                crate::metadata_writer::write_file_with_metadata(
                    self.fs(),
                    Path::new(&path),
                    &metadata,
                    &body,
                )
                .await?;
                Ok(Response::Ok)
            }

            Command::UpdateFileMetadata {
                path,
                metadata,
                body,
            } => {
                crate::metadata_writer::update_file_metadata(
                    self.fs(),
                    Path::new(&path),
                    &metadata,
                    body.as_deref(),
                )
                .await?;
                Ok(Response::Ok)
            }

            // === Attachment Operations ===
            Command::GetAttachments { path } => {
                let attachments = self.entry().get_attachments(&path).await?;
                Ok(Response::Strings(attachments))
            }

            Command::GetAncestorAttachments { path } => {
                use crate::command::{AncestorAttachmentEntry, AncestorAttachmentsResult};
                use std::collections::HashSet;

                let ws = self.workspace().inner();
                let mut entries = Vec::new();
                let mut visited = HashSet::new();
                let mut current_path = PathBuf::from(&path);

                // Maximum depth to prevent runaway traversal
                const MAX_DEPTH: usize = 100;

                // Traverse up the part_of chain
                for _ in 0..MAX_DEPTH {
                    let path_str = current_path.to_string_lossy().to_string();
                    if visited.contains(&path_str) {
                        break; // Circular reference protection
                    }
                    visited.insert(path_str.clone());

                    // Try to parse the file
                    if let Ok(index) = ws.parse_index(&current_path).await {
                        let attachments = index.frontmatter.attachments_list().to_vec();

                        // Only add if there are attachments
                        if !attachments.is_empty() {
                            entries.push(AncestorAttachmentEntry {
                                entry_path: path_str,
                                entry_title: index.frontmatter.title.clone(),
                                attachments,
                            });
                        }

                        // Move to parent via part_of
                        if let Some(part_of) = &index.frontmatter.part_of {
                            current_path = index.resolve_path(part_of);
                        } else {
                            break; // Reached root
                        }
                    } else {
                        break; // File doesn't exist or can't be parsed
                    }
                }

                Ok(Response::AncestorAttachments(AncestorAttachmentsResult {
                    entries,
                }))
            }

            // === Entry Creation/Deletion Operations ===
            Command::CreateEntry { path, options } => {
                // Derive title from filename if not provided
                let path_buf = PathBuf::from(&path);
                let title = options.title.unwrap_or_else(|| {
                    path_buf
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Untitled")
                        .to_string()
                });

                // Create the file with basic frontmatter
                let content = format!("---\ntitle: {}\n---\n\n# {}\n\n", title, title);
                self.fs()
                    .create_new(Path::new(&path), &content)
                    .await
                    .map_err(|e| DiaryxError::FileWrite {
                        path: path_buf.clone(),
                        source: e,
                    })?;

                // Set part_of if provided
                if let Some(parent) = options.part_of {
                    self.entry()
                        .set_frontmatter_property(&path, "part_of", Value::String(parent))
                        .await?;
                }

                Ok(Response::String(path))
            }

            Command::DeleteEntry { path } => {
                // Use Workspace::delete_entry which handles contents cleanup
                let ws = self.workspace().inner();
                ws.delete_entry(Path::new(&path)).await?;
                Ok(Response::Ok)
            }

            Command::MoveEntry { from, to } => {
                if from == to {
                    return Ok(Response::String(to));
                }

                // Use Workspace::move_entry which handles contents/part_of updates
                let ws = self.workspace().inner();
                ws.move_entry(Path::new(&from), Path::new(&to)).await?;
                Ok(Response::String(to))
            }

            Command::RenameEntry { path, new_filename } => {
                let from_path = PathBuf::from(&path);
                let parent_dir = from_path.parent().unwrap_or_else(|| Path::new("."));
                let to_path = parent_dir.join(&new_filename);

                if from_path == to_path {
                    return Ok(Response::String(to_path.to_string_lossy().to_string()));
                }

                // Use move_entry logic for consistency
                let ws = self.workspace().inner();
                ws.move_entry(&from_path, &to_path).await?;
                Ok(Response::String(to_path.to_string_lossy().to_string()))
            }

            Command::DuplicateEntry { path } => {
                let ws = self.workspace().inner();
                let new_path = ws.duplicate_entry(Path::new(&path)).await?;
                Ok(Response::String(new_path.to_string_lossy().to_string()))
            }

            // === Hierarchy Operations ===
            Command::ConvertToIndex { path } => {
                let fm = self.entry().get_frontmatter(&path).await?;

                // Check if already has contents
                if fm.contains_key("contents") {
                    return Ok(Response::String(path));
                }

                // Add empty contents array
                self.entry()
                    .set_frontmatter_property(&path, "contents", Value::Sequence(vec![]))
                    .await?;
                Ok(Response::String(path))
            }

            Command::ConvertToLeaf { path } => {
                // Remove contents property if it exists
                self.entry()
                    .remove_frontmatter_property(&path, "contents")
                    .await?;
                Ok(Response::String(path))
            }

            Command::CreateChildEntry { parent_path } => {
                let ws = self.workspace().inner();
                let new_path = ws.create_child_entry(Path::new(&parent_path), None).await?;
                Ok(Response::String(new_path.to_string_lossy().to_string()))
            }

            Command::AttachEntryToParent {
                entry_path,
                parent_path,
            } => {
                let ws = self.workspace().inner();
                let new_path = ws
                    .attach_and_move_entry_to_parent(
                        Path::new(&entry_path),
                        Path::new(&parent_path),
                    )
                    .await?;
                Ok(Response::String(new_path.to_string_lossy().to_string()))
            }

            Command::EnsureDailyEntry {
                workspace_path,
                daily_entry_folder,
                template,
            } => {
                use crate::config::Config;
                use chrono::Local;

                // workspace_path is the root index file (e.g., "workspace/README.md")
                let workspace_root_path = PathBuf::from(&workspace_path);
                let workspace_dir = workspace_root_path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| workspace_root_path.clone());

                let config = Config::with_options(
                    workspace_dir.clone(),
                    daily_entry_folder.clone(),
                    None,             // editor
                    None,             // default_template
                    template.clone(), // daily_template
                );

                // Get today's date
                let today = Local::now().date_naive();

                // Ensure index hierarchy exists FIRST - this finds/creates the correct month_dir
                // which may be named "01", "january", etc. depending on existing structure
                let (month_dir, month_index_path) = self
                    .ensure_daily_index_hierarchy(
                        &today,
                        &config,
                        &workspace_root_path,
                        daily_entry_folder.as_deref(),
                    )
                    .await?;

                // Construct entry path using the actual month_dir found/created
                let date_str = today.format("%Y-%m-%d").to_string();
                let entry_filename = format!("{}.md", date_str);
                let entry_path = month_dir.join(&entry_filename);

                // Check if the entry already exists
                if self.fs().exists(&entry_path).await {
                    return Ok(Response::String(entry_path.to_string_lossy().to_string()));
                }
                // No need to create_dir_all - month_dir already exists from ensure_daily_index_hierarchy

                // Get template content
                let templates_dir = workspace_dir.join("_templates");
                let template_name = template.as_deref().unwrap_or("daily");
                let template_path = templates_dir.join(format!("{}.md", template_name));

                let template_content = if self.fs().exists(&template_path).await {
                    self.fs().read_to_string(&template_path).await.ok()
                } else {
                    None
                };

                // Build context for template variables
                let title = today.format("%B %d, %Y").to_string(); // e.g., "January 15, 2026"
                // Extract the actual month index filename from the path found/created by ensure_daily_index_hierarchy
                let month_index_filename = month_index_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("month_index.md")
                    .to_string();

                // Render content (substitute template variables)
                let content = if let Some(tmpl) = template_content {
                    tmpl.replace("{{title}}", &title)
                        .replace("{{date}}", &date_str)
                        .replace("{{part_of}}", &month_index_filename)
                } else {
                    // Built-in daily template
                    format!(
                        "---\ntitle: \"{}\"\npart_of: {}\ncreated: {}\n---\n\n## Today\n\n",
                        title, month_index_filename, date_str
                    )
                };

                // Create the file
                self.fs()
                    .create_new(&entry_path, &content)
                    .await
                    .map_err(|e| DiaryxError::FileWrite {
                        path: entry_path.clone(),
                        source: e,
                    })?;

                // Add entry to month index contents (use the month_index_path from ensure_daily_index_hierarchy)
                self.add_to_index_contents(&month_index_path, &entry_filename)
                    .await?;

                Ok(Response::String(entry_path.to_string_lossy().to_string()))
            }

            Command::GetAdjacentDailyEntry { path, direction } => {
                use crate::date::get_adjacent_daily_entry_path;

                let offset = match direction.as_str() {
                    "prev" | "previous" | "-1" => -1,
                    "next" | "1" => 1,
                    _ => {
                        return Err(DiaryxError::Unsupported(format!(
                            "Invalid direction '{}'. Use 'prev' or 'next'.",
                            direction
                        )));
                    }
                };

                let path_buf = PathBuf::from(&path);
                match get_adjacent_daily_entry_path(&path_buf, offset) {
                    Some(adjacent_path) => Ok(Response::String(
                        adjacent_path.to_string_lossy().to_string(),
                    )),
                    None => {
                        // Not a daily entry or couldn't compute adjacent path
                        Err(DiaryxError::Unsupported(
                            "Path is not a daily entry or adjacent date cannot be computed."
                                .to_string(),
                        ))
                    }
                }
            }

            Command::IsDailyEntry { path } => {
                use crate::date::is_daily_entry;

                let path_buf = PathBuf::from(&path);
                Ok(Response::Bool(is_daily_entry(&path_buf)))
            }

            // === Workspace Operations ===
            Command::CreateWorkspace { path, name } => {
                let ws_path = path.unwrap_or_else(|| "workspace".to_string());
                let ws_name = name.as_deref();
                let ws = self.workspace().inner();
                let readme_path = ws
                    .init_workspace(Path::new(&ws_path), ws_name, None)
                    .await?;
                Ok(Response::String(readme_path.to_string_lossy().to_string()))
            }

            // === Validation Fix Operations ===
            Command::FixBrokenAttachment { path, attachment } => {
                let result = self
                    .validate()
                    .fixer()
                    .fix_broken_attachment(Path::new(&path), &attachment)
                    .await;
                Ok(Response::FixResult(result))
            }

            Command::FixNonPortablePath {
                path,
                property,
                old_value,
                new_value,
            } => {
                let result = self
                    .validate()
                    .fixer()
                    .fix_non_portable_path(Path::new(&path), &property, &old_value, &new_value)
                    .await;
                Ok(Response::FixResult(result))
            }

            Command::FixUnlistedFile {
                index_path,
                file_path,
            } => {
                let result = self
                    .validate()
                    .fixer()
                    .fix_unlisted_file(Path::new(&index_path), Path::new(&file_path))
                    .await;
                Ok(Response::FixResult(result))
            }

            Command::FixOrphanBinaryFile {
                index_path,
                file_path,
            } => {
                let result = self
                    .validate()
                    .fixer()
                    .fix_orphan_binary_file(Path::new(&index_path), Path::new(&file_path))
                    .await;
                Ok(Response::FixResult(result))
            }

            Command::FixMissingPartOf {
                file_path,
                index_path,
            } => {
                let result = self
                    .validate()
                    .fixer()
                    .fix_missing_part_of(Path::new(&file_path), Path::new(&index_path))
                    .await;
                Ok(Response::FixResult(result))
            }

            Command::FixAll { validation_result } => {
                let fixer = self.validate().fixer();
                let (error_fixes, warning_fixes) = fixer.fix_all(&validation_result).await;

                let total_fixed = error_fixes.iter().filter(|r| r.success).count()
                    + warning_fixes.iter().filter(|r| r.success).count();
                let total_failed = error_fixes.iter().filter(|r| !r.success).count()
                    + warning_fixes.iter().filter(|r| !r.success).count();

                Ok(Response::FixSummary(crate::command::FixSummary {
                    error_fixes,
                    warning_fixes,
                    total_fixed,
                    total_failed,
                }))
            }

            Command::FixCircularReference {
                file_path,
                part_of_value,
            } => {
                let result = self
                    .validate()
                    .fixer()
                    .fix_circular_reference(Path::new(&file_path), &part_of_value)
                    .await;
                Ok(Response::FixResult(result))
            }

            Command::GetAvailableParentIndexes {
                file_path,
                workspace_root,
            } => {
                // Find all index files between the file and the workspace root
                let ws = self.workspace().inner();
                let file = Path::new(&file_path);
                let root_index = Path::new(&workspace_root);
                let root_dir = root_index.parent().unwrap_or(root_index);

                let mut parents = Vec::new();

                // Start from the file's directory and walk up to the workspace root
                let file_dir = file.parent().unwrap_or(Path::new("."));
                let mut current = file_dir.to_path_buf();

                loop {
                    // Look for index files in this directory
                    if let Ok(files) = ws.fs_ref().list_files(&current).await {
                        for file_path in files {
                            // Check if it's a markdown file
                            if file_path.extension().is_some_and(|ext| ext == "md")
                                && !ws.fs_ref().is_dir(&file_path).await
                            {
                                // Try to parse and check if it has contents (is an index)
                                if let Ok(index) = ws.parse_index(&file_path).await
                                    && index.frontmatter.is_index()
                                {
                                    parents.push(file_path.to_string_lossy().to_string());
                                }
                            }
                        }
                    }

                    // Stop if we've reached or passed the workspace root
                    if current == root_dir || !current.starts_with(root_dir) {
                        break;
                    }

                    // Go up one level
                    match current.parent() {
                        Some(parent) if parent != current => {
                            current = parent.to_path_buf();
                        }
                        _ => break,
                    }
                }

                // Always include the workspace root if not already present
                let root_str = root_index.to_string_lossy().to_string();
                if !parents.contains(&root_str) && ws.fs_ref().exists(root_index).await {
                    parents.push(root_str);
                }

                // Sort for consistent ordering
                parents.sort();
                Ok(Response::Strings(parents))
            }

            // === Export Operations ===
            Command::GetAvailableAudiences { root_path } => {
                // Collect unique audience tags from workspace
                let ws = self.workspace().inner();
                let mut audiences = std::collections::HashSet::new();
                let mut visited = std::collections::HashSet::new();

                async fn collect_audiences<FS: AsyncFileSystem>(
                    ws: &crate::workspace::Workspace<FS>,
                    path: &Path,
                    audiences: &mut std::collections::HashSet<String>,
                    visited: &mut std::collections::HashSet<PathBuf>,
                ) {
                    if visited.contains(path) {
                        return;
                    }
                    visited.insert(path.to_path_buf());

                    if let Ok(index) = ws.parse_index(path).await {
                        if let Some(file_audiences) = &index.frontmatter.audience {
                            for a in file_audiences {
                                if a.to_lowercase() != "private" {
                                    audiences.insert(a.clone());
                                }
                            }
                        }

                        if index.frontmatter.is_index() {
                            for child_rel in index.frontmatter.contents_list() {
                                let child_path = index.resolve_path(child_rel);
                                if ws.fs_ref().exists(&child_path).await {
                                    Box::pin(collect_audiences(
                                        ws,
                                        &child_path,
                                        audiences,
                                        visited,
                                    ))
                                    .await;
                                }
                            }
                        }
                    }
                }

                collect_audiences(&ws, Path::new(&root_path), &mut audiences, &mut visited).await;
                let mut result: Vec<String> = audiences.into_iter().collect();
                result.sort();
                Ok(Response::Strings(result))
            }

            Command::ExportToMemory {
                root_path,
                audience,
            } => {
                // Plan the export first
                log::info!(
                    "[Export] ExportToMemory starting - root_path: {:?}, audience: {:?}",
                    root_path,
                    audience
                );
                let plan = self
                    .export()
                    .plan_export(Path::new(&root_path), &audience, Path::new("/tmp/export"))
                    .await?;

                log::info!(
                    "[Export] plan_export returned {} included files",
                    plan.included.len()
                );
                for included in &plan.included {
                    log::info!(
                        "[Export] planned file: {:?} -> {:?}",
                        included.source_path,
                        included.relative_path
                    );
                }

                // Read each included file
                let mut files = Vec::new();
                for included in &plan.included {
                    match self.fs().read_to_string(&included.source_path).await {
                        Ok(content) => {
                            log::info!(
                                "[Export] read success: {:?} ({} bytes)",
                                included.source_path,
                                content.len()
                            );
                            files.push(crate::command::ExportedFile {
                                path: included.relative_path.to_string_lossy().to_string(),
                                content,
                            });
                        }
                        Err(e) => {
                            log::warn!("[Export] read failed: {:?} - {}", included.source_path, e);
                        }
                    }
                }
                log::info!("[Export] ExportToMemory returning {} files", files.len());
                Ok(Response::ExportedFiles(files))
            }

            Command::ExportToHtml {
                root_path,
                audience,
            } => {
                // Plan the export first
                let plan = self
                    .export()
                    .plan_export(Path::new(&root_path), &audience, Path::new("/tmp/export"))
                    .await?;

                // Read each included file and convert path extension
                let mut files = Vec::new();
                for included in &plan.included {
                    if let Ok(content) = self.fs().read_to_string(&included.source_path).await {
                        let html_path = included
                            .relative_path
                            .to_string_lossy()
                            .replace(".md", ".html");
                        files.push(crate::command::ExportedFile {
                            path: html_path,
                            content, // TODO: Add markdown-to-HTML conversion
                        });
                    }
                }
                Ok(Response::ExportedFiles(files))
            }

            Command::ExportBinaryAttachments {
                root_path,
                audience: _,
            } => {
                // Collect all non-hidden binary files from workspace
                let root_index = Path::new(&root_path);
                let root_dir = root_index.parent().unwrap_or(root_index);

                log::info!(
                    "[Export] ExportBinaryAttachments starting - root_path: {:?}, root_dir: {:?}",
                    root_path,
                    root_dir
                );

                let mut attachments: Vec<crate::command::BinaryFileInfo> = Vec::new();
                let mut visited_dirs = std::collections::HashSet::new();

                // Helper to check if a file is a binary attachment (not markdown)
                fn is_binary_file(path: &Path) -> bool {
                    let ext = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.to_lowercase());

                    match ext.as_deref() {
                        // Text/markdown files - not binary
                        Some("md" | "txt" | "json" | "yaml" | "yml" | "toml") => false,
                        // Common binary formats
                        Some(
                            "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" | "bmp" | "pdf"
                            | "heic" | "heif" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx"
                            | "mp3" | "mp4" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "mov"
                            | "avi" | "mkv" | "webm" | "zip" | "tar" | "gz" | "rar" | "7z" | "ttf"
                            | "otf" | "woff" | "woff2" | "sqlite" | "db",
                        ) => true,
                        // Unknown extension - check if it looks like a text file
                        _ => false,
                    }
                }

                // Helper to check if a path component is hidden
                fn is_hidden_component(name: &str) -> bool {
                    name.starts_with('.')
                }

                // Recursively collect binary file paths from a directory (no data, for efficiency)
                async fn collect_binaries_recursive<FS: AsyncFileSystem>(
                    fs: &FS,
                    dir: &Path,
                    root_dir: &Path,
                    attachments: &mut Vec<crate::command::BinaryFileInfo>,
                    visited_dirs: &mut std::collections::HashSet<PathBuf>,
                ) {
                    if visited_dirs.contains(dir) {
                        log::debug!("[Export] skipping already visited dir: {:?}", dir);
                        return;
                    }
                    visited_dirs.insert(dir.to_path_buf());

                    // Skip hidden directories
                    if let Some(name) = dir.file_name().and_then(|n| n.to_str())
                        && is_hidden_component(name)
                    {
                        log::debug!("[Export] skipping hidden dir: {:?}", dir);
                        return;
                    }

                    log::info!("[Export] listing files in dir: {:?}", dir);
                    let entries = match fs.list_files(dir).await {
                        Ok(e) => {
                            log::info!(
                                "[Export] list_files returned {} entries for {:?}",
                                e.len(),
                                dir
                            );
                            e
                        }
                        Err(e) => {
                            log::warn!("[Export] list_files failed for {:?}: {}", dir, e);
                            return;
                        }
                    };

                    for entry_path in entries {
                        // Skip hidden files/dirs
                        if let Some(name) = entry_path.file_name().and_then(|n| n.to_str())
                            && is_hidden_component(name)
                        {
                            continue;
                        }

                        if fs.is_dir(&entry_path).await {
                            // Recurse into subdirectory
                            Box::pin(collect_binaries_recursive(
                                fs,
                                &entry_path,
                                root_dir,
                                attachments,
                                visited_dirs,
                            ))
                            .await;
                        } else if is_binary_file(&entry_path) {
                            // Just record the path, don't read data (for efficiency)
                            let relative_path = pathdiff::diff_paths(&entry_path, root_dir)
                                .unwrap_or_else(|| entry_path.clone());
                            log::debug!("[Export] found binary file: {:?}", entry_path);
                            attachments.push(crate::command::BinaryFileInfo {
                                source_path: entry_path.to_string_lossy().to_string(),
                                relative_path: relative_path.to_string_lossy().to_string(),
                            });
                        } else {
                            log::debug!("[Export] skipping non-binary file: {:?}", entry_path);
                        }
                    }
                }

                collect_binaries_recursive(
                    self.fs(),
                    root_dir,
                    root_dir,
                    &mut attachments,
                    &mut visited_dirs,
                )
                .await;

                log::info!(
                    "[Export] ExportBinaryAttachments returning {} attachment paths",
                    attachments.len()
                );
                Ok(Response::BinaryFilePaths(attachments))
            }

            // === Template Operations ===
            Command::ListTemplates { workspace_path } => {
                let templates_dir = PathBuf::from(workspace_path.as_deref().unwrap_or("workspace"))
                    .join("_templates");

                let mut templates = Vec::new();

                // Add built-in templates
                templates.push(crate::command::TemplateInfo {
                    name: "note".to_string(),
                    path: None,
                    source: "builtin".to_string(),
                });
                templates.push(crate::command::TemplateInfo {
                    name: "daily".to_string(),
                    path: None,
                    source: "builtin".to_string(),
                });

                // Add workspace templates
                if self.fs().is_dir(&templates_dir).await
                    && let Ok(files) = self.fs().list_files(&templates_dir).await
                {
                    for file_path in files {
                        if file_path.extension().is_some_and(|ext| ext == "md")
                            && let Some(name) = file_path.file_stem().and_then(|s| s.to_str())
                        {
                            templates.push(crate::command::TemplateInfo {
                                name: name.to_string(),
                                path: Some(file_path),
                                source: "workspace".to_string(),
                            });
                        }
                    }
                }

                Ok(Response::Templates(templates))
            }

            Command::GetTemplate {
                name,
                workspace_path,
            } => {
                let templates_dir = PathBuf::from(workspace_path.as_deref().unwrap_or("workspace"))
                    .join("_templates");
                let template_path = templates_dir.join(format!("{}.md", name));

                // Check workspace templates first
                if self.fs().exists(&template_path).await {
                    let content = self
                        .fs()
                        .read_to_string(&template_path)
                        .await
                        .map_err(|e| DiaryxError::FileRead {
                            path: template_path,
                            source: e,
                        })?;
                    return Ok(Response::String(content));
                }

                // Return built-in template
                let content = match name.as_str() {
                    "note" => "---\ntitle: \"{{title}}\"\ncreated: \"{{date}}\"\n---\n\n",
                    "daily" => {
                        "---\ntitle: \"{{title}}\"\ncreated: \"{{date}}\"\n---\n\n## Today\n\n"
                    }
                    _ => return Err(DiaryxError::TemplateNotFound(name)),
                };
                Ok(Response::String(content.to_string()))
            }

            Command::SaveTemplate {
                name,
                content,
                workspace_path,
            } => {
                let templates_dir = PathBuf::from(&workspace_path).join("_templates");
                self.fs().create_dir_all(&templates_dir).await?;

                let template_path = templates_dir.join(format!("{}.md", name));
                self.fs()
                    .write_file(&template_path, &content)
                    .await
                    .map_err(|e| DiaryxError::FileWrite {
                        path: template_path,
                        source: e,
                    })?;

                Ok(Response::Ok)
            }

            Command::DeleteTemplate {
                name,
                workspace_path,
            } => {
                let template_path = PathBuf::from(&workspace_path)
                    .join("_templates")
                    .join(format!("{}.md", name));

                self.fs().delete_file(&template_path).await.map_err(|e| {
                    DiaryxError::FileWrite {
                        path: template_path,
                        source: e,
                    }
                })?;

                Ok(Response::Ok)
            }

            // === Attachment Operations ===
            Command::UploadAttachment {
                entry_path,
                filename,
                data_base64,
            } => {
                use base64::{Engine as _, engine::general_purpose::STANDARD};

                let entry = PathBuf::from(&entry_path);
                let entry_dir = entry.parent().unwrap_or_else(|| Path::new("."));
                let attachments_dir = entry_dir.join("_attachments");

                // Create _attachments directory if needed
                self.fs().create_dir_all(&attachments_dir).await?;

                // Decode base64 data
                let data = STANDARD.decode(&data_base64).map_err(|e| {
                    DiaryxError::Unsupported(format!("Failed to decode base64: {}", e))
                })?;

                // Write file
                let dest_path = attachments_dir.join(&filename);
                self.fs()
                    .write_binary(&dest_path, &data)
                    .await
                    .map_err(|e| DiaryxError::FileWrite {
                        path: dest_path.clone(),
                        source: e,
                    })?;

                // Add to frontmatter attachments
                let attachment_rel_path = format!("_attachments/{}", filename);
                self.entry()
                    .add_attachment(&entry_path, &attachment_rel_path)
                    .await?;

                Ok(Response::String(attachment_rel_path))
            }

            Command::DeleteAttachment {
                entry_path,
                attachment_path,
            } => {
                let entry = PathBuf::from(&entry_path);
                let entry_dir = entry.parent().unwrap_or_else(|| Path::new("."));
                let full_path = entry_dir.join(&attachment_path);

                // Delete the file if it exists
                if self.fs().exists(&full_path).await {
                    self.fs().delete_file(&full_path).await.map_err(|e| {
                        DiaryxError::FileWrite {
                            path: full_path,
                            source: e,
                        }
                    })?;
                }

                // Remove from frontmatter
                self.entry()
                    .remove_attachment(&entry_path, &attachment_path)
                    .await?;

                Ok(Response::Ok)
            }

            Command::GetAttachmentData {
                entry_path,
                attachment_path,
            } => {
                use crate::utils::path::normalize_path;

                let entry = PathBuf::from(&entry_path);
                let entry_dir = entry.parent().unwrap_or_else(|| Path::new("."));
                // Normalize the path to handle .. components (important for inherited attachments)
                let full_path = normalize_path(&entry_dir.join(&attachment_path));

                let data =
                    self.fs()
                        .read_binary(&full_path)
                        .await
                        .map_err(|e| DiaryxError::FileRead {
                            path: full_path,
                            source: e,
                        })?;

                Ok(Response::Bytes(data))
            }

            Command::MoveAttachment {
                source_entry_path,
                target_entry_path,
                attachment_path,
                new_filename,
            } => {
                // Resolve source paths
                let source_entry = PathBuf::from(&source_entry_path);
                let source_dir = source_entry.parent().unwrap_or_else(|| Path::new("."));
                let source_attachment_path = source_dir.join(&attachment_path);

                // Get the original filename
                let original_filename = source_attachment_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(|| DiaryxError::InvalidPath {
                        path: source_attachment_path.clone(),
                        message: "Could not extract filename".to_string(),
                    })?;

                // Determine final filename (use new_filename if provided, otherwise original)
                let final_filename = new_filename.as_deref().unwrap_or(original_filename);

                // Resolve target paths
                let target_entry = PathBuf::from(&target_entry_path);
                let target_dir = target_entry.parent().unwrap_or_else(|| Path::new("."));
                let target_attachments_dir = target_dir.join("_attachments");
                let target_attachment_path = target_attachments_dir.join(final_filename);

                // Check for collision at destination
                if self.fs().exists(&target_attachment_path).await {
                    return Err(DiaryxError::InvalidPath {
                        path: target_attachment_path,
                        message: "File already exists at destination".to_string(),
                    });
                }

                // Read the source file data
                let data = self
                    .fs()
                    .read_binary(&source_attachment_path)
                    .await
                    .map_err(|e| DiaryxError::FileRead {
                        path: source_attachment_path.clone(),
                        source: e,
                    })?;

                // Create target _attachments directory if needed
                self.fs().create_dir_all(&target_attachments_dir).await?;

                // Write to target location
                self.fs()
                    .write_binary(&target_attachment_path, &data)
                    .await
                    .map_err(|e| DiaryxError::FileWrite {
                        path: target_attachment_path.clone(),
                        source: e,
                    })?;

                // Update frontmatter: remove from source, add to target
                self.entry()
                    .remove_attachment(&source_entry_path, &attachment_path)
                    .await?;
                let target_rel_path = format!("_attachments/{}", final_filename);
                self.entry()
                    .add_attachment(&target_entry_path, &target_rel_path)
                    .await?;

                // Delete the original file
                self.fs()
                    .delete_file(&source_attachment_path)
                    .await
                    .map_err(|e| DiaryxError::FileWrite {
                        path: source_attachment_path,
                        source: e,
                    })?;

                Ok(Response::String(target_rel_path))
            }

            // === Storage Operations ===
            Command::GetStorageUsage => {
                // This requires knowledge of the workspace path which we don't have
                // Return basic info - clients can calculate usage themselves
                Ok(Response::StorageInfo(crate::command::StorageInfo {
                    used: 0,
                    limit: None,
                    attachment_limit: None,
                }))
            }

            // === CRDT Initialization ===
            #[cfg(feature = "crdt")]
            Command::InitializeWorkspaceCrdt {
                workspace_path,
                audience,
            } => {
                use std::collections::HashSet;

                // Check CRDT is enabled first
                if self.crdt().is_none() {
                    return Err(DiaryxError::Unsupported(
                        "CRDT not enabled for this instance".to_string(),
                    ));
                }

                let ws = self.workspace().inner();

                // Find root index file
                let root_path = PathBuf::from(&workspace_path);
                let root_index = if root_path.extension().is_some_and(|ext| ext == "md") {
                    root_path.clone()
                } else {
                    ws.find_root_index_in_dir(&root_path)
                        .await?
                        .ok_or_else(|| DiaryxError::WorkspaceNotFound(root_path.clone()))?
                };

                // If audience is specified, use plan_export to get filtered file list
                let allowed_paths: Option<HashSet<PathBuf>> = if let Some(ref aud) = audience {
                    let plan = self
                        .export()
                        .plan_export(
                            &root_index,
                            aud,
                            Path::new("/tmp"), // Dummy destination, we just need included paths
                        )
                        .await?;
                    Some(
                        plan.included
                            .iter()
                            .map(|f| f.source_path.clone())
                            .collect(),
                    )
                } else {
                    None
                };

                // Build tree
                let tree = ws
                    .build_tree_with_depth(&root_index, None, &mut HashSet::new())
                    .await?;

                // Collect all files with their metadata using iterative tree walk
                let mut files_to_add: Vec<(String, crate::crdt::FileMetadata)> = Vec::new();

                // Use a stack for iterative tree traversal
                let mut stack: Vec<(&crate::workspace::TreeNode, Option<String>)> =
                    vec![(&tree, None)];

                // Get CRDT for reconciliation checks
                let crdt = self.crdt().unwrap(); // Safe - checked above

                // Track files updated from disk (file was newer than CRDT)
                let mut files_updated_from_disk: Vec<String> = Vec::new();

                while let Some((node, parent_path)) = stack.pop() {
                    let path_str = node.path.to_string_lossy().to_string();

                    // Skip files not in allowed set (if audience filtering is active)
                    if let Some(ref allowed) = allowed_paths {
                        if !allowed.contains(&node.path) {
                            log::debug!(
                                "[InitializeWorkspaceCrdt] Skipping {} (not in audience)",
                                path_str
                            );
                            continue;
                        }
                    }

                    // Get file modification time from filesystem
                    let file_mtime = self.fs().get_modified_time(&node.path).await;

                    // Get existing CRDT entry for reconciliation
                    let existing_crdt_entry = crdt.get_file(&path_str);

                    // Reconciliation logic: compare file mtime vs CRDT modified_at
                    // If CRDT has newer or equal timestamp, skip updating from file
                    if let Some(crdt_entry) = &existing_crdt_entry {
                        if !crdt_entry.deleted {
                            // If we have file mtime, compare timestamps
                            // If no file mtime available (OPFS/web), trust the CRDT if it has data
                            let should_keep_crdt = match file_mtime {
                                Some(fmtime) => crdt_entry.modified_at >= fmtime,
                                None => true, // No mtime available, trust existing CRDT entry
                            };

                            if should_keep_crdt {
                                log::debug!(
                                    "[InitializeWorkspaceCrdt] Keeping CRDT version for {} (CRDT: {}, file: {:?})",
                                    path_str,
                                    crdt_entry.modified_at,
                                    file_mtime
                                );
                                // Add children to stack to continue tree traversal
                                for child in node.children.iter().rev() {
                                    stack.push((child, Some(path_str.clone())));
                                }
                                continue;
                            }
                        }
                    }

                    // File is newer or no CRDT entry exists - read and update
                    let content = match self.entry().read_raw(&path_str).await {
                        Ok(c) => c,
                        Err(e) => {
                            log::warn!(
                                "[InitializeWorkspaceCrdt] Could not read {}: {:?}",
                                path_str,
                                e
                            );
                            continue;
                        }
                    };

                    // Parse frontmatter
                    let parsed = match crate::frontmatter::parse_or_empty(&content) {
                        Ok(p) => p,
                        Err(e) => {
                            log::warn!(
                                "[InitializeWorkspaceCrdt] Parse error for {}: {:?}",
                                path_str,
                                e
                            );
                            continue;
                        }
                    };

                    // Track if this was an update from disk (existing CRDT entry with older timestamp)
                    if existing_crdt_entry.is_some() {
                        files_updated_from_disk.push(path_str.clone());
                        log::info!(
                            "[InitializeWorkspaceCrdt] Updating {} from disk (file is newer)",
                            path_str
                        );
                    }

                    // Build FileMetadata
                    let title = parsed
                        .frontmatter
                        .get("title")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let contents: Option<Vec<String>> = parsed
                        .frontmatter
                        .get("contents")
                        .and_then(|v| v.as_sequence())
                        .map(|seq| {
                            seq.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        });

                    let file_audience: Option<Vec<String>> = parsed
                        .frontmatter
                        .get("audience")
                        .and_then(|v| v.as_sequence())
                        .map(|seq| {
                            seq.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        });

                    let description = parsed
                        .frontmatter
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let attachments_list: Vec<String> = parsed
                        .frontmatter
                        .get("attachments")
                        .and_then(|v| v.as_sequence())
                        .map(|seq| {
                            seq.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default();

                    let attachments: Vec<crate::crdt::BinaryRef> = attachments_list
                        .into_iter()
                        .map(|path| crate::crdt::BinaryRef {
                            path,
                            source: "local".to_string(),
                            hash: String::new(),
                            mime_type: String::new(),
                            size: 0,
                            uploaded_at: None,
                            deleted: false,
                        })
                        .collect();

                    // Build extra fields (everything not in core frontmatter + _body)
                    let mut extra: std::collections::HashMap<String, serde_json::Value> =
                        std::collections::HashMap::new();
                    for (key, value) in &parsed.frontmatter {
                        if ![
                            "title",
                            "part_of",
                            "contents",
                            "attachments",
                            "audience",
                            "description",
                        ]
                        .contains(&key.as_str())
                        {
                            if let Ok(json) = serde_json::to_value(value) {
                                extra.insert(key.clone(), json);
                            }
                        }
                    }
                    // Include body content in extra._body
                    if !parsed.body.is_empty() {
                        extra.insert("_body".to_string(), serde_json::Value::String(parsed.body));
                    }

                    // Use file mtime if available, otherwise current time
                    let modified_at =
                        file_mtime.unwrap_or_else(|| chrono::Utc::now().timestamp_millis());

                    let metadata = crate::crdt::FileMetadata {
                        title,
                        part_of: parent_path.clone(),
                        contents,
                        attachments,
                        deleted: false,
                        audience: file_audience,
                        description,
                        extra,
                        modified_at,
                    };

                    files_to_add.push((path_str.clone(), metadata));

                    // Add children to stack (in reverse order to process in correct order)
                    for child in node.children.iter().rev() {
                        stack.push((child, Some(path_str.clone())));
                    }
                }

                // Now populate CRDT
                let file_count = files_to_add.len();
                let updated_count = files_updated_from_disk.len();

                for (path, metadata) in files_to_add {
                    if let Err(e) = crdt.set_file(&path, metadata) {
                        log::warn!(
                            "[InitializeWorkspaceCrdt] Failed to set file {}: {:?}",
                            path,
                            e
                        );
                    }
                }

                // Save CRDT state
                crdt.save()?;

                let msg = if updated_count > 0 {
                    if audience.is_some() {
                        format!(
                            "{} files populated, {} updated from disk (audience filtered)",
                            file_count, updated_count
                        )
                    } else {
                        format!(
                            "{} files populated, {} updated from disk",
                            file_count, updated_count
                        )
                    }
                } else if audience.is_some() {
                    format!("{} files populated (audience filtered)", file_count)
                } else {
                    format!("{} files populated", file_count)
                };
                log::info!("[InitializeWorkspaceCrdt] {}", msg);

                Ok(Response::String(msg))
            }

            // === CRDT Operations ===
            #[cfg(feature = "crdt")]
            Command::GetSyncState { doc_name: _ } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                Ok(Response::Binary(crdt.get_state_vector()))
            }

            #[cfg(feature = "crdt")]
            Command::ApplyRemoteUpdate {
                doc_name: _,
                update,
            } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                let update_id = crdt.apply_update(&update, crate::crdt::UpdateOrigin::Remote)?;
                Ok(Response::UpdateId(update_id))
            }

            #[cfg(feature = "crdt")]
            Command::GetMissingUpdates {
                doc_name: _,
                remote_state_vector,
            } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                let update = crdt.get_missing_updates(&remote_state_vector)?;
                Ok(Response::Binary(update))
            }

            #[cfg(feature = "crdt")]
            Command::GetFullState { doc_name: _ } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                Ok(Response::Binary(crdt.get_full_state()))
            }

            #[cfg(feature = "crdt")]
            Command::GetHistory { doc_name, limit } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                let history_manager = crate::crdt::HistoryManager::new(crdt.storage().clone());
                let history = history_manager.get_history(&doc_name, limit)?;
                let entries: Vec<crate::command::CrdtHistoryEntry> = history
                    .into_iter()
                    .map(|u| crate::command::CrdtHistoryEntry {
                        update_id: u.update_id,
                        timestamp: u.timestamp,
                        origin: u.origin,
                        files_changed: u.files_changed,
                        device_id: u.device_id,
                        device_name: u.device_name,
                    })
                    .collect();
                Ok(Response::CrdtHistory(entries))
            }

            #[cfg(feature = "crdt")]
            Command::GetFileHistory { file_path, limit } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                let history_manager = crate::crdt::HistoryManager::new(crdt.storage().clone());
                let history = history_manager.get_file_history(&file_path, limit)?;
                let entries: Vec<crate::command::CrdtHistoryEntry> = history
                    .into_iter()
                    .map(|u| crate::command::CrdtHistoryEntry {
                        update_id: u.update_id,
                        timestamp: u.timestamp,
                        origin: u.origin,
                        files_changed: u.files_changed,
                        device_id: u.device_id,
                        device_name: u.device_name,
                    })
                    .collect();
                Ok(Response::CrdtHistory(entries))
            }

            #[cfg(feature = "crdt")]
            Command::RestoreVersion {
                doc_name,
                update_id,
            } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                let history_manager = crate::crdt::HistoryManager::new(crdt.storage().clone());
                let restore_update = history_manager.create_restore_update(&doc_name, update_id)?;
                crdt.apply_update(&restore_update, crate::crdt::UpdateOrigin::Local)?;
                crdt.save()?;
                Ok(Response::Ok)
            }

            #[cfg(feature = "crdt")]
            Command::GetVersionDiff {
                doc_name,
                from_id,
                to_id,
            } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                let history_manager = crate::crdt::HistoryManager::new(crdt.storage().clone());
                let diffs = history_manager.diff(&doc_name, from_id, to_id)?;
                Ok(Response::VersionDiff(diffs))
            }

            #[cfg(feature = "crdt")]
            Command::GetStateAt {
                doc_name,
                update_id,
            } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                let history_manager = crate::crdt::HistoryManager::new(crdt.storage().clone());
                let state = history_manager.get_state_at(&doc_name, update_id)?;
                match state {
                    Some(data) => Ok(Response::Binary(data)),
                    None => Ok(Response::Ok),
                }
            }

            #[cfg(feature = "crdt")]
            Command::GetCrdtFile { path } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                Ok(Response::CrdtFile(crdt.get_file(&path)))
            }

            #[cfg(feature = "crdt")]
            Command::SetCrdtFile { path, metadata } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                let file_metadata: crate::crdt::FileMetadata = serde_json::from_value(metadata)
                    .map_err(|e| DiaryxError::Unsupported(format!("Invalid metadata: {}", e)))?;
                crdt.set_file(&path, file_metadata)?;
                Ok(Response::Ok)
            }

            #[cfg(feature = "crdt")]
            Command::ListCrdtFiles { include_deleted } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                let files = if include_deleted {
                    crdt.list_files()
                } else {
                    crdt.list_active_files()
                };
                Ok(Response::CrdtFiles(files))
            }

            #[cfg(feature = "crdt")]
            Command::SaveCrdtState { doc_name: _ } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                crdt.save()?;
                Ok(Response::Ok)
            }

            // ==================== Body Document Commands ====================
            #[cfg(feature = "crdt")]
            Command::GetBodyContent { doc_name } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                match crdt.get_body_content(&doc_name) {
                    Some(content) => Ok(Response::String(content)),
                    None => Ok(Response::String(String::new())),
                }
            }

            #[cfg(feature = "crdt")]
            Command::SetBodyContent { doc_name, content } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                crdt.set_body_content(&doc_name, &content)?;
                Ok(Response::Ok)
            }

            #[cfg(feature = "crdt")]
            Command::GetBodySyncState { doc_name } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                match crdt.get_body_sync_state(&doc_name) {
                    Some(state) => Ok(Response::Binary(state)),
                    None => Ok(Response::Binary(Vec::new())),
                }
            }

            #[cfg(feature = "crdt")]
            Command::GetBodyFullState { doc_name } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                match crdt.get_body_full_state(&doc_name) {
                    Some(state) => Ok(Response::Binary(state)),
                    None => Ok(Response::Binary(Vec::new())),
                }
            }

            #[cfg(feature = "crdt")]
            Command::ApplyBodyUpdate { doc_name, update } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                let update_id =
                    crdt.apply_body_update(&doc_name, &update, crate::crdt::UpdateOrigin::Remote)?;
                Ok(Response::UpdateId(update_id))
            }

            #[cfg(feature = "crdt")]
            Command::GetBodyMissingUpdates {
                doc_name,
                remote_state_vector,
            } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                let diff = crdt.get_body_missing_updates(&doc_name, &remote_state_vector)?;
                Ok(Response::Binary(diff))
            }

            #[cfg(feature = "crdt")]
            Command::SaveBodyDoc { doc_name } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                crdt.save_body_doc(&doc_name)?;
                Ok(Response::Ok)
            }

            #[cfg(feature = "crdt")]
            Command::SaveAllBodyDocs => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                crdt.save_all_body_docs()?;
                Ok(Response::Ok)
            }

            #[cfg(feature = "crdt")]
            Command::ListLoadedBodyDocs => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                Ok(Response::Strings(crdt.loaded_body_docs()))
            }

            #[cfg(feature = "crdt")]
            Command::UnloadBodyDoc { doc_name } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                crdt.unload_body_doc(&doc_name);
                Ok(Response::Ok)
            }

            // ==================== Sync Protocol Commands ====================
            #[cfg(feature = "crdt")]
            Command::CreateSyncStep1 { doc_name } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                let message = crdt.create_sync_step1(&doc_name);
                Ok(Response::Binary(message))
            }

            #[cfg(feature = "crdt")]
            Command::HandleSyncMessage { doc_name, message } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                let response = crdt.handle_sync_message(&doc_name, &message)?;
                match response {
                    Some(data) => Ok(Response::Binary(data)),
                    None => Ok(Response::Ok),
                }
            }

            #[cfg(feature = "crdt")]
            Command::CreateUpdateMessage { doc_name, update } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                let message = crdt.create_update_message(&doc_name, &update);
                Ok(Response::Binary(message))
            }
        }
    }

    // ==================== Daily Entry Helper Methods ====================

    /// Ensure the daily index hierarchy exists for a given date.
    ///
    /// When `daily_entry_folder` is Some: Creates daily_index.md -> YYYY_index.md -> YYYY_month.md
    /// When `daily_entry_folder` is None: Adds YYYY_index.md directly to workspace root
    ///
    /// This function detects existing index files and directories with alternate naming conventions
    /// (e.g., `2026.md` vs `2026_index.md`, `01/` vs `january/`) to avoid creating duplicates.
    async fn ensure_daily_index_hierarchy(
        &self,
        date: &chrono::NaiveDate,
        config: &crate::config::Config,
        workspace_root_path: &Path,
        daily_entry_folder: Option<&str>,
    ) -> Result<(PathBuf, PathBuf)> {
        let daily_dir = config.daily_entry_dir();
        let year = date.format("%Y").to_string();

        // Find or create year directory (always named by year number)
        let year_dir = daily_dir.join(&year);
        self.fs().create_dir_all(&year_dir).await?;

        // Find or create year index - check for existing files with alternate names
        let year_index_path = self
            .find_or_create_year_index(&year_dir, date, workspace_root_path, daily_entry_folder)
            .await?;

        // Find or create month directory and index - check for existing with alternate names
        let (month_dir, month_index_path) = self
            .find_or_create_month_dir_and_index(&year_dir, date, &year_index_path)
            .await?;

        // Ensure the month directory exists
        self.fs().create_dir_all(&month_dir).await?;

        // Return the paths for the caller to use when creating the daily entry
        Ok((month_dir, month_index_path))
    }

    /// Find an existing year index or create one.
    /// Checks for common naming patterns: YYYY.md, YYYY_index.md
    /// Only considers files that are actual indexes (have `contents` property).
    async fn find_or_create_year_index(
        &self,
        year_dir: &Path,
        date: &chrono::NaiveDate,
        workspace_root_path: &Path,
        daily_entry_folder: Option<&str>,
    ) -> Result<PathBuf> {
        let year = date.format("%Y").to_string();
        let daily_dir = year_dir.parent().unwrap_or(year_dir);

        // Check for existing year index files (in order of preference)
        let candidates = [
            format!("{}.md", year),       // 2026.md (simpler, user-preferred)
            format!("{}_index.md", year), // 2026_index.md
        ];

        for candidate in &candidates {
            let path = year_dir.join(candidate);
            if self.is_index_file(&path).await {
                return Ok(path);
            }
        }

        // No existing index found - create one with the simpler naming
        let year_index_path = year_dir.join(format!("{}.md", year));
        let workspace_root_filename = workspace_root_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("README.md");

        if let Some(folder) = daily_entry_folder {
            // With daily_entry_folder: Ensure daily_index.md exists first
            let daily_index_path = daily_dir.join("daily_index.md");

            if !self.fs().exists(&daily_index_path).await {
                let part_of = format!("../{}", workspace_root_filename);
                self.create_daily_index(&daily_index_path, Some(&part_of))
                    .await?;

                // Add daily_index to workspace root's contents
                let daily_index_rel = format!("{}/daily_index.md", folder);
                self.add_to_index_contents(workspace_root_path, &daily_index_rel)
                    .await?;
            }

            // Create year index linking to daily_index
            self.create_year_index(&year_index_path, date, "../daily_index.md")
                .await?;
            let year_index_rel = format!("{}/{}.md", year, year);
            self.add_to_index_contents(&daily_index_path, &year_index_rel)
                .await?;
        } else {
            // Without daily_entry_folder: Link directly to workspace root
            let part_of = format!("../{}", workspace_root_filename);
            self.create_year_index(&year_index_path, date, &part_of)
                .await?;
            let year_index_rel = format!("{}/{}.md", year, year);
            self.add_to_index_contents(workspace_root_path, &year_index_rel)
                .await?;
        }

        Ok(year_index_path)
    }

    /// Find an existing month directory and index, or create them.
    /// Checks for common directory naming patterns: 01, january, 01-january
    /// Checks for common index naming patterns: YYYY_month.md, month.md, 01.md
    /// Only considers files that are actual indexes (have `contents` property).
    /// Returns (month_dir, month_index_path).
    async fn find_or_create_month_dir_and_index(
        &self,
        year_dir: &Path,
        date: &chrono::NaiveDate,
        year_index_path: &Path,
    ) -> Result<(PathBuf, PathBuf)> {
        let year = date.format("%Y").to_string();
        let month_name = date.format("%B").to_string().to_lowercase();
        let month_num = date.format("%m").to_string();

        // Check for existing month directories with valid indices
        // Only use a directory if it has a valid index file inside
        let dir_candidates = [
            month_num.clone(),                       // 01
            month_name.clone(),                      // january
            format!("{}-{}", month_num, month_name), // 01-january
        ];

        let index_candidates = [
            format!("{}_{}.md", year, month_name), // 2026_january.md
            format!("{}.md", month_name),          // january.md
            format!("{}.md", month_num),           // 01.md
        ];

        // Check directories AND their indices together
        for dir_name in &dir_candidates {
            let month_dir = year_dir.join(dir_name);
            if self.fs().exists(&month_dir).await {
                for index_name in &index_candidates {
                    let index_path = month_dir.join(index_name);
                    if self.is_index_file(&index_path).await {
                        return Ok((month_dir, index_path));
                    }
                }
            }
        }

        // Check for month index directly in year_dir (flat structure)
        // e.g., 2026/january.md instead of 2026/january/january.md
        for index_name in &index_candidates {
            let index_path = year_dir.join(index_name);
            if self.is_index_file(&index_path).await {
                // Flat index found - use numeric month dir for entries
                let month_dir = year_dir.join(&month_num);
                return Ok((month_dir, index_path));
            }
        }

        // No existing index found - create with numeric naming (consistent with date_to_path)
        let month_dir = year_dir.join(&month_num);
        let month_index_path = month_dir.join(format!("{}_{}.md", year, month_name));

        // Create the directory if it doesn't exist
        self.fs().create_dir_all(&month_dir).await?;

        // Create the index file
        self.create_month_index_with_parent(&month_index_path, date, year_index_path)
            .await?;

        // Add month index to year index contents
        let month_index_rel = format!("{}/{}_{}.md", month_num, year, month_name);
        self.add_to_index_contents(year_index_path, &month_index_rel)
            .await?;

        Ok((month_dir, month_index_path))
    }

    /// Check if a file exists and is an index file (has `contents` property in frontmatter).
    async fn is_index_file(&self, path: &Path) -> bool {
        if !self.fs().exists(path).await {
            return false;
        }

        // Read the file and check for contents property
        let content = match self.fs().read_to_string(path).await {
            Ok(c) => c,
            Err(_) => return false,
        };

        // Check if frontmatter contains "contents:"
        if content.starts_with("---\n") || content.starts_with("---\r\n") {
            // Find the end of frontmatter
            let rest = &content[4..];
            if let Some(end_idx) = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n")) {
                let frontmatter = &rest[..end_idx];
                return frontmatter.contains("contents:");
            }
        }

        false
    }

    /// Create the root daily index file.
    async fn create_daily_index(&self, path: &Path, part_of: Option<&str>) -> Result<()> {
        let part_of_line = match part_of {
            Some(p) => format!("part_of: {}\n", p),
            None => String::new(),
        };

        let content = format!(
            "---\n\
            title: Daily Entries\n\
            {}contents: []\n\
            ---\n\n\
            # Daily Entries\n\n\
            This index contains all daily journal entries organized by year and month.\n",
            part_of_line
        );

        self.fs()
            .write_file(path, &content)
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: path.to_path_buf(),
                source: e,
            })?;
        Ok(())
    }

    /// Create a year index file.
    async fn create_year_index(
        &self,
        path: &Path,
        date: &chrono::NaiveDate,
        part_of: &str,
    ) -> Result<()> {
        let year = date.format("%Y").to_string();
        let content = format!(
            "---\n\
            title: {year}\n\
            part_of: {part_of}\n\
            contents: []\n\
            ---\n\n\
            # {year}\n\n\
            Daily entries for {year}.\n"
        );

        self.fs()
            .write_file(path, &content)
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: path.to_path_buf(),
                source: e,
            })?;
        Ok(())
    }

    /// Create a month index file.
    async fn create_month_index_with_parent(
        &self,
        path: &Path,
        date: &chrono::NaiveDate,
        year_index_path: &Path,
    ) -> Result<()> {
        let year = date.format("%Y").to_string();
        let month_name = date.format("%B").to_string();
        let title = format!("{} {}", month_name, year);
        let fallback_name = format!("{}.md", year);
        let year_index_name = year_index_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&fallback_name);

        let content = format!(
            "---\n\
            title: {title}\n\
            part_of: ../{year_index_name}\n\
            contents: []\n\
            ---\n\n\
            # {title}\n\n\
            Daily entries for {title}.\n"
        );

        self.fs()
            .write_file(path, &content)
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: path.to_path_buf(),
                source: e,
            })?;
        Ok(())
    }

    /// Add an entry to an index's contents list.
    async fn add_to_index_contents(&self, index_path: &Path, entry: &str) -> Result<bool> {
        let content = match self.fs().read_to_string(index_path).await {
            Ok(c) => c,
            Err(_) => return Ok(false),
        };

        // Parse frontmatter
        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            return Ok(false);
        }

        let rest = &content[4..];
        let end_idx = match rest.find("\n---\n").or_else(|| rest.find("\n---\r\n")) {
            Some(idx) => idx,
            None => return Ok(false),
        };

        let frontmatter_str = &rest[..end_idx];
        let body = &rest[end_idx + 5..];

        // Parse YAML
        let mut frontmatter: indexmap::IndexMap<String, serde_yaml::Value> =
            serde_yaml::from_str(frontmatter_str).unwrap_or_default();

        // Get or create contents array
        let contents = frontmatter
            .entry("contents".to_string())
            .or_insert(serde_yaml::Value::Sequence(vec![]));

        if let serde_yaml::Value::Sequence(items) = contents {
            let entry_value = serde_yaml::Value::String(entry.to_string());
            if !items.contains(&entry_value) {
                items.push(entry_value);
                // Sort for consistent ordering
                items.sort_by(|a, b| {
                    let a_str = a.as_str().unwrap_or("");
                    let b_str = b.as_str().unwrap_or("");
                    a_str.cmp(b_str)
                });

                // Reconstruct file
                let yaml_str = serde_yaml::to_string(&frontmatter)?;
                let new_content = format!("---\n{}---\n{}", yaml_str, body);
                self.fs()
                    .write_file(index_path, &new_content)
                    .await
                    .map_err(|e| DiaryxError::FileWrite {
                        path: index_path.to_path_buf(),
                        source: e,
                    })?;
                return Ok(true);
            }
        }

        Ok(false)
    }
}
