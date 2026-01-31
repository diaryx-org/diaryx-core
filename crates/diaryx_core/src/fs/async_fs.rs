//! Async filesystem abstraction module.
//!
//! This module provides the `AsyncFileSystem` trait for abstracting async filesystem operations,
//! allowing different implementations for native and WASM targets.
//!
//! This is particularly useful for:
//! - WASM environments where JavaScript APIs (like IndexedDB) are inherently async
//! - Native environments using async runtimes like tokio
//! - Code that needs to await filesystem operations
//!
//! ## Object safety
//!
//! `AsyncFileSystem` is designed to be object-safe so it can be used behind
//! `dyn AsyncFileSystem` (e.g. inside trait objects like backup targets).
//! To enable this, all methods return boxed futures.

use std::future::Future;
use std::io::Result;
use std::path::{Path, PathBuf};
use std::pin::Pin;

#[cfg(test)]
pub(crate) fn block_on_test<F: Future>(f: F) -> F::Output {
    futures_lite::future::block_on(f)
}

/// A boxed future for object-safe async methods.
///
/// On native targets, futures are `Send` for compatibility with multi-threaded runtimes.
/// On WASM, there's no `Send` requirement since JavaScript is single-threaded.
#[cfg(not(target_arch = "wasm32"))]
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A boxed future for object-safe async methods.
///
/// WASM version without `Send` requirement - JavaScript is single-threaded.
#[cfg(target_arch = "wasm32")]
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Async abstraction over filesystem operations.
///
/// This trait mirrors `FileSystem` but with async methods, making it suitable
/// for environments where filesystem operations may be asynchronous (e.g., WASM
/// with IndexedDB, or native code using async I/O).
///
/// # Example
///
/// ```ignore
/// use diaryx_core::fs::AsyncFileSystem;
///
/// async fn example(fs: &dyn AsyncFileSystem) {
///     let content = fs.read_to_string(Path::new("file.md")).await.unwrap();
///     fs.write_file(Path::new("output.md"), &content).await.unwrap();
/// }
/// ```
#[cfg(not(target_arch = "wasm32"))]
pub trait AsyncFileSystem: Send + Sync {
    /// Reads the file content as a string.
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>>;

    /// Overwrites an existing file with new content.
    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>>;

    /// Creates a file ONLY if it doesn't exist.
    /// Should return an error if file exists.
    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>>;

    /// Deletes a file.
    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>>;

    /// Finds markdown files in a folder.
    fn list_md_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>>;

    /// Checks if a file or directory exists.
    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool>;

    /// Creates a directory and all parent directories.
    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>>;

    /// Checks if a path is a directory.
    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool>;

    /// Checks if a path is a symlink.
    /// Returns false for non-existent paths or on platforms that don't support symlinks.
    fn is_symlink<'a>(&'a self, _path: &'a Path) -> BoxFuture<'a, bool> {
        // Default: return false (no symlink support, e.g., in-memory or WASM)
        Box::pin(async move { false })
    }

    /// Move/rename a file from `from` to `to`.
    ///
    /// Implementations should treat this as an atomic-ish move when possible,
    /// and should error if the source does not exist or if the destination already exists.
    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>>;

    // ==================== Binary File Methods ====================
    // These methods support binary files (attachments) without base64 overhead

