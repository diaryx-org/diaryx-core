//! JavaScript-backed AsyncFileSystem implementation.
//!
//! This module provides `JsAsyncFileSystem`, which implements the `AsyncFileSystem` trait
//! by delegating all operations to JavaScript callbacks. This allows the web frontend
//! to provide its own storage backend (IndexedDB, OPFS, localStorage, etc.) while
//! the Rust/WASM code uses the standard async filesystem interface.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { JsAsyncFileSystem } from './wasm/diaryx_wasm.js';
//!
//! // Create filesystem with JavaScript callbacks
//! const fs = new JsAsyncFileSystem({
//!   readToString: async (path) => {
//!     const data = await indexedDB.get(path);
//!     return data?.content;
//!   },
//!   writeFile: async (path, content) => {
//!     await indexedDB.put({ path, content });
//!   },
//!   deleteFile: async (path) => {
//!     await indexedDB.delete(path);
//!   },
//!   exists: async (path) => {
//!     return await indexedDB.has(path);
//!   },
//!   isDir: async (path) => {
//!     return path.endsWith('/') || await hasChildren(path);
//!   },
//!   listFiles: async (dir) => {
//!     return await indexedDB.listDir(dir);
//!   },
//!   listMdFiles: async (dir) => {
//!     const files = await indexedDB.listDir(dir);
//!     return files.filter(f => f.endsWith('.md'));
//!   },
//!   createDirAll: async (path) => {
//!     // No-op for flat storage, or create directory markers
//!   },
//!   moveFile: async (from, to) => {
//!     const content = await indexedDB.get(from);
//!     await indexedDB.put({ path: to, content: content.content });
//!     await indexedDB.delete(from);
//!   },
//!   readBinary: async (path) => {
//!     const data = await indexedDB.get(path);
//!     return new Uint8Array(data?.binary);
//!   },
//!   writeBinary: async (path, data) => {
//!     await indexedDB.put({ path, binary: Array.from(data) });
//!   },
//! });
//!
//! // Now use fs with async WASM operations
//! const workspace = new DiaryxAsyncWorkspace(fs);
//! const tree = await workspace.getTree('workspace');
//! ```

use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

use diaryx_core::fs::{AsyncFileSystem, BoxFuture};
use js_sys::{Array, Function, Promise, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

// ============================================================================
// JavaScript Callback Interface
// ============================================================================

/// JavaScript callbacks for filesystem operations.
///
/// All callbacks are optional. If a callback is not provided, the operation
/// will return an appropriate error or default value.
#[wasm_bindgen]
extern "C" {
    /// JavaScript object containing filesystem callbacks.
    #[wasm_bindgen(typescript_type = "JsFileSystemCallbacks")]
    pub type JsFileSystemCallbacks;

    #[wasm_bindgen(method, getter, js_name = "readToString")]
    fn read_to_string_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "writeFile")]
    fn write_file_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "createNew")]
    fn create_new_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "deleteFile")]
    fn delete_file_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "exists")]
    fn exists_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "isDir")]
    fn is_dir_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "listFiles")]
    fn list_files_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "listMdFiles")]
    fn list_md_files_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "createDirAll")]
    fn create_dir_all_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "moveFile")]
    fn move_file_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "readBinary")]
    fn read_binary_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "writeBinary")]
    fn write_binary_cb(this: &JsFileSystemCallbacks) -> Option<Function>;
}

// ============================================================================
// JsAsyncFileSystem Implementation
// ============================================================================

/// An `AsyncFileSystem` implementation backed by JavaScript callbacks.
///
/// This struct allows Rust code to use the async filesystem interface while
/// delegating actual storage operations to JavaScript. This is useful for:
///
/// - Using IndexedDB for persistent storage in browsers
/// - Using OPFS (Origin Private File System) for better performance
/// - Integrating with existing JavaScript storage solutions
/// - Testing with mock filesystems
///
/// ## Thread Safety
///
/// This type is designed for single-threaded WASM environments. The callbacks
/// JsValue is cloned into each async operation to satisfy Send requirements,
/// but actual execution remains single-threaded.
#[wasm_bindgen]
#[derive(Clone)]
pub struct JsAsyncFileSystem {
    // We store the callbacks as a JsValue which can be cloned
    // Each async operation will clone this and work with its own copy
    callbacks: JsValue,
}

