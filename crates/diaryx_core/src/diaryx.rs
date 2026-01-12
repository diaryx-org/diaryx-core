//! Unified Diaryx API (async-first).
//!
//! This module provides the main entry point for all Diaryx operations.
//! The `Diaryx<FS>` struct wraps an async filesystem and provides access to
//! domain-specific operations through async sub-module accessors.
//!
//! # Example
//!
//! ```ignore
//! use diaryx_core::diaryx::Diaryx;
//! use diaryx_core::fs::RealFileSystem;
//! use diaryx_core::fs::SyncToAsyncFs;
//!
//! let fs = SyncToAsyncFs::new(RealFileSystem);
//! let diaryx = Diaryx::new(fs);
//!
//! // Access entry operations
//! let content = diaryx.entry().get_content("path/to/file.md").await?;
//!
//! // Access workspace operations
//! let tree = diaryx.workspace().inner().get_tree("workspace/").await?;
//! ```

use std::path::{Path, PathBuf};
#[cfg(feature = "crdt")]
use std::sync::Arc;

use indexmap::IndexMap;
use serde_yaml::Value;

use crate::error::{DiaryxError, Result};
use crate::frontmatter;
use crate::fs::AsyncFileSystem;

#[cfg(feature = "crdt")]
use crate::crdt::{CrdtStorage, WorkspaceCrdt};

// ============================================================================
// Value Conversion Helpers
// ============================================================================

/// Convert a serde_yaml::Value to serde_json::Value
pub(crate) fn yaml_to_json(yaml: Value) -> serde_json::Value {
    match yaml {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                serde_json::Value::Number(u.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::Number::from_f64(f)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            } else {
                serde_json::Value::Null
            }
        }
        Value::String(s) => serde_json::Value::String(s),
        Value::Sequence(arr) => {
            serde_json::Value::Array(arr.into_iter().map(yaml_to_json).collect())
        }
        Value::Mapping(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map
                .into_iter()
                .filter_map(|(k, v)| k.as_str().map(|s| (s.to_string(), yaml_to_json(v))))
                .collect();
            serde_json::Value::Object(obj)
        }
        Value::Tagged(tagged) => yaml_to_json(tagged.value),
    }
}

/// Convert a serde_json::Value to serde_yaml::Value
pub(crate) fn json_to_yaml(json: serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                Value::Number(u.into())
            } else if let Some(f) = n.as_f64() {
                Value::Number(serde_yaml::Number::from(f))
            } else {
                Value::Null
            }
        }
        serde_json::Value::String(s) => Value::String(s),
        serde_json::Value::Array(arr) => {
            Value::Sequence(arr.into_iter().map(json_to_yaml).collect())
        }
        serde_json::Value::Object(map) => {
            let yaml_map: serde_yaml::Mapping = map
                .into_iter()
                .map(|(k, v)| (Value::String(k), json_to_yaml(v)))
                .collect();
            Value::Mapping(yaml_map)
        }
    }
}

/// The main Diaryx instance.
///
/// This struct provides a unified API for all Diaryx operations.
/// It wraps a filesystem and provides access to domain-specific
/// operations through sub-module accessors.
pub struct Diaryx<FS: AsyncFileSystem> {
    fs: FS,
    /// CRDT workspace document (optional, requires `crdt` feature).
    #[cfg(feature = "crdt")]
    workspace_crdt: Option<WorkspaceCrdt>,
}

impl<FS: AsyncFileSystem> Diaryx<FS> {
    /// Create a new Diaryx instance with the given filesystem.
    pub fn new(fs: FS) -> Self {
        Self {
            fs,
            #[cfg(feature = "crdt")]
            workspace_crdt: None,
        }
    }

    /// Create a new Diaryx instance with CRDT support.
    ///
    /// The CRDT layer enables real-time sync and version history.
    #[cfg(feature = "crdt")]
    pub fn with_crdt(fs: FS, storage: Arc<dyn CrdtStorage>) -> Self {
        Self {
            fs,
            workspace_crdt: Some(WorkspaceCrdt::new(storage)),
        }
    }

