//! Native filesystem implementation.
//!
//! Only available on non-WASM targets.

#[cfg(not(target_arch = "wasm32"))]
use std::fs::{self, OpenOptions};
#[cfg(not(target_arch = "wasm32"))]
use std::io::{Error, ErrorKind, Result, Write};
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

#[cfg(not(target_arch = "wasm32"))]
use super::FileSystem;

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

    fn is_symlink(&self, path: &Path) -> bool {
        path.is_symlink()
    }

    fn list_files(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        if dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                files.push(entry.path());
            }
        }
        Ok(files)
    }

    fn read_binary(&self, path: &Path) -> Result<Vec<u8>> {
        fs::read(path)
    }

    fn write_binary(&self, path: &Path, content: &[u8]) -> Result<()> {
        fs::write(path, content)
    }

    fn get_modified_time(&self, path: &Path) -> Option<i64> {
        fs::metadata(path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_millis() as i64)
            })
    }
}