#[wasm_bindgen]
impl JsAsyncFileSystem {
    /// Create a new JsAsyncFileSystem with the provided callbacks.
    ///
    /// The callbacks object should implement the `JsFileSystemCallbacks` interface.
    /// All callbacks are optional - missing callbacks will cause operations to fail
    /// with appropriate errors.
    #[wasm_bindgen(constructor)]
    pub fn new(callbacks: JsValue) -> Self {
        Self { callbacks }
    }

    /// Check if the filesystem has a specific callback.
    #[wasm_bindgen]
    pub fn has_callback(&self, name: &str) -> bool {
        if let Ok(obj) = js_sys::Reflect::get(&self.callbacks, &JsValue::from_str(name)) {
            obj.is_function()
        } else {
            false
        }
    }
}

// Helper function to convert JsValue error to io::Error
fn js_to_io_error(err: JsValue) -> Error {
    let msg = if let Some(s) = err.as_string() {
        s
    } else if let Some(obj) = err.dyn_ref::<js_sys::Object>() {
        obj.to_string()
            .as_string()
            .unwrap_or_else(|| "Unknown JS error".to_string())
    } else {
        "Unknown JS error".to_string()
    };
    Error::new(ErrorKind::Other, msg)
}

// Helper function to get a callback from the callbacks object
fn get_callback(callbacks: &JsValue, name: &str) -> Option<Function> {
    js_sys::Reflect::get(callbacks, &JsValue::from_str(name))
        .ok()
        .and_then(|v| v.dyn_into::<Function>().ok())
}

// Helper to call a JS callback that returns a Promise
async fn call_async_callback(
    callbacks: &JsValue,
    name: &str,
    args: &[JsValue],
) -> std::result::Result<JsValue, Error> {
    let callback = get_callback(callbacks, name).ok_or_else(|| {
        Error::new(
            ErrorKind::Unsupported,
            format!("Callback '{}' not provided", name),
        )
    })?;

    let this = JsValue::NULL;
    let result = match args.len() {
        0 => callback.call0(&this),
        1 => callback.call1(&this, &args[0]),
        2 => callback.call2(&this, &args[0], &args[1]),
        3 => callback.call3(&this, &args[0], &args[1], &args[2]),
        _ => {
            let js_args = Array::new();
            for arg in args {
                js_args.push(arg);
            }
            callback.apply(&this, &js_args)
        }
    }
    .map_err(js_to_io_error)?;

    // If result is a Promise, await it
    if result.has_type::<Promise>() {
        let promise: Promise = result.unchecked_into();
        JsFuture::from(promise).await.map_err(js_to_io_error)
    } else {
        Ok(result)
    }
}

// ============================================================================
// AsyncFileSystem Implementation
// ============================================================================

// Note: On WASM, the AsyncFileSystem trait doesn't require Send + Sync,
// so we don't need unsafe impls here. This is possible because WASM is
// single-threaded and our BoxFuture doesn't require Send on this target.

impl AsyncFileSystem for JsAsyncFileSystem {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();

        Box::pin(async move {
            let result =
                call_async_callback(&callbacks, "readToString", &[JsValue::from_str(&path_str)])
                    .await?;

            result.as_string().ok_or_else(|| {
                Error::new(
                    ErrorKind::InvalidData,
                    "readToString did not return a string",
                )
            })
        })
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();
        let content = content.to_string();

        Box::pin(async move {
            call_async_callback(
                &callbacks,
                "writeFile",
                &[JsValue::from_str(&path_str), JsValue::from_str(&content)],
            )
            .await?;
            Ok(())
        })
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();
        let content = content.to_string();

