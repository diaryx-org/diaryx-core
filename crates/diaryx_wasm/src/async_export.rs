//! Async export operations for WASM with native Promise support.
//!
//! This module provides async export operations that work directly with
//! `JsAsyncFileSystem`, returning native JavaScript Promises. This enables
//! proper async/await patterns in the web frontend without the need for
//! synchronous wrappers or `block_on`.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { JsAsyncFileSystem, DiaryxAsyncExport } from './wasm/diaryx_wasm.js';
//!
//! // Create filesystem with your storage backend callbacks
//! const fs = new JsAsyncFileSystem({ /* callbacks */ });
//!
//! // Create async export instance
//! const exporter = new DiaryxAsyncExport(fs);
//!
//! // All methods return native Promises
//! const plan = await exporter.plan('workspace', 'public');
//! const files = await exporter.toMemory('workspace', 'public');
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use diaryx_core::export::Exporter;
use diaryx_core::fs::AsyncFileSystem;
use js_sys::Promise;
use serde::Serialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::error::IntoJsResult;
use crate::js_async_fs::JsAsyncFileSystem;

// ============================================================================
// Types
// ============================================================================

/// Export plan entry representing a file to be exported
#[derive(Debug, Serialize)]
pub struct JsAsyncExportPlanEntry {
    /// Source file path
    pub source: String,
    /// Relative path in workspace
    pub relative_path: String,
    /// Destination file path
    pub destination: String,
    /// Contents that will be filtered out
    pub filtered_contents: Vec<String>,
}

impl From<diaryx_core::export::ExportFile> for JsAsyncExportPlanEntry {
    fn from(entry: diaryx_core::export::ExportFile) -> Self {
        JsAsyncExportPlanEntry {
            source: entry.source_path.to_string_lossy().to_string(),
            relative_path: entry.relative_path.to_string_lossy().to_string(),
            destination: entry.dest_path.to_string_lossy().to_string(),
            filtered_contents: entry.filtered_contents,
        }
    }
}

/// Excluded file entry
#[derive(Debug, Serialize)]
pub struct JsAsyncExcludedFile {
    /// File path
    pub path: String,
    /// Reason for exclusion
    pub reason: String,
}

impl From<diaryx_core::export::ExcludedFile> for JsAsyncExcludedFile {
    fn from(entry: diaryx_core::export::ExcludedFile) -> Self {
        JsAsyncExcludedFile {
            path: entry.path.to_string_lossy().to_string(),
            reason: entry.reason.to_string(),
        }
    }
}

/// Export plan returned to JavaScript
#[derive(Debug, Serialize)]
pub struct JsAsyncExportPlan {
    /// Files to be exported
    pub included: Vec<JsAsyncExportPlanEntry>,
    /// Files that were excluded
    pub excluded: Vec<JsAsyncExcludedFile>,
    /// Target audience
    pub audience: String,
    /// Total number of files to export
    pub total_files: usize,
    /// Total number of excluded files
    pub total_excluded: usize,
}

impl From<diaryx_core::export::ExportPlan> for JsAsyncExportPlan {
    fn from(plan: diaryx_core::export::ExportPlan) -> Self {
        let total_files = plan.included.len();
        let total_excluded = plan.excluded.len();
        JsAsyncExportPlan {
            included: plan.included.into_iter().map(JsAsyncExportPlanEntry::from).collect(),
            excluded: plan.excluded.into_iter().map(JsAsyncExcludedFile::from).collect(),
            audience: plan.audience,
            total_files,
            total_excluded,
        }
    }
}

/// Exported file content
#[derive(Debug, Serialize)]
pub struct JsAsyncExportedFile {
    /// File path
    pub path: String,
    /// File content
    pub content: String,
}

/// Exported binary file
#[derive(Debug, Serialize)]
pub struct JsAsyncBinaryExportFile {
    /// File path
    pub path: String,
    /// Binary content as array of bytes
    pub data: Vec<u8>,
}

/// HTML export result
#[derive(Debug, Serialize)]
pub struct JsAsyncHtmlExport {
    /// HTML files
    pub files: Vec<JsAsyncExportedFile>,
    /// Total number of files
    pub total_files: usize,
}

// ============================================================================
// DiaryxAsyncExport Class
// ============================================================================

/// Async export operations with native Promise support.
///
/// Unlike `DiaryxExport` which uses `block_on` internally, this class
/// returns true JavaScript Promises that can be properly awaited.
#[wasm_bindgen]
pub struct DiaryxAsyncExport {
    fs: JsAsyncFileSystem,
}

