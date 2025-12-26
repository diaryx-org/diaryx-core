use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

/// Abstraction over filesystem operations
/// Allows for different implementations: real filesystem, in-memory (for WASM), etc.
pub trait FileSystem {
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
}

// ============================================================================
// RealFileSystem - Only available on non-WASM targets
// ============================================================================

#[cfg(not(target_arch = "wasm32"))]
use std::fs::{self, OpenOptions};
#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy)]
/// This is a simple filesystem implementation that simply maps to std::fs methods
pub struct RealFileSystem;

#[cfg(not(target_arch = "wasm32"))]
impl FileSystem for RealFileSystem {
    fn read_to_string(&self, path: &Path) -> Result<String> {
        fs::read_to_string(path)
    }

    fn write_file(&self, path: &Path, content: &str) -> Result<()> {
        fs::write(path, content)
    }

    fn delete_file(&self, path: &Path) -> Result<()> {
        fs::remove_file(path)
    }

    fn create_new(&self, path: &Path, content: &str) -> Result<()> {
        // This atomic check prevents race conditions
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(content.as_bytes())
    }

    fn list_md_files(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        if dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "md") {
                    files.push(path);
                }
            }
        }
        Ok(files)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path)
    }

    fn move_file(&self, from: &Path, to: &Path) -> Result<()> {
        if !from.exists() {
            return Err(Error::new(
                ErrorKind::NotFound,
                format!("Source file not found: {:?}", from),
            ));
        }
        if to.exists() {
            return Err(Error::new(
                ErrorKind::AlreadyExists,
                format!("Destination already exists: {:?}", to),
            ));
        }

        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::rename(from, to)
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }
}

// ============================================================================
// InMemoryFileSystem - Available on all targets, including WASM
// ============================================================================

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

/// An in-memory filesystem implementation
/// Useful for WASM targets where real filesystem access is not available
/// Also useful for testing
#[derive(Clone, Default)]
pub struct InMemoryFileSystem {
    /// Files stored as path -> content (text files)
    files: Arc<RwLock<HashMap<PathBuf, String>>>,
    /// Binary files stored as path -> bytes (attachments)
    binary_files: Arc<RwLock<HashMap<PathBuf, Vec<u8>>>>,
    /// Directories that exist (implicitly created when files are added)
    directories: Arc<RwLock<HashSet<PathBuf>>>,
}

impl InMemoryFileSystem {
    /// Create a new empty in-memory filesystem
    pub fn new() -> Self {
        Self {
            files: Arc::new(RwLock::new(HashMap::new())),
            binary_files: Arc::new(RwLock::new(HashMap::new())),
            directories: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Create a filesystem pre-populated with files
    /// Useful for loading from IndexedDB or other storage
    pub fn with_files(entries: Vec<(PathBuf, String)>) -> Self {
        let fs = Self::new();
        {
            let mut files = fs.files.write().unwrap();
            let mut dirs = fs.directories.write().unwrap();

            for (path, content) in entries {
                // Add all parent directories
                let mut current = path.as_path();
                while let Some(parent) = current.parent() {
                    if !parent.as_os_str().is_empty() {
                        dirs.insert(parent.to_path_buf());
                    }
                    current = parent;
                }
                files.insert(path, content);
            }
        }
        fs
    }

    /// Load files from a list of (path_string, content) tuples
    /// Convenience method for JavaScript interop
    pub fn load_from_entries(entries: Vec<(String, String)>) -> Self {
        let entries: Vec<(PathBuf, String)> = entries
            .into_iter()
            .map(|(path, content)| (PathBuf::from(path), content))
            .collect();
        Self::with_files(entries)
    }

    /// Export all files as (path_string, content) tuples
    /// Useful for persisting to IndexedDB or other storage
    pub fn export_entries(&self) -> Vec<(String, String)> {
        let files = self.files.read().unwrap();
        files
            .iter()
            .map(|(path, content)| (path.to_string_lossy().to_string(), content.clone()))
            .collect()
    }

    /// Export all binary files as (path_string, content_bytes) tuples
    /// For persisting attachments to IndexedDB
    pub fn export_binary_entries(&self) -> Vec<(String, Vec<u8>)> {
        let binary_files = self.binary_files.read().unwrap();
        binary_files
            .iter()
            .map(|(path, content)| (path.to_string_lossy().to_string(), content.clone()))
            .collect()
    }

    /// Load binary files from a list of (path_string, content_bytes) tuples
    pub fn load_binary_entries(&self, entries: Vec<(String, Vec<u8>)>) {
        let mut binary_files = self.binary_files.write().unwrap();
        let mut dirs = self.directories.write().unwrap();

        for (path_str, content) in entries {
            let path = PathBuf::from(&path_str);
            // Add all parent directories
            let mut current = path.as_path();
            while let Some(parent) = current.parent() {
                if !parent.as_os_str().is_empty() {
                    dirs.insert(parent.to_path_buf());
                }
                current = parent;
            }
            binary_files.insert(path, content);
        }
    }

    /// Get a list of all file paths in the filesystem
    pub fn list_all_files(&self) -> Vec<PathBuf> {
        let files = self.files.read().unwrap();
        files.keys().cloned().collect()
    }

    /// Clear all files and directories
    pub fn clear(&self) {
        let mut files = self.files.write().unwrap();
        let mut dirs = self.directories.write().unwrap();
        files.clear();
        dirs.clear();
    }

    /// Helper to normalize paths (remove . and .. components where possible)
    fn normalize_path(path: &Path) -> PathBuf {
        let mut components = Vec::new();
        for component in path.components() {
            use std::path::Component;
            match component {
                Component::CurDir => {} // Skip "."
                Component::ParentDir => {
                    // Go up one level if possible
                    if !components.is_empty() {
                        components.pop();
                    }
                }
                c => components.push(c),
            }
        }
        components.iter().collect()
    }
}

impl FileSystem for InMemoryFileSystem {
    fn read_to_string(&self, path: &Path) -> Result<String> {
        let normalized = Self::normalize_path(path);
        let files = self.files.read().unwrap();
        files
            .get(&normalized)
            .cloned()
            .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("File not found: {:?}", path)))
    }

