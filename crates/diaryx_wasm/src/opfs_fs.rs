//! OPFS (Origin Private File System) implementation of AsyncFileSystem.
//!
//! Uses the `opfs` crate to provide persistent file storage in browsers.
//! This backend works in both Web Workers (with sync access handles) and
//! the main thread (with async access).
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { OpfsFileSystem, DiaryxAsyncWorkspace } from './wasm/diaryx_wasm.js';
//!
//! const fs = await OpfsFileSystem.create();
//! const workspace = new DiaryxAsyncWorkspace(fs);
//! const tree = await workspace.getTree('workspace');
//! ```

use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

use diaryx_core::fs::{AsyncFileSystem, BoxFuture};
use futures::StreamExt;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use opfs::persistent::{self, DirectoryHandle};
use opfs::{
    CreateWritableOptions, DirectoryEntry, DirectoryHandle as DirectoryHandleTrait,
    FileHandle as FileHandleTrait, GetDirectoryHandleOptions, GetFileHandleOptions,
    WritableFileStream as WritableFileStreamTrait,
};

// ============================================================================
// OpfsFileSystem Implementation
// ============================================================================

/// AsyncFileSystem implementation backed by OPFS (Origin Private File System).
///
/// Uses the browser's private file system for persistent storage.
/// All operations are async and work directly with browser storage.
#[wasm_bindgen]
pub struct OpfsFileSystem {
    root: DirectoryHandle,
}

impl Clone for OpfsFileSystem {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
        }
    }
}

/// Get OPFS root directory in a worker-compatible way.
/// Uses js_sys::global() which works in both Window and WorkerGlobalScope.
async fn get_opfs_root() -> std::result::Result<web_sys::FileSystemDirectoryHandle, JsValue> {
    let global = js_sys::global();

    // Try to get navigator from either Window or WorkerGlobalScope
    let navigator = js_sys::Reflect::get(&global, &JsValue::from_str("navigator"))?;
    if navigator.is_undefined() {
        return Err(JsValue::from_str(
            "No navigator object found in global scope",
        ));
    }

    // Get storage from navigator
    let storage = js_sys::Reflect::get(&navigator, &JsValue::from_str("storage"))?;
    if storage.is_undefined() {
        return Err(JsValue::from_str("No storage object found on navigator"));
    }

    // Call getDirectory()
    let get_directory = js_sys::Reflect::get(&storage, &JsValue::from_str("getDirectory"))?;
    let get_directory_fn = get_directory
        .dyn_ref::<js_sys::Function>()
        .ok_or_else(|| JsValue::from_str("getDirectory is not a function"))?;

    let promise = get_directory_fn.call0(&storage)?;
    let promise = promise.dyn_into::<js_sys::Promise>()?;

    let result = JsFuture::from(promise).await?;
    result.dyn_into::<web_sys::FileSystemDirectoryHandle>()
}

#[wasm_bindgen]
impl OpfsFileSystem {
    /// Create a new OpfsFileSystem with the default app directory.
    ///
    /// This creates a "diaryx" directory in the origin-private file system.
    #[wasm_bindgen]
    pub async fn create() -> std::result::Result<OpfsFileSystem, JsValue> {
        Self::create_with_name("diaryx").await
    }