        Box::pin(async move {
            // Check if createNew callback exists
            if get_callback(&callbacks, "createNew").is_some() {
                call_async_callback(
                    &callbacks,
                    "createNew",
                    &[JsValue::from_str(&path_str), JsValue::from_str(&content)],
                )
                .await?;
                Ok(())
            } else {
                // Fall back to exists + writeFile
                let exists_result =
                    call_async_callback(&callbacks, "exists", &[JsValue::from_str(&path_str)])
                        .await?;

                if exists_result.as_bool().unwrap_or(false) {
                    return Err(Error::new(
                        ErrorKind::AlreadyExists,
                        format!("File already exists: {}", path_str),
                    ));
                }

                call_async_callback(
                    &callbacks,
                    "writeFile",
                    &[JsValue::from_str(&path_str), JsValue::from_str(&content)],
                )
                .await?;
                Ok(())
            }
        })
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();

        Box::pin(async move {
            call_async_callback(&callbacks, "deleteFile", &[JsValue::from_str(&path_str)]).await?;
            Ok(())
        })
    }

    fn list_md_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        let callbacks = self.callbacks.clone();
        let dir_str = dir.to_string_lossy().to_string();

        Box::pin(async move {
            let result =
                call_async_callback(&callbacks, "listMdFiles", &[JsValue::from_str(&dir_str)])
                    .await?;

            parse_path_array(result)
        })
    }

    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();

        Box::pin(async move {
            let result =
                call_async_callback(&callbacks, "exists", &[JsValue::from_str(&path_str)]).await;

            match result {
                Ok(v) => v.as_bool().unwrap_or(false),
                Err(_) => false,
            }
        })
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();

        Box::pin(async move {
            // createDirAll is optional - many storage backends don't need it
            if get_callback(&callbacks, "createDirAll").is_some() {
                call_async_callback(&callbacks, "createDirAll", &[JsValue::from_str(&path_str)])
                    .await?;
            }
            Ok(())
        })
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();

        Box::pin(async move {
            let result =
                call_async_callback(&callbacks, "isDir", &[JsValue::from_str(&path_str)]).await;

            match result {
                Ok(v) => v.as_bool().unwrap_or(false),
                Err(_) => false,
            }
        })
    }

    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        let callbacks = self.callbacks.clone();
        let from_str = from.to_string_lossy().to_string();
        let to_str = to.to_string_lossy().to_string();

        Box::pin(async move {
            call_async_callback(
                &callbacks,
                "moveFile",
                &[JsValue::from_str(&from_str), JsValue::from_str(&to_str)],
            )
            .await?;
            Ok(())
        })
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();

        Box::pin(async move {
            let result =
                call_async_callback(&callbacks, "readBinary", &[JsValue::from_str(&path_str)])
                    .await?;

            // Handle Uint8Array or Array
            if let Some(uint8_array) = result.dyn_ref::<Uint8Array>() {
                Ok(uint8_array.to_vec())
            } else if let Some(array) = result.dyn_ref::<Array>() {
                let mut bytes = Vec::with_capacity(array.length() as usize);
                for i in 0..array.length() {
                    let val = array.get(i);
                    let byte = val.as_f64().unwrap_or(0.0) as u8;
                    bytes.push(byte);
                }
                Ok(bytes)
            } else {
                Err(Error::new(
                    ErrorKind::InvalidData,
                    "readBinary did not return a Uint8Array or Array",
                ))
            }
        })
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();
        let content = content.to_vec();

        Box::pin(async move {
            // Convert bytes to Uint8Array
            let uint8_array = Uint8Array::new_with_length(content.len() as u32);
            uint8_array.copy_from(&content);

            call_async_callback(
                &callbacks,
                "writeBinary",
                &[JsValue::from_str(&path_str), uint8_array.into()],
            )
            .await?;
            Ok(())
        })
    }

    fn list_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        let callbacks = self.callbacks.clone();
        let dir_str = dir.to_string_lossy().to_string();

        Box::pin(async move {
            let result =
                call_async_callback(&callbacks, "listFiles", &[JsValue::from_str(&dir_str)])
                    .await?;

            parse_path_array(result)
        })
    }
}

