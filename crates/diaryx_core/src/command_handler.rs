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

            Command::EnsureDailyEntry => {
                // This requires config which we don't have access to in the core
                // Return an error suggesting this should be handled at the Tauri level
                Err(DiaryxError::Unsupported(
                    "EnsureDailyEntry requires config which is platform-specific. Use Tauri command.".to_string()
                ))
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
                let plan = self
                    .export()
                    .plan_export(Path::new(&root_path), &audience, Path::new("/tmp/export"))
                    .await?;

                // Read each included file
                let mut files = Vec::new();
                for included in &plan.included {
                    if let Ok(content) = self.fs().read_to_string(&included.source_path).await {
                        files.push(crate::command::ExportedFile {
                            path: included.relative_path.to_string_lossy().to_string(),
                            content,
                        });
                    }
                }
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
                // Collect all binary attachments from workspace
                let ws = self.workspace().inner();
                let root_index = Path::new(&root_path);
                let root_dir = root_index.parent().unwrap_or(root_index);

                let mut attachments = Vec::new();
                let mut visited = std::collections::HashSet::new();

                async fn collect_attachments<FS: AsyncFileSystem>(
                    ws: &crate::workspace::Workspace<FS>,
                    path: &Path,
                    root_dir: &Path,
                    attachments: &mut Vec<crate::command::BinaryExportFile>,
                    visited: &mut std::collections::HashSet<PathBuf>,
                ) {
                    if visited.contains(path) {
                        return;
                    }
                    visited.insert(path.to_path_buf());

                    if let Ok(index) = ws.parse_index(path).await {
                        // Check for _attachments folder
                        if let Some(entry_dir) = path.parent() {
                            let attachments_dir = entry_dir.join("_attachments");
                            if ws.fs_ref().is_dir(&attachments_dir).await
                                && let Ok(entries) = ws.fs_ref().list_files(&attachments_dir).await
                            {
                                for entry_path in entries {
                                    if !ws.fs_ref().is_dir(&entry_path).await
                                        && let Ok(data) = ws.fs_ref().read_binary(&entry_path).await
                                    {
                                        let relative_path =
                                            pathdiff::diff_paths(&entry_path, root_dir)
                                                .unwrap_or_else(|| entry_path.clone());
                                        attachments.push(crate::command::BinaryExportFile {
                                            path: relative_path.to_string_lossy().to_string(),
                                            data,
                                        });
                                    }
                                }
                            }
                        }

                        // Recurse into children
                        if index.frontmatter.is_index() {
                            for child_rel in index.frontmatter.contents_list() {
                                let child_path = index.resolve_path(child_rel);
                                if ws.fs_ref().exists(&child_path).await {
                                    Box::pin(collect_attachments(
                                        ws,
                                        &child_path,
                                        root_dir,
                                        attachments,
                                        visited,
                                    ))
                                    .await;
                                }
                            }
                        }
                    }
                }

                collect_attachments(&ws, root_index, root_dir, &mut attachments, &mut visited)
                    .await;
                Ok(Response::BinaryFiles(attachments))
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
            Command::GetHistory { doc_name: _, limit } => {
                let crdt = self.crdt().ok_or_else(|| {
                    DiaryxError::Unsupported("CRDT not enabled for this instance".to_string())
                })?;
                let history = crdt.get_history()?;
                let entries: Vec<crate::command::CrdtHistoryEntry> = history
                    .into_iter()
                    .take(limit.unwrap_or(usize::MAX))
                    .map(|u| crate::command::CrdtHistoryEntry {
                        update_id: u.update_id,
                        timestamp: u.timestamp,
                        origin: u.origin.to_string(),
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
}