    /// Create a new OpfsFileSystem with a custom root directory name.
    #[wasm_bindgen(js_name = "createWithName")]
    pub async fn create_with_name(root_name: &str) -> std::result::Result<OpfsFileSystem, JsValue> {
        // Get OPFS root using worker-compatible method
        let opfs_root = get_opfs_root()
            .await
            .map_err(|e| JsValue::from_str(&format!("Failed to get OPFS root: {:?}", e)))?;

        // Create/get the app directory using opfs crate's DirectoryHandle
        // We need to convert the web_sys handle to opfs crate's handle
        let app_dir = DirectoryHandle::from(opfs_root);

        let options = GetDirectoryHandleOptions { create: true };
        let root = app_dir
            .get_directory_handle_with_options(root_name, &options)
            .await
            .map_err(|e| JsValue::from_str(&format!("Failed to create root directory: {:?}", e)))?;

        Ok(Self { root })
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Get or create nested directories for a path.
async fn get_or_create_parent_dir(
    root: &DirectoryHandle,
    path: &Path,
) -> persistent::Result<DirectoryHandle> {
    let mut current = root.clone();

    if let Some(parent) = path.parent() {
        for component in parent.components() {
            if let std::path::Component::Normal(name) = component {
                let name_str = name.to_string_lossy();
                let options = GetDirectoryHandleOptions { create: true };
                current = current
                    .get_directory_handle_with_options(&name_str, &options)
                    .await?;
            }
        }
    }

    Ok(current)
}

/// Get directory for a path (without creating).
async fn get_parent_dir(
    root: &DirectoryHandle,
    path: &Path,
) -> persistent::Result<DirectoryHandle> {
    let mut current = root.clone();

    if let Some(parent) = path.parent() {
        for component in parent.components() {
            if let std::path::Component::Normal(name) = component {
                let name_str = name.to_string_lossy();
                let options = GetDirectoryHandleOptions { create: false };
                current = current
                    .get_directory_handle_with_options(&name_str, &options)
                    .await?;
            }
        }
    }

    Ok(current)
}

/// Get directory handle for a directory path (navigating to the directory itself).
async fn get_directory(root: &DirectoryHandle, path: &Path) -> persistent::Result<DirectoryHandle> {
    let mut current = root.clone();

    for component in path.components() {
        if let std::path::Component::Normal(name) = component {
            let name_str = name.to_string_lossy();
            let options = GetDirectoryHandleOptions { create: false };
            current = current
                .get_directory_handle_with_options(&name_str, &options)
                .await?;
        }
    }

    Ok(current)
}

/// Get the filename from a path.
fn get_filename(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(String::from)
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "Invalid filename"))
}

/// Convert opfs error to io::Error
fn opfs_to_io_error(e: persistent::Error) -> Error {
    Error::new(ErrorKind::Other, format!("{:?}", e))
}

// ============================================================================
// AsyncFileSystem Implementation
// ============================================================================

impl AsyncFileSystem for OpfsFileSystem {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let dir = get_parent_dir(&self.root, path)
                .await
                .map_err(opfs_to_io_error)?;
            let filename = get_filename(path)?;

            let options = GetFileHandleOptions { create: false };
            let file = dir
                .get_file_handle_with_options(&filename, &options)
                .await
                .map_err(opfs_to_io_error)?;

            let data = file.read().await.map_err(opfs_to_io_error)?;

