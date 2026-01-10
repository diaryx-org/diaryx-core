//! Export operations for WASM.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use diaryx_core::export::Exporter;
use diaryx_core::fs::{AsyncFileSystem, FileSystem};
use diaryx_core::workspace::Workspace;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::error::IntoJsResult;
use crate::frontmatter::extract_body;
use crate::state::{block_on, with_async_fs, with_fs};

// ============================================================================
// Types
// ============================================================================

#[derive(Serialize)]
struct ExportPlanJs {
    included: Vec<IncludedFileJs>,
    excluded: Vec<ExcludedFileJs>,
    audience: String,
}

#[derive(Serialize)]
struct IncludedFileJs {
    path: String,
    relative_path: String,
}

#[derive(Serialize)]
struct ExcludedFileJs {
    path: String,
    reason: String,
}

#[derive(Serialize)]
struct ExportedFile {
    path: String,
    content: String,
}

#[derive(Serialize)]
struct BinaryExportFile {
    path: String,
    data: Vec<u8>,
}

// ============================================================================
// DiaryxExport Class
// ============================================================================

/// Export operations for exporting workspace content.
#[wasm_bindgen]
pub struct DiaryxExport;

#[wasm_bindgen]
impl DiaryxExport {
    /// Create a new DiaryxExport instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Get all available audience tags from the workspace.
    #[wasm_bindgen]
    pub fn get_audiences(&self, root_path: &str) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let ws = Workspace::new(fs);
            let mut audiences: HashSet<String> = HashSet::new();

