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

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Result as IoResult;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use diaryx_core::crdt::{BodyDocManager, CrdtStorage, MemoryStorage, WorkspaceCrdt};
use diaryx_core::diaryx::Diaryx;
use diaryx_core::frontmatter;
use diaryx_core::fs::{
    AsyncFileSystem, CallbackRegistry, CrdtFs, EventEmittingFs, FileSystemEvent,
    InMemoryFileSystem, SyncToAsyncFs,
};
use diaryx_core::workspace::Workspace;
use js_sys::Promise;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::fsa_fs::FsaFileSystem;
use crate::indexeddb_fs::IndexedDbFileSystem;
use crate::opfs_fs::OpfsFileSystem;
use crate::wasm_sqlite_storage::WasmSqliteStorage;

// ============================================================================
// Storage Backend Enum
// ============================================================================

/// Internal enum to hold either storage backend
enum StorageBackend {
    Opfs(OpfsFileSystem),
    IndexedDb(IndexedDbFileSystem),
    /// File System Access API - user-selected directory on their real filesystem
    Fsa(FsaFileSystem),
    /// In-memory filesystem - used for guest mode in share sessions (web only)
    InMemory(SyncToAsyncFs<InMemoryFileSystem>),
}

impl Clone for StorageBackend {
    fn clone(&self) -> Self {
        match self {
            StorageBackend::Opfs(fs) => StorageBackend::Opfs(fs.clone()),
            StorageBackend::IndexedDb(fs) => StorageBackend::IndexedDb(fs.clone()),
            StorageBackend::Fsa(fs) => StorageBackend::Fsa(fs.clone()),
            StorageBackend::InMemory(fs) => StorageBackend::InMemory(fs.clone()),
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
            StorageBackend::InMemory(fs) => fs.read_to_string(path),
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
            StorageBackend::InMemory(fs) => fs.write_file(path, content),
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
            StorageBackend::InMemory(fs) => fs.create_new(path, content),
        }
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> diaryx_core::fs::BoxFuture<'a, IoResult<()>> {
        match self {
            StorageBackend::Opfs(fs) => fs.delete_file(path),
            StorageBackend::IndexedDb(fs) => fs.delete_file(path),
            StorageBackend::Fsa(fs) => fs.delete_file(path),
            StorageBackend::InMemory(fs) => fs.delete_file(path),
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
            StorageBackend::InMemory(fs) => fs.list_md_files(dir),
        }
    }

    fn exists<'a>(&'a self, path: &'a Path) -> diaryx_core::fs::BoxFuture<'a, bool> {
        match self {
            StorageBackend::Opfs(fs) => fs.exists(path),
            StorageBackend::IndexedDb(fs) => fs.exists(path),
            StorageBackend::Fsa(fs) => fs.exists(path),
            StorageBackend::InMemory(fs) => fs.exists(path),
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
            StorageBackend::InMemory(fs) => fs.create_dir_all(path),
        }
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> diaryx_core::fs::BoxFuture<'a, bool> {
        match self {
            StorageBackend::Opfs(fs) => fs.is_dir(path),
            StorageBackend::IndexedDb(fs) => fs.is_dir(path),
            StorageBackend::Fsa(fs) => fs.is_dir(path),
            StorageBackend::InMemory(fs) => fs.is_dir(path),
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
            StorageBackend::InMemory(fs) => fs.move_file(from, to),
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
            StorageBackend::InMemory(fs) => fs.read_binary(path),
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
            StorageBackend::InMemory(fs) => fs.write_binary(path, content),
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
            StorageBackend::InMemory(fs) => fs.list_files(dir),
        }
    }

    fn get_modified_time<'a>(
        &'a self,
        path: &'a Path,
    ) -> diaryx_core::fs::BoxFuture<'a, Option<i64>> {
        match self {
            StorageBackend::Opfs(fs) => fs.get_modified_time(path),
            StorageBackend::IndexedDb(fs) => fs.get_modified_time(path),
            StorageBackend::Fsa(fs) => fs.get_modified_time(path),
            StorageBackend::InMemory(fs) => fs.get_modified_time(path),
        }
    }
}

// ============================================================================
// WASM-specific Callback Registry
// ============================================================================

/// WASM-specific callback registry for filesystem events.
///
/// Unlike the thread-safe `CallbackRegistry` in diaryx_core, this version
/// stores JS functions directly using `Rc<RefCell>` since WASM is single-threaded.
struct WasmCallbackRegistry {
    callbacks: RefCell<HashMap<u64, js_sys::Function>>,
    next_id: AtomicU64,
}