    /// Read binary file content.
    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        Box::pin(async move {
            // Default implementation: read as string and convert to bytes
            self.read_to_string(path).await.map(|s| s.into_bytes())
        })
    }

    /// Write binary content to a file.
    fn write_binary<'a>(
        &'a self,
        _path: &'a Path,
        _content: &'a [u8],
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Default implementation: not supported
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Binary write not supported",
            ))
        })
    }

    /// List all files in a directory (not recursive).
    fn list_files<'a>(&'a self, _dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move {
            // Default: return empty
            Ok(vec![])
        })
    }

    /// Recursively list all markdown files in a directory and its subdirectories.
    fn list_md_files_recursive<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move {
            let mut all_files = self.list_md_files(dir).await?;

            // Get subdirectories and recurse
            if let Ok(entries) = self.list_files(dir).await {
                for entry in entries {
                    if self.is_dir(&entry).await
                        && let Ok(subdir_files) = self.list_md_files_recursive(&entry).await
                    {
                        all_files.extend(subdir_files);
                    }
                }
            }

            Ok(all_files)
        })
    }

    /// Recursively list ALL files and directories in a directory.
    fn list_all_files_recursive<'a>(
        &'a self,
        dir: &'a Path,
    ) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move {
            let mut all_entries = Vec::new();

            if let Ok(entries) = self.list_files(dir).await {
                for entry in entries {
                    all_entries.push(entry.clone());
                    if self.is_dir(&entry).await
                        && let Ok(subdir_entries) = self.list_all_files_recursive(&entry).await
                    {
                        all_entries.extend(subdir_entries);
                    }
                }
            }

            Ok(all_entries)
        })
    }

    /// Get file modification time as milliseconds since Unix epoch.
    ///
    /// Returns `None` if the file doesn't exist or the modification time
    /// cannot be determined (e.g., in WASM environments without real filesystem).
    fn get_modified_time<'a>(&'a self, _path: &'a Path) -> BoxFuture<'a, Option<i64>> {
        Box::pin(async move { None })
    }

    // ==================== Sync Write Markers ====================
    // These methods support preventing CRDT feedback loops during sync operations.
    // Default implementations are no-ops; CrdtFs overrides to actually track.

    /// Mark a path as being written from sync (skip CRDT updates if applicable).
    ///
    /// Call this before writing a file from a remote sync operation to prevent
    /// the CRDT layer from generating a new update for data that came FROM the CRDT.
    /// This prevents infinite sync loops.
    ///
    /// Default implementation is a no-op. Override in CrdtFs to track sync writes.
    fn mark_sync_write_start(&self, _path: &Path) {
        // Default: no-op
    }

    /// Clear the sync write marker for a path.
    ///
    /// Call this after a sync write operation completes.
    ///
    /// Default implementation is a no-op. Override in CrdtFs to track sync writes.
    fn mark_sync_write_end(&self, _path: &Path) {
        // Default: no-op
    }
}

/// Async abstraction over filesystem operations (WASM version).
///
/// This is the WASM-specific version without Send + Sync bounds since
/// JavaScript environments are single-threaded.
#[cfg(target_arch = "wasm32")]
pub trait AsyncFileSystem {
    /// Reads the file content as a string.
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>>;

    /// Overwrites an existing file with new content.
    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>>;

    /// Creates a file ONLY if it doesn't exist.
    /// Should return an error if file exists.
    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>>;

    /// Deletes a file.
    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>>;

    /// Finds markdown files in a folder.
    fn list_md_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>>;

    /// Checks if a file or directory exists.
    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool>;

    /// Creates a directory and all parent directories.
    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>>;

    /// Checks if a path is a directory.
    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool>;

    /// Checks if a path is a symlink.
    /// Returns false for non-existent paths or on platforms that don't support symlinks.
    fn is_symlink<'a>(&'a self, _path: &'a Path) -> BoxFuture<'a, bool> {
        // Default: return false (no symlink support, e.g., in-memory or WASM)
        Box::pin(async move { false })
    }

    /// Move/rename a file from `from` to `to`.
    ///
    /// Implementations should treat this as an atomic-ish move when possible,
    /// and should error if the source does not exist or if the destination already exists.
    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>>;

    // ==================== Binary File Methods ====================
    // These methods support binary files (attachments) without base64 overhead

