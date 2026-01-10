//! File System Access API implementation of AsyncFileSystem.
//!
//! Uses the File System Access API to read/write files in a user-selected
//! directory on their actual filesystem. Unlike OPFS, this edits real files.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { FsaFileSystem, DiaryxBackend } from './wasm/diaryx_wasm.js';
//!
//! // User must trigger this via a gesture (click/keypress)
//! const dirHandle = await window.showDirectoryPicker();
//! const backend = await DiaryxBackend.createFromDirectoryHandle(dirHandle);
//! ```
//!
//! ## Browser Support
//! - Chrome/Edge: ✅ Supported
//! - Firefox: ❌ Not supported
//! - Safari: ❌ Not supported

use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

use diaryx_core::fs::{AsyncFileSystem, BoxFuture};
use futures::StreamExt;
use wasm_bindgen::prelude::*;

use opfs::persistent::{self, DirectoryHandle};
use opfs::{
    CreateWritableOptions, DirectoryEntry, DirectoryHandle as DirectoryHandleTrait,
    FileHandle as FileHandleTrait, GetDirectoryHandleOptions, GetFileHandleOptions,
    WritableFileStream as WritableFileStreamTrait,
};

// ============================================================================
// FsaFileSystem Implementation
// ============================================================================

/// AsyncFileSystem implementation backed by File System Access API.
///
/// Allows editing files directly on the user's filesystem in a directory
/// they select via `showDirectoryPicker()`.
#[wasm_bindgen]
pub struct FsaFileSystem {
    root: DirectoryHandle,
}

impl Clone for FsaFileSystem {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
        }
    }
}

#[wasm_bindgen]
impl FsaFileSystem {
    /// Create a new FsaFileSystem from a user-selected directory handle.
    ///
    /// The handle must be obtained from `window.showDirectoryPicker()` in JavaScript.
    #[wasm_bindgen(js_name = "fromHandle")]
    pub fn from_handle(handle: web_sys::FileSystemDirectoryHandle) -> Self {
        // Convert web_sys handle to opfs crate's DirectoryHandle
        let root = DirectoryHandle::from(handle);
        Self { root }
    }
}

// ============================================================================
// Helper Functions (same as opfs_fs.rs)
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

impl AsyncFileSystem for FsaFileSystem {
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
                if let Ok((name, _)) = entry_result {
                    files.push(dir_path.join(&name));
                }
            }

            Ok(files)
        })
    }
}