impl WasmCallbackRegistry {
    fn new() -> Self {
        Self {
            callbacks: RefCell::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    fn subscribe(&self, callback: js_sys::Function) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.callbacks.borrow_mut().insert(id, callback);
        id
    }

    fn unsubscribe(&self, id: u64) -> bool {
        self.callbacks.borrow_mut().remove(&id).is_some()
    }

    fn emit(&self, event: &FileSystemEvent) {
        if let Ok(json) = serde_json::to_string(event) {
            let callbacks = self.callbacks.borrow();
            for callback in callbacks.values() {
                let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(&json));
            }
        }
    }

    fn subscriber_count(&self) -> usize {
        self.callbacks.borrow().len()
    }
}

// ============================================================================
// Thread-local Bridge for Event Forwarding
// ============================================================================

// Thread-local storage for the WASM event registry.
// This allows the Rust CallbackRegistry to forward events to JS subscribers.
// Safe because WASM is single-threaded.
thread_local! {
    static WASM_EVENT_REGISTRY: RefCell<Option<Rc<WasmCallbackRegistry>>> = RefCell::new(None);
}

/// Create a bridge callback that forwards events from Rust's CallbackRegistry
/// to the WASM-specific WasmCallbackRegistry (which holds JS functions).
fn create_event_bridge() -> Arc<dyn Fn(&FileSystemEvent) + Send + Sync> {
    Arc::new(|event: &FileSystemEvent| {
        WASM_EVENT_REGISTRY.with(|reg| {
            if let Some(registry) = reg.borrow().as_ref() {
                registry.emit(event);
            }
        });
    })
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
    /// Filesystem stack: EventEmittingFs<CrdtFs<StorageBackend>>
    /// - EventEmittingFs: Emits events to JS subscribers
    /// - CrdtFs: Automatically updates CRDT on file operations
    /// - StorageBackend: OPFS, IndexedDB, FSA, or InMemory
    fs: Rc<EventEmittingFs<CrdtFs<StorageBackend>>>,
    /// CRDT storage for sync and history features.
    crdt_storage: Arc<dyn CrdtStorage>,
    /// Workspace CRDT for file metadata sync.
    workspace_crdt: Arc<WorkspaceCrdt>,
    /// Body document manager for file content sync.
    body_doc_manager: Arc<BodyDocManager>,
    /// WASM-specific event callback registry for JS subscribers.
    wasm_event_registry: Rc<WasmCallbackRegistry>,
    /// Rust event registry that bridges to WASM registry.
    rust_event_registry: Arc<CallbackRegistry>,
}

#[wasm_bindgen]
impl DiaryxBackend {
    // ========================================================================
    // Factory Methods
    // ========================================================================

    /// Create a new DiaryxBackend with OPFS storage.
    ///
    /// This attempts to use persistent SQLite-based CRDT storage (via sql.js).
    /// If SQLite storage is not available (JS bridge not initialized), falls back
    /// to in-memory CRDT storage.
    ///
    /// For persistent CRDT storage, call `initializeSqliteStorage()` in JavaScript
    /// before creating the backend:
    ///
    /// ```javascript
    /// import { initializeSqliteStorage } from './lib/storage/sqliteStorageBridge.js';
    /// await initializeSqliteStorage();
    /// const backend = await DiaryxBackend.createOpfs();
    /// ```
    #[wasm_bindgen(js_name = "createOpfs")]
    pub async fn create_opfs() -> std::result::Result<DiaryxBackend, JsValue> {
        let opfs = OpfsFileSystem::create().await?;
        let storage_backend = StorageBackend::Opfs(opfs);

        // Create event registries
        let wasm_event_registry = Rc::new(WasmCallbackRegistry::new());
        let rust_event_registry = Arc::new(CallbackRegistry::new());

        // Set up thread-local for bridge (safe because WASM is single-threaded)
        WASM_EVENT_REGISTRY.with(|reg| {
            *reg.borrow_mut() = Some(Rc::clone(&wasm_event_registry));
        });

        // Register bridge callback to forward Rust events to JS
        rust_event_registry.subscribe(create_event_bridge());

        // Try to use persistent SQLite storage, fall back to memory storage
        let crdt_storage: Arc<dyn CrdtStorage> = match WasmSqliteStorage::new() {
            Ok(storage) => {
                log::info!("Using persistent SQLite CRDT storage");
                Arc::new(storage)
            }
            Err(e) => {
                log::warn!(
                    "SQLite CRDT storage not available, using memory storage: {:?}",
                    e
                );
                Arc::new(MemoryStorage::new())
            }
        };

        // Create shared CRDT instances with event callbacks
        let workspace_crdt = {
            let mut crdt = WorkspaceCrdt::load(Arc::clone(&crdt_storage))
                .map_err(|e| JsValue::from_str(&format!("Failed to load CRDT: {}", e)))?;
            // Set event callback to forward CRDT events to the Rust registry
            let registry = Arc::clone(&rust_event_registry);
            crdt.set_event_callback(Arc::new(move |event| {
                registry.emit(event);
            }));
            Arc::new(crdt)
        };

        let body_doc_manager = {
            let manager = BodyDocManager::new(Arc::clone(&crdt_storage));
            // Set event callback to forward CRDT events to the Rust registry
            let registry = Arc::clone(&rust_event_registry);
            manager.set_event_callback(Arc::new(move |event| {
                registry.emit(event);
            }));
            Arc::new(manager)
        };

        // Build decorator stack: EventEmittingFs<CrdtFs<StorageBackend>>
        let crdt_fs = CrdtFs::new(
            storage_backend,
            Arc::clone(&workspace_crdt),
            Arc::clone(&body_doc_manager),
        );
        let event_fs = EventEmittingFs::with_registry(crdt_fs, Arc::clone(&rust_event_registry));
        let fs = Rc::new(event_fs);

        Ok(Self {
            fs,
            crdt_storage,
            workspace_crdt,
            body_doc_manager,
            wasm_event_registry,
            rust_event_registry,
        })
    }

