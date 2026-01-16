//! Unified async backend for WASM with native OPFS/IndexedDB storage.
//!
//! This module provides a single entry point for all workspace operations,
//! working directly with native storage backends (no InMemoryFileSystem).
//!
//! ## API: `execute()` / `executeJs()`
//!
//! All operations go through the unified command API:
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
//! ## Special Methods
//!
//! A few methods are kept outside the command API for specific reasons:
//! - `getConfig` / `saveConfig`: WASM-specific config stored in root frontmatter
//! - `readBinary` / `writeBinary`: Efficient Uint8Array handling without base64 overhead

use std::io::Result as IoResult;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use diaryx_core::crdt::{CrdtStorage, MemoryStorage};
use diaryx_core::diaryx::Diaryx;
use diaryx_core::frontmatter;
use diaryx_core::fs::AsyncFileSystem;
use diaryx_core::workspace::Workspace;
use js_sys::Promise;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::fsa_fs::FsaFileSystem;
use crate::indexeddb_fs::IndexedDbFileSystem;
use crate::opfs_fs::OpfsFileSystem;

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
///
/// ## Usage
///
/// All operations go through `execute()` or `executeJs()`:
///
/// ```javascript
/// const backend = await DiaryxBackend.createOpfs();
/// const response = await backend.executeJs({
///   type: 'GetEntry',
///   params: { path: 'workspace/notes.md' }
/// });
/// ```
#[wasm_bindgen]
pub struct DiaryxBackend {
    fs: Rc<StorageBackend>,
    /// CRDT storage for sync and history features.
    crdt_storage: Arc<dyn CrdtStorage>,
}

#[wasm_bindgen]
impl DiaryxBackend {
    // ========================================================================
    // Factory Methods
    // ========================================================================

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
    /// This is the primary unified API for all operations.
    ///
    /// ## Example
    /// ```javascript
    /// const command = { type: 'GetEntry', params: { path: 'workspace/notes.md' } };
    /// const responseJson = await backend.execute(JSON.stringify(command));
    /// const response = JSON.parse(responseJson);
    /// ```
    #[wasm_bindgen]
    pub async fn execute(&self, command_json: &str) -> std::result::Result<String, JsValue> {
        use diaryx_core::Command;

        // Parse the command from JSON
        let cmd: Command = serde_json::from_str(command_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid command JSON: {}", e)))?;

        // Create a Diaryx instance with CRDT support, loading existing state from storage.
        // This is critical for P2P sync - we must load updates stored by previous commands.
        let diaryx = Diaryx::with_crdt_load((*self.fs).clone(), Arc::clone(&self.crdt_storage))
            .map_err(|e| JsValue::from_str(&format!("Failed to load CRDT state: {}", e)))?;

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
        use diaryx_core::Command;

        // Parse command from JS object
        let cmd: Command = serde_wasm_bindgen::from_value(command)?;

        // Create a Diaryx instance with CRDT support, loading existing state from storage.
        // This is critical for P2P sync - we must load updates stored by previous commands.
        let diaryx = Diaryx::with_crdt_load((*self.fs).clone(), Arc::clone(&self.crdt_storage))
            .map_err(|e| JsValue::from_str(&format!("Failed to load CRDT state: {}", e)))?;

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
    // Config (WASM-specific - stored in root index frontmatter)
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
    // Binary Operations (kept for efficiency - no base64 overhead)
    // ========================================================================

    /// Read binary file.
    /// 
    /// Returns data as Uint8Array for efficient handling without base64 encoding.
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
    /// 
    /// Accepts Uint8Array for efficient handling without base64 encoding.
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
}
