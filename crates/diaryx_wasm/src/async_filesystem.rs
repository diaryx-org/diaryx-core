//! Async filesystem operations for WASM with JavaScript Promise interop.
//!
//! This module provides an async wrapper around the in-memory filesystem
//! that works with `wasm-bindgen-futures` for proper JavaScript Promise support.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { DiaryxAsyncFilesystem } from './wasm/diaryx_wasm.js';
//!
//! const asyncFs = new DiaryxAsyncFilesystem();
//!
//! // All methods return Promises
//! const content = await asyncFs.read_file('workspace/README.md');
//! await asyncFs.write_file('workspace/new.md', '# New File');
//! const exists = await asyncFs.file_exists('workspace/new.md');
//! ```

use diaryx_core::fs::{AsyncFileSystem, FileSystem, InMemoryFileSystem, SyncToAsyncFs};
use serde::{Deserialize, Serialize};
use std::path::Path;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::state::{replace_fs, with_fs, with_fs_mut};

// ============================================================================
// Types
// ============================================================================

#[derive(Serialize, Deserialize)]
struct BinaryEntry {
    path: String,
    data: Vec<u8>,
}

/// Result of a backup operation
#[derive(Serialize)]
pub struct JsAsyncBackupResult {
    pub success: bool,
    pub files_processed: usize,
    pub text_files: usize,
    pub binary_files: usize,
    pub error: Option<String>,
}

/// Result of listing files
#[derive(Serialize)]
pub struct JsFileList {
    pub files: Vec<String>,
    pub count: usize,
}

// ============================================================================
// DiaryxAsyncFilesystem Class
// ============================================================================

/// Async filesystem operations for WASM with JavaScript Promise support.
///
/// This class provides async methods that return JavaScript Promises,
/// making it suitable for use with async/await in JavaScript.
///
/// While the underlying InMemoryFileSystem is synchronous, this wrapper
/// provides a Promise-based API that:
/// 1. Enables consistent async/await patterns in JavaScript
/// 2. Allows for future integration with truly async operations (e.g., IndexedDB)
/// 3. Works well with JavaScript's event loop
#[wasm_bindgen]
pub struct DiaryxAsyncFilesystem;

#[wasm_bindgen]
impl DiaryxAsyncFilesystem {
    /// Create a new DiaryxAsyncFilesystem instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Read a file's content (async).
    ///
    /// Returns a Promise that resolves to the file content as a string.
    #[wasm_bindgen]
    pub fn read_file(&self, path: String) -> js_sys::Promise {
        future_to_promise(async move {
            let async_fs = with_fs(|fs| SyncToAsyncFs::new(fs.clone()));
            let content = async_fs
                .read_to_string(Path::new(&path))
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(JsValue::from_str(&content))
        })
    }