    /// Create a new Diaryx instance with CRDT support, loading existing state.
    ///
    /// This attempts to load the workspace CRDT state from storage.
    /// If no existing state is found, creates a new empty workspace CRDT.
    #[cfg(feature = "crdt")]
    pub fn with_crdt_load(fs: FS, storage: Arc<dyn CrdtStorage>) -> Result<Self> {
        let workspace_crdt = WorkspaceCrdt::load(storage)?;
        Ok(Self {
            fs,
            workspace_crdt: Some(workspace_crdt),
        })
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

    /// Get CRDT operations accessor.
    ///
    /// This provides methods for CRDT sync, history, and file metadata.
    /// Returns `None` if CRDT support is not enabled.
    #[cfg(feature = "crdt")]
    pub fn crdt(&self) -> Option<CrdtOps<'_, FS>> {
        self.workspace_crdt.as_ref().map(|crdt| CrdtOps {
            _diaryx: self,
            crdt,
        })
    }

    /// Check if CRDT support is enabled for this instance.
    #[cfg(feature = "crdt")]
    pub fn has_crdt(&self) -> bool {
        self.workspace_crdt.is_some()
    }
}

impl<FS: AsyncFileSystem + Clone> Diaryx<FS> {
    /// Get search operations accessor.
    ///
    /// Provides methods for searching workspace files by content or frontmatter.
    pub fn search(&self) -> SearchOps<'_, FS> {
        SearchOps { diaryx: self }
    }

    /// Get export operations accessor.
    ///
    /// Provides methods for exporting workspace files filtered by audience.
    pub fn export(&self) -> ExportOps<'_, FS> {
        ExportOps { diaryx: self }
    }

    /// Get validation operations accessor.
    ///
    /// Provides methods for validating workspace link integrity.
    pub fn validate(&self) -> ValidateOps<'_, FS> {
        ValidateOps { diaryx: self }
    }

    // execute() is implemented in command_handler.rs
}

// ============================================================================
// Entry Operations
// ============================================================================

/// Entry operations accessor.
///
/// Provides methods for reading/writing file content and frontmatter.
pub struct EntryOps<'a, FS: AsyncFileSystem> {
    diaryx: &'a Diaryx<FS>,
}

impl<'a, FS: AsyncFileSystem> EntryOps<'a, FS> {
    // -------------------- Frontmatter Methods --------------------

    /// Get all frontmatter properties for a file.
    ///
    /// Returns an empty map if no frontmatter exists.
    pub async fn get_frontmatter(&self, path: &str) -> Result<IndexMap<String, Value>> {
        let content = self.read_raw(path).await?;
        match frontmatter::parse(&content) {
            Ok(parsed) => Ok(parsed.frontmatter),
            Err(DiaryxError::NoFrontmatter(_)) => Ok(IndexMap::new()),
            Err(e) => Err(e),
        }
    }

    /// Get a specific frontmatter property.
    ///
    /// Returns `Ok(None)` if the property doesn't exist or no frontmatter.
    pub async fn get_frontmatter_property(&self, path: &str, key: &str) -> Result<Option<Value>> {
        let frontmatter = self.get_frontmatter(path).await?;
        Ok(frontmatter.get(key).cloned())
    }

    /// Set a frontmatter property.
    ///
    /// Creates frontmatter if none exists.
    pub async fn set_frontmatter_property(
        &self,
        path: &str,
        key: &str,
        value: Value,
    ) -> Result<()> {
        let content = self.read_raw_or_empty(path).await?;
        let mut parsed = frontmatter::parse_or_empty(&content)?;
        frontmatter::set_property(&mut parsed.frontmatter, key, value);
        self.write_parsed(path, &parsed).await
    }

    /// Remove a frontmatter property.
    pub async fn remove_frontmatter_property(&self, path: &str, key: &str) -> Result<()> {
        let content = match self.read_raw(path).await {
            Ok(c) => c,
            Err(_) => return Ok(()), // File doesn't exist, nothing to remove
        };

        let mut parsed = match frontmatter::parse(&content) {
            Ok(p) => p,
            Err(DiaryxError::NoFrontmatter(_)) => return Ok(()),
            Err(e) => return Err(e),
        };

        frontmatter::remove_property(&mut parsed.frontmatter, key);
        self.write_parsed(path, &parsed).await
    }