    /// Read binary file content.
    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        Box::pin(async move {
            // Default implementation: read as string and convert to bytes
            self.read_to_string(path).await.map(|s| s.into_bytes())
        })
    }

    /// Write binary content to a file.
    fn write_binary<'a>(
        &'a self,
        _path: &'a Path,
        _content: &'a [u8],
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Default implementation: not supported
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Binary write not supported",
            ))
        })
    }

    /// List all files in a directory (not recursive).
    fn list_files<'a>(&'a self, _dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move {
            // Default: return empty
            Ok(vec![])
        })
    }

    /// Recursively list all markdown files in a directory and its subdirectories.
    fn list_md_files_recursive<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move {
            let mut all_files = self.list_md_files(dir).await?;

            // Get subdirectories and recurse
            if let Ok(entries) = self.list_files(dir).await {
                for entry in entries {
                    if self.is_dir(&entry).await {
                        if let Ok(subdir_files) = self.list_md_files_recursive(&entry).await {
                            all_files.extend(subdir_files);
                        }
                    }
                }
            }

            Ok(all_files)
        })
    }

    /// Recursively list ALL files and directories in a directory.
    fn list_all_files_recursive<'a>(
        &'a self,
        dir: &'a Path,
    ) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move {
            let mut all_entries = Vec::new();

            if let Ok(entries) = self.list_files(dir).await {
                for entry in entries {
                    all_entries.push(entry.clone());
                    if self.is_dir(&entry).await {
                        if let Ok(subdir_entries) = self.list_all_files_recursive(&entry).await {
                            all_entries.extend(subdir_entries);
                        }
                    }
                }
            }

            Ok(all_entries)
        })
    }

    /// Get file modification time as milliseconds since Unix epoch.
    ///
    /// Returns `None` if the file doesn't exist or the modification time
    /// cannot be determined (e.g., in WASM environments without real filesystem).
    fn get_modified_time<'a>(&'a self, _path: &'a Path) -> BoxFuture<'a, Option<i64>> {
        Box::pin(async move { None })
    }

    // ==================== Sync Write Markers ====================
    // These methods support preventing CRDT feedback loops during sync operations.
    // Default implementations are no-ops; CrdtFs overrides to actually track.

    /// Mark a path as being written from sync (skip CRDT updates if applicable).
    ///
    /// Call this before writing a file from a remote sync operation to prevent
    /// the CRDT layer from generating a new update for data that came FROM the CRDT.
    /// This prevents infinite sync loops.
    ///
    /// Default implementation is a no-op. Override in CrdtFs to track sync writes.
    fn mark_sync_write_start(&self, _path: &Path) {
        // Default: no-op
    }

    /// Clear the sync write marker for a path.
    ///
    /// Call this after a sync write operation completes.
    ///
    /// Default implementation is a no-op. Override in CrdtFs to track sync writes.
    fn mark_sync_write_end(&self, _path: &Path) {
        // Default: no-op
    }
}

// ============================================================================
// Adapter: Sync FileSystem -> AsyncFileSystem
// ============================================================================

use super::FileSystem;

/// Wrapper that adapts a synchronous `FileSystem` to `AsyncFileSystem`.
///
/// This is useful for wrapping `InMemoryFileSystem` or other sync implementations
/// to be used in async contexts. The operations complete immediately since the
/// underlying implementation is synchronous.
///
/// # Example
///
/// ```ignore
/// use diaryx_core::fs::{InMemoryFileSystem, SyncToAsyncFs, AsyncFileSystem};
///
/// let sync_fs = InMemoryFileSystem::new();
/// let async_fs = SyncToAsyncFs::new(sync_fs);
///
/// // Now you can use async_fs in async code
/// async {
///     let content = async_fs.read_to_string(Path::new("file.md")).await;
/// };
/// ```
#[derive(Clone)]
pub struct SyncToAsyncFs<F: FileSystem> {
    inner: F,
}

impl<F: FileSystem> SyncToAsyncFs<F> {
    /// Create a new async wrapper around a synchronous filesystem.
    pub fn new(fs: F) -> Self {
        Self { inner: fs }
    }

    /// Get a reference to the inner synchronous filesystem.
    pub fn inner(&self) -> &F {
        &self.inner
    }

