//! WebAssembly bindings for Diaryx core functionality.
//!
//! This crate provides a complete backend implementation for the web frontend,
//! using an in-memory filesystem that can be persisted to IndexedDB.
//!
//! ## Architecture
//!
//! The crate is organized into typed classes for different domains:
//!
//! - [`DiaryxWorkspace`] - Workspace tree operations
//! - [`DiaryxEntry`] - Entry CRUD operations
//! - [`DiaryxFrontmatter`] - Frontmatter manipulation
//! - [`DiaryxSearch`] - Workspace search
//! - [`DiaryxTemplate`] - Template management
//! - [`DiaryxValidation`] - Link integrity validation
//! - [`DiaryxExport`] - Export with audience filtering
//! - [`DiaryxAttachment`] - Attachment upload/download
//! - [`DiaryxFilesystem`] - Low-level filesystem operations
//!
//! ## Error Handling
//!
//! All methods return `Result<T, JsValue>` for JavaScript interop.

mod async_export;
mod async_filesystem;
mod async_search;
mod async_validation;
mod async_workspace;
mod attachment;
mod backend;
mod entry;
mod error;
mod export;
mod filesystem;
mod frontmatter;
mod indexeddb_fs;
mod js_async_fs;
mod opfs_fs;
mod search;
mod state;
mod template;
mod utils;
mod validation;
mod workspace;

// Re-export classes
pub use async_filesystem::DiaryxAsyncFilesystem;
pub use attachment::DiaryxAttachment;
pub use entry::DiaryxEntry;
pub use export::DiaryxExport;
pub use filesystem::DiaryxFilesystem;
pub use frontmatter::DiaryxFrontmatter;
pub use search::DiaryxSearch;
pub use template::DiaryxTemplate;
pub use validation::DiaryxValidation;
pub use workspace::DiaryxWorkspace;

// Re-export async classes with native Promise support
pub use async_export::DiaryxAsyncExport;
pub use async_search::DiaryxAsyncSearch;
pub use async_validation::DiaryxAsyncValidation;
pub use async_workspace::DiaryxAsyncWorkspace;
pub use backend::DiaryxBackend;
pub use indexeddb_fs::IndexedDbFileSystem;
pub use js_async_fs::JsAsyncFileSystem;
pub use opfs_fs::OpfsFileSystem;

// Re-export utility functions
pub use entry::slugify_title;
pub use frontmatter::{extract_body, parse_frontmatter, serialize_frontmatter};
pub use utils::{now_timestamp, today_formatted};

use wasm_bindgen::prelude::*;

// ============================================================================
// Initialization
// ============================================================================

#[cfg(feature = "console_error_panic_hook")]
pub fn set_panic_hook() {
    console_error_panic_hook::set_once();
}

/// Initialize the WASM module. Called automatically on module load.
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    set_panic_hook();
}

// ============================================================================
// Legacy API (for backwards compatibility)
// ============================================================================

/// Load files into the in-memory filesystem.
#[wasm_bindgen]
pub fn load_files(entries: JsValue) -> Result<(), JsValue> {
    DiaryxFilesystem::new().load_files(entries)
}

/// Export all files from the in-memory filesystem.
#[wasm_bindgen]
pub fn export_files() -> Result<JsValue, JsValue> {
    DiaryxFilesystem::new().export_files()
}

/// Export binary files from the in-memory filesystem.
#[wasm_bindgen]
pub fn export_binary_files() -> Result<JsValue, JsValue> {
    DiaryxFilesystem::new().export_binary_files()
}

/// Load binary files into the filesystem.
#[wasm_bindgen]
pub fn load_binary_files(entries: JsValue) -> Result<(), JsValue> {
    DiaryxFilesystem::new().load_binary_files(entries)
}

/// Get backup data for persistence.
#[wasm_bindgen]
pub fn get_backup_data() -> Result<JsValue, JsValue> {
    DiaryxFilesystem::new().get_backup_data()
}

