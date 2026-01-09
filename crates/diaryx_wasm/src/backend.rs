//! Unified async backend for WASM with native OPFS/IndexedDB storage.
//!
//! This module provides a single entry point for all workspace operations,
//! working directly with native storage backends (no InMemoryFileSystem).
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { DiaryxBackend } from './wasm/diaryx_wasm.js';
//!
//! // Create backend with OPFS storage
//! const backend = await DiaryxBackend.createOpfs();
//!
//! // Or with IndexedDB fallback
//! const backend = await DiaryxBackend.createIndexedDb();
//!
//! // All operations are async
//! const tree = await backend.getTree('workspace');
//! const entry = await backend.getEntry('workspace/journal/2024-01-08.md');
//! ```

use std::collections::HashSet;
use std::io::Result as IoResult;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use diaryx_core::fs::AsyncFileSystem;
use diaryx_core::search::{Searcher, SearchQuery, SearchMatch};
use diaryx_core::validate::Validator;
use diaryx_core::workspace::Workspace;
use diaryx_core::frontmatter;
use js_sys::Promise;
use serde::Serialize;
use serde_wasm_bindgen;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::error::IntoJsResult;
use crate::indexeddb_fs::IndexedDbFileSystem;
use crate::opfs_fs::OpfsFileSystem;

// ============================================================================
// Types
// ============================================================================

/// Tree node returned to JavaScript
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JsTreeNode {
    pub name: String,
    pub description: Option<String>,
    pub path: String,
    pub children: Vec<JsTreeNode>,
}

impl From<diaryx_core::workspace::TreeNode> for JsTreeNode {
    fn from(node: diaryx_core::workspace::TreeNode) -> Self {
        JsTreeNode {
            name: node.name,
            description: node.description,
            path: node.path.to_string_lossy().to_string(),
            children: node.children.into_iter().map(JsTreeNode::from).collect(),
        }
    }
}

/// Search result item
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JsSearchResult {
    pub path: String,
    pub title: Option<String>,
    pub matches: Vec<JsSearchMatch>,
}

/// Single search match
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JsSearchMatch {
    pub line_number: usize,
    pub line_content: String,
    pub match_start: usize,
    pub match_end: usize,
}

// ============================================================================
// Storage Backend Enum
// ============================================================================

/// Internal enum to hold either storage backend
enum StorageBackend {
    Opfs(OpfsFileSystem),
    IndexedDb(IndexedDbFileSystem),
}

impl Clone for StorageBackend {
    fn clone(&self) -> Self {
        match self {
            StorageBackend::Opfs(fs) => StorageBackend::Opfs(fs.clone()),
            StorageBackend::IndexedDb(fs) => StorageBackend::IndexedDb(fs.clone()),
        }
    }
}

// Implement AsyncFileSystem by delegating to inner type
impl AsyncFileSystem for StorageBackend {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> diaryx_core::fs::BoxFuture<'a, IoResult<String>> {
        match self {
            StorageBackend::Opfs(fs) => fs.read_to_string(path),
            StorageBackend::IndexedDb(fs) => fs.read_to_string(path),
        }
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> diaryx_core::fs::BoxFuture<'a, IoResult<()>> {
        match self {
            StorageBackend::Opfs(fs) => fs.write_file(path, content),
            StorageBackend::IndexedDb(fs) => fs.write_file(path, content),
        }
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> diaryx_core::fs::BoxFuture<'a, IoResult<()>> {
        match self {
            StorageBackend::Opfs(fs) => fs.create_new(path, content),
            StorageBackend::IndexedDb(fs) => fs.create_new(path, content),
        }
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> diaryx_core::fs::BoxFuture<'a, IoResult<()>> {
        match self {
            StorageBackend::Opfs(fs) => fs.delete_file(path),
            StorageBackend::IndexedDb(fs) => fs.delete_file(path),
        }
    }

    fn list_md_files<'a>(&'a self, dir: &'a Path) -> diaryx_core::fs::BoxFuture<'a, IoResult<Vec<PathBuf>>> {
        match self {
            StorageBackend::Opfs(fs) => fs.list_md_files(dir),
            StorageBackend::IndexedDb(fs) => fs.list_md_files(dir),
        }
    }

    fn exists<'a>(&'a self, path: &'a Path) -> diaryx_core::fs::BoxFuture<'a, bool> {
        match self {
            StorageBackend::Opfs(fs) => fs.exists(path),
            StorageBackend::IndexedDb(fs) => fs.exists(path),
        }
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> diaryx_core::fs::BoxFuture<'a, IoResult<()>> {
        match self {
            StorageBackend::Opfs(fs) => fs.create_dir_all(path),
            StorageBackend::IndexedDb(fs) => fs.create_dir_all(path),
        }
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> diaryx_core::fs::BoxFuture<'a, bool> {
        match self {
            StorageBackend::Opfs(fs) => fs.is_dir(path),
            StorageBackend::IndexedDb(fs) => fs.is_dir(path),
        }
    }

    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> diaryx_core::fs::BoxFuture<'a, IoResult<()>> {
        match self {
            StorageBackend::Opfs(fs) => fs.move_file(from, to),
            StorageBackend::IndexedDb(fs) => fs.move_file(from, to),
        }
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> diaryx_core::fs::BoxFuture<'a, IoResult<Vec<u8>>> {
        match self {
            StorageBackend::Opfs(fs) => fs.read_binary(path),
            StorageBackend::IndexedDb(fs) => fs.read_binary(path),
        }
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> diaryx_core::fs::BoxFuture<'a, IoResult<()>> {
        match self {
            StorageBackend::Opfs(fs) => fs.write_binary(path, content),
            StorageBackend::IndexedDb(fs) => fs.write_binary(path, content),
        }
    }

    fn list_files<'a>(&'a self, dir: &'a Path) -> diaryx_core::fs::BoxFuture<'a, IoResult<Vec<PathBuf>>> {
        match self {
            StorageBackend::Opfs(fs) => fs.list_files(dir),
            StorageBackend::IndexedDb(fs) => fs.list_files(dir),
        }
    }
}

// ============================================================================
// DiaryxBackend Class
// ============================================================================

