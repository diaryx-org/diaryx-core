//! Unified async backend for WASM with native OPFS/IndexedDB storage.
//!
//! This module provides a single entry point for all workspace operations,
//! working directly with native storage backends (no InMemoryFileSystem).
//!
//! ## Preferred API: `execute()` / `executeJs()`
//!
//! The recommended way to use this backend is through the unified command API:
//!
//! ```javascript
//! import { DiaryxBackend } from './wasm/diaryx_wasm.js';
//!
//! const backend = await DiaryxBackend.createOpfs();
//!
//! // Use execute() with Command objects
//! const response = await backend.execute(JSON.stringify({
//!   type: 'GetEntry',
//!   params: { path: 'workspace/journal/2024-01-08.md' }
//! }));
//!
//! // Or executeJs() with JavaScript objects directly
//! const response = await backend.executeJs({
//!   type: 'GetWorkspaceTree',
//!   params: { path: 'workspace/index.md' }
//! });
//! ```
//!
//! ## Legacy API (deprecated)
//!
//! Individual methods like `getTree()`, `getEntry()`, etc. are still available
//! for backwards compatibility but will be removed in a future version.
//! Please migrate to `execute()` or `executeJs()`.

use std::collections::HashSet;
use std::io::Result as IoResult;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use diaryx_core::crdt::{CrdtStorage, MemoryStorage};
use diaryx_core::diaryx::Diaryx;
use diaryx_core::frontmatter;
use diaryx_core::fs::AsyncFileSystem;
use diaryx_core::search::{SearchMatch, SearchQuery, Searcher};
use diaryx_core::validate::Validator;
use diaryx_core::workspace::Workspace;
use js_sys::Promise;
use serde::Serialize;
use serde_wasm_bindgen;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::error::IntoJsResult;
use crate::fsa_fs::FsaFileSystem;
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
    /// File System Access API - user-selected directory on their real filesystem
    Fsa(FsaFileSystem),
}

impl Clone for StorageBackend {
    fn clone(&self) -> Self {
        match self {
            StorageBackend::Opfs(fs) => StorageBackend::Opfs(fs.clone()),
            StorageBackend::IndexedDb(fs) => StorageBackend::IndexedDb(fs.clone()),
            StorageBackend::Fsa(fs) => StorageBackend::Fsa(fs.clone()),
        }
    }
}

// Implement AsyncFileSystem by delegating to inner type
impl AsyncFileSystem for StorageBackend {
    fn read_to_string<'a>(
        &'a self,
        path: &'a Path,
    ) -> diaryx_core::fs::BoxFuture<'a, IoResult<String>> {
        match self {
            StorageBackend::Opfs(fs) => fs.read_to_string(path),
            StorageBackend::IndexedDb(fs) => fs.read_to_string(path),
            StorageBackend::Fsa(fs) => fs.read_to_string(path),
        }
    }

    fn write_file<'a>(
        &'a self,
        path: &'a Path,
        content: &'a str,
    ) -> diaryx_core::fs::BoxFuture<'a, IoResult<()>> {
        match self {
            StorageBackend::Opfs(fs) => fs.write_file(path, content),
            StorageBackend::IndexedDb(fs) => fs.write_file(path, content),
            StorageBackend::Fsa(fs) => fs.write_file(path, content),
        }
    }

    fn create_new<'a>(
        &'a self,
        path: &'a Path,
        content: &'a str,
    ) -> diaryx_core::fs::BoxFuture<'a, IoResult<()>> {
        match self {
            StorageBackend::Opfs(fs) => fs.create_new(path, content),
            StorageBackend::IndexedDb(fs) => fs.create_new(path, content),
            StorageBackend::Fsa(fs) => fs.create_new(path, content),
        }
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> diaryx_core::fs::BoxFuture<'a, IoResult<()>> {
        match self {
            StorageBackend::Opfs(fs) => fs.delete_file(path),
            StorageBackend::IndexedDb(fs) => fs.delete_file(path),
            StorageBackend::Fsa(fs) => fs.delete_file(path),
        }
    }

    fn list_md_files<'a>(
        &'a self,
        dir: &'a Path,
    ) -> diaryx_core::fs::BoxFuture<'a, IoResult<Vec<PathBuf>>> {
        match self {
            StorageBackend::Opfs(fs) => fs.list_md_files(dir),
            StorageBackend::IndexedDb(fs) => fs.list_md_files(dir),
            StorageBackend::Fsa(fs) => fs.list_md_files(dir),
        }
    }

    fn exists<'a>(&'a self, path: &'a Path) -> diaryx_core::fs::BoxFuture<'a, bool> {
        match self {
            StorageBackend::Opfs(fs) => fs.exists(path),
            StorageBackend::IndexedDb(fs) => fs.exists(path),
            StorageBackend::Fsa(fs) => fs.exists(path),
        }
    }

    fn create_dir_all<'a>(
        &'a self,
        path: &'a Path,
    ) -> diaryx_core::fs::BoxFuture<'a, IoResult<()>> {
        match self {
            StorageBackend::Opfs(fs) => fs.create_dir_all(path),
            StorageBackend::IndexedDb(fs) => fs.create_dir_all(path),
            StorageBackend::Fsa(fs) => fs.create_dir_all(path),
        }
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> diaryx_core::fs::BoxFuture<'a, bool> {
        match self {
            StorageBackend::Opfs(fs) => fs.is_dir(path),
            StorageBackend::IndexedDb(fs) => fs.is_dir(path),
            StorageBackend::Fsa(fs) => fs.is_dir(path),
        }
    }

    fn move_file<'a>(
        &'a self,
        from: &'a Path,
        to: &'a Path,
    ) -> diaryx_core::fs::BoxFuture<'a, IoResult<()>> {
        match self {
            StorageBackend::Opfs(fs) => fs.move_file(from, to),
            StorageBackend::IndexedDb(fs) => fs.move_file(from, to),
            StorageBackend::Fsa(fs) => fs.move_file(from, to),
        }
    }

    fn read_binary<'a>(
        &'a self,
        path: &'a Path,
    ) -> diaryx_core::fs::BoxFuture<'a, IoResult<Vec<u8>>> {
        match self {
            StorageBackend::Opfs(fs) => fs.read_binary(path),
            StorageBackend::IndexedDb(fs) => fs.read_binary(path),
            StorageBackend::Fsa(fs) => fs.read_binary(path),
        }
    }

    fn write_binary<'a>(
        &'a self,
        path: &'a Path,
        content: &'a [u8],
    ) -> diaryx_core::fs::BoxFuture<'a, IoResult<()>> {
        match self {
            StorageBackend::Opfs(fs) => fs.write_binary(path, content),
            StorageBackend::IndexedDb(fs) => fs.write_binary(path, content),
            StorageBackend::Fsa(fs) => fs.write_binary(path, content),
        }
    }

    fn list_files<'a>(
        &'a self,
        dir: &'a Path,
    ) -> diaryx_core::fs::BoxFuture<'a, IoResult<Vec<PathBuf>>> {
        match self {
            StorageBackend::Opfs(fs) => fs.list_files(dir),
            StorageBackend::IndexedDb(fs) => fs.list_files(dir),
            StorageBackend::Fsa(fs) => fs.list_files(dir),
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
    /// CRDT storage for sync and history features.
    crdt_storage: Arc<dyn CrdtStorage>,
}

