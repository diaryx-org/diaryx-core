//! Command pattern API for unified command execution.
//!
//! This module provides a unified command pattern interface that eliminates
//! redundancy across different runtime environments (WASM, Tauri, CLI).
//!
//! # Usage
//!
//! ```ignore
//! use diaryx_core::{Command, Response, Diaryx};
//!
//! let cmd = Command::GetEntry { path: "notes/hello.md".to_string() };
//! let response = diaryx.execute(cmd).await?;
//!
//! if let Response::Entry(entry) = response {
//!     println!("Title: {:?}", entry.title);
//! }
//! ```

use std::path::PathBuf;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::export::ExportPlan;
use crate::search::SearchResults;
use crate::validate::{FixResult, ValidationResult};
use crate::workspace::TreeNode;

// ============================================================================
// Command Types
// ============================================================================

/// All commands that can be executed against a Diaryx instance.
///
/// Commands are serializable for cross-runtime usage (WASM, IPC, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "params")]
pub enum Command {
    // === Entry Operations ===
    /// Get an entry's content and metadata.
    GetEntry {
        /// Path to the entry file.
        path: String,
    },

    /// Save an entry's content.
    SaveEntry {
        /// Path to the entry file.
        path: String,
        /// New markdown content.
        content: String,
    },

    /// Create a new entry.
    CreateEntry {
        /// Path where the entry should be created.
        path: String,
        /// Optional creation options.
        #[serde(default)]
        options: CreateEntryOptions,
    },

    /// Delete an entry.
    DeleteEntry {
        /// Path to the entry to delete.
        path: String,
    },

    /// Move/rename an entry.
    MoveEntry {
        /// Existing path to the entry file.
        from: String,
        /// New path for the entry file.
        to: String,
    },

    /// Rename an entry file.
    RenameEntry {
        /// Path to the entry to rename.
        path: String,
        /// New filename (e.g., "new-name.md").
        new_filename: String,
    },

    /// Convert a leaf file to an index file with a directory.
    ConvertToIndex {
        /// Path to the leaf file to convert.
        path: String,
    },

    /// Convert an empty index file back to a leaf file.
    ConvertToLeaf {
        /// Path to the index file to convert.
        path: String,
    },

    /// Create a new child entry under a parent.
    CreateChildEntry {
        /// Path to the parent entry.
        parent_path: String,
    },

    /// Attach an existing entry to a parent index.
    AttachEntryToParent {
        /// Path to the entry to attach.
        entry_path: String,
        /// Path to the parent index file.
        parent_path: String,
    },

    /// Ensure today's daily entry exists.
    EnsureDailyEntry,

    // === Workspace Operations ===
    /// Get the workspace tree structure.
    GetWorkspaceTree {
        /// Optional path to a specific workspace.
        path: Option<String>,
        /// Optional maximum depth to traverse.
        depth: Option<u32>,
    },

    /// Get the filesystem tree (for "Show All Files" mode).
    GetFilesystemTree {
        /// Optional path to the workspace directory.
        path: Option<String>,
        /// Whether to include hidden files.
        #[serde(default)]
        show_hidden: bool,
    },

    /// Create a new workspace.
    CreateWorkspace {
        /// Path where the workspace should be created.
        path: Option<String>,
        /// Name of the workspace.
        name: Option<String>,
    },

    // === Frontmatter Operations ===
    /// Get all frontmatter properties for an entry.
    GetFrontmatter {
        /// Path to the entry file.
        path: String,
    },

    /// Set a frontmatter property.
    SetFrontmatterProperty {
        /// Path to the entry file.
        path: String,
        /// Property key.
        key: String,
        /// Property value.
        value: JsonValue,
    },

    /// Remove a frontmatter property.
    RemoveFrontmatterProperty {
        /// Path to the entry file.
        path: String,
        /// Property key to remove.
        key: String,
    },

    // === Search ===
    /// Search the workspace for entries.
    SearchWorkspace {
        /// Search pattern.
        pattern: String,
        /// Search options.
        #[serde(default)]
        options: SearchOptions,
    },

    // === Validation ===
    /// Validate workspace links.
    ValidateWorkspace {
        /// Optional path to workspace.
        path: Option<String>,
    },