    // -------------------- Content Methods --------------------

    /// Get the body content of a file, excluding frontmatter.
    pub async fn get_content(&self, path: &str) -> Result<String> {
        let content = self.read_raw_or_empty(path).await?;
        let parsed = frontmatter::parse_or_empty(&content)?;
        Ok(parsed.body)
    }

    /// Set the body content of a file, preserving frontmatter.
    ///
    /// Creates frontmatter if none exists.
    pub async fn set_content(&self, path: &str, body: &str) -> Result<()> {
        let content = self.read_raw_or_empty(path).await?;
        let mut parsed = frontmatter::parse_or_empty(&content)?;
        parsed.body = body.to_string();
        self.write_parsed(path, &parsed).await
    }

    /// Save content and update the 'updated' timestamp.
    ///
    /// This is a convenience method for the common save operation.
    pub async fn save_content(&self, path: &str, body: &str) -> Result<()> {
        self.set_content(path, body).await?;
        self.touch_updated(path).await
    }

    /// Update the 'updated' timestamp to the current time.
    pub async fn touch_updated(&self, path: &str) -> Result<()> {
        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.set_frontmatter_property(path, "updated", Value::String(timestamp))
            .await
    }

    /// Append content to the end of a file's body.
    pub async fn append_content(&self, path: &str, content: &str) -> Result<()> {
        let raw = self.read_raw_or_empty(path).await?;
        let mut parsed = frontmatter::parse_or_empty(&raw)?;

        parsed.body = if parsed.body.is_empty() {
            content.to_string()
        } else if parsed.body.ends_with('\n') {
            format!("{}{}", parsed.body, content)
        } else {
            format!("{}\n{}", parsed.body, content)
        };

        self.write_parsed(path, &parsed).await
    }

    // -------------------- Raw I/O Methods --------------------

    /// Read the raw file content (including frontmatter).
    pub async fn read_raw(&self, path: &str) -> Result<String> {
        let path_buf = PathBuf::from(path);
        self.diaryx
            .fs
            .read_to_string(Path::new(path))
            .await
            .map_err(|e| DiaryxError::FileRead {
                path: path_buf,
                source: e,
            })
    }

    /// Read the raw file content, returning empty string if file doesn't exist.
    async fn read_raw_or_empty(&self, path: &str) -> Result<String> {
        match self.diaryx.fs.read_to_string(Path::new(path)).await {
            Ok(content) => Ok(content),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(DiaryxError::FileRead {
                path: PathBuf::from(path),
                source: e,
            }),
        }
    }

    /// Write a parsed file back to disk.
    async fn write_parsed(&self, path: &str, parsed: &frontmatter::ParsedFile) -> Result<()> {
        let content = frontmatter::serialize(&parsed.frontmatter, &parsed.body)?;
        self.diaryx
            .fs
            .write_file(Path::new(path), &content)
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: PathBuf::from(path),
                source: e,
            })
    }

    // -------------------- Attachment Methods --------------------

    /// Get the list of attachments for a file.
    pub async fn get_attachments(&self, path: &str) -> Result<Vec<String>> {
        let frontmatter = self.get_frontmatter(path).await?;
        Ok(frontmatter::get_string_array(&frontmatter, "attachments"))
    }

    /// Add an attachment to a file's attachments list.
    pub async fn add_attachment(&self, path: &str, attachment_path: &str) -> Result<()> {
        let content = self.read_raw_or_empty(path).await?;
        let mut parsed = frontmatter::parse_or_empty(&content)?;

        let attachments = parsed
            .frontmatter
            .entry("attachments".to_string())
            .or_insert(Value::Sequence(vec![]));

        if let Value::Sequence(list) = attachments {
            let new_attachment = Value::String(attachment_path.to_string());
            if !list.contains(&new_attachment) {
                list.push(new_attachment);
            }
        }

        self.write_parsed(path, &parsed).await
    }

    /// Remove an attachment from a file's attachments list.
    pub async fn remove_attachment(&self, path: &str, attachment_path: &str) -> Result<()> {
        let content = match self.read_raw(path).await {
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

        self.write_parsed(path, &parsed).await
    }

    // -------------------- Frontmatter Sorting --------------------

    /// Sort frontmatter keys according to a pattern.
    ///
    /// Pattern is comma-separated keys, with "*" meaning "rest alphabetically".
    /// Example: "title,description,*" puts title first, description second, rest alphabetically
    pub async fn sort_frontmatter(&self, path: &str, pattern: Option<&str>) -> Result<()> {
        let content = match self.read_raw(path).await {
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

        self.write_parsed(path, &sorted_parsed).await
    }
}

// ============================================================================
// Workspace Operations (placeholder - delegates to existing Workspace)
// ============================================================================

/// Workspace operations accessor.
///
/// This provides methods for traversing the workspace tree,
/// managing files, and working with the index hierarchy.
pub struct WorkspaceOps<'a, FS: AsyncFileSystem> {
    diaryx: &'a Diaryx<FS>,
}