    /// Write content to a file (async).
    ///
    /// Returns a Promise that resolves when the write is complete.
    #[wasm_bindgen]
    pub fn write_file(&self, path: String, content: String) -> js_sys::Promise {
        future_to_promise(async move {
            with_fs_mut(|fs| {
                fs.write_file(Path::new(&path), &content)
                    .map_err(|e| JsValue::from_str(&e.to_string()))
            })?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Create a new file (fails if exists) (async).
    ///
    /// Returns a Promise that resolves when the file is created.
    #[wasm_bindgen]
    pub fn create_new(&self, path: String, content: String) -> js_sys::Promise {
        future_to_promise(async move {
            with_fs_mut(|fs| {
                fs.create_new(Path::new(&path), &content)
                    .map_err(|e| JsValue::from_str(&e.to_string()))
            })?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Delete a file (async).
    ///
    /// Returns a Promise that resolves when the file is deleted.
    #[wasm_bindgen]
    pub fn delete_file(&self, path: String) -> js_sys::Promise {
        future_to_promise(async move {
            with_fs_mut(|fs| {
                fs.delete_file(Path::new(&path))
                    .map_err(|e| JsValue::from_str(&e.to_string()))
            })?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Check if a file exists (async).
    ///
    /// Returns a Promise that resolves to a boolean.
    #[wasm_bindgen]
    pub fn file_exists(&self, path: String) -> js_sys::Promise {
        future_to_promise(async move {
            let exists = with_fs(|fs| FileSystem::exists(fs, Path::new(&path)));
            Ok(JsValue::from_bool(exists))
        })
    }

    /// Check if a path is a directory (async).
    ///
    /// Returns a Promise that resolves to a boolean.
    #[wasm_bindgen]
    pub fn is_dir(&self, path: String) -> js_sys::Promise {
        future_to_promise(async move {
            let is_dir = with_fs(|fs| fs.is_dir(Path::new(&path)));
            Ok(JsValue::from_bool(is_dir))
        })
    }

    /// Create a directory and all parent directories (async).
    ///
    /// Returns a Promise that resolves when the directory is created.
    #[wasm_bindgen]
    pub fn create_dir_all(&self, path: String) -> js_sys::Promise {
        future_to_promise(async move {
            with_fs_mut(|fs| {
                fs.create_dir_all(Path::new(&path))
                    .map_err(|e| JsValue::from_str(&e.to_string()))
            })?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Move/rename a file (async).
    ///
    /// Returns a Promise that resolves when the move is complete.
    #[wasm_bindgen]
    pub fn move_file(&self, from: String, to: String) -> js_sys::Promise {
        future_to_promise(async move {
            with_fs_mut(|fs| {
                fs.move_file(Path::new(&from), Path::new(&to))
                    .map_err(|e| JsValue::from_str(&e.to_string()))
            })?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// List markdown files in a directory (async).
    ///
    /// Returns a Promise that resolves to an array of file paths.
    #[wasm_bindgen]
    pub fn list_md_files(&self, dir: String) -> js_sys::Promise {
        future_to_promise(async move {
            let files = with_fs(|fs| {
                fs.list_md_files(Path::new(&dir))
                    .map_err(|e| JsValue::from_str(&e.to_string()))
            })?;

            let result = JsFileList {
                count: files.len(),
                files: files
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect(),
            };

            serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
        })
    }

    /// List all files in a directory (async).
    ///
    /// Returns a Promise that resolves to an array of file paths.
    #[wasm_bindgen]
    pub fn list_files(&self, dir: String) -> js_sys::Promise {
        future_to_promise(async move {
            let files = with_fs(|fs| {
                fs.list_files(Path::new(&dir))
                    .map_err(|e| JsValue::from_str(&e.to_string()))
            })?;

            let result = JsFileList {
                count: files.len(),
                files: files
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect(),
            };

            serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
        })
    }

    /// Recursively list all markdown files (async).
    ///
    /// Returns a Promise that resolves to an array of file paths.
    #[wasm_bindgen]
    pub fn list_md_files_recursive(&self, dir: String) -> js_sys::Promise {
        future_to_promise(async move {
            let async_fs = with_fs(|fs| SyncToAsyncFs::new(fs.clone()));
            let files = async_fs
                .list_md_files_recursive(Path::new(&dir))
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let result = JsFileList {
                count: files.len(),
                files: files
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect(),
            };

            serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
        })
    }

    /// Recursively list all files and directories (async).
    ///
    /// Returns a Promise that resolves to an array of paths.
    #[wasm_bindgen]
    pub fn list_all_files_recursive(&self, dir: String) -> js_sys::Promise {
        future_to_promise(async move {
            let async_fs = with_fs(|fs| SyncToAsyncFs::new(fs.clone()));
            let files = async_fs
                .list_all_files_recursive(Path::new(&dir))
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let result = JsFileList {
                count: files.len(),
                files: files
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect(),
            };

            serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
        })
    }

    /// Read binary file content (async).
    ///
    /// Returns a Promise that resolves to a Uint8Array.
    #[wasm_bindgen]
    pub fn read_binary(&self, path: String) -> js_sys::Promise {
        future_to_promise(async move {
            let data = with_fs(|fs| {
                fs.read_binary(Path::new(&path))
                    .map_err(|e| JsValue::from_str(&e.to_string()))
            })?;

            Ok(js_sys::Uint8Array::from(&data[..]).into())
        })
    }

    /// Write binary content to a file (async).
    ///
    /// Accepts a Uint8Array or Array of numbers.
    /// Returns a Promise that resolves when the write is complete.
    #[wasm_bindgen]
    pub fn write_binary(&self, path: String, data: js_sys::Uint8Array) -> js_sys::Promise {
        let data_vec = data.to_vec();
        future_to_promise(async move {
            with_fs_mut(|fs| {
                fs.write_binary(Path::new(&path), &data_vec)
                    .map_err(|e| JsValue::from_str(&e.to_string()))
            })?;
            Ok(JsValue::UNDEFINED)
        })
    }

    // ========================================================================
    // Bulk Operations
    // ========================================================================

    /// Load files into the in-memory filesystem from JavaScript (async).
    ///
    /// Accepts an array of [path, content] tuples.
    /// Returns a Promise that resolves when all files are loaded.
    #[wasm_bindgen]
    pub fn load_files(&self, entries: JsValue) -> js_sys::Promise {
        future_to_promise(async move {
            let entries: Vec<(String, String)> = serde_wasm_bindgen::from_value(entries)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            replace_fs(InMemoryFileSystem::load_from_entries(entries));
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Export all files from the in-memory filesystem (async).
    ///
    /// Returns a Promise that resolves to an array of [path, content] tuples.
    #[wasm_bindgen]
    pub fn export_files(&self) -> js_sys::Promise {
        future_to_promise(async move {
            let entries = with_fs(|fs| fs.export_entries());
            serde_wasm_bindgen::to_value(&entries).map_err(|e| JsValue::from_str(&e.to_string()))
        })
    }

    /// Export all binary files from the in-memory filesystem (async).
    ///
    /// Returns a Promise that resolves to an array of { path, data } objects.
    #[wasm_bindgen]
    pub fn export_binary_files(&self) -> js_sys::Promise {
        future_to_promise(async move {
            let entries = with_fs(|fs| fs.export_binary_entries());
            let serializable: Vec<BinaryEntry> = entries
                .into_iter()
                .map(|(path, data)| BinaryEntry { path, data })
                .collect();
            serde_wasm_bindgen::to_value(&serializable)
                .map_err(|e| JsValue::from_str(&e.to_string()))
        })
    }

    /// Load binary files into the in-memory filesystem (async).
    ///
    /// Accepts an array of { path, data } objects.
    /// Returns a Promise that resolves when all files are loaded.
    #[wasm_bindgen]
    pub fn load_binary_files(&self, entries: JsValue) -> js_sys::Promise {
        future_to_promise(async move {
            let binary_entries: Vec<BinaryEntry> = serde_wasm_bindgen::from_value(entries)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            let entries: Vec<(String, Vec<u8>)> = binary_entries
                .into_iter()
                .map(|e| (e.path, e.data))
                .collect();

            with_fs_mut(|fs| {
                fs.load_binary_entries(entries);
            });

            Ok(JsValue::UNDEFINED)
        })
    }

    /// Get backup data for persistence to IndexedDB (async).
    ///
    /// Returns a Promise that resolves to a backup data object containing
    /// both text and binary files.
    #[wasm_bindgen]
    pub fn get_backup_data(&self) -> js_sys::Promise {
        future_to_promise(async move {
            let text_entries = with_fs(|fs| fs.export_entries());
            let binary_entries = with_fs(|fs| fs.export_binary_entries());

            #[derive(Serialize)]
            struct BackupData {
                text_files: Vec<(String, String)>,
                binary_files: Vec<BinaryEntry>,
                text_count: usize,
                binary_count: usize,
            }

            let binary_files: Vec<BinaryEntry> = binary_entries
                .into_iter()
                .map(|(path, data)| BinaryEntry { path, data })
                .collect();

            let data = BackupData {
                text_count: text_entries.len(),
                binary_count: binary_files.len(),
                text_files: text_entries,
                binary_files,
            };

            serde_wasm_bindgen::to_value(&data).map_err(|e| JsValue::from_str(&e.to_string()))
        })
    }

    /// Restore from backup data (async).
    ///
    /// Returns a Promise that resolves to a result object.
    #[wasm_bindgen]
    pub fn restore_from_backup(&self, data: JsValue) -> js_sys::Promise {
        future_to_promise(async move {
            #[derive(Deserialize)]
            struct BackupData {
                text_files: Vec<(String, String)>,
                binary_files: Vec<BinaryEntry>,
            }

            let backup: BackupData = serde_wasm_bindgen::from_value(data)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            replace_fs(InMemoryFileSystem::load_from_entries(
                backup.text_files.clone(),
            ));

            let binary_entries: Vec<(String, Vec<u8>)> = backup
                .binary_files
                .iter()
                .map(|e| (e.path.clone(), e.data.clone()))
                .collect();

            with_fs_mut(|fs| {
                fs.load_binary_entries(binary_entries);
            });

            let result = JsAsyncBackupResult {
                success: true,
                files_processed: backup.text_files.len() + backup.binary_files.len(),
                text_files: backup.text_files.len(),
                binary_files: backup.binary_files.len(),
                error: None,
            };

            serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
        })
    }

    /// Clear all files and directories (async).
    ///
    /// Returns a Promise that resolves when the filesystem is cleared.
    #[wasm_bindgen]
    pub fn clear(&self) -> js_sys::Promise {
        future_to_promise(async move {
            with_fs_mut(|fs| {
                fs.clear();
            });
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Get a list of all file paths in the filesystem (async).
    ///
    /// Returns a Promise that resolves to an array of paths.
    #[wasm_bindgen]
    pub fn list_all_files(&self) -> js_sys::Promise {
        future_to_promise(async move {
            let files = with_fs(|fs| fs.list_all_files());
            let paths: Vec<String> = files
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();

            let result = JsFileList {
                count: paths.len(),
                files: paths,
            };

            serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
        })
    }
}

impl Default for DiaryxAsyncFilesystem {
    fn default() -> Self {
        Self::new()
    }
}