#[wasm_bindgen]
impl DiaryxBackend {
    /// Create a new DiaryxBackend with OPFS storage.
    #[wasm_bindgen(js_name = "createOpfs")]
    pub async fn create_opfs() -> std::result::Result<DiaryxBackend, JsValue> {
        let opfs = OpfsFileSystem::create().await?;
        let fs = Rc::new(StorageBackend::Opfs(opfs));
        let crdt_storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        Ok(Self { fs, crdt_storage })
    }

    /// Create a new DiaryxBackend with IndexedDB storage.
    #[wasm_bindgen(js_name = "createIndexedDb")]
    pub async fn create_indexed_db() -> std::result::Result<DiaryxBackend, JsValue> {
        let idb = IndexedDbFileSystem::create().await?;
        let fs = Rc::new(StorageBackend::IndexedDb(idb));
        let crdt_storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        Ok(Self { fs, crdt_storage })
    }

    /// Create backend with specific storage type.
    #[wasm_bindgen(js_name = "create")]
    pub async fn create(storage_type: &str) -> std::result::Result<DiaryxBackend, JsValue> {
        match storage_type.to_lowercase().as_str() {
            "opfs" => Self::create_opfs().await,
            "indexeddb" | "indexed_db" => Self::create_indexed_db().await,
            _ => Err(JsValue::from_str(&format!(
                "Unknown storage type: {}",
                storage_type
            ))),
        }
    }

    /// Create a new DiaryxBackend from a user-selected directory handle.
    ///
    /// This uses the File System Access API to read/write files directly
    /// on the user's filesystem. The handle must be obtained from
    /// `window.showDirectoryPicker()` in JavaScript.
    ///
    /// ## Browser Support
    /// - Chrome/Edge: ✅ Supported
    /// - Firefox: ❌ Not supported
    /// - Safari: ❌ Not supported
    ///
    /// ## Example
    /// ```javascript
    /// // User must trigger this via a gesture (click/keypress)
    /// const dirHandle = await window.showDirectoryPicker();
    /// const backend = await DiaryxBackend.createFromDirectoryHandle(dirHandle);
    /// ```
    #[wasm_bindgen(js_name = "createFromDirectoryHandle")]
    pub fn create_from_directory_handle(
        handle: web_sys::FileSystemDirectoryHandle,
    ) -> std::result::Result<DiaryxBackend, JsValue> {
        let fsa = FsaFileSystem::from_handle(handle);
        let fs = Rc::new(StorageBackend::Fsa(fsa));
        let crdt_storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        Ok(Self { fs, crdt_storage })
    }

    // ========================================================================
    // Unified Command API
    // ========================================================================