/// Restore from backup data.
#[wasm_bindgen]
pub fn restore_from_backup(data: JsValue) -> Result<JsValue, JsValue> {
    DiaryxFilesystem::new().restore_from_backup(data)
}

/// Check if a file exists.
#[wasm_bindgen]
pub fn file_exists(path: &str) -> bool {
    DiaryxFilesystem::new().file_exists(path)
}

/// Read a file's content.
#[wasm_bindgen]
pub fn read_file(path: &str) -> Result<String, JsValue> {
    DiaryxFilesystem::new().read_file(path)
}

/// Write content to a file.
#[wasm_bindgen]
pub fn write_file(path: &str, content: &str) -> Result<(), JsValue> {
    DiaryxFilesystem::new().write_file(path, content)
}

/// Delete a file.
#[wasm_bindgen]
pub fn delete_file(path: &str) -> Result<(), JsValue> {
    DiaryxFilesystem::new().delete_file(path)
}

/// Get the workspace tree structure.
#[wasm_bindgen]
pub fn get_workspace_tree(workspace_path: &str, depth: Option<u32>) -> Result<JsValue, JsValue> {
    DiaryxWorkspace::new().get_tree(workspace_path, depth)
}

/// Create a new workspace.
#[wasm_bindgen]
pub fn create_workspace(path: &str, name: &str) -> Result<(), JsValue> {
    DiaryxWorkspace::new().create(path, name)
}

/// Get the filesystem tree.
#[wasm_bindgen]
pub fn get_filesystem_tree(workspace_path: &str, show_hidden: bool) -> Result<JsValue, JsValue> {
    DiaryxWorkspace::new().get_filesystem_tree(workspace_path, show_hidden)
}

/// Validate workspace links.
#[wasm_bindgen]
pub fn validate_workspace(workspace_path: &str) -> Result<JsValue, JsValue> {
    DiaryxValidation::new().validate(workspace_path)
}

/// Validate a single file's links.
#[wasm_bindgen]
pub fn validate_file(file_path: &str) -> Result<JsValue, JsValue> {
    DiaryxValidation::new().validate_file(file_path)
}

/// Fix a broken part_of reference by removing it.
#[wasm_bindgen]
pub fn fix_broken_part_of(file_path: &str) -> Result<JsValue, JsValue> {
    DiaryxValidation::new().fix_broken_part_of(file_path)
}

/// Fix a broken contents reference by removing it.
#[wasm_bindgen]
pub fn fix_broken_contents_ref(index_path: &str, target: &str) -> Result<JsValue, JsValue> {
    DiaryxValidation::new().fix_broken_contents_ref(index_path, target)
}

/// Fix a broken attachment reference by removing it.
#[wasm_bindgen]
pub fn fix_broken_attachment(file_path: &str, attachment: &str) -> Result<JsValue, JsValue> {
    DiaryxValidation::new().fix_broken_attachment(file_path, attachment)
}

/// Fix a non-portable path by normalizing it.
#[wasm_bindgen]
pub fn fix_non_portable_path(
    file_path: &str,
    property: &str,
    old_value: &str,
    new_value: &str,
) -> Result<JsValue, JsValue> {
    DiaryxValidation::new().fix_non_portable_path(file_path, property, old_value, new_value)
}

/// Add an unlisted file to an index's contents.
#[wasm_bindgen]
pub fn fix_unlisted_file(index_path: &str, file_path: &str) -> Result<JsValue, JsValue> {
    DiaryxValidation::new().fix_unlisted_file(index_path, file_path)
}

/// Add an orphan binary file to an index's attachments.
#[wasm_bindgen]
pub fn fix_orphan_binary_file(index_path: &str, file_path: &str) -> Result<JsValue, JsValue> {
    DiaryxValidation::new().fix_orphan_binary_file(index_path, file_path)
}