    /// Create a new DiaryxBackend with IndexedDB storage.
    ///
    /// This attempts to use persistent SQLite-based CRDT storage (via sql.js).
    /// If SQLite storage is not available, falls back to in-memory CRDT storage.
    #[wasm_bindgen(js_name = "createIndexedDb")]
    pub async fn create_indexed_db() -> std::result::Result<DiaryxBackend, JsValue> {
        let idb = IndexedDbFileSystem::create().await?;
        let storage_backend = StorageBackend::IndexedDb(idb);

        // Create event registries
        let wasm_event_registry = Rc::new(WasmCallbackRegistry::new());
        let rust_event_registry = Arc::new(CallbackRegistry::new());

        // Set up thread-local for bridge (safe because WASM is single-threaded)
        WASM_EVENT_REGISTRY.with(|reg| {
            *reg.borrow_mut() = Some(Rc::clone(&wasm_event_registry));
        });

        // Register bridge callback to forward Rust events to JS
        rust_event_registry.subscribe(create_event_bridge());

        // Try to use persistent SQLite storage, fall back to memory storage
        let crdt_storage: Arc<dyn CrdtStorage> = match WasmSqliteStorage::new() {
            Ok(storage) => {
                log::info!("Using persistent SQLite CRDT storage");
                Arc::new(storage)
            }
            Err(e) => {
                log::warn!(
                    "SQLite CRDT storage not available, using memory storage: {:?}",
                    e
                );
                Arc::new(MemoryStorage::new())
            }
        };

        // Create shared CRDT instances with event callbacks
        let workspace_crdt = {
            let mut crdt = WorkspaceCrdt::load(Arc::clone(&crdt_storage))
                .map_err(|e| JsValue::from_str(&format!("Failed to load CRDT: {}", e)))?;
            // Set event callback to forward CRDT events to the Rust registry
            let registry = Arc::clone(&rust_event_registry);
            crdt.set_event_callback(Arc::new(move |event| {
                registry.emit(event);
            }));
            Arc::new(crdt)
        };

        let body_doc_manager = {
            let manager = BodyDocManager::new(Arc::clone(&crdt_storage));
            // Set event callback to forward CRDT events to the Rust registry
            let registry = Arc::clone(&rust_event_registry);
            manager.set_event_callback(Arc::new(move |event| {
                registry.emit(event);
            }));
            Arc::new(manager)
        };

        // Build decorator stack: EventEmittingFs<CrdtFs<StorageBackend>>
        let crdt_fs = CrdtFs::new(
            storage_backend,
            Arc::clone(&workspace_crdt),
            Arc::clone(&body_doc_manager),
        );
        let event_fs = EventEmittingFs::with_registry(crdt_fs, Arc::clone(&rust_event_registry));
        let fs = Rc::new(event_fs);

        Ok(Self {
            fs,
            crdt_storage,
            workspace_crdt,
            body_doc_manager,
            wasm_event_registry,
            rust_event_registry,
        })
    }

