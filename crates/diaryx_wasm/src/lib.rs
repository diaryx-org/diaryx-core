#![cfg(target_arch = "wasm32")]
//! WebAssembly bindings for Diaryx core functionality.
//!
//! This crate provides a complete backend implementation for the web frontend,
//! using native storage backends (OPFS, IndexedDB, or File System Access API).
//!
//! ## Architecture
//!
//! The primary entry point is [`DiaryxBackend`], which provides a unified
//! command-based API for all workspace operations.
//!
//! ## Usage
//!
//! ```javascript
//! import init, { DiaryxBackend } from './diaryx_wasm.js';
//!
//! await init();
//! const backend = await DiaryxBackend.createOpfs();
//!
//! // Use execute() with Command objects
//! const response = await backend.execute(JSON.stringify({
//!   type: 'GetEntry',
//!   params: { path: 'workspace/journal/2024-01-08.md' }
//! }));
//! ```
//!
//! ## Error Handling
//!
//! All methods return `Result<T, JsValue>` for JavaScript interop.

mod backend;
mod error;
mod fsa_fs;
mod indexeddb_fs;
mod js_async_fs;
mod opfs_fs;
mod utils;
mod wasm_sqlite_storage;

// Re-export WASM SQLite storage for external use
pub use wasm_sqlite_storage::WasmSqliteStorage;

// Re-export the main backend class
pub use backend::DiaryxBackend;

// Re-export filesystem implementations
pub use fsa_fs::FsaFileSystem;
pub use indexeddb_fs::IndexedDbFileSystem;
pub use js_async_fs::JsAsyncFileSystem;
pub use opfs_fs::OpfsFileSystem;

// Re-export utility functions
pub use utils::{now_timestamp, today_formatted};

use wasm_bindgen::prelude::*;

// ============================================================================
// Initialization
// ============================================================================

#[cfg(feature = "console_error_panic_hook")]
pub fn set_panic_hook() {
    console_error_panic_hook::set_once();
}

/// Initialize the WASM module. Called automatically on module load.
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    set_panic_hook();

    // Initialize console logging for Rust log macros
    console_log::init_with_level(log::Level::Debug).ok();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    #[test]
    fn test_module_loads() {
        // Basic test to ensure module compiles
        assert!(true);
    }
}