/// Fix a missing part_of by setting it to point to the given index.
#[wasm_bindgen]
pub fn fix_missing_part_of(file_path: &str, index_path: &str) -> Result<JsValue, JsValue> {
    DiaryxValidation::new().fix_missing_part_of(file_path, index_path)
}

/// Fix all errors and fixable warnings in a validation result.
#[wasm_bindgen]
pub fn fix_all_validation_issues(validation_result: JsValue) -> Result<JsValue, JsValue> {
    DiaryxValidation::new().fix_all(validation_result)
}

/// Get an entry's content and metadata.
#[wasm_bindgen]
pub fn get_entry(path: &str) -> Result<JsValue, JsValue> {
    DiaryxEntry::new().get(path)
}

/// Save an entry's content.
#[wasm_bindgen]
pub fn save_entry(path: &str, content: &str) -> Result<(), JsValue> {
    DiaryxEntry::new().save(path, content)
}

/// Get entry's raw content.
#[wasm_bindgen]
pub fn read_entry_raw(path: &str) -> Result<String, JsValue> {
    DiaryxEntry::new().read_raw(path)
}

/// Create a new entry.
#[wasm_bindgen]
pub fn create_entry(path: &str, options: JsValue) -> Result<String, JsValue> {
    DiaryxEntry::new().create(path, options)
}

/// Delete an entry.
#[wasm_bindgen]
pub fn delete_entry(path: &str) -> Result<(), JsValue> {
    DiaryxEntry::new().delete(path)
}

/// Move/rename an entry.
#[wasm_bindgen]
pub fn move_entry(from_path: &str, to_path: &str) -> Result<String, JsValue> {
    DiaryxEntry::new().move_entry(from_path, to_path)
}

/// Attach an entry to a parent.
#[wasm_bindgen]
pub fn attach_entry_to_parent(entry_path: &str, parent_path: &str) -> Result<String, JsValue> {
    DiaryxEntry::new().attach_to_parent(entry_path, parent_path)
}

/// Convert a leaf file to an index.
#[wasm_bindgen]
pub fn convert_to_index(path: &str) -> Result<String, JsValue> {
    DiaryxEntry::new().convert_to_index(path)
}

/// Convert an index back to a leaf.
#[wasm_bindgen]
pub fn convert_to_leaf(path: &str) -> Result<String, JsValue> {
    DiaryxEntry::new().convert_to_leaf(path)
}

/// Create a child entry under a parent.
#[wasm_bindgen]
pub fn create_child_entry(parent_path: &str) -> Result<String, JsValue> {
    DiaryxEntry::new().create_child(parent_path)
}

/// Rename an entry.
#[wasm_bindgen]
pub fn rename_entry(path: &str, new_filename: &str) -> Result<String, JsValue> {
    DiaryxEntry::new().rename(path, new_filename)
}

/// Ensure today's daily entry exists.
#[wasm_bindgen]
pub fn ensure_daily_entry() -> Result<String, JsValue> {
    DiaryxEntry::new().ensure_daily()
}

/// Get all frontmatter.
#[wasm_bindgen]
pub fn get_frontmatter(path: &str) -> Result<JsValue, JsValue> {
    DiaryxFrontmatter::new().get_all(path)
}

/// Set a frontmatter property.
#[wasm_bindgen]
pub fn set_frontmatter_property(path: &str, key: &str, value: JsValue) -> Result<(), JsValue> {
    DiaryxFrontmatter::new().set_property(path, key, value)
}

/// Remove a frontmatter property.
#[wasm_bindgen]
pub fn remove_frontmatter_property(path: &str, key: &str) -> Result<(), JsValue> {
    DiaryxFrontmatter::new().remove_property(path, key)
}

/// Search the workspace.
#[wasm_bindgen]
pub fn search_workspace(pattern: &str, options: JsValue) -> Result<JsValue, JsValue> {
    DiaryxSearch::new().search(pattern, options)
}