            String::from_utf8(data).map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))
        })
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        let content = content.to_string();
        Box::pin(async move {
            let dir = get_or_create_parent_dir(&self.root, path)
                .await
                .map_err(opfs_to_io_error)?;
            let filename = get_filename(path)?;

            let file_options = GetFileHandleOptions { create: true };
            let mut file = dir
                .get_file_handle_with_options(&filename, &file_options)
                .await
                .map_err(opfs_to_io_error)?;

            let write_options = CreateWritableOptions {
                keep_existing_data: false,
            };
            let mut writer = file
                .create_writable_with_options(&write_options)
                .await
                .map_err(opfs_to_io_error)?;

            writer
                .write_at_cursor_pos(content.into_bytes())
                .await
                .map_err(opfs_to_io_error)?;

            writer.close().await.map_err(opfs_to_io_error)?;

            Ok(())
        })
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        let content = content.to_string();
        Box::pin(async move {
            // Check if file exists first
            if self.exists(path).await {
                return Err(Error::new(
                    ErrorKind::AlreadyExists,
                    format!("File already exists: {}", path.display()),
                ));
            }

            let dir = get_or_create_parent_dir(&self.root, path)
                .await
                .map_err(opfs_to_io_error)?;
            let filename = get_filename(path)?;

            let file_options = GetFileHandleOptions { create: true };
            let mut file = dir
                .get_file_handle_with_options(&filename, &file_options)
                .await
                .map_err(opfs_to_io_error)?;

            let write_options = CreateWritableOptions {
                keep_existing_data: false,
            };
            let mut writer = file
                .create_writable_with_options(&write_options)
                .await
                .map_err(opfs_to_io_error)?;

            writer
                .write_at_cursor_pos(content.into_bytes())
                .await
                .map_err(opfs_to_io_error)?;

            writer.close().await.map_err(opfs_to_io_error)?;

            Ok(())
        })
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut dir = get_parent_dir(&self.root, path)
                .await
                .map_err(opfs_to_io_error)?;
            let filename = get_filename(path)?;

            dir.remove_entry(&filename)
                .await
                .map_err(opfs_to_io_error)?;

            Ok(())
        })
    }

    fn list_md_files<'a>(&'a self, dir_path: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move {
            let dir = if dir_path.as_os_str().is_empty() || dir_path == Path::new(".") {
                self.root.clone()
            } else {
                get_directory(&self.root, dir_path)
                    .await
                    .map_err(opfs_to_io_error)?
            };

            let mut entries_stream = dir.entries().await.map_err(opfs_to_io_error)?;

            let mut md_files = Vec::new();
            while let Some(entry_result) = entries_stream.next().await {
                if let Ok((name, entry)) = entry_result {
                    if matches!(entry, DirectoryEntry::File(_)) && name.ends_with(".md") {
                        md_files.push(dir_path.join(&name));
                    }
                }
            }

            Ok(md_files)
        })
    }

    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            // Try to get the parent directory
            let dir = match get_parent_dir(&self.root, path).await {
                Ok(d) => d,
                Err(_) => return false,
            };

            let filename = match get_filename(path) {
                Ok(f) => f,
                Err(_) => return false,
            };

            // Try to get the file handle
            let options = GetFileHandleOptions { create: false };
            dir.get_file_handle_with_options(&filename, &options)
                .await
                .is_ok()
        })
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut current = self.root.clone();

            for component in path.components() {
                if let std::path::Component::Normal(name) = component {
                    let name_str = name.to_string_lossy();
                    let options = GetDirectoryHandleOptions { create: true };
                    current = current
                        .get_directory_handle_with_options(&name_str, &options)
                        .await
                        .map_err(opfs_to_io_error)?;
                }
            }

            Ok(())
        })
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        Box::pin(async move { get_directory(&self.root, path).await.is_ok() })
    }

    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Read the source file
            let content = self.read_to_string(from).await?;

            // Write to destination
            self.write_file(to, &content).await?;

            // Delete source
            self.delete_file(from).await?;

            Ok(())
        })
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        Box::pin(async move {
            let dir = get_parent_dir(&self.root, path)
                .await
                .map_err(opfs_to_io_error)?;
            let filename = get_filename(path)?;

            let options = GetFileHandleOptions { create: false };
            let file = dir
                .get_file_handle_with_options(&filename, &options)
                .await
                .map_err(opfs_to_io_error)?;

            file.read().await.map_err(opfs_to_io_error)
        })
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        let content = content.to_vec();
        Box::pin(async move {
            let dir = get_or_create_parent_dir(&self.root, path)
                .await
                .map_err(opfs_to_io_error)?;
            let filename = get_filename(path)?;

            let file_options = GetFileHandleOptions { create: true };
            let mut file = dir
                .get_file_handle_with_options(&filename, &file_options)
                .await
                .map_err(opfs_to_io_error)?;

            let write_options = CreateWritableOptions {
                keep_existing_data: false,
            };
            let mut writer = file
                .create_writable_with_options(&write_options)
                .await
                .map_err(opfs_to_io_error)?;

            writer
                .write_at_cursor_pos(content)
                .await
                .map_err(opfs_to_io_error)?;

            writer.close().await.map_err(opfs_to_io_error)?;

            Ok(())
        })
    }

    fn list_files<'a>(&'a self, dir_path: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move {
            let dir = if dir_path.as_os_str().is_empty() || dir_path == Path::new(".") {
                self.root.clone()
            } else {
                get_directory(&self.root, dir_path)
                    .await
                    .map_err(opfs_to_io_error)?
            };

            let mut entries_stream = dir.entries().await.map_err(opfs_to_io_error)?;

            let mut files = Vec::new();
            while let Some(entry_result) = entries_stream.next().await {
                if let Ok((name, entry)) = entry_result {
                    // Only include actual files, not directories
                    if matches!(entry, DirectoryEntry::File(_)) {
                        files.push(dir_path.join(&name));
                    }
                }
            }

            Ok(files)
        })
    }
}