            fn collect_audiences<FS: AsyncFileSystem>(
                ws: &Workspace<FS>,
                path: &Path,
                audiences: &mut HashSet<String>,
                visited: &mut HashSet<PathBuf>,
            ) {
                if visited.contains(path) {
                    return;
                }
                visited.insert(path.to_path_buf());

                if let Ok(index) = block_on(ws.parse_index(path)) {
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
                            if block_on(ws.fs_ref().exists(&child_path)) {
                                collect_audiences(ws, &child_path, audiences, visited);
                            }
                        }
                    }
                }
            }

            let mut visited = HashSet::new();
            collect_audiences(&ws, Path::new(root_path), &mut audiences, &mut visited);

            let mut result: Vec<String> = audiences.into_iter().collect();
            result.sort();

            serde_wasm_bindgen::to_value(&result).js_err()
        })
    }

    /// Plan an export operation.
    #[wasm_bindgen]
    pub fn plan(&self, root_path: &str, audience: &str) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            if audience == "*" {
                let ws = Workspace::new(fs);
                let mut included = Vec::new();
                let root = Path::new(root_path);
                let root_dir = root.parent().unwrap_or(root);

                fn collect_all<FS: AsyncFileSystem>(
                    ws: &Workspace<FS>,
                    path: &Path,
                    root_dir: &Path,
                    included: &mut Vec<IncludedFileJs>,
                    visited: &mut HashSet<PathBuf>,
                ) {
                    if visited.contains(path) {
                        return;
                    }
                    visited.insert(path.to_path_buf());

                    if let Ok(index) = block_on(ws.parse_index(path)) {
                        let relative_path = pathdiff::diff_paths(path, root_dir)
                            .unwrap_or_else(|| path.to_path_buf());

                        included.push(IncludedFileJs {
                            path: path.to_string_lossy().to_string(),
                            relative_path: relative_path.to_string_lossy().to_string(),
                        });

                        if index.frontmatter.is_index() {
                            for child_rel in index.frontmatter.contents_list() {
                                let child_path = index.resolve_path(child_rel);
                                if block_on(ws.fs_ref().exists(&child_path)) {
                                    collect_all(ws, &child_path, root_dir, included, visited);
                                }
                            }
                        }
                    }
                }

                let mut visited = HashSet::new();
                collect_all(&ws, root, root_dir, &mut included, &mut visited);

                let result = ExportPlanJs {
                    included,
                    excluded: vec![],
                    audience: "*".to_string(),
                };

                return serde_wasm_bindgen::to_value(&result).js_err();
            }

            let exporter = Exporter::new(fs);

            let plan = block_on(exporter.plan_export(
                Path::new(root_path),
                audience,
                Path::new("/export"),
            ))
            .js_err()?;

            let result = ExportPlanJs {
                included: plan
                    .included
                    .iter()
                    .map(|f| IncludedFileJs {
                        path: f.source_path.to_string_lossy().to_string(),
                        relative_path: f.relative_path.to_string_lossy().to_string(),
                    })
                    .collect(),
                excluded: plan
                    .excluded
                    .iter()
                    .map(|f| ExcludedFileJs {
                        path: f.path.to_string_lossy().to_string(),
                        reason: f.reason.to_string(),
                    })
                    .collect(),
                audience: plan.audience,
            };

            serde_wasm_bindgen::to_value(&result).js_err()
        })
    }

    /// Export files to memory as markdown.
    #[wasm_bindgen]
    pub fn to_memory(&self, root_path: &str, audience: &str) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            if audience == "*" {
                let ws = Workspace::new(fs);
                let mut files: Vec<ExportedFile> = Vec::new();
                let root = Path::new(root_path);
                let root_dir = root.parent().unwrap_or(root);

                fn collect_all<FS: AsyncFileSystem>(
                    ws: &Workspace<FS>,
                    path: &Path,
                    root_dir: &Path,
                    files: &mut Vec<ExportedFile>,
                    visited: &mut HashSet<PathBuf>,
                ) {
                    if visited.contains(path) {
                        return;
                    }
                    visited.insert(path.to_path_buf());

                    if let Ok(index) = block_on(ws.parse_index(path)) {
                        let relative_path = pathdiff::diff_paths(path, root_dir)
                            .unwrap_or_else(|| path.to_path_buf());

                        if let Ok(content) = block_on(ws.fs_ref().read_to_string(path)) {
                            let processed = remove_audience_from_content(&content);
                            files.push(ExportedFile {
                                path: relative_path.to_string_lossy().to_string(),
                                content: processed,
                            });
                        }

                        if index.frontmatter.is_index() {
                            for child_rel in index.frontmatter.contents_list() {
                                let child_path = index.resolve_path(child_rel);
                                if block_on(ws.fs_ref().exists(&child_path)) {
                                    collect_all(ws, &child_path, root_dir, files, visited);
                                }
                            }
                        }
                    }
                }

                let mut visited = HashSet::new();
                collect_all(&ws, root, root_dir, &mut files, &mut visited);

                return serde_wasm_bindgen::to_value(&files).js_err();
            }

            let exporter = Exporter::new(fs.clone());

            let plan = block_on(exporter.plan_export(
                Path::new(root_path),
                audience,
                Path::new("/export"),
            ))
            .js_err()?;

            let mut files: Vec<ExportedFile> = Vec::new();

            for export_file in &plan.included {
                let content =
                    block_on(fs.clone().read_to_string(&export_file.source_path)).js_err()?;

                let processed = if !export_file.filtered_contents.is_empty() {
                    filter_contents_and_audience(&content, &export_file.filtered_contents)
                } else {
                    remove_audience_from_content(&content)
                };

                files.push(ExportedFile {
                    path: export_file.relative_path.to_string_lossy().to_string(),
                    content: processed,
                });
            }

            serde_wasm_bindgen::to_value(&files).js_err()
        })
    }

    /// Export files to memory as HTML.
    #[wasm_bindgen]
    pub fn to_html(&self, root_path: &str, audience: &str) -> Result<JsValue, JsValue> {
        use comrak::{Options, markdown_to_html};

        with_async_fs(|fs| {
            fn convert_md_to_html(markdown: &str) -> String {
                let mut options = Options::default();
                options.extension.strikethrough = true;
                options.extension.table = true;
                options.extension.autolink = true;
                options.extension.tasklist = true;
                options.render.r#unsafe = true;

                let html_body = markdown_to_html(markdown, &options);

                format!(
                    r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; max-width: 800px; margin: 0 auto; padding: 2rem; line-height: 1.6; }}
        pre {{ background: #f4f4f4; padding: 1rem; overflow-x: auto; }}
        code {{ background: #f4f4f4; padding: 0.2rem 0.4rem; }}
        img {{ max-width: 100%; }}
        table {{ border-collapse: collapse; width: 100%; }}
        th, td {{ border: 1px solid #ddd; padding: 0.5rem; text-align: left; }}
    </style>
</head>
<body>
{}
</body>
</html>"#,
                    html_body
                )
            }

            if audience == "*" {
                let ws = Workspace::new(fs);
                let mut files: Vec<ExportedFile> = Vec::new();
                let root = Path::new(root_path);
                let root_dir = root.parent().unwrap_or(root);

                fn collect_all<FS: AsyncFileSystem>(
                    ws: &Workspace<FS>,
                    path: &Path,
                    root_dir: &Path,
                    files: &mut Vec<ExportedFile>,
                    visited: &mut HashSet<PathBuf>,
                    convert_fn: &dyn Fn(&str) -> String,
                ) {
                    if visited.contains(path) {
                        return;
                    }
                    visited.insert(path.to_path_buf());

                    if let Ok(index) = block_on(ws.parse_index(path)) {
                        let relative_path = pathdiff::diff_paths(path, root_dir)
                            .unwrap_or_else(|| path.to_path_buf());

                        if let Ok(content) = block_on(ws.fs_ref().read_to_string(path)) {
                            let body = extract_body(&content);
                            let html = convert_fn(&body);
                            let html_path = relative_path.to_string_lossy().replace(".md", ".html");

                            files.push(ExportedFile {
                                path: html_path,
                                content: html,
                            });
                        }

                        if index.frontmatter.is_index() {
                            for child_rel in index.frontmatter.contents_list() {
                                let child_path = index.resolve_path(child_rel);
                                if block_on(ws.fs_ref().exists(&child_path)) {
                                    collect_all(
                                        ws,
                                        &child_path,
                                        root_dir,
                                        files,
                                        visited,
                                        convert_fn,
                                    );
                                }
                            }
                        }
                    }
                }

                let mut visited = HashSet::new();
                collect_all(
                    &ws,
                    root,
                    root_dir,
                    &mut files,
                    &mut visited,
                    &convert_md_to_html,
                );

                return serde_wasm_bindgen::to_value(&files).js_err();
            }

            let exporter = Exporter::new(fs.clone());

            let plan = block_on(exporter.plan_export(
                Path::new(root_path),
                audience,
                Path::new("/export"),
            ))
            .js_err()?;

            let mut files: Vec<ExportedFile> = Vec::new();

            for export_file in &plan.included {
                let content =
                    block_on(fs.clone().read_to_string(&export_file.source_path)).js_err()?;
                let body = extract_body(&content);
                let html = convert_md_to_html(&body);
                let html_path = export_file
                    .relative_path
                    .to_string_lossy()
                    .replace(".md", ".html");

                files.push(ExportedFile {
                    path: html_path,
                    content: html,
                });
            }

            serde_wasm_bindgen::to_value(&files).js_err()
        })
    }

    /// Export binary attachment files.
    #[wasm_bindgen]
    pub fn binary_attachments(&self, root_path: &str, _audience: &str) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let ws = Workspace::new(fs);
            let root = Path::new(root_path);
            let root_dir = root.parent().unwrap_or(root);
            let mut binary_files: Vec<BinaryExportFile> = Vec::new();
            let mut visited_entries: HashSet<PathBuf> = HashSet::new();
            let mut visited_attachment_dirs: HashSet<PathBuf> = HashSet::new();

            fn collect_attachments<FS: AsyncFileSystem>(
                ws: &Workspace<FS>,
                entry_path: &Path,
                root_dir: &Path,
                binary_files: &mut Vec<BinaryExportFile>,
                visited_entries: &mut HashSet<PathBuf>,
                visited_attachment_dirs: &mut HashSet<PathBuf>,
            ) {
                if visited_entries.contains(entry_path) {
                    return;
                }
                visited_entries.insert(entry_path.to_path_buf());

                if let Ok(index) = block_on(ws.parse_index(entry_path)) {
                    let entry_dir = entry_path.parent().unwrap_or(Path::new("."));
                    let attachments_dir = entry_dir.join("_attachments");

                    if block_on(ws.fs_ref().is_dir(&attachments_dir))
                        && !visited_attachment_dirs.contains(&attachments_dir)
                    {
                        visited_attachment_dirs.insert(attachments_dir.clone());

                        if let Ok(files) = block_on(ws.fs_ref().list_files(&attachments_dir)) {
                            for file_path in files {
                                if !block_on(ws.fs_ref().is_dir(&file_path))
                                    && let Ok(data) = block_on(ws.fs_ref().read_binary(&file_path))
                                {
                                    let relative_path = pathdiff::diff_paths(&file_path, root_dir)
                                        .unwrap_or_else(|| file_path.clone());

                                    binary_files.push(BinaryExportFile {
                                        path: relative_path.to_string_lossy().to_string(),
                                        data,
                                    });
                                }
                            }
                        }
                    }

                    if index.frontmatter.is_index() {
                        for child_rel in index.frontmatter.contents_list() {
                            let child_path = index.resolve_path(child_rel);
                            if block_on(ws.fs_ref().exists(&child_path)) {
                                collect_attachments(
                                    ws,
                                    &child_path,
                                    root_dir,
                                    binary_files,
                                    visited_entries,
                                    visited_attachment_dirs,
                                );
                            }
                        }
                    }
                }
            }

            collect_attachments(
                &ws,
                root,
                root_dir,
                &mut binary_files,
                &mut visited_entries,
                &mut visited_attachment_dirs,
            );

            serde_wasm_bindgen::to_value(&binary_files).js_err()
        })
    }
}