    fn write_file(&self, path: &Path, content: &str) -> Result<()> {
        let normalized = Self::normalize_path(path);

        // Ensure parent directories exist
        if let Some(parent) = normalized.parent() {
            self.create_dir_all(parent)?;
        }

        let mut files = self.files.write().unwrap();
        files.insert(normalized, content.to_string());
        Ok(())
    }

    fn create_new(&self, path: &Path, content: &str) -> Result<()> {
        let normalized = Self::normalize_path(path);

        // Check if file exists first
        {
            let files = self.files.read().unwrap();
            if files.contains_key(&normalized) {
                return Err(Error::new(
                    ErrorKind::AlreadyExists,
                    format!("File already exists: {:?}", path),
                ));
            }
        }

        // Ensure parent directories exist
        if let Some(parent) = normalized.parent() {
            self.create_dir_all(parent)?;
        }

        let mut files = self.files.write().unwrap();
        files.insert(normalized, content.to_string());
        Ok(())
    }

    fn delete_file(&self, path: &Path) -> Result<()> {
        let normalized = Self::normalize_path(path);

        // Try text files first
        {
            let mut files = self.files.write().unwrap();
            if files.remove(&normalized).is_some() {
                return Ok(());
            }
        }

        // Try binary files
        {
            let mut binary_files = self.binary_files.write().unwrap();
            if binary_files.remove(&normalized).is_some() {
                return Ok(());
            }
        }

        Err(Error::new(
            ErrorKind::NotFound,
            format!("File not found: {:?}", path),
        ))
    }

    fn list_md_files(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        let normalized = Self::normalize_path(dir);
        let files = self.files.read().unwrap();

        let mut result = Vec::new();
        for path in files.keys() {
            // Check if the file is directly in this directory (not in a subdirectory)
            if let Some(parent) = path.parent()
                && parent == normalized
                && path.extension().is_some_and(|ext| ext == "md")
            {
                result.push(path.clone());
            }
        }
        Ok(result)
    }

    fn exists(&self, path: &Path) -> bool {
        let normalized = Self::normalize_path(path);
        let files = self.files.read().unwrap();
        let binary_files = self.binary_files.read().unwrap();
        let dirs = self.directories.read().unwrap();
        files.contains_key(&normalized)
            || binary_files.contains_key(&normalized)
            || dirs.contains(&normalized)
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        let normalized = Self::normalize_path(path);
        let mut dirs = self.directories.write().unwrap();

        // Add the directory and all parent directories
        let mut current = normalized.as_path();
        loop {
            if !current.as_os_str().is_empty() {
                dirs.insert(current.to_path_buf());
            }
            match current.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => {
                    current = parent;
                }
                _ => break,
            }
        }

