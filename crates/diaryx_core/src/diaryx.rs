//! Unified Diaryx API.
//!
//! This module provides the main entry point for all Diaryx operations.
//! The `Diaryx<FS>` struct wraps a filesystem and provides access to
//! domain-specific operations through sub-module accessors.
//!
//! # Example
//!
//! ```ignore
//! use diaryx_core::diaryx::Diaryx;
//! use diaryx_core::fs::RealFileSystem;
//!
//! let fs = RealFileSystem;
//! let diaryx = Diaryx::new(fs);
//!
//! // Access entry operations
//! let content = diaryx.entry().get_content("path/to/file.md")?;
//!
//! // Access workspace operations
//! let tree = diaryx.workspace().get_tree("workspace/")?;
//! ```

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde_yaml::Value;

use crate::error::{DiaryxError, Result};
use crate::fs::FileSystem;
use crate::frontmatter;

/// The main Diaryx instance.
///
/// This struct provides a unified API for all Diaryx operations.
/// It wraps a filesystem and provides access to domain-specific
/// operations through sub-module accessors.
pub struct Diaryx<FS: FileSystem> {
    fs: FS,
}

impl<FS: FileSystem> Diaryx<FS> {
    /// Create a new Diaryx instance with the given filesystem.
    pub fn new(fs: FS) -> Self {
        Self { fs }
    }

    /// Get a reference to the underlying filesystem.
    pub fn fs(&self) -> &FS {
        &self.fs
    }

    /// Get entry operations accessor.
    ///
    /// This provides methods for reading/writing file content and frontmatter.
    pub fn entry(&self) -> EntryOps<'_, FS> {
        EntryOps { diaryx: self }
    }

    /// Get workspace operations accessor.
    ///
    /// This provides methods for traversing the workspace tree,
    /// managing files, and working with the index hierarchy.
    pub fn workspace(&self) -> WorkspaceOps<'_, FS> {
        WorkspaceOps { diaryx: self }
    }
}

impl<FS: FileSystem + Clone> Diaryx<FS> {
    /// Get search operations accessor.
    ///
    /// This provides methods for searching workspace content and frontmatter.
    pub fn search(&self) -> SearchOps<'_, FS> {
        SearchOps { diaryx: self }
    }

    /// Get export operations accessor.
    ///
    /// This provides methods for exporting workspaces with audience filtering.
    pub fn export(&self) -> ExportOps<'_, FS> {
        ExportOps { diaryx: self }
    }

    /// Get validation operations accessor.
    ///
    /// This provides methods for validating workspace link integrity.
    pub fn validate(&self) -> ValidateOps<'_, FS> {
        ValidateOps { diaryx: self }
    }
}

// ============================================================================
// Entry Operations
// ============================================================================

/// Entry operations accessor.
///
/// Provides methods for reading/writing file content and frontmatter.
pub struct EntryOps<'a, FS: FileSystem> {
    diaryx: &'a Diaryx<FS>,
}

impl<'a, FS: FileSystem> EntryOps<'a, FS> {
    // -------------------- Frontmatter Methods --------------------

    /// Get all frontmatter properties for a file.
    ///
    /// Returns an empty map if no frontmatter exists.
    pub fn get_frontmatter(&self, path: &str) -> Result<IndexMap<String, Value>> {
        let content = self.read_raw(path)?;
        match frontmatter::parse(&content) {
            Ok(parsed) => Ok(parsed.frontmatter),
            Err(DiaryxError::NoFrontmatter(_)) => Ok(IndexMap::new()),
            Err(e) => Err(e),
        }
    }

    /// Get a specific frontmatter property.
    ///
    /// Returns `Ok(None)` if the property doesn't exist or no frontmatter.
    pub fn get_frontmatter_property(&self, path: &str, key: &str) -> Result<Option<Value>> {
        let frontmatter = self.get_frontmatter(path)?;
        Ok(frontmatter.get(key).cloned())
    }

    /// Set a frontmatter property.
    ///
    /// Creates frontmatter if none exists.
    pub fn set_frontmatter_property(&self, path: &str, key: &str, value: Value) -> Result<()> {
        let content = self.read_raw_or_empty(path)?;
        let mut parsed = frontmatter::parse_or_empty(&content)?;
        frontmatter::set_property(&mut parsed.frontmatter, key, value);
        self.write_parsed(path, &parsed)
    }

    /// Remove a frontmatter property.
    pub fn remove_frontmatter_property(&self, path: &str, key: &str) -> Result<()> {
        let content = match self.read_raw(path) {
            Ok(c) => c,
            Err(_) => return Ok(()), // File doesn't exist, nothing to remove
        };
        
        let mut parsed = match frontmatter::parse(&content) {
            Ok(p) => p,
            Err(DiaryxError::NoFrontmatter(_)) => return Ok(()),
            Err(e) => return Err(e),
        };

        frontmatter::remove_property(&mut parsed.frontmatter, key);
        self.write_parsed(path, &parsed)
    }