    /// Execute a command and return the response as JSON string.
    ///
    /// This is the primary unified API for all operations, replacing the many
    /// individual method calls with a single entry point.
    ///
    /// ## Example
    /// ```javascript
    /// const command = { type: 'GetEntry', params: { path: 'workspace/notes.md' } };
    /// const responseJson = await backend.execute(JSON.stringify(command));
    /// const response = JSON.parse(responseJson);
    /// ```
    #[wasm_bindgen]
    pub async fn execute(&self, command_json: &str) -> std::result::Result<String, JsValue> {
        use diaryx_core::{Command, Response};

        // Parse the command from JSON
        let cmd: Command = serde_json::from_str(command_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid command JSON: {}", e)))?;

        // Create a Diaryx instance with CRDT support
        let diaryx = Diaryx::with_crdt((*self.fs).clone(), Arc::clone(&self.crdt_storage));

        // Execute the command
        let result = diaryx
            .execute(cmd)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Serialize the response to JSON
        serde_json::to_string(&result)
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize response: {}", e)))
    }

    /// Execute a command from a JavaScript object directly.
    ///
    /// This avoids JSON serialization overhead for better performance.
    #[wasm_bindgen(js_name = "executeJs")]
    pub async fn execute_js(&self, command: JsValue) -> std::result::Result<JsValue, JsValue> {
        use diaryx_core::{Command, Response};

        // Parse command from JS object
        let cmd: Command = serde_wasm_bindgen::from_value(command)?;

        // Create a Diaryx instance with CRDT support
        let diaryx = Diaryx::with_crdt((*self.fs).clone(), Arc::clone(&self.crdt_storage));

        // Execute the command
        let result = diaryx
            .execute(cmd)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Convert response to JsValue
        serde_wasm_bindgen::to_value(&result)
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize response: {}", e)))
    }

    // ========================================================================
    // Root Index Discovery
    // ========================================================================

    /// Find the root index file in a directory.
    /// A root index is any .md file with `contents` property and no `part_of`.
    #[wasm_bindgen(js_name = "findRootIndex")]
    pub fn find_root_index(&self, dir_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&*fs);
            let dir = PathBuf::from(&dir_path);

            match ws.find_root_index_in_dir(&dir).await {
                Ok(Some(path)) => Ok(JsValue::from_str(&path.to_string_lossy())),
                Ok(None) => Ok(JsValue::NULL),
                Err(e) => Err(JsValue::from_str(&format!(
                    "Failed to find root index: {}",
                    e
                ))),
            }
        })
    }

    /// List all subdirectories in a given path.
    #[wasm_bindgen(js_name = "listDirectories")]
    pub fn list_directories(&self, dir_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let dir = PathBuf::from(&dir_path);

            // Get all entries and filter to directories only
            let mut directories = Vec::new();

            // Use the filesystem to check for directories
            // We'll list md files in the directory and collect parent directory names
            // Actually, we need a different approach - check if subdirectories exist
            // For now, return common directory names that might contain workspaces
            if dir.as_os_str().is_empty() || dir == PathBuf::from(".") {
                // Check for known workspace directories
                for name in &["workspace", "journal", "notes", "diary"] {
                    if fs.is_dir(Path::new(name)).await {
                        directories.push(name.to_string());
                    }
                }

                // Also try to find any directory that contains an index file
                // by checking common patterns - this is limited but works for most cases
            }

            serde_wasm_bindgen::to_value(&directories).js_err()
        })
    }

    // ========================================================================
    // Config (stored in root index frontmatter as diaryx_* keys)
    // ========================================================================

    /// Get the current configuration from root index frontmatter.
    /// Config keys are stored as `diaryx_*` properties.
    #[wasm_bindgen(js_name = "getConfig")]
    pub fn get_config(&self) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&*fs);

            // Find root index - try current directory first ("." for FSA mode)
            let root_path = ws
                .find_root_index_in_dir(Path::new("."))
                .await
                .ok()
                .flatten();

            // Fallback: try "workspace" directory for OPFS mode
            let root_path = match root_path {
                Some(p) => Some(p),
                None => ws
                    .find_root_index_in_dir(Path::new("workspace"))
                    .await
                    .ok()
                    .flatten(),
            };

            let root_path = match root_path {
                Some(p) => p,
                None => {
                    // Return default config if no root found
                    let default = r#"{"default_workspace":"."}"#;
                    return js_sys::JSON::parse(default)
                        .map_err(|e| JsValue::from_str(&format!("JSON error: {:?}", e)));
                }
            };

            // Read frontmatter from root index
            match ws.parse_index(&root_path).await {
                Ok(index) => {
                    // Extract diaryx_* keys from extra
                    let mut config = serde_json::Map::new();

                    // Set default_workspace to root index's directory
                    if let Some(parent) = root_path.parent() {
                        let ws_path = if parent.as_os_str().is_empty() {
                            "."
                        } else {
                            &parent.to_string_lossy()
                        };
                        config.insert(
                            "default_workspace".to_string(),
                            serde_json::Value::String(ws_path.to_string()),
                        );
                    }

                    // Extract diaryx_* keys
                    for (key, value) in &index.frontmatter.extra {
                        if let Some(config_key) = key.strip_prefix("diaryx_") {
                            // Convert serde_yaml::Value to serde_json::Value
                            if let Ok(json_str) = serde_yaml::to_string(value) {
                                if let Ok(json_val) =
                                    serde_json::from_str::<serde_json::Value>(&json_str)
                                {
                                    config.insert(config_key.to_string(), json_val);
                                }
                            }
                        }
                    }

                    let config_obj = serde_json::Value::Object(config);
                    let config_str = serde_json::to_string(&config_obj)
                        .map_err(|e| JsValue::from_str(&format!("JSON error: {:?}", e)))?;

                    js_sys::JSON::parse(&config_str)
                        .map_err(|e| JsValue::from_str(&format!("JSON parse error: {:?}", e)))
                }
                Err(_) => {
                    // Return default config
                    let default = r#"{"default_workspace":"."}"#;
                    js_sys::JSON::parse(default)
                        .map_err(|e| JsValue::from_str(&format!("JSON error: {:?}", e)))
                }
            }
        })
    }

    /// Save configuration to root index frontmatter.
    /// Config keys are stored as `diaryx_*` properties.
    #[wasm_bindgen(js_name = "saveConfig")]
    pub fn save_config(&self, config_js: JsValue) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&*fs);

            // Find root index
            let root_path = ws
                .find_root_index_in_dir(Path::new("."))
                .await
                .ok()
                .flatten();

            // Fallback: try "workspace" directory for OPFS mode
            let root_path = match root_path {
                Some(p) => Some(p),
                None => ws
                    .find_root_index_in_dir(Path::new("workspace"))
                    .await
                    .ok()
                    .flatten(),
            };

            let root_path = match root_path {
                Some(p) if fs.exists(&p).await => p,
                _ => return Err(JsValue::from_str("No root index found to save config")),
            };

            // Parse config from JS
            let config_str = js_sys::JSON::stringify(&config_js)
                .map_err(|e| JsValue::from_str(&format!("Failed to stringify config: {:?}", e)))?;
            let config: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(&String::from(config_str))
                    .map_err(|e| JsValue::from_str(&format!("Invalid config JSON: {:?}", e)))?;

            // Read current file
            let content = fs
                .read_to_string(&root_path)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Parse frontmatter
            let mut parsed = frontmatter::parse_or_empty(&content)
                .map_err(|e| JsValue::from_str(&format!("Failed to parse frontmatter: {:?}", e)))?;

            // Update diaryx_* keys (skip default_workspace as it's derived)
            for (key, value) in config {
                if key != "default_workspace" {
                    let yaml_key = format!("diaryx_{}", key);
                    // Convert JSON value to YAML
                    let yaml_str = serde_json::to_string(&value)
                        .map_err(|e| JsValue::from_str(&format!("JSON error: {:?}", e)))?;
                    let yaml_val: serde_yaml::Value = serde_yaml::from_str(&yaml_str)
                        .map_err(|e| JsValue::from_str(&format!("YAML error: {:?}", e)))?;
                    parsed.frontmatter.insert(yaml_key, yaml_val);
                }
            }

            // Serialize and write back
            let new_content = frontmatter::serialize(&parsed.frontmatter, &parsed.body)
                .map_err(|e| JsValue::from_str(&format!("Failed to serialize: {:?}", e)))?;

            fs.write_file(&root_path, &new_content)
                .await
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

            // If root_path is a directory, don't try to use it as an index file directly
            // This prevents "workspace" (directory) from being returned as the root index path
            // if parse_index somehow succeeds or if we fall through incorrectly.
            let root_index = if ws.fs_ref().is_dir(&root_path).await {
                None
            } else {
                ws.find_root_index_in_dir(&root_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?
            };

            let root_index = match root_index {
                Some(idx) => idx,
                None => ws
                    .find_any_index_in_dir(&root_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?
                    .ok_or_else(|| {
                        JsValue::from_str(&format!("No workspace found at '{}'", workspace_path))
                    })?,
            };

            let max_depth = depth.map(|d| d as usize);
            let mut visited = HashSet::new();

            let tree = ws
                .build_tree_with_depth(&root_index, max_depth, &mut visited)
                .await
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
                return Err(JsValue::from_str(&format!(
                    "Workspace already exists at '{}'",
                    path
                )));
            }

            let content = format!(
                "---\ntitle: \"{}\"\ncontents: []\n---\n\n# {}\n",
                name, name
            );

            fs.create_dir_all(&PathBuf::from(&path))
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            fs.write_file(&index_path, &content)
                .await
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
                let name = path
                    .file_name()
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
            let tree = build_tree(&*fs, &root_path, show_hidden)
                .await
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
            let content = fs
                .read_to_string(&entry_path)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Parse frontmatter
            let parsed = frontmatter::parse_or_empty(&content)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Serialize frontmatter as Object (default is Map for IndexMap)
            let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
            let frontmatter_js = parsed
                .frontmatter
                .serialize(&serializer)
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
            fs.write_file(&entry_path, &content)
                .await
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
                fs.create_dir_all(parent)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
            }

            let title = title.unwrap_or_else(|| {
                entry_path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Untitled".to_string())
            });

            let content = format!("---\ntitle: \"{}\"\n---\n\n", title);

            fs.write_file(&entry_path, &content)
                .await
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
            fs.delete_file(&entry_path)
                .await
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
                fs.create_dir_all(parent)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
            }

            fs.move_file(&from, &to)
                .await
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
            let content = fs
                .read_to_string(&entry_path)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let parsed = frontmatter::parse_or_empty(&content)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
            parsed.frontmatter.serialize(&serializer).js_err()
        })
    }

    /// Set a frontmatter property.
    #[wasm_bindgen(js_name = "setFrontmatterProperty")]
    pub fn set_frontmatter_property(&self, path: String, key: String, value: JsValue) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let entry_path = PathBuf::from(&path);
            let content = fs
                .read_to_string(&entry_path)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let mut parsed = frontmatter::parse_or_empty(&content)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Convert JsValue to serde_yaml::Value
            let yaml_value: serde_yaml::Value = serde_wasm_bindgen::from_value(value)
                .map_err(|e| JsValue::from_str(&format!("Invalid value: {:?}", e)))?;

            parsed.frontmatter.insert(key, yaml_value);

            let new_content = frontmatter::serialize(&parsed.frontmatter, &parsed.body)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            fs.write_file(&entry_path, &new_content)
                .await
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
            let files = fs
                .list_md_files(&workspace_root)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let mut results = Vec::new();
            for file_path in files {
                if let Ok(Some(file_result)) = searcher.search_file(&file_path, &search_query).await
                {
                    if file_result.has_matches() {
                        results.push(JsSearchResult {
                            path: file_result.path.to_string_lossy().to_string(),
                            title: file_result.title,
                            matches: file_result
                                .matches
                                .into_iter()
                                .map(|m| JsSearchMatch {
                                    line_number: m.line_number,
                                    line_content: m.line_content,
                                    match_start: m.match_start,
                                    match_end: m.match_end,
                                })
                                .collect(),
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
    /// Uses depth limit of 2 to match tree view and improve performance.
    #[wasm_bindgen(js_name = "validateWorkspace")]
    pub fn validate_workspace(&self, workspace_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let validator = Validator::new(&*fs);
            // Use depth limit of 2 to match tree view (TREE_INITIAL_DEPTH)
            let results = validator
                .validate_workspace(&PathBuf::from(&workspace_path), Some(2))
                .await
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
            let content = fs
                .read_to_string(&PathBuf::from(&path))
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(JsValue::from_str(&content))
        })
    }

    /// Write content to a file.
    #[wasm_bindgen(js_name = "writeFile")]
    pub fn write_file(&self, path: String, content: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            fs.write_file(&PathBuf::from(&path), &content)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Read binary file.
    #[wasm_bindgen(js_name = "readBinary")]
    pub fn read_binary(&self, path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let data = fs
                .read_binary(&PathBuf::from(&path))
                .await
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
            fs.write_binary(&PathBuf::from(&path), &data_vec)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Delete a file.
    #[wasm_bindgen(js_name = "deleteFile")]
    pub fn delete_file(&self, path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            fs.delete_file(&PathBuf::from(&path))
                .await
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
            let result = ws
                .attach_and_move_entry_to_parent(
                    &PathBuf::from(&entry_path),
                    &PathBuf::from(&parent_path),
                )
                .await
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
            let result = ws
                .convert_to_index(&PathBuf::from(&path))
                .await
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
            let result = ws
                .convert_to_leaf(&PathBuf::from(&path))
                .await
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
            let result = ws
                .create_child_entry(&PathBuf::from(&parent_path), None)
                .await
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
            let result = ws
                .rename_entry(&PathBuf::from(&path), &new_filename)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(JsValue::from_str(&result.to_string_lossy()))
        })
    }

    /// Duplicate an entry, creating a copy.
    #[wasm_bindgen(js_name = "duplicateEntry")]
    pub fn duplicate_entry(&self, path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&*fs);
            let result = ws
                .duplicate_entry(&PathBuf::from(&path))
                .await
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
            let month_name = today.format("%B").to_string();
            let day_filename = today.format("%Y-%m-%d").to_string();

            // Path components
            let workspace_path = PathBuf::from("workspace");
            let daily_base = workspace_path.join("Daily");
            let year_dir = daily_base.join(&year);
            let month_dir = year_dir.join(&month);

            // Create directory structure: workspace/Daily/YYYY/MM/
            fs.create_dir_all(&month_dir)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Ensure Daily index exists
            let daily_index_path = daily_base.join("index.md");
            if !fs.exists(&daily_index_path).await {
                let content = "---\ntitle: \"Daily\"\npart_of: \"../index.md\"\ncontents: []\n---\n\n# Daily\n";
                fs.write_file(&daily_index_path, content)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                // Add Daily index to root workspace contents
                let root_index_path = workspace_path.join("index.md");
                if fs.exists(&root_index_path).await {
                    Self::add_to_contents(&*fs, &root_index_path, "Daily/index.md").await?;
                }
            }

            // Ensure year index exists
            let year_index_path = year_dir.join("index.md");
            if !fs.exists(&year_index_path).await {
                let content = format!(
                    "---\ntitle: \"{}\"\npart_of: \"../index.md\"\ncontents: []\n---\n\n# {}\n",
                    year, year
                );
                fs.write_file(&year_index_path, &content)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                // Add year index to Daily index contents
                Self::add_to_contents(&*fs, &daily_index_path, &format!("{}/index.md", year))
                    .await?;
            }

            // Ensure month index exists
            let month_index_path = month_dir.join("index.md");
            let month_title = format!("{} {}", month_name, year);
            if !fs.exists(&month_index_path).await {
                let content = format!(
                    "---\ntitle: \"{}\"\npart_of: \"../index.md\"\ncontents: []\n---\n\n# {}\n",
                    month_title, month_title
                );
                fs.write_file(&month_index_path, &content)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                // Add month index to year index contents
                Self::add_to_contents(&*fs, &year_index_path, &format!("{}/index.md", month))
                    .await?;
            }

            // Create the daily entry
            let daily_path = month_dir.join(format!("{}.md", day_filename));

            if !fs.exists(&daily_path).await {
                let title = today.format("%B %d, %Y").to_string();
                let content = format!(
                    "---\ntitle: \"{}\"\ncreated: \"{}\"\npart_of: \"index.md\"\n---\n\n",
                    title,
                    chrono::Utc::now().to_rfc3339()
                );
                fs.write_file(&daily_path, &content)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                // Add to month index contents
                Self::add_to_contents(&*fs, &month_index_path, &format!("{}.md", day_filename))
                    .await?;
            }

            Ok(JsValue::from_str(&daily_path.to_string_lossy()))
        })
    }

    /// Helper to add an entry to an index's contents list.
    async fn add_to_contents(
        fs: &StorageBackend,
        index_path: &Path,
        entry: &str,
    ) -> Result<(), JsValue> {
        if !fs.exists(index_path).await {
            return Ok(());
        }

        let index_content = fs
            .read_to_string(index_path)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        if let Ok(mut parsed) = frontmatter::parse_or_empty(&index_content) {
            let contents = parsed
                .frontmatter
                .get("contents")
                .and_then(|v| v.as_sequence())
                .cloned()
                .unwrap_or_default();

            // Check if already in contents
            let already_exists = contents
                .iter()
                .any(|v| v.as_str().map(|s| s == entry).unwrap_or(false));

            if !already_exists {
                let mut new_contents = contents;
                new_contents.push(serde_yaml::Value::String(entry.to_string()));
                parsed.frontmatter.insert(
                    "contents".to_string(),
                    serde_yaml::Value::Sequence(new_contents),
                );

                let new_index_content = frontmatter::serialize(&parsed.frontmatter, &parsed.body)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                fs.write_file(index_path, &new_index_content)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
            }
        }

        Ok(())
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
            let content = fs
                .read_to_string(&entry_path)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let mut parsed = frontmatter::parse_or_empty(&content)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            parsed.frontmatter.shift_remove(&key);

            let new_content = frontmatter::serialize(&parsed.frontmatter, &parsed.body)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            fs.write_file(&entry_path, &new_content)
                .await
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
            let templates_dir =
                PathBuf::from(workspace_path.as_deref().unwrap_or("workspace")).join("_templates");

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
            let templates_dir =
                PathBuf::from(workspace_path.as_deref().unwrap_or("workspace")).join("_templates");
            let template_path = templates_dir.join(format!("{}.md", name));

            if fs.exists(&template_path).await {
                let content = fs
                    .read_to_string(&template_path)
                    .await
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
            fs.create_dir_all(&templates_dir)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let template_path = templates_dir.join(format!("{}.md", name));
            fs.write_file(&template_path, &content)
                .await
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

            fs.delete_file(&template_path)
                .await
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
            let content = fs
                .read_to_string(&path)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let parsed = frontmatter::parse_or_empty(&content)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let attachments: Vec<String> = parsed
                .frontmatter
                .get("attachments")
                .and_then(|v| {
                    if let serde_yaml::Value::Sequence(seq) = v {
                        Some(
                            seq.iter()
                                .filter_map(|item| item.as_str().map(String::from))
                                .collect(),
                        )
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
    pub fn upload_attachment(
        &self,
        entry_path: String,
        filename: String,
        data_base64: String,
    ) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            // Decode base64
            let data = base64_decode(&data_base64)
                .map_err(|e| JsValue::from_str(&format!("Base64 decode error: {}", e)))?;

            let entry_dir = PathBuf::from(&entry_path)
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            let attachments_dir = entry_dir.join("_attachments");
            let attachment_path = attachments_dir.join(&filename);

            fs.create_dir_all(&attachments_dir)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            fs.write_binary(&attachment_path, &data)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let relative_path = format!("_attachments/{}", filename);

            // Add to frontmatter
            let entry_path_buf = PathBuf::from(&entry_path);
            let content = fs
                .read_to_string(&entry_path_buf)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let mut parsed = frontmatter::parse_or_empty(&content)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let mut attachments: Vec<serde_yaml::Value> = parsed
                .frontmatter
                .get("attachments")
                .and_then(|v| {
                    if let serde_yaml::Value::Sequence(seq) = v {
                        Some(seq.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_default();

            if !attachments
                .iter()
                .any(|a| a.as_str() == Some(&relative_path))
            {
                attachments.push(serde_yaml::Value::String(relative_path.clone()));
                parsed.frontmatter.insert(
                    "attachments".to_string(),
                    serde_yaml::Value::Sequence(attachments),
                );

                let new_content = frontmatter::serialize(&parsed.frontmatter, &parsed.body)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                fs.write_file(&entry_path_buf, &new_content)
                    .await
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
            let entry_dir = PathBuf::from(&entry_path)
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            let full_path = entry_dir.join(&attachment_path);

            // Delete the file
            if fs.exists(&full_path).await {
                fs.delete_file(&full_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
            }

            // Remove from frontmatter
            let entry_path_buf = PathBuf::from(&entry_path);
            let content = fs
                .read_to_string(&entry_path_buf)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let mut parsed = frontmatter::parse_or_empty(&content)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            if let Some(serde_yaml::Value::Sequence(attachments)) =
                parsed.frontmatter.get_mut("attachments")
            {
                attachments.retain(|a| a.as_str() != Some(&attachment_path));

                let new_content = frontmatter::serialize(&parsed.frontmatter, &parsed.body)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                fs.write_file(&entry_path_buf, &new_content)
                    .await
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
            use diaryx_core::path_utils::normalize_path;

            let entry_dir = PathBuf::from(&entry_path)
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            // Normalize path to handle .. components (important for inherited attachments)
            let full_path = normalize_path(&entry_dir.join(&attachment_path));

            let data = fs
                .read_binary(&full_path)
                .await
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

            // Determine the starting index file:
            // - If root is a file and parseable as an index, use it directly
            // - Otherwise, treat root as a directory and find a root index in it
            let start_index = if !fs.is_dir(&root).await {
                if ws.parse_index(&root).await.is_ok() {
                    Some(root.clone())
                } else {
                    None
                }
            } else {
                ws.find_root_index_in_dir(&root).await.ok().flatten()
            };

            if let Some(root_index) = start_index {
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

            // Collect files with audience filtering
            // audience_filter: "*" means include all (no filtering), otherwise filter by audience
            async fn collect_files<FS: AsyncFileSystem>(
                ws: &Workspace<FS>,
                path: &Path,
                root_dir: &Path,
                audience_filter: &str,
                inherited_visible: bool,
                included: &mut Vec<serde_json::Value>,
                visited: &mut HashSet<PathBuf>,
            ) {
                if visited.contains(path) {
                    return;
                }
                visited.insert(path.to_path_buf());

                if let Ok(index) = ws.parse_index(path).await {
                    // Check audience visibility
                    let visible = if audience_filter == "*" {
                        // Export all - only exclude private files
                        !index.frontmatter.is_private()
                    } else {
                        // Check if visible to specific audience
                        match index.frontmatter.is_visible_to(audience_filter) {
                            Some(true) => true,
                            Some(false) => false,
                            None => inherited_visible, // Inherit from parent
                        }
                    };

                    if visible {
                        let relative = pathdiff::diff_paths(path, root_dir)
                            .unwrap_or_else(|| path.to_path_buf());
                        included.push(serde_json::json!({
                            "path": path.to_string_lossy(),
                            "relative_path": relative.to_string_lossy()
                        }));
                    }

                    // Always traverse children (they might be visible even if parent isn't)
                    let dir = index.directory().unwrap_or(Path::new(""));
                    for child_ref in index.frontmatter.contents_list() {
                        let child_path = dir.join(child_ref);
                        Box::pin(collect_files(
                            ws,
                            &child_path,
                            root_dir,
                            audience_filter,
                            visible, // Pass visibility to children
                            included,
                            visited,
                        ))
                        .await;
                    }
                }
            }

            // Determine the starting index file:
            // - If root is a file and parseable as an index, use it directly
            // - Otherwise, treat root as a directory and find a root index in it
            let start_index = if !fs.is_dir(&root).await {
                // It's a file - try to parse it as an index
                if ws.parse_index(&root).await.is_ok() {
                    Some(root.clone())
                } else {
                    None
                }
            } else {
                // It's a directory - find root index in it
                ws.find_root_index_in_dir(&root).await.ok().flatten()
            };

            if let Some(root_index) = start_index {
                let root_dir = root_index.parent().unwrap_or(&root);
                // Start with inherited_visible = true (root is visible by default)
                collect_files(
                    &ws,
                    &root_index,
                    root_dir,
                    &audience,
                    true,
                    &mut included,
                    &mut visited,
                )
                .await;
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
    pub fn export_to_memory(&self, root_path: String, audience: String) -> Promise {
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
                audience_filter: &str,
                inherited_visible: bool,
                files: &mut Vec<serde_json::Value>,
                visited: &mut HashSet<PathBuf>,
            ) {
                if visited.contains(path) {
                    return;
                }
                visited.insert(path.to_path_buf());

                if let Ok(index) = ws.parse_index(path).await {
                    // Check audience visibility
                    let visible = if audience_filter == "*" {
                        !index.frontmatter.is_private()
                    } else {
                        match index.frontmatter.is_visible_to(audience_filter) {
                            Some(true) => true,
                            Some(false) => false,
                            None => inherited_visible,
                        }
                    };

                    if visible {
                        if let Ok(content) = ws.fs_ref().read_to_string(path).await {
                            let relative = pathdiff::diff_paths(path, root_dir)
                                .unwrap_or_else(|| path.to_path_buf());
                            files.push(serde_json::json!({
                                "path": relative.to_string_lossy(),
                                "content": content
                            }));
                        }
                    }

                    let dir = index.directory().unwrap_or(Path::new(""));
                    for child_ref in index.frontmatter.contents_list() {
                        let child_path = dir.join(child_ref);
                        Box::pin(collect_files(
                            ws,
                            &child_path,
                            root_dir,
                            audience_filter,
                            visible,
                            files,
                            visited,
                        ))
                        .await;
                    }
                }
            }

            // Determine the starting index file
            let start_index = if !fs.is_dir(&root).await {
                if ws.parse_index(&root).await.is_ok() {
                    Some(root.clone())
                } else {
                    None
                }
            } else {
                ws.find_root_index_in_dir(&root).await.ok().flatten()
            };

            if let Some(root_index) = start_index {
                let root_dir = root_index.parent().unwrap_or(&root);
                collect_files(
                    &ws,
                    &root_index,
                    root_dir,
                    &audience,
                    true,
                    &mut files,
                    &mut visited,
                )
                .await;
            }

            serde_wasm_bindgen::to_value(&files).js_err()
        })
    }

    /// Export files to memory as HTML.
    #[wasm_bindgen(js_name = "exportToHtml")]
    pub fn export_to_html(&self, root_path: String, audience: String) -> Promise {
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
                audience_filter: &str,
                inherited_visible: bool,
                files: &mut Vec<serde_json::Value>,
                visited: &mut HashSet<PathBuf>,
            ) {
                if visited.contains(path) {
                    return;
                }
                visited.insert(path.to_path_buf());

                if let Ok(index) = ws.parse_index(path).await {
                    // Check audience visibility
                    let visible = if audience_filter == "*" {
                        !index.frontmatter.is_private()
                    } else {
                        match index.frontmatter.is_visible_to(audience_filter) {
                            Some(true) => true,
                            Some(false) => false,
                            None => inherited_visible,
                        }
                    };

                    if visible {
                        if let Ok(content) = ws.fs_ref().read_to_string(path).await {
                            let relative = pathdiff::diff_paths(path, root_dir)
                                .unwrap_or_else(|| path.to_path_buf());
                            // Simple markdown to HTML - just wrap in pre for now
                            let html = format!(
                                "<html><body><pre>{}</pre></body></html>",
                                content.replace("<", "&lt;").replace(">", "&gt;")
                            );
                            files.push(serde_json::json!({
                                "path": relative.with_extension("html").to_string_lossy(),
                                "content": html
                            }));
                        }
                    }

                    let dir = index.directory().unwrap_or(Path::new(""));
                    for child_ref in index.frontmatter.contents_list() {
                        let child_path = dir.join(child_ref);
                        Box::pin(collect_files(
                            ws,
                            &child_path,
                            root_dir,
                            audience_filter,
                            visible,
                            files,
                            visited,
                        ))
                        .await;
                    }
                }
            }

            // Determine the starting index file
            let start_index = if !fs.is_dir(&root).await {
                if ws.parse_index(&root).await.is_ok() {
                    Some(root.clone())
                } else {
                    None
                }
            } else {
                ws.find_root_index_in_dir(&root).await.ok().flatten()
            };

            if let Some(root_index) = start_index {
                let root_dir = root_index.parent().unwrap_or(&root);
                collect_files(
                    &ws,
                    &root_index,
                    root_dir,
                    &audience,
                    true,
                    &mut files,
                    &mut visited,
                )
                .await;
            }

            serde_wasm_bindgen::to_value(&files).js_err()
        })
    }

    /// Export binary attachments.
    #[wasm_bindgen(js_name = "exportBinaryAttachments")]
    pub fn export_binary_attachments(&self, root_path: String, audience: String) -> Promise {
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
                audience_filter: &str,
                inherited_visible: bool,
                binary_files: &mut Vec<serde_json::Value>,
                visited_entries: &mut HashSet<PathBuf>,
                visited_attachment_dirs: &mut HashSet<PathBuf>,
            ) {
                if visited_entries.contains(entry_path) {
                    return;
                }
                visited_entries.insert(entry_path.to_path_buf());

                if let Ok(index) = ws.parse_index(entry_path).await {
                    // Check audience visibility
                    let visible = if audience_filter == "*" {
                        !index.frontmatter.is_private()
                    } else {
                        match index.frontmatter.is_visible_to(audience_filter) {
                            Some(true) => true,
                            Some(false) => false,
                            None => inherited_visible,
                        }
                    };

                    let dir = index.directory().unwrap_or(Path::new(""));

                    // Only include attachments if entry is visible
                    if visible {
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
                    }

                    // Recurse into children (they might be visible even if parent isn't)
                    for child_ref in index.frontmatter.contents_list() {
                        let child_path = dir.join(child_ref);
                        Box::pin(collect_attachments(
                            ws,
                            &child_path,
                            root_dir,
                            audience_filter,
                            visible,
                            binary_files,
                            visited_entries,
                            visited_attachment_dirs,
                        ))
                        .await;
                    }
                }
            }

            // Determine the starting index file
            let start_index = if !fs.is_dir(&root).await {
                if ws.parse_index(&root).await.is_ok() {
                    Some(root.clone())
                } else {
                    None
                }
            } else {
                ws.find_root_index_in_dir(&root).await.ok().flatten()
            };

            if let Some(root_index) = start_index {
                let root_dir = root_index.parent().unwrap_or(&root);
                collect_attachments(
                    &ws,
                    &root_index,
                    root_dir,
                    &audience,
                    true,
                    &mut binary_files,
                    &mut visited_entries,
                    &mut visited_attachment_dirs,
                )
                .await;
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
            let results = validator
                .validate_file(&PathBuf::from(&file_path))
                .await
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
            }))
            .js_err()
        })
    }

    /// Fix a broken contents reference by removing it.
    #[wasm_bindgen(js_name = "fixBrokenContentsRef")]
    pub fn fix_broken_contents_ref(&self, index_path: String, target: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = diaryx_core::validate::ValidationFixer::new(&*fs);
            let result = fixer
                .fix_broken_contents_ref(&PathBuf::from(&index_path), &target)
                .await;

            serde_wasm_bindgen::to_value(&serde_json::json!({
                "success": result.success,
                "message": result.message
            }))
            .js_err()
        })
    }

    /// Fix a broken attachment reference by removing it.
    #[wasm_bindgen(js_name = "fixBrokenAttachment")]
    pub fn fix_broken_attachment(&self, file_path: String, attachment: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = diaryx_core::validate::ValidationFixer::new(&*fs);
            let result = fixer
                .fix_broken_attachment(&PathBuf::from(&file_path), &attachment)
                .await;

            serde_wasm_bindgen::to_value(&serde_json::json!({
                "success": result.success,
                "message": result.message
            }))
            .js_err()
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
            let result = fixer
                .fix_non_portable_path(
                    &PathBuf::from(&file_path),
                    &property,
                    &old_value,
                    &new_value,
                )
                .await;

            serde_wasm_bindgen::to_value(&serde_json::json!({
                "success": result.success,
                "message": result.message
            }))
            .js_err()
        })
    }

    /// Add an unlisted file to an index's contents.
    #[wasm_bindgen(js_name = "fixUnlistedFile")]
    pub fn fix_unlisted_file(&self, index_path: String, file_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = diaryx_core::validate::ValidationFixer::new(&*fs);
            let result = fixer
                .fix_unlisted_file(&PathBuf::from(&index_path), &PathBuf::from(&file_path))
                .await;

            serde_wasm_bindgen::to_value(&serde_json::json!({
                "success": result.success,
                "message": result.message
            }))
            .js_err()
        })
    }

    /// Add an orphan binary file to an index's attachments.
    #[wasm_bindgen(js_name = "fixOrphanBinaryFile")]
    pub fn fix_orphan_binary_file(&self, index_path: String, file_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = diaryx_core::validate::ValidationFixer::new(&*fs);
            let result = fixer
                .fix_orphan_binary_file(&PathBuf::from(&index_path), &PathBuf::from(&file_path))
                .await;

            serde_wasm_bindgen::to_value(&serde_json::json!({
                "success": result.success,
                "message": result.message
            }))
            .js_err()
        })
    }

    /// Fix a missing part_of by setting it to point to the given index.
    #[wasm_bindgen(js_name = "fixMissingPartOf")]
    pub fn fix_missing_part_of(&self, file_path: String, index_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = diaryx_core::validate::ValidationFixer::new(&*fs);
            let result = fixer
                .fix_missing_part_of(&PathBuf::from(&file_path), &PathBuf::from(&index_path))
                .await;

            serde_wasm_bindgen::to_value(&serde_json::json!({
                "success": result.success,
                "message": result.message
            }))
            .js_err()
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
                .map_err(|e| {
                    JsValue::from_str(&format!("Failed to parse validation result: {:?}", e))
                })?;

            let mut error_fixes = Vec::new();
            let mut warning_fixes = Vec::new();

            // Fix errors
            if let Some(errors) = result.get("errors").and_then(|e| e.as_array()) {
                for err in errors {
                    let fix_result = match err.get("type").and_then(|t| t.as_str()) {
                        Some("BrokenPartOf") => {
                            if let Some(file) = err.get("file").and_then(|f| f.as_str()) {
                                Some(fixer.fix_broken_part_of(&PathBuf::from(file)).await)
                            } else {
                                None
                            }
                        }
                        Some("BrokenContentsRef") => {
                            if let (Some(index), Some(target)) = (
                                err.get("index").and_then(|i| i.as_str()),
                                err.get("target").and_then(|t| t.as_str()),
                            ) {
                                Some(
                                    fixer
                                        .fix_broken_contents_ref(&PathBuf::from(index), target)
                                        .await,
                                )
                            } else {
                                None
                            }
                        }
                        Some("BrokenAttachment") => {
                            if let (Some(file), Some(attachment)) = (
                                err.get("file").and_then(|f| f.as_str()),
                                err.get("attachment").and_then(|a| a.as_str()),
                            ) {
                                Some(
                                    fixer
                                        .fix_broken_attachment(&PathBuf::from(file), attachment)
                                        .await,
                                )
                            } else {
                                None
                            }
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
                                warn.get("file").and_then(|f| f.as_str()),
                            ) {
                                Some(
                                    fixer
                                        .fix_unlisted_file(
                                            &PathBuf::from(index),
                                            &PathBuf::from(file),
                                        )
                                        .await,
                                )
                            } else {
                                None
                            }
                        }
                        Some("NonPortablePath") => {
                            if let (Some(file), Some(property), Some(value), Some(suggested)) = (
                                warn.get("file").and_then(|f| f.as_str()),
                                warn.get("property").and_then(|p| p.as_str()),
                                warn.get("value").and_then(|v| v.as_str()),
                                warn.get("suggested").and_then(|s| s.as_str()),
                            ) {
                                Some(
                                    fixer
                                        .fix_non_portable_path(
                                            &PathBuf::from(file),
                                            property,
                                            value,
                                            suggested,
                                        )
                                        .await,
                                )
                            } else {
                                None
                            }
                        }
                        Some("OrphanBinaryFile") => {
                            if let (Some(file), Some(index)) = (
                                warn.get("file").and_then(|f| f.as_str()),
                                warn.get("suggested_index").and_then(|i| i.as_str()),
                            ) {
                                Some(
                                    fixer
                                        .fix_orphan_binary_file(
                                            &PathBuf::from(index),
                                            &PathBuf::from(file),
                                        )
                                        .await,
                                )
                            } else {
                                None
                            }
                        }
                        Some("MissingPartOf") => {
                            if let (Some(file), Some(index)) = (
                                warn.get("file").and_then(|f| f.as_str()),
                                warn.get("suggested_index").and_then(|i| i.as_str()),
                            ) {
                                Some(
                                    fixer
                                        .fix_missing_part_of(
                                            &PathBuf::from(file),
                                            &PathBuf::from(index),
                                        )
                                        .await,
                                )
                            } else {
                                None
                            }
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

            let total_fixed = error_fixes
                .iter()
                .filter(|r| r.get("success").and_then(|s| s.as_bool()).unwrap_or(false))
                .count()
                + warning_fixes
                    .iter()
                    .filter(|r| r.get("success").and_then(|s| s.as_bool()).unwrap_or(false))
                    .count();
            let total_failed = error_fixes
                .iter()
                .filter(|r| !r.get("success").and_then(|s| s.as_bool()).unwrap_or(false))
                .count()
                + warning_fixes
                    .iter()
                    .filter(|r| !r.get("success").and_then(|s| s.as_bool()).unwrap_or(false))
                    .count();

            let summary = serde_json::json!({
                "error_fixes": error_fixes,
                "warning_fixes": warning_fixes,
                "total_fixed": total_fixed,
                "total_failed": total_failed
            });

            serde_wasm_bindgen::to_value(&summary).js_err()
        })
    }

    // ========================================================================
    // CRDT Sync API
    // ========================================================================
    //
    // Note: CRDT operations are available through the execute() command API.
    // The sync commands (CreateSyncStep1, HandleSyncMessage, etc.) require
    // a CRDT-enabled Diaryx instance. For full sync support in the browser,
    // use the JavaScript sync adapters that work with the execute() API.
    //
    // Example usage via execute():
    // ```javascript
    // // Get sync state
    // const response = await backend.executeJs({
    //   type: 'CreateSyncStep1',
    //   params: { doc_name: 'workspace' }
    // });
    //
    // // Handle sync message from server
    // const response = await backend.executeJs({
    //   type: 'HandleSyncMessage',
    //   params: { doc_name: 'workspace', message: Array.from(messageBytes) }
    // });
    // ```
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