        Ok(())
    }

    fn is_dir(&self, path: &Path) -> bool {
        let normalized = Self::normalize_path(path);
        let dirs = self.directories.read().unwrap();
        dirs.contains(&normalized)
    }

    fn move_file(&self, from: &Path, to: &Path) -> Result<()> {
        let from_norm = Self::normalize_path(from);
        let to_norm = Self::normalize_path(to);

        if from_norm == to_norm {
            return Ok(());
        }

        // Check if this is a directory move
        let is_dir = self.is_dir(&from_norm);

        if is_dir {
            // Moving a directory: relocate all files within it
            let files_to_move: Vec<(PathBuf, String)>;
            {
                let files = self.files.read().unwrap();
                files_to_move = files
                    .iter()
                    .filter(|(path, _)| path.starts_with(&from_norm))
                    .map(|(path, content)| (path.clone(), content.clone()))
                    .collect();
            }

            if files_to_move.is_empty() && !self.is_dir(&from_norm) {
                return Err(Error::new(
                    ErrorKind::NotFound,
                    format!("Source directory not found or empty: {:?}", from),
                ));
            }

            // Check destination doesn't already exist as a file or directory
            {
                let files = self.files.read().unwrap();
                let dirs = self.directories.read().unwrap();
                if files.contains_key(&to_norm) || dirs.contains(&to_norm) {
                    return Err(Error::new(
                        ErrorKind::AlreadyExists,
                        format!("Destination already exists: {:?}", to),
                    ));
                }
            }

            // Move all files to new location
            {
                let mut files = self.files.write().unwrap();
                for (old_path, content) in files_to_move {
                    files.remove(&old_path);
                    // Replace the source prefix with the destination prefix
                    let relative = old_path.strip_prefix(&from_norm).unwrap();
                    let new_path = to_norm.join(relative);
                    files.insert(new_path, content);
                }
            }

            // Update directories: remove old, add new
            {
                let mut dirs = self.directories.write().unwrap();
                // Remove old directory and its subdirectories
                let old_dirs: Vec<PathBuf> = dirs
                    .iter()
                    .filter(|d| d.starts_with(&from_norm) || **d == from_norm)
                    .cloned()
                    .collect();
                for old_dir in old_dirs {
                    dirs.remove(&old_dir);
                    // Add corresponding new directory
                    if old_dir == from_norm {
                        dirs.insert(to_norm.clone());
                    } else if let Ok(relative) = old_dir.strip_prefix(&from_norm) {
                        dirs.insert(to_norm.join(relative));
                    }
                }

                // Ensure parent directories of destination exist
                let mut current = to_norm.as_path();
                loop {
                    match current.parent() {
                        Some(parent) if !parent.as_os_str().is_empty() => {
                            dirs.insert(parent.to_path_buf());
                            current = parent;
                        }
                        _ => break,
                    }
                }
            }

            Ok(())
        } else {
            // Moving a single file (original behavior)
            // Validate existence and destination availability up-front.
            {
                let files = self.files.read().unwrap();

                if !files.contains_key(&from_norm) {
                    return Err(Error::new(
                        ErrorKind::NotFound,
                        format!("Source file not found: {:?}", from),
                    ));
                }

                if files.contains_key(&to_norm) {
                    return Err(Error::new(
                        ErrorKind::AlreadyExists,
                        format!("Destination already exists: {:?}", to),
                    ));
                }
            }

            // Ensure destination parent directories exist.
            if let Some(parent) = to_norm.parent() {
                self.create_dir_all(parent)?;
            }

            // Perform the move.
            let mut files = self.files.write().unwrap();
            let content = files.remove(&from_norm).ok_or_else(|| {
                Error::new(
                    ErrorKind::NotFound,
                    format!("Source file not found: {:?}", from),
                )
            })?;
            files.insert(to_norm, content);

            Ok(())
        }
    }

    fn read_binary(&self, path: &Path) -> Result<Vec<u8>> {
        let normalized = Self::normalize_path(path);

        // First check binary files
        {
            let binary_files = self.binary_files.read().unwrap();
            if let Some(data) = binary_files.get(&normalized) {
                return Ok(data.clone());
            }
        }

        // Fall back to text files (convert to bytes)
        let files = self.files.read().unwrap();
        files
            .get(&normalized)
            .map(|s| s.as_bytes().to_vec())
            .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("File not found: {:?}", path)))
    }

    fn write_binary(&self, path: &Path, content: &[u8]) -> Result<()> {
        let normalized = Self::normalize_path(path);

        // Ensure parent directories exist
        if let Some(parent) = normalized.parent() {
            self.create_dir_all(parent)?;
        }

        let mut binary_files = self.binary_files.write().unwrap();
        binary_files.insert(normalized, content.to_vec());
        Ok(())
    }

    fn list_files(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        let normalized = Self::normalize_path(dir);
        let files = self.files.read().unwrap();
        let binary_files = self.binary_files.read().unwrap();

        let mut result = Vec::new();

        // Check text files
        for path in files.keys() {
            if let Some(parent) = path.parent()
                && parent == normalized
            {
                result.push(path.clone());
            }
        }

        // Check binary files
        for path in binary_files.keys() {
            if let Some(parent) = path.parent()
                && parent == normalized
            {
                result.push(path.clone());
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_fs_basic_operations() {
        let fs = InMemoryFileSystem::new();

        // Create and read a file
        fs.write_file(Path::new("test.md"), "Hello, World!")
            .unwrap();
        assert_eq!(
            fs.read_to_string(Path::new("test.md")).unwrap(),
            "Hello, World!"
        );

        // Check existence
        assert!(fs.exists(Path::new("test.md")));
        assert!(!fs.exists(Path::new("nonexistent.md")));

        // Delete file
        fs.delete_file(Path::new("test.md")).unwrap();
        assert!(!fs.exists(Path::new("test.md")));
    }

    #[test]
    fn test_in_memory_fs_create_new() {
        let fs = InMemoryFileSystem::new();

        // Create new file
        fs.create_new(Path::new("new.md"), "Content").unwrap();
        assert_eq!(fs.read_to_string(Path::new("new.md")).unwrap(), "Content");

        // Try to create same file again - should fail
        let result = fs.create_new(Path::new("new.md"), "Other content");
        assert!(result.is_err());
    }

    #[test]
    fn test_in_memory_fs_directories() {
        let fs = InMemoryFileSystem::new();

        // Create a file in a nested directory
        fs.write_file(Path::new("a/b/c/file.md"), "Content")
            .unwrap();

        // Parent directories should exist
        assert!(fs.is_dir(Path::new("a")));
        assert!(fs.is_dir(Path::new("a/b")));
        assert!(fs.is_dir(Path::new("a/b/c")));

        // File should exist
        assert!(fs.exists(Path::new("a/b/c/file.md")));
    }

    #[test]
    fn test_in_memory_fs_list_md_files() {
        let fs = InMemoryFileSystem::new();

        fs.write_file(Path::new("dir/file1.md"), "Content 1")
            .unwrap();
        fs.write_file(Path::new("dir/file2.md"), "Content 2")
            .unwrap();
        fs.write_file(Path::new("dir/file.txt"), "Not markdown")
            .unwrap();
        fs.write_file(Path::new("dir/subdir/file3.md"), "Content 3")
            .unwrap();

        let md_files = fs.list_md_files(Path::new("dir")).unwrap();

        // Should only include direct children that are .md files
        assert_eq!(md_files.len(), 2);
        assert!(md_files.contains(&PathBuf::from("dir/file1.md")));
        assert!(md_files.contains(&PathBuf::from("dir/file2.md")));
    }

    #[test]
    fn test_in_memory_fs_export_import() {
        let fs = InMemoryFileSystem::new();

        fs.write_file(Path::new("file1.md"), "Content 1").unwrap();
        fs.write_file(Path::new("dir/file2.md"), "Content 2")
            .unwrap();

        // Export
        let entries = fs.export_entries();
        assert_eq!(entries.len(), 2);

        // Import into new filesystem
        let fs2 = InMemoryFileSystem::load_from_entries(entries);

        // Verify contents
        assert_eq!(
            fs2.read_to_string(Path::new("file1.md")).unwrap(),
            "Content 1"
        );
        assert_eq!(
            fs2.read_to_string(Path::new("dir/file2.md")).unwrap(),
            "Content 2"
        );
    }

    #[test]
    fn test_in_memory_fs_path_normalization() {
        let fs = InMemoryFileSystem::new();

        fs.write_file(Path::new("dir/file.md"), "Content").unwrap();

        // Should be able to read with different path representations
        assert!(fs.exists(Path::new("dir/file.md")));
        assert!(fs.exists(Path::new("dir/./file.md")));
        assert!(fs.exists(Path::new("dir/subdir/../file.md")));
    }
}
