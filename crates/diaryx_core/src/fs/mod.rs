//! Filesystem abstraction module.
//!
//! This module provides the `FileSystem` trait for abstracting filesystem operations,
//! allowing different implementations for native and WASM targets.
//!
//! For async operations, see the `AsyncFileSystem` trait and `SyncToAsyncFs` adapter.
//!
//! ## Decorators
//!
//! The filesystem module includes a decorator pattern for extending filesystem behavior:
//!
//! - [`EventEmittingFs`]: Emits events for all filesystem operations
//! - [`CrdtFs`]: Automatically updates CRDT on file operations (requires `crdt` feature)
//! - [`DecoratedFsBuilder`]: Builder for composing decorators (requires `crdt` feature)
//!
//! ### Example (with `crdt` feature)
//!
//! ```ignore
//! use diaryx_core::fs::{DecoratedFsBuilder, InMemoryFileSystem, SyncToAsyncFs};
//! use diaryx_core::crdt::MemoryStorage;
//! use std::sync::Arc;
//!
//! let base_fs = SyncToAsyncFs::new(InMemoryFileSystem::new());
//! let storage = Arc::new(MemoryStorage::new());
//!
//! let decorated = DecoratedFsBuilder::new(base_fs)
//!     .with_crdt(storage)
//!     .build();
//!
//! // All writes now update CRDT and emit events
//! ```

mod async_fs;
mod memory;
#[cfg(not(target_arch = "wasm32"))]
mod native;

// Decorator modules
mod callback_registry;
mod event_fs;
mod events;

// CRDT-dependent decorators
#[cfg(feature = "crdt")]
mod crdt_fs;
#[cfg(feature = "crdt")]
mod decorator_stack;

pub use async_fs::{AsyncFileSystem, BoxFuture, SyncToAsyncFs};

#[cfg(test)]
pub(crate) use async_fs::block_on_test;
pub use memory::InMemoryFileSystem;
#[cfg(not(target_arch = "wasm32"))]
pub use native::RealFileSystem;

// Export event types and callback registry (always available)
pub use callback_registry::{CallbackRegistry, EventCallback, SubscriptionId};
pub use event_fs::EventEmittingFs;
pub use events::FileSystemEvent;

// Export CRDT-dependent decorators
#[cfg(feature = "crdt")]
pub use crdt_fs::CrdtFs;
#[cfg(feature = "crdt")]
pub use decorator_stack::{DecoratedFs, DecoratedFsBuilder};

use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

/// Abstraction over filesystem operations
/// Allows for different implementations: real filesystem, in-memory (for WASM), etc.
/// Send + Sync required for multi-threaded environments (e.g., Tauri)
pub trait FileSystem: Send + Sync {
    /// Reads the file content (for parsing frontmatter)
    fn read_to_string(&self, path: &Path) -> Result<String>;

    /// Overwrites an existing file (for updating frontmatter)
    fn write_file(&self, path: &Path, content: &str) -> Result<()>;

    /// Creates a file ONLY if it doesn't exist (for new posts)
    /// Should return an error if file exists.
    fn create_new(&self, path: &Path, content: &str) -> Result<()>;

    /// Deletes a file
    fn delete_file(&self, path: &Path) -> Result<()>;

    /// Finds markdown files in a folder
    fn list_md_files(&self, dir: &Path) -> Result<Vec<PathBuf>>;

    /// Checks if a file exists
    fn exists(&self, path: &Path) -> bool;

    /// Creates a directory and all parent directories
    fn create_dir_all(&self, path: &Path) -> Result<()>;

    /// Checks if a path is a directory
    fn is_dir(&self, path: &Path) -> bool;

    /// Move/rename a file from `from` to `to`.
    ///
    /// Implementations should treat this as an atomic-ish move when possible,
    /// and should error if the source does not exist or if the destination already exists.
    fn move_file(&self, from: &Path, to: &Path) -> Result<()>;

    // ==================== Binary File Methods ====================
    // These methods support binary files (attachments) without base64 overhead

    /// Read binary file content
    fn read_binary(&self, path: &Path) -> Result<Vec<u8>> {
        // Default implementation: read as string and convert to bytes
        self.read_to_string(path).map(|s| s.into_bytes())
    }

    /// Write binary content to a file
    fn write_binary(&self, _path: &Path, _content: &[u8]) -> Result<()> {
        // Default implementation: not supported
        Err(Error::new(
            ErrorKind::Unsupported,
            "Binary write not supported",
        ))
    }

    /// List all files in a directory (not recursive)
    fn list_files(&self, _dir: &Path) -> Result<Vec<PathBuf>> {
        // Default: return empty
        Ok(vec![])
    }

    /// Recursively list all markdown files in a directory and its subdirectories
    fn list_md_files_recursive(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        let mut all_files = self.list_md_files(dir)?;

        // Get subdirectories and recurse
        if let Ok(entries) = self.list_files(dir) {
            for entry in entries {
                if self.is_dir(&entry)
                    && let Ok(subdir_files) = self.list_md_files_recursive(&entry)
                {
                    all_files.extend(subdir_files);
                }
            }
        }

        Ok(all_files)
    }

    /// Recursively list ALL files and directories in a directory
    fn list_all_files_recursive(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        let mut all_entries = Vec::new();

        if let Ok(entries) = self.list_files(dir) {
            for entry in entries {
                all_entries.push(entry.clone());
                if self.is_dir(&entry)
                    && let Ok(subdir_entries) = self.list_all_files_recursive(&entry)
                {
                    all_entries.extend(subdir_entries);
                }
            }
        }

        Ok(all_entries)
    }

    /// Get file modification time as milliseconds since Unix epoch.
    ///
    /// Returns `None` if the file doesn't exist or the modification time
    /// cannot be determined.
    fn get_modified_time(&self, _path: &Path) -> Option<i64> {
        None
    }
}

// Blanket implementation for references to FileSystem
impl<T: FileSystem> FileSystem for &T {
    fn read_to_string(&self, path: &Path) -> Result<String> {
        (*self).read_to_string(path)
    }

    fn write_file(&self, path: &Path, content: &str) -> Result<()> {
        (*self).write_file(path, content)
    }

    fn create_new(&self, path: &Path, content: &str) -> Result<()> {
        (*self).create_new(path, content)
    }

    fn delete_file(&self, path: &Path) -> Result<()> {
        (*self).delete_file(path)
    }

    fn list_md_files(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        (*self).list_md_files(dir)
    }

    fn exists(&self, path: &Path) -> bool {
        (*self).exists(path)
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        (*self).create_dir_all(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        (*self).is_dir(path)
    }

    fn move_file(&self, from: &Path, to: &Path) -> Result<()> {
        (*self).move_file(from, to)
    }

    fn read_binary(&self, path: &Path) -> Result<Vec<u8>> {
        (*self).read_binary(path)
    }

    fn write_binary(&self, path: &Path, content: &[u8]) -> Result<()> {
        (*self).write_binary(path, content)
    }

    fn list_files(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        (*self).list_files(dir)
    }

    fn get_modified_time(&self, path: &Path) -> Option<i64> {
        (*self).get_modified_time(path)
    }
}