impl Default for DiaryxExport {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Remove audience property from content.
pub fn remove_audience_from_content(content: &str) -> String {
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        return content.to_string();
    }

    let rest = &content[4..];
    let Some(end_idx) = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n")) else {
        return content.to_string();
    };

    let frontmatter_str = &rest[..end_idx];
    let body = &rest[end_idx + 5..];

    if let Ok(mut frontmatter) = serde_yaml::from_str::<serde_yaml::Value>(frontmatter_str)
        && let Some(map) = frontmatter.as_mapping_mut()
        && map
            .remove(serde_yaml::Value::String("audience".to_string()))
            .is_some()
        && let Ok(new_fm) = serde_yaml::to_string(&frontmatter)
    {
        return format!("---\n{}---\n{}", new_fm, body);
    }

    content.to_string()
}

/// Filter contents array and remove audience.
pub fn filter_contents_and_audience(content: &str, filtered: &[String]) -> String {
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        return content.to_string();
    }

    let rest = &content[4..];
    let Some(end_idx) = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n")) else {
        return content.to_string();
    };

    let frontmatter_str = &rest[..end_idx];
    let body = &rest[end_idx + 5..];

    if let Ok(mut frontmatter) = serde_yaml::from_str::<serde_yaml::Value>(frontmatter_str) {
        if let Some(map) = frontmatter.as_mapping_mut() {
            map.remove(serde_yaml::Value::String("audience".to_string()));

            if let Some(contents) = map.get_mut(serde_yaml::Value::String("contents".to_string()))
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
        }

        if let Ok(new_fm) = serde_yaml::to_string(&frontmatter) {
            return format!("---\n{}---\n{}", new_fm, body);
        }
    }

    content.to_string()
}
