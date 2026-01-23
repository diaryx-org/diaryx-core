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
use crate::validate::{FixResult, ValidationResult, ValidationResultWithMeta};
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

    /// Duplicate an entry, creating a copy.
    DuplicateEntry {
        /// Path to the entry to duplicate.
        path: String,
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
    /// Returns the path to the daily entry (created if it didn't exist).
    EnsureDailyEntry {
        /// Workspace path (directory containing the workspace root index).
        workspace_path: String,
        /// Optional subfolder for daily entries (e.g., "Daily" or "Journal/Daily").
        /// If not provided, entries are created in the workspace root.
        #[serde(default)]
        daily_entry_folder: Option<String>,
        /// Optional template name to use for new entries.
        /// Falls back to "daily" built-in template if not provided.
        #[serde(default)]
        template: Option<String>,
    },

    /// Get the path to an adjacent daily entry (previous or next day).
    /// Returns null if the path is not a daily entry.
    GetAdjacentDailyEntry {
        /// Path to the current daily entry.
        path: String,
        /// Direction: "prev" for previous day, "next" for next day.
        direction: String,
    },

    /// Check if a path is a daily entry.
    IsDailyEntry {
        /// Path to check.
        path: String,
    },

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

    /// Get attachments from current entry and all ancestor indexes.
    /// Traverses up the `part_of` chain to collect inherited attachments.
    GetAncestorAttachments {
        /// Path to the entry file.
        path: String,
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

    /// Write a file with metadata as YAML frontmatter + body content.
    /// This generates the YAML frontmatter from the metadata and writes it to the file.
    WriteFileWithMetadata {
        /// Path to the file to write.
        path: String,
        /// File metadata to write as frontmatter.
        metadata: serde_json::Value,
        /// Body content (markdown after frontmatter).
        body: String,
    },

    /// Update file's frontmatter metadata, preserving existing body.
    /// If body is provided, it replaces the existing body.
    UpdateFileMetadata {
        /// Path to the file to update.
        path: String,
        /// File metadata to write as frontmatter.
        metadata: serde_json::Value,
        /// Optional new body content. If not provided, existing body is preserved.
        body: Option<String>,
    },

    // === Storage ===
    /// Get storage usage information.
    GetStorageUsage,

    // === CRDT Initialization ===
    /// Initialize workspace CRDT by scanning filesystem and populating state.
    ///
    /// This replaces the frontend's `setupWorkspaceCrdt()` logic by:
    /// 1. Finding the root index file
    /// 2. Recursively scanning all files in the workspace tree
    /// 3. Populating the CRDT with file metadata and body content
    ///
    /// If `audience` is provided, only files visible to that audience are included
    /// (uses the same filtering logic as `PlanExport`).
    ///
    /// Returns the number of files populated.
    #[cfg(feature = "crdt")]
    InitializeWorkspaceCrdt {
        /// Path to workspace root (directory or root index file).
        workspace_path: String,
        /// Optional audience filter. If provided, only files visible to this audience
        /// are included in CRDT (e.g., "family", "public", or "*" for all non-private).
        audience: Option<String>,
    },

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

    /// Get the history for a specific file, combining body and workspace changes.
    #[cfg(feature = "crdt")]
    GetFileHistory {
        /// File path in workspace.
        file_path: String,
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
    /// If `write_to_disk` is true, writes changed files to disk after applying updates.
    #[cfg(feature = "crdt")]
    HandleSyncMessage {
        /// Document name (use "workspace" for workspace CRDT).
        doc_name: String,
        /// The incoming message bytes.
        message: Vec<u8>,
        /// If true, write changed files to disk after applying updates.
        #[serde(default)]
        write_to_disk: bool,
    },

    /// Create an update message to broadcast local changes.
    #[cfg(feature = "crdt")]
    CreateUpdateMessage {
        /// Document name (use "workspace" for workspace CRDT).
        doc_name: String,
        /// The update bytes to send.
        update: Vec<u8>,
    },

    // ==================== Sync Handler Commands ====================
    /// Configure the sync handler for guest mode.
    ///
    /// In guest mode, storage paths are prefixed to isolate guest data.
    #[cfg(feature = "crdt")]
    ConfigureSyncHandler {
        /// Guest join code (None to disable guest mode).
        guest_join_code: Option<String>,
        /// Whether the guest uses OPFS (requires path prefixing).
        #[serde(default)]
        uses_opfs: bool,
    },

    /// Apply a remote workspace update with disk write side effects.
    ///
    /// This processes a remote CRDT update and optionally writes the
    /// changed files to disk, emitting FileSystemEvents.
    #[cfg(feature = "crdt")]
    ApplyRemoteWorkspaceUpdateWithEffects {
        /// Binary update data.
        update: Vec<u8>,
        /// If true, write changed files to disk. If false, only apply to CRDT.
        #[serde(default)]
        write_to_disk: bool,
    },

    /// Apply a remote body update with disk write side effects.
    ///
    /// This processes a remote body CRDT update and optionally writes
    /// the body content to disk.
    #[cfg(feature = "crdt")]
    ApplyRemoteBodyUpdateWithEffects {
        /// Document name (file path).
        doc_name: String,
        /// Binary update data.
        update: Vec<u8>,
        /// If true, write body to disk. If false, only apply to CRDT.
        #[serde(default)]
        write_to_disk: bool,
    },

    /// Convert a canonical path to a storage path.
    ///
    /// For guests using OPFS, this prefixes with `guest/{join_code}/`.
    /// For hosts or in-memory guests, returns the path unchanged.
    #[cfg(feature = "crdt")]
    GetStoragePath {
        /// Canonical path (e.g., "notes/hello.md").
        canonical_path: String,
    },

    /// Convert a storage path to a canonical path.
    ///
    /// Strips the `guest/{join_code}/` prefix if present for OPFS guests.
    #[cfg(feature = "crdt")]
    GetCanonicalPath {
        /// Storage path (possibly with guest prefix).
        storage_path: String,
    },
}

// ============================================================================
// Response Types
// ============================================================================

/// Response from a command execution.
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

    /// Validation result response (with computed metadata for frontend).
    ValidationResult(ValidationResultWithMeta),

    /// Fix result response.
    FixResult(FixResult),

    /// Fix summary response.
    FixSummary(FixSummary),

    /// Export plan response.
    ExportPlan(ExportPlan),

    /// Exported files response.
    ExportedFiles(Vec<ExportedFile>),

    /// Binary files response (includes data - use for small files only).
    BinaryFiles(Vec<BinaryExportFile>),

    /// Binary file paths response (no data - for efficient listing).
    BinaryFilePaths(Vec<BinaryFileInfo>),

    /// Templates list response.
    Templates(Vec<TemplateInfo>),

    /// String array response.
    Strings(Vec<String>),

    /// Bytes response (base64 encoded).
    Bytes(Vec<u8>),

    /// Storage info response.
    StorageInfo(StorageInfo),

    /// Ancestor attachments response.
    AncestorAttachments(AncestorAttachmentsResult),

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

/// Data for a single diary entry.
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

/// Options for creating a new entry.
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

/// Options for searching entries.
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

/// An exported file with its path and content.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct ExportedFile {
    /// Relative path.
    pub path: String,
    /// File content.
    pub content: String,
}

/// A binary file with its path and data.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct BinaryExportFile {
    /// Relative path.
    pub path: String,
    /// Binary data.
    pub data: Vec<u8>,
}

/// Binary file path info (without data) for efficient transfer.
/// Use this when you need to list files and fetch data separately.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct BinaryFileInfo {
    /// Source path (absolute, for reading).
    pub source_path: String,
    /// Relative path (for zip file structure).
    pub relative_path: String,
}

/// Information about a template.
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

/// Information about storage usage.
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

/// Summary of fix operations performed.
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

/// A single entry's attachments in the ancestor chain.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct AncestorAttachmentEntry {
    /// Path to the entry file.
    pub entry_path: String,
    /// Title of the entry (from frontmatter).
    pub entry_title: Option<String>,
    /// List of attachment paths for this entry.
    pub attachments: Vec<String>,
}

/// Result of GetAncestorAttachments command.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct AncestorAttachmentsResult {
    /// Attachments from current entry and all ancestors.
    /// Ordered from current entry first, then ancestors (closest to root).
    pub entries: Vec<AncestorAttachmentEntry>,
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
    /// Files that were changed in this update.
    pub files_changed: Vec<String>,
    /// Device ID that created this update (for multi-device attribution).
    pub device_id: Option<String>,
    /// Human-readable device name.
    pub device_name: Option<String>,
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