    /// Unwrap and return the inner synchronous filesystem.
    pub fn into_inner(self) -> F {
        self.inner
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<F: FileSystem + Send + Sync> AsyncFileSystem for SyncToAsyncFs<F> {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move { self.inner.read_to_string(path) })
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.inner.write_file(path, content) })
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.inner.create_new(path, content) })
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.inner.delete_file(path) })
    }

    fn list_md_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move { self.inner.list_md_files(dir) })
    }

    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        Box::pin(async move { self.inner.exists(path) })
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.inner.create_dir_all(path) })
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        Box::pin(async move { self.inner.is_dir(path) })
    }

    fn is_symlink<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        Box::pin(async move { self.inner.is_symlink(path) })
    }

    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.inner.move_file(from, to) })
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        Box::pin(async move { self.inner.read_binary(path) })
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.inner.write_binary(path, content) })
    }

    fn list_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move { self.inner.list_files(dir) })
    }

    fn get_modified_time<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Option<i64>> {
        Box::pin(async move { self.inner.get_modified_time(path) })
    }
}

#[cfg(target_arch = "wasm32")]
impl<F: FileSystem> AsyncFileSystem for SyncToAsyncFs<F> {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move { self.inner.read_to_string(path) })
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.inner.write_file(path, content) })
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.inner.create_new(path, content) })
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.inner.delete_file(path) })
    }

    fn list_md_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move { self.inner.list_md_files(dir) })
    }

    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        Box::pin(async move { self.inner.exists(path) })
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.inner.create_dir_all(path) })
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        Box::pin(async move { self.inner.is_dir(path) })
    }

    fn is_symlink<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        Box::pin(async move { self.inner.is_symlink(path) })
    }

    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.inner.move_file(from, to) })
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        Box::pin(async move { self.inner.read_binary(path) })
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.inner.write_binary(path, content) })
    }

    fn list_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move { self.inner.list_files(dir) })
    }

    fn get_modified_time<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Option<i64>> {
        Box::pin(async move { self.inner.get_modified_time(path) })
    }
}

// Blanket implementation for references to AsyncFileSystem (native)
#[cfg(not(target_arch = "wasm32"))]
impl<T: AsyncFileSystem + ?Sized> AsyncFileSystem for &T {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        (*self).read_to_string(path)
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        (*self).write_file(path, content)
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        (*self).create_new(path, content)
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        (*self).delete_file(path)
    }

    fn list_md_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        (*self).list_md_files(dir)
    }

    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        (*self).exists(path)
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        (*self).create_dir_all(path)
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        (*self).is_dir(path)
    }

    fn is_symlink<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        (*self).is_symlink(path)
    }

    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        (*self).move_file(from, to)
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        (*self).read_binary(path)
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        (*self).write_binary(path, content)
    }

    fn list_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        (*self).list_files(dir)
    }

    fn get_modified_time<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Option<i64>> {
        (*self).get_modified_time(path)
    }

    fn mark_sync_write_start(&self, path: &Path) {
        (*self).mark_sync_write_start(path)
    }

    fn mark_sync_write_end(&self, path: &Path) {
        (*self).mark_sync_write_end(path)
    }
}