/// Unified async backend with native storage.
///
/// This is the main entry point for all workspace operations in WASM.
/// It wraps either OPFS or IndexedDB storage and provides a complete
/// async API for workspace, entry, search, and validation operations.
#[wasm_bindgen]
pub struct DiaryxBackend {
    fs: Rc<StorageBackend>,
}

#[wasm_bindgen]
impl DiaryxBackend {
    /// Create a new DiaryxBackend with OPFS storage.
    #[wasm_bindgen(js_name = "createOpfs")]
    pub async fn create_opfs() -> std::result::Result<DiaryxBackend, JsValue> {
        let opfs = OpfsFileSystem::create().await?;
        let fs = Rc::new(StorageBackend::Opfs(opfs));
        Ok(Self { fs })
    }

    /// Create a new DiaryxBackend with IndexedDB storage.
    #[wasm_bindgen(js_name = "createIndexedDb")]
    pub async fn create_indexed_db() -> std::result::Result<DiaryxBackend, JsValue> {
        let idb = IndexedDbFileSystem::create().await?;
        let fs = Rc::new(StorageBackend::IndexedDb(idb));
        Ok(Self { fs })
    }

    /// Create backend with specific storage type.
    #[wasm_bindgen(js_name = "create")]
    pub async fn create(storage_type: &str) -> std::result::Result<DiaryxBackend, JsValue> {
        match storage_type.to_lowercase().as_str() {
            "opfs" => Self::create_opfs().await,
            "indexeddb" | "indexed_db" => Self::create_indexed_db().await,
            _ => Err(JsValue::from_str(&format!("Unknown storage type: {}", storage_type))),
        }
    }

    // ========================================================================
    // Config (stored as JSON in config/config.json)
    // ========================================================================

    /// Get the current configuration.
    #[wasm_bindgen(js_name = "getConfig")]
    pub fn get_config(&self) -> Promise {
        let fs = self.fs.clone();
        
        future_to_promise(async move {
            let config_path = PathBuf::from("config/config.json");
            match fs.read_to_string(&config_path).await {
                Ok(content) => {
                    // Parse JSON and return
                    js_sys::JSON::parse(&content)
                        .map_err(|e| JsValue::from_str(&format!("Invalid config JSON: {:?}", e)))
                }
                Err(_) => {
                    // Return default config
                    let default = r#"{"default_workspace":"workspace"}"#;
                    js_sys::JSON::parse(default)
                        .map_err(|e| JsValue::from_str(&format!("JSON error: {:?}", e)))
                }
            }
        })
    }

    /// Save configuration.
    #[wasm_bindgen(js_name = "saveConfig")]
    pub fn save_config(&self, config_js: JsValue) -> Promise {
        let fs = self.fs.clone();
        
        future_to_promise(async move {
            let config_str = js_sys::JSON::stringify(&config_js)
                .map_err(|e| JsValue::from_str(&format!("Failed to stringify config: {:?}", e)))?;
            
            let config_dir = PathBuf::from("config");
            let config_path = config_dir.join("config.json");
            
            fs.create_dir_all(&config_dir).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            
            fs.write_file(&config_path, &String::from(config_str)).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            
            Ok(JsValue::UNDEFINED)
        })
    }

    // ========================================================================
    // Workspace Operations
    // ========================================================================