    // -------------------- Content Methods --------------------

    /// Get the body content of a file, excluding frontmatter.
    pub fn get_content(&self, path: &str) -> Result<String> {
        let content = self.read_raw_or_empty(path)?;
        let parsed = frontmatter::parse_or_empty(&content)?;
        Ok(parsed.body)
    }

    /// Set the body content of a file, preserving frontmatter.
    ///
    /// Creates frontmatter if none exists.
    pub fn set_content(&self, path: &str, body: &str) -> Result<()> {
        let content = self.read_raw_or_empty(path)?;
        let mut parsed = frontmatter::parse_or_empty(&content)?;
        parsed.body = body.to_string();
        self.write_parsed(path, &parsed)
    }

    /// Save content and update the 'updated' timestamp.
    ///
    /// This is a convenience method for the common save operation.
    pub fn save_content(&self, path: &str, body: &str) -> Result<()> {
        self.set_content(path, body)?;
        self.touch_updated(path)
    }

    /// Update the 'updated' timestamp to the current time.
    pub fn touch_updated(&self, path: &str) -> Result<()> {
        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.set_frontmatter_property(path, "updated", Value::String(timestamp))
    }

    /// Append content to the end of a file's body.
    pub fn append_content(&self, path: &str, content: &str) -> Result<()> {
        let raw = self.read_raw_or_empty(path)?;
        let mut parsed = frontmatter::parse_or_empty(&raw)?;
        
        parsed.body = if parsed.body.is_empty() {
            content.to_string()
        } else if parsed.body.ends_with('\n') {
            format!("{}{}", parsed.body, content)
        } else {
            format!("{}\n{}", parsed.body, content)
        };
        
        self.write_parsed(path, &parsed)
    }

    // -------------------- Raw I/O Methods --------------------

    /// Read the raw file content (including frontmatter).
    pub fn read_raw(&self, path: &str) -> Result<String> {
        let path_buf = PathBuf::from(path);
        self.diaryx.fs.read_to_string(Path::new(path)).map_err(|e| DiaryxError::FileRead {
            path: path_buf,
            source: e,
        })
    }

    /// Read the raw file content, returning empty string if file doesn't exist.
    fn read_raw_or_empty(&self, path: &str) -> Result<String> {
        match self.diaryx.fs.read_to_string(Path::new(path)) {
            Ok(content) => Ok(content),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(DiaryxError::FileRead {
                path: PathBuf::from(path),
                source: e,
            }),
        }
    }

    /// Write a parsed file back to disk.
    fn write_parsed(&self, path: &str, parsed: &frontmatter::ParsedFile) -> Result<()> {
        let content = frontmatter::serialize(&parsed.frontmatter, &parsed.body)?;
        self.diaryx.fs.write_file(Path::new(path), &content).map_err(|e| DiaryxError::FileWrite {
            path: PathBuf::from(path),
            source: e,
        })
    }

    // -------------------- Attachment Methods --------------------

    /// Get the list of attachments for a file.
    pub fn get_attachments(&self, path: &str) -> Result<Vec<String>> {
        let frontmatter = self.get_frontmatter(path)?;
        Ok(frontmatter::get_string_array(&frontmatter, "attachments"))
    }

    /// Add an attachment to a file's attachments list.
    pub fn add_attachment(&self, path: &str, attachment_path: &str) -> Result<()> {
        let content = self.read_raw_or_empty(path)?;
        let mut parsed = frontmatter::parse_or_empty(&content)?;

        let attachments = parsed.frontmatter
            .entry("attachments".to_string())
            .or_insert(Value::Sequence(vec![]));

        if let Value::Sequence(list) = attachments {
            let new_attachment = Value::String(attachment_path.to_string());
            if !list.contains(&new_attachment) {
                list.push(new_attachment);
            }
        }

        self.write_parsed(path, &parsed)
    }

    /// Remove an attachment from a file's attachments list.
    pub fn remove_attachment(&self, path: &str, attachment_path: &str) -> Result<()> {
        let content = match self.read_raw(path) {
            Ok(c) => c,
            Err(_) => return Ok(()),
        };

        let mut parsed = match frontmatter::parse(&content) {
            Ok(p) => p,
            Err(DiaryxError::NoFrontmatter(_)) => return Ok(()),
            Err(e) => return Err(e),
        };

        if let Some(Value::Sequence(list)) = parsed.frontmatter.get_mut("attachments") {
            list.retain(|item| {
                if let Value::String(s) = item {
                    s != attachment_path
                } else {
                    true
                }
            });

            if list.is_empty() {
                parsed.frontmatter.shift_remove("attachments");
            }
        }

        self.write_parsed(path, &parsed)
    }

    // -------------------- Frontmatter Sorting --------------------