// Helper function to parse a JS array of strings into Vec<PathBuf>
fn parse_path_array(value: JsValue) -> Result<Vec<PathBuf>> {
    if let Some(array) = value.dyn_ref::<Array>() {
        let mut paths = Vec::with_capacity(array.length() as usize);
        for i in 0..array.length() {
            let item = array.get(i);
            if let Some(s) = item.as_string() {
                paths.push(PathBuf::from(s));
            }
        }
        Ok(paths)
    } else {
        Err(Error::new(
            ErrorKind::InvalidData,
            "Expected array of strings",
        ))
    }
}

// ============================================================================
// TypeScript Type Definitions
// ============================================================================

#[wasm_bindgen(typescript_custom_section)]
const TS_APPEND_CONTENT: &'static str = r#"
/**
 * Callbacks for JsAsyncFileSystem operations.
 * 
 * All callbacks should return Promises. If a callback is not provided,
 * the corresponding operation will fail with an error.
 */
export interface JsFileSystemCallbacks {
    /**
     * Read a file's content as a string.
     * @param path - The file path to read
     * @returns Promise resolving to the file content
     */
    readToString?: (path: string) => Promise<string>;
    
    /**
     * Write content to a file, creating or overwriting it.
     * @param path - The file path to write
     * @param content - The content to write
     */
    writeFile?: (path: string, content: string) => Promise<void>;
    
    /**
     * Create a new file, failing if it already exists.
     * @param path - The file path to create
     * @param content - The initial content
     */
    createNew?: (path: string, content: string) => Promise<void>;
    
    /**
     * Delete a file.
     * @param path - The file path to delete
     */
    deleteFile?: (path: string) => Promise<void>;
    
    /**
     * Check if a path exists.
     * @param path - The path to check
     * @returns Promise resolving to true if the path exists
     */
    exists?: (path: string) => Promise<boolean>;
    
    /**
     * Check if a path is a directory.
     * @param path - The path to check
     * @returns Promise resolving to true if the path is a directory
     */
    isDir?: (path: string) => Promise<boolean>;
    
    /**
     * List all files in a directory (not recursive).
     * @param dir - The directory path
     * @returns Promise resolving to array of file paths
     */
    listFiles?: (dir: string) => Promise<string[]>;
    
    /**
     * List markdown files in a directory (not recursive).
     * @param dir - The directory path
     * @returns Promise resolving to array of .md file paths
     */
    listMdFiles?: (dir: string) => Promise<string[]>;
    
    /**
     * Create a directory and all parent directories.
     * @param path - The directory path to create
     */
    createDirAll?: (path: string) => Promise<void>;
    
    /**
     * Move/rename a file.
     * @param from - The source path
     * @param to - The destination path
     */
    moveFile?: (from: string, to: string) => Promise<void>;
    
    /**
     * Read binary file content.
     * @param path - The file path to read
     * @returns Promise resolving to file content as Uint8Array
     */
    readBinary?: (path: string) => Promise<Uint8Array>;
    
    /**
     * Write binary content to a file.
     * @param path - The file path to write
     * @param data - The binary content as Uint8Array
     */
    writeBinary?: (path: string, data: Uint8Array) => Promise<void>;
}
"#;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_js_async_filesystem_creation() {
        // Just verify the struct can be created with a null JsValue
        let _fs = JsAsyncFileSystem::new(JsValue::NULL);
    }

    #[test]
    fn test_js_async_filesystem_clone() {
        let fs1 = JsAsyncFileSystem::new(JsValue::NULL);
        let fs2 = fs1.clone();
        // Both should be independent clones
        assert!(!fs1.has_callback("test"));
        assert!(!fs2.has_callback("test"));
    }
}