// Blanket implementation for references to AsyncFileSystem (WASM)
#[cfg(target_arch = "wasm32")]
impl<T: AsyncFileSystem + ?Sized> AsyncFileSystem for &T {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        (*self).read_to_string(path)
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        (*self).write_file(path, content)
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        (*self).create_new(path, content)
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        (*self).delete_file(path)
    }

    fn list_md_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        (*self).list_md_files(dir)
    }

    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        (*self).exists(path)
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        (*self).create_dir_all(path)
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        (*self).is_dir(path)
    }

    fn is_symlink<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        (*self).is_symlink(path)
    }

    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        (*self).move_file(from, to)
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        (*self).read_binary(path)
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        (*self).write_binary(path, content)
    }

    fn list_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        (*self).list_files(dir)
    }

    fn get_modified_time<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Option<i64>> {
        (*self).get_modified_time(path)
    }

    fn mark_sync_write_start(&self, path: &Path) {
        (*self).mark_sync_write_start(path)
    }

    fn mark_sync_write_end(&self, path: &Path) {
        (*self).mark_sync_write_end(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFileSystem;

    #[test]
    fn test_sync_to_async_wrapper() {
        let sync_fs = InMemoryFileSystem::new();

        // Write a file using sync API
        sync_fs.write_file(Path::new("test.md"), "# Hello").unwrap();

        // Wrap in async adapter
        let async_fs = SyncToAsyncFs::new(sync_fs);

        // Use futures::executor to run the async code in a test
        // Note: In real async code, you'd use an async runtime
        let content = futures_lite_test_block_on(async_fs.read_to_string(Path::new("test.md")));
        assert_eq!(content.unwrap(), "# Hello");

        let exists = futures_lite_test_block_on(async_fs.exists(Path::new("test.md")));
        assert!(exists);

        let not_exists = futures_lite_test_block_on(async_fs.exists(Path::new("nonexistent.md")));
        assert!(!not_exists);
    }

    #[test]
    fn test_async_write_and_read() {
        let sync_fs = InMemoryFileSystem::new();
        let async_fs = SyncToAsyncFs::new(sync_fs);

        // Write using async API
        let write_result =
            futures_lite_test_block_on(async_fs.write_file(Path::new("new.md"), "New content"));
        assert!(write_result.is_ok());

        // Read it back
        let content = futures_lite_test_block_on(async_fs.read_to_string(Path::new("new.md")));
        assert_eq!(content.unwrap(), "New content");
    }

    #[test]
    fn test_async_create_new() {
        let sync_fs = InMemoryFileSystem::new();
        let async_fs = SyncToAsyncFs::new(sync_fs);

        // Create new file
        let result =
            futures_lite_test_block_on(async_fs.create_new(Path::new("created.md"), "Created!"));
        assert!(result.is_ok());

        // Try to create again - should fail
        let result2 =
            futures_lite_test_block_on(async_fs.create_new(Path::new("created.md"), "Again!"));
        assert!(result2.is_err());
    }

    #[test]
    fn test_async_directory_operations() {
        let sync_fs = InMemoryFileSystem::new();
        let async_fs = SyncToAsyncFs::new(sync_fs);

        // Create directory
        let result = futures_lite_test_block_on(async_fs.create_dir_all(Path::new("a/b/c")));
        assert!(result.is_ok());

        // Check it's a directory
        let is_dir = futures_lite_test_block_on(async_fs.is_dir(Path::new("a/b/c")));
        assert!(is_dir);

        // Check parent is also a directory
        let parent_is_dir = futures_lite_test_block_on(async_fs.is_dir(Path::new("a/b")));
        assert!(parent_is_dir);
    }

    #[test]
    fn test_inner_access() {
        let sync_fs = InMemoryFileSystem::new();
        sync_fs.write_file(Path::new("test.md"), "content").unwrap();

        let async_fs = SyncToAsyncFs::new(sync_fs);

        // Access inner
        assert!(async_fs.inner().exists(Path::new("test.md")));

        // Unwrap
        let recovered = async_fs.into_inner();
        assert!(recovered.exists(Path::new("test.md")));
    }

    /// Simple blocking executor for tests only.
    /// In production, use a proper async runtime.
    fn futures_lite_test_block_on<F: Future>(f: F) -> F::Output {
        use std::pin::pin;
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

        // Create a no-op waker
        const VTABLE: RawWakerVTable = RawWakerVTable::new(
            |_| RawWaker::new(std::ptr::null(), &VTABLE), // clone
            |_| {},                                       // wake
            |_| {},                                       // wake_by_ref
            |_| {},                                       // drop
        );

        let raw_waker = RawWaker::new(std::ptr::null(), &VTABLE);
        let waker = unsafe { Waker::from_raw(raw_waker) };
        let mut cx = Context::from_waker(&waker);

        let mut pinned = pin!(f);
        loop {
            match pinned.as_mut().poll(&mut cx) {
                Poll::Ready(output) => return output,
                Poll::Pending => {
                    // For our sync-wrapped futures, this should never happen
                    // But we handle it anyway by spinning
                    std::hint::spin_loop();
                }
            }
        }
    }
}