    /// Create backend with specific storage type.
    #[wasm_bindgen(js_name = "create")]
    pub async fn create(storage_type: &str) -> std::result::Result<DiaryxBackend, JsValue> {
        match storage_type.to_lowercase().as_str() {
            "opfs" => Self::create_opfs().await,
            "indexeddb" | "indexed_db" => Self::create_indexed_db().await,
            "memory" | "inmemory" | "in_memory" => Self::create_in_memory(),
            _ => Err(JsValue::from_str(&format!(
                "Unknown storage type: {}",
                storage_type
            ))),
        }
    }

    /// Create a new DiaryxBackend with in-memory storage.
    ///
    /// This is used for guest mode in share sessions. Files are stored
    /// only in memory and are cleared when the session ends.
    ///
    /// ## Use Cases
    /// - Guest mode in share sessions (web)
    /// - Testing
    ///
    /// ## Example
    /// ```javascript
    /// const backend = DiaryxBackend.createInMemory();
    /// // Files are stored in memory only
    /// ```
    #[wasm_bindgen(js_name = "createInMemory")]
    pub fn create_in_memory() -> std::result::Result<DiaryxBackend, JsValue> {
        let mem_fs = InMemoryFileSystem::new();
        let async_fs = SyncToAsyncFs::new(mem_fs);
        let storage_backend = StorageBackend::InMemory(async_fs);

        // Create event registries
        let wasm_event_registry = Rc::new(WasmCallbackRegistry::new());
        let rust_event_registry = Arc::new(CallbackRegistry::new());

        // Set up thread-local for bridge (safe because WASM is single-threaded)
        WASM_EVENT_REGISTRY.with(|reg| {
            *reg.borrow_mut() = Some(Rc::clone(&wasm_event_registry));
        });

        // Register bridge callback to forward Rust events to JS
        rust_event_registry.subscribe(create_event_bridge());

        // In-memory storage for both filesystem and CRDT
        let crdt_storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());

        // Create shared CRDT instances with event callbacks
        let workspace_crdt = {
            let mut crdt = WorkspaceCrdt::new(Arc::clone(&crdt_storage));
            // Set event callback to forward CRDT events to the Rust registry
            let registry = Arc::clone(&rust_event_registry);
            crdt.set_event_callback(Arc::new(move |event| {
                registry.emit(event);
            }));
            Arc::new(crdt)
        };

        let body_doc_manager = {
            let manager = BodyDocManager::new(Arc::clone(&crdt_storage));
            // Set event callback to forward CRDT events to the Rust registry
            let registry = Arc::clone(&rust_event_registry);
            manager.set_event_callback(Arc::new(move |event| {
                registry.emit(event);
            }));
            Arc::new(manager)
        };

        // Build decorator stack: EventEmittingFs<CrdtFs<StorageBackend>>
        let crdt_fs = CrdtFs::new(
            storage_backend,
            Arc::clone(&workspace_crdt),
            Arc::clone(&body_doc_manager),
        );
        let event_fs = EventEmittingFs::with_registry(crdt_fs, Arc::clone(&rust_event_registry));
        let fs = Rc::new(event_fs);

        Ok(Self {
            fs,
            crdt_storage,
            workspace_crdt,
            body_doc_manager,
            wasm_event_registry,
            rust_event_registry,
        })
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
        let storage_backend = StorageBackend::Fsa(fsa);

        // Create event registries
        let wasm_event_registry = Rc::new(WasmCallbackRegistry::new());
        let rust_event_registry = Arc::new(CallbackRegistry::new());

        // Set up thread-local for bridge (safe because WASM is single-threaded)
        WASM_EVENT_REGISTRY.with(|reg| {
            *reg.borrow_mut() = Some(Rc::clone(&wasm_event_registry));
        });

        // Register bridge callback to forward Rust events to JS
        rust_event_registry.subscribe(create_event_bridge());

        // Try to use persistent SQLite storage, fall back to memory storage
        let crdt_storage: Arc<dyn CrdtStorage> = match WasmSqliteStorage::new() {
            Ok(storage) => {
                log::info!("Using persistent SQLite CRDT storage");
                Arc::new(storage)
            }
            Err(e) => {
                log::warn!(
                    "SQLite CRDT storage not available, using memory storage: {:?}",
                    e
                );
                Arc::new(MemoryStorage::new())
            }
        };

        // Create shared CRDT instances with event callbacks
        let workspace_crdt = {
            let mut crdt = WorkspaceCrdt::new(Arc::clone(&crdt_storage));
            // Set event callback to forward CRDT events to the Rust registry
            let registry = Arc::clone(&rust_event_registry);
            crdt.set_event_callback(Arc::new(move |event| {
                registry.emit(event);
            }));
            Arc::new(crdt)
        };

