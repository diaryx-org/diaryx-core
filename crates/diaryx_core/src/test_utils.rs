//! Test utilities for diaryx_core
//!
//! This module provides shared testing infrastructure, including a mock filesystem
//! that can be used across all test modules.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::fs::FileSystem;

/// A mock filesystem for testing.
///
/// Uses `Arc<Mutex<HashMap>>` for thread-safety and allows cloning
/// while sharing the same underlying file storage.
#[derive(Clone, Default)]
pub struct MockFileSystem {
    files: Arc<Mutex<HashMap<PathBuf, String>>>,
}

impl MockFileSystem {
    /// Create a new empty mock filesystem.
    pub fn new() -> Self {
        Self {
            files: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Add a file to the mock filesystem (builder pattern).
    pub fn with_file(self, path: &str, content: &str) -> Self {
        self.files
            .lock()
            .unwrap()
            .insert(PathBuf::from(path), content.to_string());
        self
    }

    /// Get the content of a file (for test assertions).
    pub fn get_content(&self, path: &str) -> Option<String> {
        self.files
            .lock()
            .unwrap()
            .get(&PathBuf::from(path))
            .cloned()
    }
}

impl FileSystem for MockFileSystem {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "File not found"))
    }

    fn write_file(&self, path: &Path, content: &str) -> io::Result<()> {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), content.to_string());
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        self.files.lock().unwrap().contains_key(path)
    }

    fn create_new(&self, path: &Path, content: &str) -> io::Result<()> {
        let mut files = self.files.lock().unwrap();
        if files.contains_key(path) {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "File exists"));
        }
        files.insert(path.to_path_buf(), content.to_string());
        Ok(())
    }

    fn delete_file(&self, path: &Path) -> io::Result<()> {
        self.files.lock().unwrap().remove(path);
        Ok(())
    }

    fn list_md_files(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        let files = self.files.lock().unwrap();
        let mut result = Vec::new();
        for path in files.keys() {
            if path.parent() == Some(dir) && path.extension().is_some_and(|ext| ext == "md") {
                result.push(path.clone());
            }
        }
        Ok(result)
    }

    fn create_dir_all(&self, _path: &Path) -> io::Result<()> {
        // Mock implementation - directories are implicit
        Ok(())
    }

    fn is_dir(&self, _path: &Path) -> bool {
        // Mock implementation - assume any non-file path could be a directory
        false
    }

    fn move_file(&self, from: &Path, to: &Path) -> io::Result<()> {
        let mut files = self.files.lock().unwrap();

        if !files.contains_key(from) {
            return Err(io::Error::new(io::ErrorKind::NotFound, "File not found"));
        }
        if files.contains_key(to) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Destination exists",
            ));
        }

        let content = files
            .remove(from)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "File not found"))?;
        files.insert(to.to_path_buf(), content);

        Ok(())
    }
}