    /// Validate a single file's links.
    ValidateFile {
        /// Path to the file to validate.
        path: String,
    },

    /// Fix a broken part_of reference.
    FixBrokenPartOf {
        /// Path to the file with the broken reference.
        path: String,
    },

    /// Fix a broken contents reference.
    FixBrokenContentsRef {
        /// Path to the index file.
        index_path: String,
        /// The broken reference to remove.
        target: String,
    },

    /// Fix a broken attachment reference.
    FixBrokenAttachment {
        /// Path to the file with the broken attachment.
        path: String,
        /// The broken attachment reference.
        attachment: String,
    },

    /// Fix a non-portable path.
    FixNonPortablePath {
        /// Path to the file.
        path: String,
        /// Property name.
        property: String,
        /// Current value.
        old_value: String,
        /// New value.
        new_value: String,
    },

    /// Add an unlisted file to an index's contents.
    FixUnlistedFile {
        /// Path to the index file.
        index_path: String,
        /// Path to the file to add.
        file_path: String,
    },

    /// Add an orphan binary file to an index's attachments.
    FixOrphanBinaryFile {
        /// Path to the index file.
        index_path: String,
        /// Path to the binary file.
        file_path: String,
    },

    /// Fix a missing part_of reference.
    FixMissingPartOf {
        /// Path to the file missing part_of.
        file_path: String,
        /// Path to the index file to reference.
        index_path: String,
    },

    /// Fix all validation issues.
    FixAll {
        /// The validation result to fix.
        validation_result: ValidationResult,
    },

    // === Export ===
    /// Get available audiences.
    GetAvailableAudiences {
        /// Root path to scan.
        root_path: String,
    },

    /// Plan an export operation.
    PlanExport {
        /// Root path.
        root_path: String,
        /// Target audience.
        audience: String,
    },

    /// Export to memory.
    ExportToMemory {
        /// Root path.
        root_path: String,
        /// Target audience.
        audience: String,
    },

    /// Export to HTML.
    ExportToHtml {
        /// Root path.
        root_path: String,
        /// Target audience.
        audience: String,
    },

    /// Export binary attachments.
    ExportBinaryAttachments {
        /// Root path.
        root_path: String,
        /// Target audience.
        audience: String,
    },

    // === Templates ===
    /// List available templates.
    ListTemplates {
        /// Optional workspace path.
        workspace_path: Option<String>,
    },

    /// Get a template's content.
    GetTemplate {
        /// Template name.
        name: String,
        /// Optional workspace path.
        workspace_path: Option<String>,
    },

    /// Save a template.
    SaveTemplate {
        /// Template name.
        name: String,
        /// Template content.
        content: String,
        /// Workspace path.
        workspace_path: String,
    },

    /// Delete a template.
    DeleteTemplate {
        /// Template name.
        name: String,
        /// Workspace path.
        workspace_path: String,
    },

    // === Attachments ===
    /// Get attachments for an entry.
    GetAttachments {
        /// Path to the entry file.
        path: String,
    },

    /// Upload an attachment.
    UploadAttachment {
        /// Path to the entry file.
        entry_path: String,
        /// Filename for the attachment.
        filename: String,
        /// Base64 encoded data.
        data_base64: String,
    },

    /// Delete an attachment.
    DeleteAttachment {
        /// Path to the entry file.
        entry_path: String,
        /// Path to the attachment.
        attachment_path: String,
    },

    /// Get attachment data.
    GetAttachmentData {
        /// Path to the entry file.
        entry_path: String,
        /// Path to the attachment.
        attachment_path: String,
    },

    // === File System ===
    /// Check if a file exists.
    FileExists {
        /// Path to check.
        path: String,
    },

    /// Read a file's content.
    ReadFile {
        /// Path to read.
        path: String,
    },

    /// Write content to a file.
    WriteFile {
        /// Path to write.
        path: String,
        /// Content to write.
        content: String,
    },

    /// Delete a file.
    DeleteFile {
        /// Path to delete.
        path: String,
    },

    // === Storage ===
    /// Get storage usage information.
    GetStorageUsage,
}

// ============================================================================
// Response Types
// ============================================================================