        let body_doc_manager = {
            let manager = BodyDocManager::new(Arc::clone(&crdt_storage));
            // Set event callback to forward CRDT events to the Rust registry
            let registry = Arc::clone(&rust_event_registry);
            manager.set_event_callback(Arc::new(move |event| {
                registry.emit(event);
            }));
            Arc::new(manager)
        };

        // Build decorator stack: EventEmittingFs<CrdtFs<StorageBackend>>
        let crdt_fs = CrdtFs::new(
            storage_backend,
            Arc::clone(&workspace_crdt),
            Arc::clone(&body_doc_manager),
        );
        let event_fs = EventEmittingFs::with_registry(crdt_fs, Arc::clone(&rust_event_registry));
        let fs = Rc::new(event_fs);

        Ok(Self {
            fs,
            crdt_storage,
            workspace_crdt,
            body_doc_manager,
            wasm_event_registry,
            rust_event_registry,
        })
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

        // Use stored CRDT instances with event callbacks configured.
        // This is critical for remote CRDT updates to trigger UI notifications.
        // The shared Arc<WorkspaceCrdt> and Arc<BodyDocManager> have event callbacks
        // that forward events to the JS event registry.
        let diaryx = Diaryx::with_crdt_instances(
            (*self.fs).clone(),
            Arc::clone(&self.workspace_crdt),
            Arc::clone(&self.body_doc_manager),
        );

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

        // Use stored CRDT instances with event callbacks configured.
        // This is critical for remote CRDT updates to trigger UI notifications.
        // The shared Arc<WorkspaceCrdt> and Arc<BodyDocManager> have event callbacks
        // that forward events to the JS event registry.
        let diaryx = Diaryx::with_crdt_instances(
            (*self.fs).clone(),
            Arc::clone(&self.workspace_crdt),
            Arc::clone(&self.body_doc_manager),
        );

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

    // ========================================================================
    // Event Subscription API
    // ========================================================================

    /// Subscribe to filesystem events.
    ///
    /// The callback will be invoked with a JSON-serialized FileSystemEvent
    /// whenever filesystem operations occur (create, delete, rename, move, etc.).
    ///
    /// Returns a subscription ID that can be used to unsubscribe later.
    ///
    /// ## Example
    ///
    /// ```javascript
    /// const id = backend.onFileSystemEvent((eventJson) => {
    ///     const event = JSON.parse(eventJson);
    ///     console.log('File event:', event.type, event.path);
    /// });
    ///
    /// // Later, to unsubscribe:
    /// backend.offFileSystemEvent(id);
    /// ```
    #[wasm_bindgen(js_name = "onFileSystemEvent")]
    pub fn on_filesystem_event(&self, callback: js_sys::Function) -> u64 {
        self.wasm_event_registry.subscribe(callback)
    }

    /// Unsubscribe from filesystem events.
    ///
    /// Returns `true` if the subscription was found and removed.
    ///
    /// ## Example
    ///
    /// ```javascript
    /// const id = backend.onFileSystemEvent(handler);
    /// // ... later ...
    /// const removed = backend.offFileSystemEvent(id);
    /// console.log('Subscription removed:', removed);
    /// ```
    #[wasm_bindgen(js_name = "offFileSystemEvent")]
    pub fn off_filesystem_event(&self, id: u64) -> bool {
        self.wasm_event_registry.unsubscribe(id)
    }

    /// Emit a filesystem event.
    ///
    /// This is primarily used internally but can be called from JavaScript
    /// to manually trigger events (e.g., for testing or manual sync scenarios).
    ///
    /// The event should be a JSON string matching the FileSystemEvent format.
    ///
    /// ## Example
    ///
    /// ```javascript
    /// backend.emitFileSystemEvent(JSON.stringify({
    ///     type: 'FileCreated',
    ///     path: 'workspace/notes.md',
    ///     frontmatter: { title: 'Notes' }
    /// }));
    /// ```
    #[wasm_bindgen(js_name = "emitFileSystemEvent")]
    pub fn emit_filesystem_event(&self, event_json: &str) -> std::result::Result<(), JsValue> {
        let event: FileSystemEvent = serde_json::from_str(event_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid event JSON: {}", e)))?;
        self.wasm_event_registry.emit(&event);
        Ok(())
    }

    /// Get the number of active event subscriptions.
    #[wasm_bindgen(js_name = "eventSubscriberCount")]
    pub fn event_subscriber_count(&self) -> usize {
        self.wasm_event_registry.subscriber_count()
    }
}