/// List templates.
#[wasm_bindgen]
pub fn list_templates(workspace_path: Option<String>) -> Result<JsValue, JsValue> {
    DiaryxTemplate::new().list(workspace_path)
}

/// Get a template.
#[wasm_bindgen]
pub fn get_template(name: &str, workspace_path: Option<String>) -> Result<String, JsValue> {
    DiaryxTemplate::new().get(name, workspace_path)
}

/// Save a template.
#[wasm_bindgen]
pub fn save_template(name: &str, content: &str, workspace_path: &str) -> Result<(), JsValue> {
    DiaryxTemplate::new().save(name, content, workspace_path)
}

/// Delete a template.
#[wasm_bindgen]
pub fn delete_template(name: &str, workspace_path: &str) -> Result<(), JsValue> {
    DiaryxTemplate::new().delete(name, workspace_path)
}

/// Add an attachment.
#[wasm_bindgen]
pub fn add_attachment(entry_path: &str, attachment_path: &str) -> Result<(), JsValue> {
    DiaryxAttachment::new().add(entry_path, attachment_path)
}

/// Remove an attachment.
#[wasm_bindgen]
pub fn remove_attachment(entry_path: &str, attachment_path: &str) -> Result<(), JsValue> {
    DiaryxAttachment::new().remove(entry_path, attachment_path)
}

/// Get attachments for an entry.
#[wasm_bindgen]
pub fn get_attachments(entry_path: &str) -> Result<JsValue, JsValue> {
    DiaryxAttachment::new().list(entry_path)
}

/// Upload an attachment.
#[wasm_bindgen]
pub fn upload_attachment(
    entry_path: &str,
    filename: &str,
    data_base64: &str,
) -> Result<String, JsValue> {
    DiaryxAttachment::new().upload(entry_path, filename, data_base64)
}

/// Delete an attachment.
#[wasm_bindgen]
pub fn delete_attachment(entry_path: &str, attachment_path: &str) -> Result<(), JsValue> {
    DiaryxAttachment::new().delete(entry_path, attachment_path)
}

/// Read attachment data.
#[wasm_bindgen]
pub fn read_attachment_data(
    entry_path: &str,
    attachment_path: &str,
) -> Result<js_sys::Uint8Array, JsValue> {
    DiaryxAttachment::new().read_data(entry_path, attachment_path)
}

/// Get storage usage.
#[wasm_bindgen]
pub fn get_storage_usage() -> Result<JsValue, JsValue> {
    DiaryxAttachment::new().get_storage_usage()
}

/// Get available audiences.
#[wasm_bindgen]
pub fn get_available_audiences(root_path: &str) -> Result<JsValue, JsValue> {
    DiaryxExport::new().get_audiences(root_path)
}

/// Plan an export.
#[wasm_bindgen]
pub fn plan_export(root_path: &str, audience: &str) -> Result<JsValue, JsValue> {
    DiaryxExport::new().plan(root_path, audience)
}

/// Export to memory.
#[wasm_bindgen]
pub fn export_to_memory(root_path: &str, audience: &str) -> Result<JsValue, JsValue> {
    DiaryxExport::new().to_memory(root_path, audience)
}

/// Export to HTML.
#[wasm_bindgen]
pub fn export_to_html(root_path: &str, audience: &str) -> Result<JsValue, JsValue> {
    DiaryxExport::new().to_html(root_path, audience)
}

/// Export binary attachments.
#[wasm_bindgen]
pub fn export_binary_attachments(root_path: &str, audience: &str) -> Result<JsValue, JsValue> {
    DiaryxExport::new().binary_attachments(root_path, audience)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_body() {
        let content = "---\ntitle: Test\n---\n\n# Hello\n\nWorld";
        let body = extract_body(content);
        assert_eq!(body, "# Hello\n\nWorld");
    }

    #[test]
    fn test_extract_body_no_frontmatter() {
        let content = "# Hello\n\nWorld";
        let body = extract_body(content);
        assert_eq!(body, "# Hello\n\nWorld");
    }
}