/// Responses from command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum Response {
    /// Command completed successfully with no data.
    Ok,

    /// String response.
    String(String),

    /// Boolean response.
    Bool(bool),

    /// Entry data response.
    Entry(EntryData),

    /// Tree node response.
    Tree(TreeNode),

    /// Frontmatter response.
    Frontmatter(IndexMap<String, JsonValue>),

    /// Search results response.
    SearchResults(SearchResults),

    /// Validation result response.
    ValidationResult(ValidationResult),

    /// Fix result response.
    FixResult(FixResult),

    /// Fix summary response.
    FixSummary(FixSummary),

    /// Export plan response.
    ExportPlan(ExportPlan),

    /// Exported files response.
    ExportedFiles(Vec<ExportedFile>),

    /// Binary files response.
    BinaryFiles(Vec<BinaryExportFile>),

    /// Templates list response.
    Templates(Vec<TemplateInfo>),

    /// String array response.
    Strings(Vec<String>),

    /// Bytes response (base64 encoded).
    Bytes(Vec<u8>),

    /// Storage info response.
    StorageInfo(StorageInfo),
}

// ============================================================================
// Helper Types
// ============================================================================

/// Entry data returned from GetEntry command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryData {
    /// Path to the entry.
    pub path: PathBuf,
    /// Title from frontmatter.
    pub title: Option<String>,
    /// All frontmatter properties.
    pub frontmatter: IndexMap<String, JsonValue>,
    /// Body content (after frontmatter).
    pub content: String,
}

/// Options for creating an entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CreateEntryOptions {
    /// Title for the entry.
    pub title: Option<String>,
    /// Parent to attach to.
    pub part_of: Option<String>,
    /// Template to use.
    pub template: Option<String>,
}

/// Options for searching.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchOptions {
    /// Workspace path to search in.
    pub workspace_path: Option<String>,
    /// Whether to search frontmatter.
    #[serde(default)]
    pub search_frontmatter: bool,
    /// Specific property to search.
    pub property: Option<String>,
    /// Case sensitive search.
    #[serde(default)]
    pub case_sensitive: bool,
}

/// Exported file (markdown content).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedFile {
    /// Relative path.
    pub path: String,
    /// File content.
    pub content: String,
}

/// Exported binary file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryExportFile {
    /// Relative path.
    pub path: String,
    /// Binary data.
    pub data: Vec<u8>,
}

/// Template information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateInfo {
    /// Template name.
    pub name: String,
    /// Path to template file (None for built-in).
    pub path: Option<PathBuf>,
    /// Source of the template.
    pub source: String,
}

/// Storage usage information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageInfo {
    /// Bytes used.
    pub used: u64,
    /// Storage limit (if any).
    pub limit: Option<u64>,
    /// Attachment size limit.
    pub attachment_limit: Option<u64>,
}

/// Summary of fix operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixSummary {
    /// Results from fixing errors.
    pub error_fixes: Vec<FixResult>,
    /// Results from fixing warnings.
    pub warning_fixes: Vec<FixResult>,
    /// Total number of issues fixed.
    pub total_fixed: usize,
    /// Total number of fixes that failed.
    pub total_failed: usize,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = Command::GetEntry {
            path: "notes/hello.md".to_string(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("GetEntry"));
        assert!(json.contains("notes/hello.md"));

        // Deserialize back
        let cmd2: Command = serde_json::from_str(&json).unwrap();
        if let Command::GetEntry { path } = cmd2 {
            assert_eq!(path, "notes/hello.md");
        } else {
            panic!("Wrong command type");
        }
    }

    #[test]
    fn test_response_serialization() {
        let resp = Response::String("hello".to_string());
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("String"));
        assert!(json.contains("hello"));

        // Deserialize back
        let resp2: Response = serde_json::from_str(&json).unwrap();
        if let Response::String(s) = resp2 {
            assert_eq!(s, "hello");
        } else {
            panic!("Wrong response type");
        }
    }

    #[test]
    fn test_create_entry_options_default() {
        let opts = CreateEntryOptions::default();
        assert!(opts.title.is_none());
        assert!(opts.part_of.is_none());
        assert!(opts.template.is_none());
    }

    #[test]
    fn test_search_options_default() {
        let opts = SearchOptions::default();
        assert!(!opts.search_frontmatter);
        assert!(!opts.case_sensitive);
        assert!(opts.property.is_none());
    }
}