impl<'a, FS: AsyncFileSystem> WorkspaceOps<'a, FS> {
    /// Get access to the underlying Workspace struct for full functionality.
    pub fn inner(&self) -> crate::workspace::Workspace<&'a FS> {
        crate::workspace::Workspace::new(&self.diaryx.fs)
    }
}

// ============================================================================
// Search Operations (placeholder - delegates to existing Searcher)
// ============================================================================

/// Search operations accessor.
///
/// Provides methods for searching workspace files by content or frontmatter.
pub struct SearchOps<'a, FS: AsyncFileSystem> {
    diaryx: &'a Diaryx<FS>,
}

impl<'a, FS: AsyncFileSystem + Clone> SearchOps<'a, FS> {
    /// Get access to the underlying Searcher struct for full functionality.
    pub fn inner(&self) -> crate::search::Searcher<FS> {
        crate::search::Searcher::new(self.diaryx.fs.clone())
    }

    /// Search the entire workspace for a pattern.
    pub async fn search_workspace(
        &self,
        workspace_root: &std::path::Path,
        query: &crate::search::SearchQuery,
    ) -> crate::error::Result<crate::search::SearchResults> {
        self.inner().search_workspace(workspace_root, query).await
    }

    /// Search a single file for a pattern.
    pub async fn search_file(
        &self,
        path: &std::path::Path,
        query: &crate::search::SearchQuery,
    ) -> crate::error::Result<Option<crate::search::FileSearchResult>> {
        self.inner().search_file(path, query).await
    }
}

// ============================================================================
// Export Operations (placeholder - delegates to existing Exporter)
// ============================================================================

/// Export operations accessor.
///
/// Provides methods for exporting workspace files filtered by audience.
pub struct ExportOps<'a, FS: AsyncFileSystem> {
    diaryx: &'a Diaryx<FS>,
}

impl<'a, FS: AsyncFileSystem + Clone> ExportOps<'a, FS> {
    /// Get access to the underlying Exporter struct for full functionality.
    pub fn inner(&self) -> crate::export::Exporter<FS> {
        crate::export::Exporter::new(self.diaryx.fs.clone())
    }

    /// Plan an export operation without executing it.
    pub async fn plan_export(
        &self,
        workspace_root: &std::path::Path,
        audience: &str,
        destination: &std::path::Path,
    ) -> crate::error::Result<crate::export::ExportPlan> {
        self.inner()
            .plan_export(workspace_root, audience, destination)
            .await
    }

    /// Execute an export plan.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn execute_export(
        &self,
        plan: &crate::export::ExportPlan,
        options: &crate::export::ExportOptions,
    ) -> crate::error::Result<crate::export::ExportStats> {
        self.inner().execute_export(plan, options).await
    }
}

// ============================================================================
// Validate Operations (placeholder - delegates to existing Validator)
// ============================================================================

/// Validation operations accessor.
///
/// Provides methods for validating workspace link integrity.
pub struct ValidateOps<'a, FS: AsyncFileSystem> {
    diaryx: &'a Diaryx<FS>,
}

