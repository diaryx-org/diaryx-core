use std::io::Result;
use std::path::{Path, PathBuf};

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

    /// (Recommended Addition) Finds markdown files in a folder
    fn list_md_files(&self, dir: &Path) -> Result<Vec<PathBuf>>;

    /// Checks if a file exists
    fn exists(&self, path: &Path) -> bool;
}

use std::fs::{self, OpenOptions};
use std::io::Write;

#[derive(Clone, Copy)]
pub struct RealFileSystem;

impl FileSystem for RealFileSystem {
    fn read_to_string(&self, path: &Path) -> Result<String> {
        std::fs::read_to_string(path)
    }

    fn write_file(&self, path: &Path, content: &str) -> Result<()> {
        std::fs::write(path, content)
    }

    fn delete_file(&self, path: &Path) -> Result<()> {
        std::fs::remove_file(path)
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
}