#[wasm_bindgen]
impl DiaryxAsyncExport {
    /// Create a new DiaryxAsyncExport with the provided filesystem.
    #[wasm_bindgen(constructor)]
    pub fn new(fs: JsAsyncFileSystem) -> Self {
        Self { fs }
    }

    /// Get available audiences from the workspace.
    ///
    /// Scans all files in the workspace and returns unique audience values
    /// found in frontmatter.
    ///
    /// @param root_path - Path to the workspace root
    /// @returns Promise resolving to array of audience names
    #[wasm_bindgen(js_name = "getAudiences")]
    pub fn get_audiences(&self, root_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let path = PathBuf::from(&root_path);

            // Find all markdown files and collect unique audiences
            let mut audiences = HashSet::new();
            
            let md_files = fs
                .list_md_files_recursive(&path)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            for file_path in md_files {
                let content = fs
                    .read_to_string(&file_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                if let Some(file_audiences) = get_audiences_from_frontmatter(&content) {
                    for audience in file_audiences {
                        audiences.insert(audience);
                    }
                }
            }

            let audiences_vec: Vec<String> = audiences.into_iter().collect();
            serde_wasm_bindgen::to_value(&audiences_vec).js_err()
        })
    }

    /// Plan an export operation.
    ///
    /// Returns a list of files that would be exported for the given audience.
    ///
    /// @param root_path - Path to the workspace root
    /// @param audience - Target audience to export for
    /// @returns Promise resolving to export plan
    #[wasm_bindgen]
    pub fn plan(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            // Plan export to a temporary destination (we won't actually use it)
            let dest = PathBuf::from("_export_temp");
            let plan = exporter
                .plan_export(&path, &audience, &dest)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_plan: JsAsyncExportPlan = plan.into();
            serde_wasm_bindgen::to_value(&js_plan).js_err()
        })
    }

    /// Export files to memory (returns file contents).
    ///
    /// Returns an array of files with their content, suitable for
    /// downloading or further processing.
    ///
    /// @param root_path - Path to the workspace root
    /// @param audience - Target audience to export for
    /// @returns Promise resolving to array of exported files
    #[wasm_bindgen(js_name = "toMemory")]
    pub fn to_memory(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            // Plan the export
            let dest = PathBuf::from("_export");
            let plan = exporter
                .plan_export(&path, &audience, &dest)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Read each file's content
            let mut exported_files = Vec::new();
            for entry in plan.included {
                let content = fs
                    .read_to_string(&entry.source_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                exported_files.push(JsAsyncExportedFile {
                    path: entry.relative_path.to_string_lossy().to_string(),
                    content,
                });
            }

            serde_wasm_bindgen::to_value(&exported_files).js_err()
        })
    }

    /// Export files to HTML format.
    ///
    /// Converts markdown files to HTML using comrak.
    ///
    /// @param root_path - Path to the workspace root
    /// @param audience - Target audience to export for
    /// @returns Promise resolving to array of HTML files
    #[wasm_bindgen(js_name = "toHtml")]
    pub fn to_html(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            // Plan the export
            let dest = PathBuf::from("_export");
            let plan = exporter
                .plan_export(&path, &audience, &dest)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Convert each file to HTML
            let mut html_files = Vec::new();
            for entry in plan.included {
                let content = fs
                    .read_to_string(&entry.source_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                // Extract body (remove frontmatter) and convert to HTML
                let body = extract_body(&content);
                let html = markdown_to_html(&body);

                // Change extension to .html
                let html_path = entry
                    .relative_path
                    .with_extension("html")
                    .to_string_lossy()
                    .to_string();

                html_files.push(JsAsyncExportedFile {
                    path: html_path,
                    content: html,
                });
            }

            let result = JsAsyncHtmlExport {
                total_files: html_files.len(),
                files: html_files,
            };

            serde_wasm_bindgen::to_value(&result).js_err()
        })
    }

    /// Export binary attachments for a given audience.
    ///
    /// Returns binary files (images, etc.) associated with entries
    /// that match the target audience.
    ///
    /// @param root_path - Path to the workspace root
    /// @param audience - Target audience to export for
    /// @returns Promise resolving to array of binary files
    #[wasm_bindgen(js_name = "binaryAttachments")]
    pub fn binary_attachments(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            // Plan the export to get the list of files
            let dest = PathBuf::from("_export");
            let plan = exporter
                .plan_export(&path, &audience, &dest)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Collect attachments from all exported files
            let mut binary_files = Vec::new();
            for entry in plan.included {
                // Read the file and check for attachments in frontmatter
                let content = fs
                    .read_to_string(&entry.source_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                if let Some(attachments) = get_attachments_from_frontmatter(&content) {
                    let source_dir = entry.source_path.parent().unwrap_or(&entry.source_path);

                    for attachment in attachments {
                        let attachment_path = source_dir.join(&attachment);
                        if fs.exists(&attachment_path).await {
                            match fs.read_binary(&attachment_path).await {
                                Ok(data) => {
                                    binary_files.push(JsAsyncBinaryExportFile {
                                        path: attachment,
                                        data,
                                    });
                                }
                                Err(_) => {
                                    // Skip files that can't be read
                                    continue;
                                }
                            }
                        }
                    }
                }
            }

            serde_wasm_bindgen::to_value(&binary_files).js_err()
        })
    }
}