    /// Get the workspace tree structure.
    #[wasm_bindgen(js_name = "getTree")]
    pub fn get_tree(&self, workspace_path: String, depth: Option<u32>) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&*fs);
            let root_path = PathBuf::from(&workspace_path);

            let root_index = ws.find_root_index_in_dir(&root_path).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let root_index = match root_index {
                Some(idx) => idx,
                None => ws.find_any_index_in_dir(&root_path).await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?
                    .ok_or_else(|| JsValue::from_str(&format!("No workspace found at '{}'", workspace_path)))?,
            };

            let max_depth = depth.map(|d| d as usize);
            let mut visited = HashSet::new();

            let tree = ws.build_tree_with_depth(&root_index, max_depth, &mut visited).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_tree: JsTreeNode = tree.into();
            serde_wasm_bindgen::to_value(&js_tree).js_err()
        })
    }

    /// Create a new workspace.
    #[wasm_bindgen(js_name = "createWorkspace")]
    pub fn create_workspace(&self, path: String, name: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let index_path = PathBuf::from(&path).join("index.md");

            if fs.exists(&index_path).await {
                return Err(JsValue::from_str(&format!("Workspace already exists at '{}'", path)));
            }

            let content = format!(
                "---\ntitle: \"{}\"\ncontents: []\n---\n\n# {}\n",
                name, name
            );

            fs.create_dir_all(&PathBuf::from(&path)).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            fs.write_file(&index_path, &content).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            Ok(JsValue::UNDEFINED)
        })
    }

    /// Get filesystem tree for "Show All Files" mode.
    #[wasm_bindgen(js_name = "getFilesystemTree")]
    pub fn get_filesystem_tree(&self, workspace_path: String, show_hidden: bool) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            async fn build_tree(
                fs: &StorageBackend,
                path: &Path,
                show_hidden: bool,
            ) -> Result<JsTreeNode, String> {
                let name = path.file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string());

                if !show_hidden && name.starts_with('.') {
                    return Err("hidden".to_string());
                }

                let mut children = Vec::new();

                if fs.is_dir(path).await {
                    if let Ok(entries) = fs.list_files(path).await {
                        for entry in entries {
                            if let Ok(child) = Box::pin(build_tree(fs, &entry, show_hidden)).await {
                                children.push(child);
                            }
                        }
                    }
                    children.sort_by(|a, b| {
                        let a_is_dir = !a.children.is_empty();
                        let b_is_dir = !b.children.is_empty();
                        match (a_is_dir, b_is_dir) {
                            (true, false) => std::cmp::Ordering::Less,
                            (false, true) => std::cmp::Ordering::Greater,
                            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                        }
                    });
                }

                Ok(JsTreeNode {
                    name,
                    description: None,
                    path: path.to_string_lossy().to_string(),
                    children,
                })
            }

            let root_path = PathBuf::from(&workspace_path);
            let tree = build_tree(&*fs, &root_path, show_hidden).await
                .map_err(|e| JsValue::from_str(&e))?;

            serde_wasm_bindgen::to_value(&tree).js_err()
        })
    }

    // ========================================================================
    // Entry Operations
    // ========================================================================

    /// Get an entry's content and frontmatter.
    #[wasm_bindgen(js_name = "getEntry")]
    pub fn get_entry(&self, path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let entry_path = PathBuf::from(&path);
            let content = fs.read_to_string(&entry_path).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Parse frontmatter
            let parsed = frontmatter::parse_or_empty(&content)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let frontmatter_js = serde_wasm_bindgen::to_value(&parsed.frontmatter)
                .unwrap_or(JsValue::NULL);

            // Return as JS object
            let result = js_sys::Object::new();
            js_sys::Reflect::set(&result, &"path".into(), &JsValue::from_str(&path))?;
            js_sys::Reflect::set(&result, &"content".into(), &JsValue::from_str(&parsed.body))?;
            js_sys::Reflect::set(&result, &"frontmatter".into(), &frontmatter_js)?;
            
            Ok(result.into())
        })
    }

    /// Save an entry's content.
    #[wasm_bindgen(js_name = "saveEntry")]
    pub fn save_entry(&self, path: String, content: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let entry_path = PathBuf::from(&path);
            fs.write_file(&entry_path, &content).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Create a new entry.
    #[wasm_bindgen(js_name = "createEntry")]
    pub fn create_entry(&self, path: String, title: Option<String>) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let entry_path = PathBuf::from(&path);
            
            if fs.exists(&entry_path).await {
                return Err(JsValue::from_str(&format!("File already exists: {}", path)));
            }

            if let Some(parent) = entry_path.parent() {
                fs.create_dir_all(parent).await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
            }

            let title = title.unwrap_or_else(|| {
                entry_path.file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Untitled".to_string())
            });

            let content = format!("---\ntitle: \"{}\"\n---\n\n", title);
            
            fs.write_file(&entry_path, &content).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            Ok(JsValue::from_str(&path))
        })
    }

    /// Delete an entry.
    #[wasm_bindgen(js_name = "deleteEntry")]
    pub fn delete_entry(&self, path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let entry_path = PathBuf::from(&path);
            fs.delete_file(&entry_path).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Move/rename an entry.
    #[wasm_bindgen(js_name = "moveEntry")]
    pub fn move_entry(&self, from_path: String, to_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let from = PathBuf::from(&from_path);
            let to = PathBuf::from(&to_path);

            if let Some(parent) = to.parent() {
                fs.create_dir_all(parent).await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
            }

            fs.move_file(&from, &to).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            Ok(JsValue::from_str(&to_path))
        })
    }

    // ========================================================================
    // Frontmatter Operations
    // ========================================================================

    /// Get frontmatter for an entry.
    #[wasm_bindgen(js_name = "getFrontmatter")]
    pub fn get_frontmatter(&self, path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let entry_path = PathBuf::from(&path);
            let content = fs.read_to_string(&entry_path).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let parsed = frontmatter::parse_or_empty(&content)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            serde_wasm_bindgen::to_value(&parsed.frontmatter).js_err()
        })
    }

    /// Set a frontmatter property.
    #[wasm_bindgen(js_name = "setFrontmatterProperty")]
    pub fn set_frontmatter_property(&self, path: String, key: String, value: JsValue) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let entry_path = PathBuf::from(&path);
            let content = fs.read_to_string(&entry_path).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let mut parsed = frontmatter::parse_or_empty(&content)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Convert JsValue to serde_yaml::Value
            let yaml_value: serde_yaml::Value = serde_wasm_bindgen::from_value(value)
                .map_err(|e| JsValue::from_str(&format!("Invalid value: {:?}", e)))?;

            parsed.frontmatter.insert(key, yaml_value);

            let new_content = frontmatter::serialize(&parsed.frontmatter, &parsed.body)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            
            fs.write_file(&entry_path, &new_content).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            Ok(JsValue::UNDEFINED)
        })
    }

    // ========================================================================
    // Search Operations
    // ========================================================================

    /// Search the workspace.
    #[wasm_bindgen(js_name = "search")]
    pub fn search(&self, workspace_path: String, query: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let searcher = Searcher::new(&*fs);
            let workspace_root = PathBuf::from(&workspace_path);
            let search_query = SearchQuery::content(&query);
            
            // Get all markdown files and search them
            let files = fs.list_md_files(&workspace_root).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let mut results = Vec::new();
            for file_path in files {
                if let Ok(Some(file_result)) = searcher.search_file(&file_path, &search_query).await {
                    if file_result.has_matches() {
                        results.push(JsSearchResult {
                            path: file_result.path.to_string_lossy().to_string(),
                            title: file_result.title,
                            matches: file_result.matches.into_iter().map(|m| JsSearchMatch {
                                line_number: m.line_number,
                                line_content: m.line_content,
                                match_start: m.match_start,
                                match_end: m.match_end,
                            }).collect(),
                        });
                    }
                }
            }

            serde_wasm_bindgen::to_value(&results).js_err()
        })
    }

    // ========================================================================
    // Validation Operations
    // ========================================================================

    /// Validate workspace links.
    #[wasm_bindgen(js_name = "validateWorkspace")]
    pub fn validate_workspace(&self, workspace_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let validator = Validator::new(&*fs);
            let results = validator.validate_workspace(&PathBuf::from(&workspace_path)).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            serde_wasm_bindgen::to_value(&results).js_err()
        })
    }

    // ========================================================================
    // File Operations  
    // ========================================================================

    /// Check if a file exists.
    #[wasm_bindgen(js_name = "fileExists")]
    pub fn file_exists(&self, path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exists = fs.exists(&PathBuf::from(&path)).await;
            Ok(JsValue::from_bool(exists))
        })
    }

    /// Read a file's content.
    #[wasm_bindgen(js_name = "readFile")]
    pub fn read_file(&self, path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let content = fs.read_to_string(&PathBuf::from(&path)).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(JsValue::from_str(&content))
        })
    }

    /// Write content to a file.
    #[wasm_bindgen(js_name = "writeFile")]
    pub fn write_file(&self, path: String, content: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            fs.write_file(&PathBuf::from(&path), &content).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Read binary file.
    #[wasm_bindgen(js_name = "readBinary")]
    pub fn read_binary(&self, path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let data = fs.read_binary(&PathBuf::from(&path)).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(js_sys::Uint8Array::from(data.as_slice()).into())
        })
    }

    /// Write binary file.
    #[wasm_bindgen(js_name = "writeBinary")]
    pub fn write_binary(&self, path: String, data: js_sys::Uint8Array) -> Promise {
        let fs = self.fs.clone();
        let data_vec = data.to_vec();

        future_to_promise(async move {
            fs.write_binary(&PathBuf::from(&path), &data_vec).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Delete a file.
    #[wasm_bindgen(js_name = "deleteFile")]
    pub fn delete_file(&self, path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            fs.delete_file(&PathBuf::from(&path)).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(JsValue::UNDEFINED)
        })
    }

    // ========================================================================
    // Advanced Entry Operations
    // ========================================================================

    /// Attach an entry to a parent index.
    #[wasm_bindgen(js_name = "attachToParent")]
    pub fn attach_to_parent(&self, entry_path: String, parent_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&*fs);
            let result = ws.attach_and_move_entry_to_parent(
                &PathBuf::from(&entry_path),
                &PathBuf::from(&parent_path),
            ).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(JsValue::from_str(&result.to_string_lossy()))
        })
    }

    /// Convert a leaf file to an index.
    #[wasm_bindgen(js_name = "convertToIndex")]
    pub fn convert_to_index(&self, path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&*fs);
            let result = ws.convert_to_index(&PathBuf::from(&path)).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(JsValue::from_str(&result.to_string_lossy()))
        })
    }

    /// Convert an index back to a leaf file.
    #[wasm_bindgen(js_name = "convertToLeaf")]
    pub fn convert_to_leaf(&self, path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&*fs);
            let result = ws.convert_to_leaf(&PathBuf::from(&path)).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(JsValue::from_str(&result.to_string_lossy()))
        })
    }

    /// Create a new child entry under a parent.
    #[wasm_bindgen(js_name = "createChildEntry")]
    pub fn create_child_entry(&self, parent_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&*fs);
            let result = ws.create_child_entry(&PathBuf::from(&parent_path), None).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(JsValue::from_str(&result.to_string_lossy()))
        })
    }

    /// Rename an entry file.
    #[wasm_bindgen(js_name = "renameEntry")]
    pub fn rename_entry(&self, path: String, new_filename: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&*fs);
            let result = ws.rename_entry(&PathBuf::from(&path), &new_filename).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(JsValue::from_str(&result.to_string_lossy()))
        })
    }

    /// Ensure today's daily entry exists.
    #[wasm_bindgen(js_name = "ensureDailyEntry")]
    pub fn ensure_daily_entry(&self) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            use chrono::Local;
            
            let today = Local::now().date_naive();
            let year = today.format("%Y").to_string();
            let month = today.format("%m").to_string();
            let day_filename = today.format("%Y-%m-%d").to_string();
            
            // Create directory structure: workspace/Daily/YYYY/MM/
            let daily_dir = PathBuf::from("workspace").join("Daily").join(&year).join(&month);
            fs.create_dir_all(&daily_dir).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            
            let daily_path = daily_dir.join(format!("{}.md", day_filename));
            
            // Check if it exists
            if !fs.exists(&daily_path).await {
                let title = today.format("%B %d, %Y").to_string();
                let content = format!(
                    "---\ntitle: \"{}\"\ncreated: \"{}\"\n---\n\n",
                    title,
                    chrono::Utc::now().to_rfc3339()
                );
                fs.write_file(&daily_path, &content).await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
            }
            
            Ok(JsValue::from_str(&daily_path.to_string_lossy()))
        })
    }

    /// Convert a title to a kebab-case filename.
    #[wasm_bindgen(js_name = "slugifyTitle")]
    pub fn slugify_title(&self, title: String) -> String {
        diaryx_core::entry::slugify_title(&title)
    }

    // ========================================================================
    // Advanced Frontmatter Operations
    // ========================================================================

    /// Remove a frontmatter property.
    #[wasm_bindgen(js_name = "removeFrontmatterProperty")]
    pub fn remove_frontmatter_property(&self, path: String, key: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let entry_path = PathBuf::from(&path);
            let content = fs.read_to_string(&entry_path).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let mut parsed = frontmatter::parse_or_empty(&content)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            parsed.frontmatter.shift_remove(&key);

            let new_content = frontmatter::serialize(&parsed.frontmatter, &parsed.body)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            
            fs.write_file(&entry_path, &new_content).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            Ok(JsValue::UNDEFINED)
        })
    }

    // ========================================================================
    // Template Operations
    // ========================================================================

    /// List available templates.
    #[wasm_bindgen(js_name = "listTemplates")]
    pub fn list_templates(&self, workspace_path: Option<String>) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            // Templates are stored in workspace/_templates/
            let templates_dir = PathBuf::from(workspace_path.as_deref().unwrap_or("workspace"))
                .join("_templates");
            
            let mut templates = Vec::new();
            
            // Add built-in templates
            templates.push(serde_json::json!({
                "name": "note",
                "path": "",
                "source": "builtin"
            }));
            templates.push(serde_json::json!({
                "name": "daily",
                "path": "",
                "source": "builtin"
            }));
            
            // List workspace templates
            if fs.is_dir(&templates_dir).await {
                if let Ok(files) = fs.list_md_files(&templates_dir).await {
                    for file in files {
                        if let Some(name) = file.file_stem().and_then(|n| n.to_str()) {
                            templates.push(serde_json::json!({
                                "name": name,
                                "path": file.to_string_lossy(),
                                "source": "workspace"
                            }));
                        }
                    }
                }
            }
            
            serde_wasm_bindgen::to_value(&templates).js_err()
        })
    }

    /// Get a template's content.
    #[wasm_bindgen(js_name = "getTemplate")]
    pub fn get_template(&self, name: String, workspace_path: Option<String>) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            // Check workspace templates first
            let templates_dir = PathBuf::from(workspace_path.as_deref().unwrap_or("workspace"))
                .join("_templates");
            let template_path = templates_dir.join(format!("{}.md", name));
            
            if fs.exists(&template_path).await {
                let content = fs.read_to_string(&template_path).await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                return Ok(JsValue::from_str(&content));
            }
            
            // Return built-in template
            let content = match name.as_str() {
                "note" => "---\ntitle: \"{{title}}\"\ncreated: \"{{date}}\"\n---\n\n",
                "daily" => "---\ntitle: \"{{title}}\"\ncreated: \"{{date}}\"\n---\n\n## Today\n\n",
                _ => return Err(JsValue::from_str(&format!("Template not found: {}", name))),
            };
            
            Ok(JsValue::from_str(content))
        })
    }

    /// Save a user template.
    #[wasm_bindgen(js_name = "saveTemplate")]
    pub fn save_template(&self, name: String, content: String, workspace_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let templates_dir = PathBuf::from(&workspace_path).join("_templates");
            fs.create_dir_all(&templates_dir).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            
            let template_path = templates_dir.join(format!("{}.md", name));
            fs.write_file(&template_path, &content).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Delete a user template.
    #[wasm_bindgen(js_name = "deleteTemplate")]
    pub fn delete_template(&self, name: String, workspace_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let template_path = PathBuf::from(&workspace_path)
                .join("_templates")
                .join(format!("{}.md", name));
            
            fs.delete_file(&template_path).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            
            Ok(JsValue::UNDEFINED)
        })
    }

    // ========================================================================
    // Attachment Operations
    // ========================================================================

    /// List attachments for an entry.
    #[wasm_bindgen(js_name = "listAttachments")]
    pub fn list_attachments(&self, entry_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let path = PathBuf::from(&entry_path);
            let content = fs.read_to_string(&path).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            
            let parsed = frontmatter::parse_or_empty(&content)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            
            let attachments: Vec<String> = parsed.frontmatter
                .get("attachments")
                .and_then(|v| {
                    if let serde_yaml::Value::Sequence(seq) = v {
                        Some(seq.iter()
                            .filter_map(|item| item.as_str().map(String::from))
                            .collect())
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            
            serde_wasm_bindgen::to_value(&attachments).js_err()
        })
    }

    /// Upload an attachment file.
    #[wasm_bindgen(js_name = "uploadAttachment")]
    pub fn upload_attachment(&self, entry_path: String, filename: String, data_base64: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            // Decode base64
            let data = base64_decode(&data_base64)
                .map_err(|e| JsValue::from_str(&format!("Base64 decode error: {}", e)))?;
            
            let entry_dir = PathBuf::from(&entry_path).parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            let attachments_dir = entry_dir.join("_attachments");
            let attachment_path = attachments_dir.join(&filename);
            
            fs.create_dir_all(&attachments_dir).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            fs.write_binary(&attachment_path, &data).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            
            let relative_path = format!("_attachments/{}", filename);
            
            // Add to frontmatter
            let entry_path_buf = PathBuf::from(&entry_path);
            let content = fs.read_to_string(&entry_path_buf).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            
            let mut parsed = frontmatter::parse_or_empty(&content)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            
            let mut attachments: Vec<serde_yaml::Value> = parsed.frontmatter
                .get("attachments")
                .and_then(|v| {
                    if let serde_yaml::Value::Sequence(seq) = v {
                        Some(seq.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            
            if !attachments.iter().any(|a| a.as_str() == Some(&relative_path)) {
                attachments.push(serde_yaml::Value::String(relative_path.clone()));
                parsed.frontmatter.insert(
                    "attachments".to_string(),
                    serde_yaml::Value::Sequence(attachments)
                );
                
                let new_content = frontmatter::serialize(&parsed.frontmatter, &parsed.body)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                fs.write_file(&entry_path_buf, &new_content).await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
            }
            
            Ok(JsValue::from_str(&relative_path))
        })
    }

    /// Delete an attachment file.
    #[wasm_bindgen(js_name = "deleteAttachment")]
    pub fn delete_attachment(&self, entry_path: String, attachment_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let entry_dir = PathBuf::from(&entry_path).parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            let full_path = entry_dir.join(&attachment_path);
            
            // Delete the file
            if fs.exists(&full_path).await {
                fs.delete_file(&full_path).await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
            }
            
            // Remove from frontmatter
            let entry_path_buf = PathBuf::from(&entry_path);
            let content = fs.read_to_string(&entry_path_buf).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            
            let mut parsed = frontmatter::parse_or_empty(&content)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            
            if let Some(serde_yaml::Value::Sequence(attachments)) = parsed.frontmatter.get_mut("attachments") {
                attachments.retain(|a| a.as_str() != Some(&attachment_path));
                
                let new_content = frontmatter::serialize(&parsed.frontmatter, &parsed.body)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                fs.write_file(&entry_path_buf, &new_content).await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
            }
            
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Get attachment data.
    #[wasm_bindgen(js_name = "getAttachmentData")]
    pub fn get_attachment_data(&self, entry_path: String, attachment_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let entry_dir = PathBuf::from(&entry_path).parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            let full_path = entry_dir.join(&attachment_path);
            
            let data = fs.read_binary(&full_path).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            
            Ok(js_sys::Uint8Array::from(data.as_slice()).into())
        })
    }

    /// Get storage usage information.
    #[wasm_bindgen(js_name = "getStorageUsage")]
    pub fn get_storage_usage(&self) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            // For now return approximate values - real calculation would need to traverse
            let info = serde_json::json!({
                "used": 0,
                "limit": 100 * 1024 * 1024,  // 100MB
                "attachment_limit": 5 * 1024 * 1024  // 5MB
            });
            serde_wasm_bindgen::to_value(&info).js_err()
        })
    }

    // ========================================================================
    // Export Operations
    // ========================================================================

    /// Get available audience tags.
    #[wasm_bindgen(js_name = "getAvailableAudiences")]
    pub fn get_available_audiences(&self, root_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&*fs);
            let root = PathBuf::from(&root_path);
            
            let mut audiences = HashSet::new();
            let mut visited = HashSet::new();
            
            async fn collect_audiences<FS: AsyncFileSystem>(
                ws: &Workspace<FS>,
                path: &Path,
                audiences: &mut HashSet<String>,
                visited: &mut HashSet<PathBuf>,
            ) {
                if visited.contains(path) {
                    return;
                }
                visited.insert(path.to_path_buf());
                
                if let Ok(index) = ws.parse_index(path).await {
                    if let Some(audience_list) = &index.frontmatter.audience {
                        audiences.extend(audience_list.iter().cloned());
                    }
                    
                    let dir = index.directory().unwrap_or(Path::new(""));
                    for child_ref in index.frontmatter.contents_list() {
                        let child_path = dir.join(child_ref);
                        Box::pin(collect_audiences(ws, &child_path, audiences, visited)).await;
                    }
                }
            }
            
            // Find root index first
            if let Ok(Some(root_index)) = ws.find_root_index_in_dir(&root).await {
                collect_audiences(&ws, &root_index, &mut audiences, &mut visited).await;
            }
            
            let mut result: Vec<String> = audiences.into_iter().collect();
            result.sort();
            
            serde_wasm_bindgen::to_value(&result).js_err()
        })
    }

    /// Plan an export operation.
    #[wasm_bindgen(js_name = "planExport")]
    pub fn plan_export(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&*fs);
            let root = PathBuf::from(&root_path);
            
            let mut included = Vec::new();
            let mut visited = HashSet::new();
            
            async fn collect_files<FS: AsyncFileSystem>(
                ws: &Workspace<FS>,
                path: &Path,
                root_dir: &Path,
                included: &mut Vec<serde_json::Value>,
                visited: &mut HashSet<PathBuf>,
            ) {
                if visited.contains(path) {
                    return;
                }
                visited.insert(path.to_path_buf());
                
                if let Ok(index) = ws.parse_index(path).await {
                    let relative = pathdiff::diff_paths(path, root_dir)
                        .unwrap_or_else(|| path.to_path_buf());
                    included.push(serde_json::json!({
                        "path": path.to_string_lossy(),
                        "relative_path": relative.to_string_lossy()
                    }));
                    
                    let dir = index.directory().unwrap_or(Path::new(""));
                    for child_ref in index.frontmatter.contents_list() {
                        let child_path = dir.join(child_ref);
                        Box::pin(collect_files(ws, &child_path, root_dir, included, visited)).await;
                    }
                }
            }
            
            if let Ok(Some(root_index)) = ws.find_root_index_in_dir(&root).await {
                let root_dir = root_index.parent().unwrap_or(&root);
                collect_files(&ws, &root_index, root_dir, &mut included, &mut visited).await;
            }
            
            let plan = serde_json::json!({
                "included": included,
                "excluded": [],
                "audience": audience
            });
            
            serde_wasm_bindgen::to_value(&plan).js_err()
        })
    }

    /// Export files to memory as markdown.
    #[wasm_bindgen(js_name = "exportToMemory")]
    pub fn export_to_memory(&self, root_path: String, _audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&*fs);
            let root = PathBuf::from(&root_path);
            
            let mut files = Vec::new();
            let mut visited = HashSet::new();
            
            async fn collect_files<FS: AsyncFileSystem>(
                ws: &Workspace<FS>,
                path: &Path,
                root_dir: &Path,
                files: &mut Vec<serde_json::Value>,
                visited: &mut HashSet<PathBuf>,
            ) {
                if visited.contains(path) {
                    return;
                }
                visited.insert(path.to_path_buf());
                
                if let Ok(content) = ws.fs_ref().read_to_string(path).await {
                    let relative = pathdiff::diff_paths(path, root_dir)
                        .unwrap_or_else(|| path.to_path_buf());
                    files.push(serde_json::json!({
                        "path": relative.to_string_lossy(),
                        "content": content
                    }));
                    
                    if let Ok(index) = ws.parse_index(path).await {
                        let dir = index.directory().unwrap_or(Path::new(""));
                        for child_ref in index.frontmatter.contents_list() {
                            let child_path = dir.join(child_ref);
                            Box::pin(collect_files(ws, &child_path, root_dir, files, visited)).await;
                        }
                    }
                }
            }
            
            if let Ok(Some(root_index)) = ws.find_root_index_in_dir(&root).await {
                let root_dir = root_index.parent().unwrap_or(&root);
                collect_files(&ws, &root_index, root_dir, &mut files, &mut visited).await;
            }
            
            serde_wasm_bindgen::to_value(&files).js_err()
        })
    }

    /// Export files to memory as HTML.
    #[wasm_bindgen(js_name = "exportToHtml")]
    pub fn export_to_html(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            // For now, just return the markdown files - HTML conversion can happen in JS
            let ws = Workspace::new(&*fs);
            let root = PathBuf::from(&root_path);
            
            let mut files = Vec::new();
            let mut visited = HashSet::new();
            
            async fn collect_files<FS: AsyncFileSystem>(
                ws: &Workspace<FS>,
                path: &Path,
                root_dir: &Path,
                files: &mut Vec<serde_json::Value>,
                visited: &mut HashSet<PathBuf>,
            ) {
                if visited.contains(path) {
                    return;
                }
                visited.insert(path.to_path_buf());
                
                if let Ok(content) = ws.fs_ref().read_to_string(path).await {
                    let relative = pathdiff::diff_paths(path, root_dir)
                        .unwrap_or_else(|| path.to_path_buf());
                    // Simple markdown to HTML - just wrap in pre for now
                    let html = format!("<html><body><pre>{}</pre></body></html>", 
                        content.replace("<", "&lt;").replace(">", "&gt;"));
                    files.push(serde_json::json!({
                        "path": relative.with_extension("html").to_string_lossy(),
                        "content": html
                    }));
                    
                    if let Ok(index) = ws.parse_index(path).await {
                        let dir = index.directory().unwrap_or(Path::new(""));
                        for child_ref in index.frontmatter.contents_list() {
                            let child_path = dir.join(child_ref);
                            Box::pin(collect_files(ws, &child_path, root_dir, files, visited)).await;
                        }
                    }
                }
            }
            
            if let Ok(Some(root_index)) = ws.find_root_index_in_dir(&root).await {
                let root_dir = root_index.parent().unwrap_or(&root);
                collect_files(&ws, &root_index, root_dir, &mut files, &mut visited).await;
            }
            
            serde_wasm_bindgen::to_value(&files).js_err()
        })
    }

    /// Export binary attachments.
    #[wasm_bindgen(js_name = "exportBinaryAttachments")]
    pub fn export_binary_attachments(&self, root_path: String, _audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&*fs);
            let root = PathBuf::from(&root_path);
            
            let mut binary_files: Vec<serde_json::Value> = Vec::new();
            let mut visited_entries = HashSet::new();
            let mut visited_attachment_dirs = HashSet::new();
            
            async fn collect_attachments<FS: AsyncFileSystem>(
                ws: &Workspace<FS>,
                entry_path: &Path,
                root_dir: &Path,
                binary_files: &mut Vec<serde_json::Value>,
                visited_entries: &mut HashSet<PathBuf>,
                visited_attachment_dirs: &mut HashSet<PathBuf>,
            ) {
                if visited_entries.contains(entry_path) {
                    return;
                }
                visited_entries.insert(entry_path.to_path_buf());
                
                if let Ok(index) = ws.parse_index(entry_path).await {
                    let dir = index.directory().unwrap_or(Path::new(""));
                    
                    // Check for _attachments directory
                    let attachments_dir = dir.join("_attachments");
                    if !visited_attachment_dirs.contains(&attachments_dir) {
                        visited_attachment_dirs.insert(attachments_dir.clone());
                        
                        if let Ok(files) = ws.fs_ref().list_files(&attachments_dir).await {
                            for file in files {
                                if let Ok(data) = ws.fs_ref().read_binary(&file).await {
                                    let relative = pathdiff::diff_paths(&file, root_dir)
                                        .unwrap_or_else(|| file.clone());
                                    binary_files.push(serde_json::json!({
                                        "path": relative.to_string_lossy(),
                                        "data": data
                                    }));
                                }
                            }
                        }
                    }
                    
                    // Recurse into children
                    for child_ref in index.frontmatter.contents_list() {
                        let child_path = dir.join(child_ref);
                        Box::pin(collect_attachments(
                            ws, &child_path, root_dir, binary_files, 
                            visited_entries, visited_attachment_dirs
                        )).await;
                    }
                }
            }
            
            if let Ok(Some(root_index)) = ws.find_root_index_in_dir(&root).await {
                let root_dir = root_index.parent().unwrap_or(&root);
                collect_attachments(
                    &ws, &root_index, root_dir, &mut binary_files,
                    &mut visited_entries, &mut visited_attachment_dirs
                ).await;
            }
            
            serde_wasm_bindgen::to_value(&binary_files).js_err()
        })
    }

    // ========================================================================
    // Validation Fix Operations
    // ========================================================================

    /// Validate a single file's links.
    #[wasm_bindgen(js_name = "validateFile")]
    pub fn validate_file(&self, file_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let validator = Validator::new(&*fs);
            let results = validator.validate_file(&PathBuf::from(&file_path)).await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            serde_wasm_bindgen::to_value(&results).js_err()
        })
    }

    /// Fix a broken part_of reference by removing it.
    #[wasm_bindgen(js_name = "fixBrokenPartOf")]
    pub fn fix_broken_part_of(&self, file_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = diaryx_core::validate::ValidationFixer::new(&*fs);
            let result = fixer.fix_broken_part_of(&PathBuf::from(&file_path)).await;
            
            serde_wasm_bindgen::to_value(&serde_json::json!({
                "success": result.success,
                "message": result.message
            })).js_err()
        })
    }

    /// Fix a broken contents reference by removing it.
    #[wasm_bindgen(js_name = "fixBrokenContentsRef")]
    pub fn fix_broken_contents_ref(&self, index_path: String, target: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = diaryx_core::validate::ValidationFixer::new(&*fs);
            let result = fixer.fix_broken_contents_ref(&PathBuf::from(&index_path), &target).await;
            
            serde_wasm_bindgen::to_value(&serde_json::json!({
                "success": result.success,
                "message": result.message
            })).js_err()
        })
    }

    /// Fix a broken attachment reference by removing it.
    #[wasm_bindgen(js_name = "fixBrokenAttachment")]
    pub fn fix_broken_attachment(&self, file_path: String, attachment: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = diaryx_core::validate::ValidationFixer::new(&*fs);
            let result = fixer.fix_broken_attachment(&PathBuf::from(&file_path), &attachment).await;
            
            serde_wasm_bindgen::to_value(&serde_json::json!({
                "success": result.success,
                "message": result.message
            })).js_err()
        })
    }

    /// Fix a non-portable path by normalizing it.
    #[wasm_bindgen(js_name = "fixNonPortablePath")]
    pub fn fix_non_portable_path(
        &self,
        file_path: String,
        property: String,
        old_value: String,
        new_value: String,
    ) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = diaryx_core::validate::ValidationFixer::new(&*fs);
            let result = fixer.fix_non_portable_path(
                &PathBuf::from(&file_path),
                &property,
                &old_value,
                &new_value,
            ).await;
            
            serde_wasm_bindgen::to_value(&serde_json::json!({
                "success": result.success,
                "message": result.message
            })).js_err()
        })
    }

    /// Add an unlisted file to an index's contents.
    #[wasm_bindgen(js_name = "fixUnlistedFile")]
    pub fn fix_unlisted_file(&self, index_path: String, file_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = diaryx_core::validate::ValidationFixer::new(&*fs);
            let result = fixer.fix_unlisted_file(
                &PathBuf::from(&index_path),
                &PathBuf::from(&file_path),
            ).await;
            
            serde_wasm_bindgen::to_value(&serde_json::json!({
                "success": result.success,
                "message": result.message
            })).js_err()
        })
    }

    /// Add an orphan binary file to an index's attachments.
    #[wasm_bindgen(js_name = "fixOrphanBinaryFile")]
    pub fn fix_orphan_binary_file(&self, index_path: String, file_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = diaryx_core::validate::ValidationFixer::new(&*fs);
            let result = fixer.fix_orphan_binary_file(
                &PathBuf::from(&index_path),
                &PathBuf::from(&file_path),
            ).await;
            
            serde_wasm_bindgen::to_value(&serde_json::json!({
                "success": result.success,
                "message": result.message
            })).js_err()
        })
    }

    /// Fix a missing part_of by setting it to point to the given index.
    #[wasm_bindgen(js_name = "fixMissingPartOf")]
    pub fn fix_missing_part_of(&self, file_path: String, index_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = diaryx_core::validate::ValidationFixer::new(&*fs);
            let result = fixer.fix_missing_part_of(
                &PathBuf::from(&file_path),
                &PathBuf::from(&index_path),
            ).await;
            
            serde_wasm_bindgen::to_value(&serde_json::json!({
                "success": result.success,
                "message": result.message
            })).js_err()
        })
    }

    /// Fix all errors and fixable warnings in a validation result.
    #[wasm_bindgen(js_name = "fixAll")]
    pub fn fix_all(&self, validation_result: JsValue) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = diaryx_core::validate::ValidationFixer::new(&*fs);
            
            // Parse the validation result
            let result: serde_json::Value = serde_wasm_bindgen::from_value(validation_result)
                .map_err(|e| JsValue::from_str(&format!("Failed to parse validation result: {:?}", e)))?;
            
            let mut error_fixes = Vec::new();
            let mut warning_fixes = Vec::new();
            
            // Fix errors
            if let Some(errors) = result.get("errors").and_then(|e| e.as_array()) {
                for err in errors {
                    let fix_result = match err.get("type").and_then(|t| t.as_str()) {
                        Some("BrokenPartOf") => {
                            if let Some(file) = err.get("file").and_then(|f| f.as_str()) {
                                Some(fixer.fix_broken_part_of(&PathBuf::from(file)).await)
                            } else { None }
                        }
                        Some("BrokenContentsRef") => {
                            if let (Some(index), Some(target)) = (
                                err.get("index").and_then(|i| i.as_str()),
                                err.get("target").and_then(|t| t.as_str())
                            ) {
                                Some(fixer.fix_broken_contents_ref(&PathBuf::from(index), target).await)
                            } else { None }
                        }
                        Some("BrokenAttachment") => {
                            if let (Some(file), Some(attachment)) = (
                                err.get("file").and_then(|f| f.as_str()),
                                err.get("attachment").and_then(|a| a.as_str())
                            ) {
                                Some(fixer.fix_broken_attachment(&PathBuf::from(file), attachment).await)
                            } else { None }
                        }
                        _ => None,
                    };
                    
                    if let Some(r) = fix_result {
                        error_fixes.push(serde_json::json!({
                            "success": r.success,
                            "message": r.message
                        }));
                    }
                }
            }
            
            // Fix warnings
            if let Some(warnings) = result.get("warnings").and_then(|w| w.as_array()) {
                for warn in warnings {
                    let fix_result = match warn.get("type").and_then(|t| t.as_str()) {
                        Some("UnlistedFile") => {
                            if let (Some(index), Some(file)) = (
                                warn.get("index").and_then(|i| i.as_str()),
                                warn.get("file").and_then(|f| f.as_str())
                            ) {
                                Some(fixer.fix_unlisted_file(&PathBuf::from(index), &PathBuf::from(file)).await)
                            } else { None }
                        }
                        Some("NonPortablePath") => {
                            if let (Some(file), Some(property), Some(value), Some(suggested)) = (
                                warn.get("file").and_then(|f| f.as_str()),
                                warn.get("property").and_then(|p| p.as_str()),
                                warn.get("value").and_then(|v| v.as_str()),
                                warn.get("suggested").and_then(|s| s.as_str())
                            ) {
                                Some(fixer.fix_non_portable_path(&PathBuf::from(file), property, value, suggested).await)
                            } else { None }
                        }
                        Some("OrphanBinaryFile") => {
                            if let (Some(file), Some(index)) = (
                                warn.get("file").and_then(|f| f.as_str()),
                                warn.get("suggested_index").and_then(|i| i.as_str())
                            ) {
                                Some(fixer.fix_orphan_binary_file(&PathBuf::from(index), &PathBuf::from(file)).await)
                            } else { None }
                        }
                        Some("MissingPartOf") => {
                            if let (Some(file), Some(index)) = (
                                warn.get("file").and_then(|f| f.as_str()),
                                warn.get("suggested_index").and_then(|i| i.as_str())
                            ) {
                                Some(fixer.fix_missing_part_of(&PathBuf::from(file), &PathBuf::from(index)).await)
                            } else { None }
                        }
                        _ => None,
                    };
                    
                    if let Some(r) = fix_result {
                        warning_fixes.push(serde_json::json!({
                            "success": r.success,
                            "message": r.message
                        }));
                    }
                }
            }
            
            let total_fixed = error_fixes.iter().filter(|r| r.get("success").and_then(|s| s.as_bool()).unwrap_or(false)).count()
                + warning_fixes.iter().filter(|r| r.get("success").and_then(|s| s.as_bool()).unwrap_or(false)).count();
            let total_failed = error_fixes.iter().filter(|r| !r.get("success").and_then(|s| s.as_bool()).unwrap_or(false)).count()
                + warning_fixes.iter().filter(|r| !r.get("success").and_then(|s| s.as_bool()).unwrap_or(false)).count();
            
            let summary = serde_json::json!({
                "error_fixes": error_fixes,
                "warning_fixes": warning_fixes,
                "total_fixed": total_fixed,
                "total_failed": total_failed
            });
            
            serde_wasm_bindgen::to_value(&summary).js_err()
        })
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Simple base64 decoder
fn base64_decode(input: &str) -> std::result::Result<Vec<u8>, String> {
    let data = if let Some(pos) = input.find(',') {
        &input[pos + 1..]
    } else {
        input
    };

    const DECODE_TABLE: [i8; 256] = {
        let mut table = [-1i8; 256];
        let mut i = 0u8;
        while i < 26 {
            table[(b'A' + i) as usize] = i as i8;
            table[(b'a' + i) as usize] = (i + 26) as i8;
            i += 1;
        }
        let mut i = 0u8;
        while i < 10 {
            table[(b'0' + i) as usize] = (i + 52) as i8;
            i += 1;
        }
        table[b'+' as usize] = 62;
        table[b'/' as usize] = 63;
        table[b'=' as usize] = 0;
        table
    };

    let bytes: Vec<u8> = data.bytes().filter(|&b| b != b'\n' && b != b'\r').collect();
    let mut output = Vec::with_capacity(bytes.len() * 3 / 4);

    for chunk in bytes.chunks(4) {
        if chunk.len() < 4 {
            break;
        }

        let a = DECODE_TABLE[chunk[0] as usize];
        let b = DECODE_TABLE[chunk[1] as usize];
        let c = DECODE_TABLE[chunk[2] as usize];
        let d = DECODE_TABLE[chunk[3] as usize];

        if a < 0 || b < 0 {
            return Err("Invalid base64 character".to_string());
        }

        output.push(((a as u8) << 2) | ((b as u8) >> 4));
        if chunk[2] != b'=' {
            output.push(((b as u8) << 4) | ((c as u8) >> 2));
        }
        if chunk[3] != b'=' {
            output.push(((c as u8) << 6) | (d as u8));
        }
    }

    Ok(output)
}