impl<'a, FS: AsyncFileSystem + Clone> ValidateOps<'a, FS> {
    /// Get access to the underlying Validator struct for full functionality.
    pub fn inner(&self) -> crate::validate::Validator<FS> {
        crate::validate::Validator::new(self.diaryx.fs.clone())
    }

    /// Validate all links starting from a workspace root index.
    pub async fn validate_workspace(
        &self,
        root_path: &std::path::Path,
    ) -> crate::error::Result<crate::validate::ValidationResult> {
        self.inner().validate_workspace(root_path).await
    }

    /// Validate a single file's links.
    pub async fn validate_file(
        &self,
        file_path: &std::path::Path,
    ) -> crate::error::Result<crate::validate::ValidationResult> {
        self.inner().validate_file(file_path).await
    }

    /// Get a fixer for validation issues.
    pub fn fixer(&self) -> crate::validate::ValidationFixer<FS> {
        crate::validate::ValidationFixer::new(self.diaryx.fs.clone())
    }
}

// ============================================================================
// CRDT Operations
// ============================================================================

/// CRDT operations accessor.
///
/// Provides methods for CRDT sync, history, and file metadata.
#[cfg(feature = "crdt")]
pub struct CrdtOps<'a, FS: AsyncFileSystem> {
    _diaryx: &'a Diaryx<FS>,
    crdt: &'a WorkspaceCrdt,
}

#[cfg(feature = "crdt")]
impl<'a, FS: AsyncFileSystem> CrdtOps<'a, FS> {
    /// Get the state vector for sync.
    pub fn get_state_vector(&self) -> Vec<u8> {
        self.crdt.encode_state_vector()
    }

    /// Get the full state as an update (for initial sync).
    pub fn get_full_state(&self) -> Vec<u8> {
        self.crdt.encode_state_as_update()
    }

    /// Get updates needed by a remote peer (based on their state vector).
    pub fn get_missing_updates(&self, remote_state_vector: &[u8]) -> Result<Vec<u8>> {
        self.crdt.encode_diff(remote_state_vector)
    }

    /// Apply an update from a remote peer.
    pub fn apply_update(
        &self,
        update: &[u8],
        origin: crate::crdt::UpdateOrigin,
    ) -> Result<Option<i64>> {
        self.crdt.apply_update(update, origin)
    }

    /// Get file metadata from CRDT.
    pub fn get_file(&self, path: &str) -> Option<crate::crdt::FileMetadata> {
        self.crdt.get_file(path)
    }

    /// Set file metadata in CRDT.
    pub fn set_file(&self, path: &str, metadata: crate::crdt::FileMetadata) {
        self.crdt.set_file(path, metadata);
    }

    /// Delete a file (marks as deleted in CRDT).
    pub fn delete_file(&self, path: &str) {
        self.crdt.delete_file(path);
    }

    /// List all files (including deleted).
    pub fn list_files(&self) -> Vec<(String, crate::crdt::FileMetadata)> {
        self.crdt.list_files()
    }

    /// List active (non-deleted) files only.
    pub fn list_active_files(&self) -> Vec<(String, crate::crdt::FileMetadata)> {
        self.crdt.list_active_files()
    }

    /// Get update history.
    pub fn get_history(&self) -> Result<Vec<crate::crdt::CrdtUpdate>> {
        self.crdt.get_history()
    }

    /// Get updates since a given ID.
    pub fn get_updates_since(&self, since_id: i64) -> Result<Vec<crate::crdt::CrdtUpdate>> {
        self.crdt.get_updates_since(since_id)
    }

    /// Save CRDT state to storage.
    pub fn save(&self) -> Result<()> {
        self.crdt.save()
    }