impl Default for DiaryxAsyncExport {
    fn default() -> Self {
        Self {
            fs: JsAsyncFileSystem::new(JsValue::NULL),
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Extract body content from markdown (removes frontmatter)
fn extract_body(content: &str) -> String {
    if !content.starts_with("---") {
        return content.to_string();
    }

    // Find the closing ---
    if let Some(end_idx) = content[3..].find("\n---") {
        let after_frontmatter = &content[3 + end_idx + 4..];
        after_frontmatter.trim_start().to_string()
    } else {
        content.to_string()
    }
}

/// Convert markdown to HTML using comrak
fn markdown_to_html(markdown: &str) -> String {
    use comrak::{markdown_to_html as comrak_md_to_html, Options};
    
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    // Note: comrak uses `r#unsafe` because `unsafe` is a reserved word in Rust
    options.render.r#unsafe = true;

    comrak_md_to_html(markdown, &options)
}

/// Get audiences list from frontmatter
fn get_audiences_from_frontmatter(content: &str) -> Option<Vec<String>> {
    if !content.starts_with("---") {
        return None;
    }

    // Find the closing ---
    let end_idx = content[3..].find("\n---")?;
    let frontmatter = &content[3..3 + end_idx];

    // Parse YAML
    let yaml: serde_yaml::Value = serde_yaml::from_str(frontmatter).ok()?;
    let audience = yaml.get("audience")?;

    match audience {
        serde_yaml::Value::Sequence(seq) => {
            Some(
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
            )
        }
        serde_yaml::Value::String(s) => Some(vec![s.clone()]),
        _ => None,
    }
}

/// Get attachments list from frontmatter
fn get_attachments_from_frontmatter(content: &str) -> Option<Vec<String>> {
    if !content.starts_with("---") {
        return None;
    }

    // Find the closing ---
    let end_idx = content[3..].find("\n---")?;
    let frontmatter = &content[3..3 + end_idx];

    // Parse YAML
    let yaml: serde_yaml::Value = serde_yaml::from_str(frontmatter).ok()?;
    let attachments = yaml.get("attachments")?;

    match attachments {
        serde_yaml::Value::Sequence(seq) => {
            Some(
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
            )
        }
        _ => None,
    }
}

// ============================================================================
// TypeScript Type Definitions
// ============================================================================

#[wasm_bindgen(typescript_custom_section)]
const TS_APPEND_CONTENT: &'static str = r#"
/**
 * Export plan entry representing a file to be exported.
 */
export interface AsyncExportPlanEntry {
    /** Source file path */
    source: string;
    /** Relative path in workspace */
    relative_path: string;
    /** Destination file path */
    destination: string;
    /** Contents that will be filtered out */
    filtered_contents: string[];
}

/**
 * Excluded file entry.
 */
export interface AsyncExcludedFile {
    /** File path */
    path: string;
    /** Reason for exclusion */
    reason: string;
}

/**
 * Export plan containing files to be exported.
 */
export interface AsyncExportPlan {
    /** Files to be exported */
    included: AsyncExportPlanEntry[];
    /** Files that were excluded */
    excluded: AsyncExcludedFile[];
    /** Target audience */
    audience: string;
    /** Total number of files to export */
    total_files: number;
    /** Total number of excluded files */
    total_excluded: number;
}

/**
 * Exported file with content.
 */
export interface AsyncExportedFile {
    /** File path */
    path: string;
    /** File content */
    content: string;
}

/**
 * Exported binary file.
 */
export interface AsyncBinaryExportFile {
    /** File path */
    path: string;
    /** Binary content as byte array */
    data: number[];
}

/**
 * HTML export result.
 */
export interface AsyncHtmlExport {
    /** HTML files */
    files: AsyncExportedFile[];
    /** Total number of files */
    total_files: number;
}
"#;