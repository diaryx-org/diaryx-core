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
use ts_rs::TS;

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
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
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
    /// Find the root index file in a directory.
    /// Returns the path to the root index (a file with `contents` but no `part_of`).
    FindRootIndex {
        /// Directory to search in.
        directory: String,
    },

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
        /// Optional maximum depth to traverse.
        depth: Option<u32>,
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

    /// Fix a circular reference by removing part_of from a file.
    FixCircularReference {
        /// Path to the file to edit.
        file_path: String,
        /// The part_of value to remove.
        part_of_value: String,
    },

    /// Get available parent indexes for a file (for "Choose parent" picker).
    GetAvailableParentIndexes {
        /// Path to the file that needs a parent.
        file_path: String,
        /// Workspace root to limit scope.
        workspace_root: String,
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

    /// Move an attachment from one entry to another.
    MoveAttachment {
        /// Path to the source entry file.
        source_entry_path: String,
        /// Path to the target entry file.
        target_entry_path: String,
        /// Relative path to the attachment (e.g., "_attachments/image.png").
        attachment_path: String,
        /// Optional new filename (for handling collisions).
        new_filename: Option<String>,
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

    // === CRDT Sync Operations ===
    /// Get the CRDT state vector for sync.
    #[cfg(feature = "crdt")]
    GetSyncState {
        /// Document name (e.g., "workspace").
        doc_name: String,
    },

    /// Apply an update from a remote peer.
    #[cfg(feature = "crdt")]
    ApplyRemoteUpdate {
        /// Document name.
        doc_name: String,
        /// Binary update data.
        update: Vec<u8>,
    },

    /// Get updates since a given state for sync.
    #[cfg(feature = "crdt")]
    GetMissingUpdates {
        /// Document name.
        doc_name: String,
        /// Remote state vector to diff against.
        remote_state_vector: Vec<u8>,
    },

    /// Get the full encoded state as an update.
    #[cfg(feature = "crdt")]
    GetFullState {
        /// Document name.
        doc_name: String,
    },

    // === CRDT History Operations ===
    /// Get the version history for a document.
    #[cfg(feature = "crdt")]
    GetHistory {
        /// Document name.
        doc_name: String,
        /// Optional limit on number of entries.
        limit: Option<usize>,
    },

    /// Restore a document to a previous version.
    #[cfg(feature = "crdt")]
    RestoreVersion {
        /// Document name.
        doc_name: String,
        /// Update ID to restore to.
        update_id: i64,
    },

    /// Get the diff between two versions of a document.
    #[cfg(feature = "crdt")]
    GetVersionDiff {
        /// Document name.
        doc_name: String,
        /// Starting update ID.
        from_id: i64,
        /// Ending update ID.
        to_id: i64,
    },

    /// Get the state of a document at a specific point in history.
    #[cfg(feature = "crdt")]
    GetStateAt {
        /// Document name.
        doc_name: String,
        /// Update ID to reconstruct state at.
        update_id: i64,
    },

    // === CRDT File Metadata Operations ===
    /// Get file metadata from CRDT.
    #[cfg(feature = "crdt")]
    GetCrdtFile {
        /// File path in workspace.
        path: String,
    },

    /// Set file metadata in CRDT.
    #[cfg(feature = "crdt")]
    SetCrdtFile {
        /// File path in workspace.
        path: String,
        /// File metadata as JSON.
        metadata: serde_json::Value,
    },

    /// List all files in CRDT.
    #[cfg(feature = "crdt")]
    ListCrdtFiles {
        /// Whether to include deleted files.
        #[serde(default)]
        include_deleted: bool,
    },

    /// Save CRDT state to persistent storage.
    #[cfg(feature = "crdt")]
    SaveCrdtState {
        /// Document name.
        doc_name: String,
    },

    // ==================== Body Document Commands ====================
    /// Get body content from a document CRDT.
    #[cfg(feature = "crdt")]
    GetBodyContent {
        /// Document name (file path).
        doc_name: String,
    },

    /// Set body content in a document CRDT.
    #[cfg(feature = "crdt")]
    SetBodyContent {
        /// Document name (file path).
        doc_name: String,
        /// New content.
        content: String,
    },

    /// Get sync state (state vector) for a body document.
    #[cfg(feature = "crdt")]
    GetBodySyncState {
        /// Document name (file path).
        doc_name: String,
    },

    /// Get full state of a body document as an update.
    #[cfg(feature = "crdt")]
    GetBodyFullState {
        /// Document name (file path).
        doc_name: String,
    },

    /// Apply an update to a body document.
    #[cfg(feature = "crdt")]
    ApplyBodyUpdate {
        /// Document name (file path).
        doc_name: String,
        /// Binary update data.
        update: Vec<u8>,
    },

    /// Get updates needed by a remote peer for a body document.
    #[cfg(feature = "crdt")]
    GetBodyMissingUpdates {
        /// Document name (file path).
        doc_name: String,
        /// Remote state vector.
        remote_state_vector: Vec<u8>,
    },

    /// Save a body document to storage.
    #[cfg(feature = "crdt")]
    SaveBodyDoc {
        /// Document name (file path).
        doc_name: String,
    },

    /// Save all body documents to storage.
    #[cfg(feature = "crdt")]
    SaveAllBodyDocs,

    /// Get list of loaded body documents.
    #[cfg(feature = "crdt")]
    ListLoadedBodyDocs,

    /// Unload a body document from memory.
    #[cfg(feature = "crdt")]
    UnloadBodyDoc {
        /// Document name (file path).
        doc_name: String,
    },

    // ==================== Sync Protocol Commands ====================
    /// Create a SyncStep1 message for initiating sync.
    ///
    /// Returns the encoded message that should be sent to the sync server.
    #[cfg(feature = "crdt")]
    CreateSyncStep1 {
        /// Document name (use "workspace" for workspace CRDT).
        doc_name: String,
    },

    /// Handle an incoming sync message.
    ///
    /// Returns an optional response message to send back.
    #[cfg(feature = "crdt")]
    HandleSyncMessage {
        /// Document name (use "workspace" for workspace CRDT).
        doc_name: String,
        /// The incoming message bytes.
        message: Vec<u8>,
    },

    /// Create an update message to broadcast local changes.
    #[cfg(feature = "crdt")]
    CreateUpdateMessage {
        /// Document name (use "workspace" for workspace CRDT).
        doc_name: String,
        /// The update bytes to send.
        update: Vec<u8>,
    },
}

// ============================================================================
// Response Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
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

    /// Binary data response (for CRDT state vectors, updates).
    #[cfg(feature = "crdt")]
    Binary(Vec<u8>),

    /// CRDT file metadata response.
    #[cfg(feature = "crdt")]
    CrdtFile(Option<crate::crdt::FileMetadata>),

    /// CRDT files list response.
    #[cfg(feature = "crdt")]
    CrdtFiles(Vec<(String, crate::crdt::FileMetadata)>),

    /// CRDT history response.
    #[cfg(feature = "crdt")]
    CrdtHistory(Vec<CrdtHistoryEntry>),

    /// Update ID response.
    #[cfg(feature = "crdt")]
    UpdateId(Option<i64>),

    /// Version diff response.
    #[cfg(feature = "crdt")]
    VersionDiff(Vec<crate::crdt::FileDiff>),

    /// History entries response (newer format with more details).
    #[cfg(feature = "crdt")]
    HistoryEntries(Vec<crate::crdt::HistoryEntry>),
}

// ============================================================================
// Helper Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct CreateEntryOptions {
    /// Title for the entry.
    pub title: Option<String>,
    /// Parent to attach to.
    pub part_of: Option<String>,
    /// Template to use.
    pub template: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct ExportedFile {
    /// Relative path.
    pub path: String,
    /// File content.
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct BinaryExportFile {
    /// Relative path.
    pub path: String,
    /// Binary data.
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct TemplateInfo {
    /// Template name.
    pub name: String,
    /// Path to template file (None for built-in).
    pub path: Option<PathBuf>,
    /// Source of the template.
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct StorageInfo {
    /// Bytes used.
    pub used: u64,
    /// Storage limit (if any).
    pub limit: Option<u64>,
    /// Attachment size limit.
    pub attachment_limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
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

/// CRDT history entry for version tracking.
#[cfg(feature = "crdt")]
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct CrdtHistoryEntry {
    /// Update ID.
    pub update_id: i64,
    /// Timestamp of the update (Unix milliseconds).
    pub timestamp: i64,
    /// Origin of the update.
    pub origin: String,
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