    /// Get the number of files in the CRDT.
    pub fn file_count(&self) -> usize {
        self.crdt.file_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::SyncToAsyncFs;
    use crate::test_utils::MockFileSystem;

    #[test]
    fn test_entry_get_set_content() {
        let fs =
            MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\n\nOriginal content");

        let diaryx = Diaryx::new(SyncToAsyncFs::new(fs));

        // Get content
        let content = crate::fs::block_on_test(diaryx.entry().get_content("test.md")).unwrap();
        assert_eq!(content.trim(), "Original content");

        // Set content
        crate::fs::block_on_test(diaryx.entry().set_content("test.md", "\nNew content")).unwrap();

        let content = crate::fs::block_on_test(diaryx.entry().get_content("test.md")).unwrap();
        assert_eq!(content.trim(), "New content");
    }

    #[test]
    fn test_entry_get_frontmatter() {
        let fs = MockFileSystem::new()
            .with_file("test.md", "---\ntitle: My Title\nauthor: John\n---\n\nBody");

        let diaryx = Diaryx::new(SyncToAsyncFs::new(fs));

        let fm = crate::fs::block_on_test(diaryx.entry().get_frontmatter("test.md")).unwrap();
        assert_eq!(fm.get("title").unwrap().as_str().unwrap(), "My Title");
        assert_eq!(fm.get("author").unwrap().as_str().unwrap(), "John");
    }

    #[test]
    fn test_entry_set_frontmatter_property() {
        let fs = MockFileSystem::new().with_file("test.md", "---\ntitle: Original\n---\n\nBody");

        let diaryx = Diaryx::new(SyncToAsyncFs::new(fs));

        crate::fs::block_on_test(diaryx.entry().set_frontmatter_property(
            "test.md",
            "title",
            Value::String("Updated".to_string()),
        ))
        .unwrap();

        let fm = crate::fs::block_on_test(diaryx.entry().get_frontmatter("test.md")).unwrap();
        assert_eq!(fm.get("title").unwrap().as_str().unwrap(), "Updated");
    }

    #[cfg(feature = "crdt")]
    mod crdt_tests {
        use super::*;
        use crate::crdt::MemoryStorage;

        #[test]
        fn test_diaryx_with_crdt() {
            let fs = MockFileSystem::new();
            let storage = Arc::new(MemoryStorage::new());
            let diaryx = Diaryx::with_crdt(SyncToAsyncFs::new(fs), storage);

            // Verify CRDT is available
            assert!(diaryx.has_crdt());
            let crdt = diaryx.crdt().unwrap();

            // Test file operations
            let metadata = crate::crdt::FileMetadata::new(Some("Test File".to_string()));
            crdt.set_file("test.md", metadata);

            let retrieved = crdt.get_file("test.md");
            assert!(retrieved.is_some());
            assert_eq!(retrieved.unwrap().title, Some("Test File".to_string()));

            // Test file count
            assert_eq!(crdt.file_count(), 1);
        }

        #[test]
        fn test_diaryx_without_crdt() {
            let fs = MockFileSystem::new();
            let diaryx = Diaryx::new(SyncToAsyncFs::new(fs));

            // CRDT should not be available
            assert!(!diaryx.has_crdt());
            assert!(diaryx.crdt().is_none());
        }

        #[test]
        fn test_diaryx_crdt_sync() {
            let fs1 = MockFileSystem::new();
            let fs2 = MockFileSystem::new();
            let storage1 = Arc::new(MemoryStorage::new());
            let storage2 = Arc::new(MemoryStorage::new());

            let diaryx1 = Diaryx::with_crdt(SyncToAsyncFs::new(fs1), storage1);
            let diaryx2 = Diaryx::with_crdt(SyncToAsyncFs::new(fs2), storage2);

            let crdt1 = diaryx1.crdt().unwrap();
            let crdt2 = diaryx2.crdt().unwrap();

            // Add file on first instance
            let metadata = crate::crdt::FileMetadata::new(Some("Shared File".to_string()));
            crdt1.set_file("shared.md", metadata);

            // Get state and apply to second instance
            let state = crdt1.get_full_state();
            crdt2
                .apply_update(&state, crate::crdt::UpdateOrigin::Remote)
                .unwrap();

            // Verify sync worked
            let retrieved = crdt2.get_file("shared.md");
            assert!(retrieved.is_some());
            assert_eq!(retrieved.unwrap().title, Some("Shared File".to_string()));
        }
    }
}