    /// Sort frontmatter keys according to a pattern.
    ///
    /// Pattern is comma-separated keys, with "*" meaning "rest alphabetically".
    /// Example: "title,description,*" puts title first, description second, rest alphabetically
    pub fn sort_frontmatter(&self, path: &str, pattern: Option<&str>) -> Result<()> {
        let content = match self.read_raw(path) {
            Ok(c) => c,
            Err(_) => return Ok(()),
        };

        let parsed = match frontmatter::parse(&content) {
            Ok(p) => p,
            Err(DiaryxError::NoFrontmatter(_)) => return Ok(()),
            Err(e) => return Err(e),
        };

        let sorted_frontmatter = match pattern {
            Some(p) => frontmatter::sort_by_pattern(parsed.frontmatter, p),
            None => frontmatter::sort_alphabetically(parsed.frontmatter),
        };

        let sorted_parsed = frontmatter::ParsedFile {
            frontmatter: sorted_frontmatter,
            body: parsed.body,
        };

        self.write_parsed(path, &sorted_parsed)
    }
}

// ============================================================================
// Workspace Operations (placeholder - delegates to existing Workspace)
// ============================================================================

/// Workspace operations accessor.
pub struct WorkspaceOps<'a, FS: FileSystem> {
    diaryx: &'a Diaryx<FS>,
}

impl<'a, FS: FileSystem> WorkspaceOps<'a, FS> {
    /// Get access to the underlying Workspace struct for full functionality.
    ///
    /// This is a bridge to the existing Workspace API during the refactoring.
    pub fn inner(&self) -> crate::workspace::Workspace<&'a FS> {
        crate::workspace::Workspace::new(&self.diaryx.fs)
    }
}

// ============================================================================
// Search Operations (placeholder - delegates to existing Searcher)
// ============================================================================

/// Search operations accessor.
pub struct SearchOps<'a, FS: FileSystem + Clone> {
    diaryx: &'a Diaryx<FS>,
}

impl<'a, FS: FileSystem + Clone> SearchOps<'a, FS> {
    /// Get access to the underlying Searcher struct for full functionality.
    pub fn inner(&self) -> crate::search::Searcher<FS> {
        crate::search::Searcher::new(self.diaryx.fs.clone())
    }
}

// ============================================================================
// Export Operations (placeholder - delegates to existing Exporter)
// ============================================================================

/// Export operations accessor.
pub struct ExportOps<'a, FS: FileSystem + Clone> {
    diaryx: &'a Diaryx<FS>,
}

impl<'a, FS: FileSystem + Clone> ExportOps<'a, FS> {
    /// Get access to the underlying Exporter struct for full functionality.
    pub fn inner(&self) -> crate::export::Exporter<FS> {
        crate::export::Exporter::new(self.diaryx.fs.clone())
    }
}

// ============================================================================
// Validate Operations (placeholder - delegates to existing Validator)
// ============================================================================

/// Validate operations accessor.
pub struct ValidateOps<'a, FS: FileSystem + Clone> {
    diaryx: &'a Diaryx<FS>,
}

impl<'a, FS: FileSystem + Clone> ValidateOps<'a, FS> {
    /// Get access to the underlying Validator struct for full functionality.
    pub fn inner(&self) -> crate::validate::Validator<FS> {
        crate::validate::Validator::new(self.diaryx.fs.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::MockFileSystem;

    #[test]
    fn test_entry_get_set_content() {
        let fs = MockFileSystem::new()
            .with_file("test.md", "---\ntitle: Test\n---\n\nOriginal content");
        
        let diaryx = Diaryx::new(fs);
        
        // Get content
        let content = diaryx.entry().get_content("test.md").unwrap();
        assert_eq!(content.trim(), "Original content");
        
        // Set content
        diaryx.entry().set_content("test.md", "\nNew content").unwrap();
        
        let content = diaryx.entry().get_content("test.md").unwrap();
        assert_eq!(content.trim(), "New content");
    }

    #[test]
    fn test_entry_get_frontmatter() {
        let fs = MockFileSystem::new()
            .with_file("test.md", "---\ntitle: My Title\nauthor: John\n---\n\nBody");
        
        let diaryx = Diaryx::new(fs);
        
        let fm = diaryx.entry().get_frontmatter("test.md").unwrap();
        assert_eq!(fm.get("title").unwrap().as_str().unwrap(), "My Title");
        assert_eq!(fm.get("author").unwrap().as_str().unwrap(), "John");
    }

    #[test]
    fn test_entry_set_frontmatter_property() {
        let fs = MockFileSystem::new()
            .with_file("test.md", "---\ntitle: Original\n---\n\nBody");
        
        let diaryx = Diaryx::new(fs);
        
        diaryx.entry().set_frontmatter_property("test.md", "title", Value::String("Updated".to_string())).unwrap();
        
        let fm = diaryx.entry().get_frontmatter("test.md").unwrap();
        assert_eq!(fm.get("title").unwrap().as_str().unwrap(), "Updated");
    }
}
