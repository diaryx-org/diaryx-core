## User (checkpoint)

[@Diaryx async first CLI WASM Tauri](zed:///agent/thread/044f8ec9-72a8-40b8-997b-9c743b01594b?name=Diaryx+async+first+CLI+WASM+Tauri)
crates/diaryx_core has been updated to be async-first (see its README for details). The CLI (crates/diaryx) has been mostly updated, as has crates/diaryx_wasm and Tauri (apps/tauri). Can we make diaryx_wasm implement the Async filesystem trait natively (with javascript async/await/promises) so we can greatly simplify apps/web?

**Tool Call: Read file `crates/diaryx_core/README.md`**
Status: Completed

````diaryx-core/crates/diaryx_core/README.md
---
title: Diaryx Core Library

author: adammharris

audience:
  - public

part_of: ../../README.md
---

# Diaryx Core Library

This is the `diaryx_core` library! It contains shared code for the Diaryx clients.

## Async-first Architecture

This library uses an **async-first** design. All core modules (`Workspace`, `Validator`, `Exporter`, `Searcher`, `Publisher`) use the `AsyncFileSystem` trait for filesystem operations.

**For CLI/native code:** Wrap a sync filesystem with `SyncToAsyncFs` and use `futures_lite::future::block_on()`:

```rust,ignore
use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use diaryx_core::workspace::Workspace;

let fs = SyncToAsyncFs::new(RealFileSystem);
let workspace = Workspace::new(fs);

// Use block_on for sync contexts
let tree = futures_lite::future::block_on(
    workspace.build_tree(Path::new("README.md"))
);
```

**For WASM:** Implement `AsyncFileSystem` directly using JS promises/IndexedDB.

## Quick overview

```markdown
diaryx_core
└── src
    ├── backup.rs ("Backup" is making a ZIP file of all the markdown files)
    ├── config.rs (configuration for the core to share)
    ├── diaryx.rs (Central data structure used)
    ├── entry (Functionality to manipulate entries)
    │   ├── helpers.rs
    │   └── mod.rs
    ├── error.rs (Shared error types)
    ├── export.rs (Like backup, but filtering by "audience" trait)
    ├── frontmatter.rs (Operations to read and manipulate frontmatter in markdown files)
    ├── fs (Filesystem abstraction)
    │   ├── async_fs.rs (Async filesystem trait and SyncToAsyncFs adapter)
    │   ├── memory.rs (In-memory filesystem, used by WASM/web client)
    │   ├── mod.rs
    │   └── native.rs (Actual filesystem [std::fs] used by Tauri/CLI)
    ├── lib.rs
    ├── publish (Uses comrak to export to HTML)
    │   ├── mod.rs
    │   └── types.rs
    ├── search.rs (Searching by frontmatter or content)
    ├── template.rs (Templating functionality, mostly for daily files)
    ├── test_utils.rs (Feature-gated unit test utility functions)
    ├── utils
    │   ├── date.rs (chrono for date and time manipulation)
    │   ├── mod.rs
    │   └── path.rs (finding relative paths, etc.)
    ├── validate.rs (Validating and fixing incorrectly organized workspaces)
    └── workspace (organizing collections of markdown files as "workspaces")
        ├── mod.rs
        └── types.rs
```

## Provided functionality

### Managing frontmatter

Full key-value operations for managing frontmatter properties:

- `set_frontmatter_property`
- `get_frontmatter_property`
- `rename_frontmatter_property`
- `remove_frontmatter_property`
- `get_all_frontmatter`

Also, sorting frontmatter properties:

- `sort_frontmatter`
- `sort_alphabetically`
- `sort_by_pattern`

## Managing file content

Operations for managing content of markdown files separate from frontmatter:

- `set_content`
- `get_content`
- `append_content`
- `clear_content`

## Search

Search frontmatter or content separately:

- `SearchQuery::content`
- `SearchQuery::frontmatter`

## Export

```rust,ignore
use diaryx_core::export::{ExportOptions, ExportPlan, Exporter};
use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use std::path::Path;

let workspace_root = Path::new("./workspace");
let audience = "public";
let destination = Path::new("./export");
let fs = SyncToAsyncFs::new(RealFileSystem);
let exporter = Exporter::new(fs);

// Use futures_lite::future::block_on for sync contexts
let plan = futures_lite::future::block_on(
    exporter.plan_export(&workspace_root, audience, destination)
).unwrap();

let force = false;
let keep_audience = false;
let options = ExportOptions {
    force,
    keep_audience,
};

let result = futures_lite::future::block_on(
    exporter.execute_export(&plan, &options)
);

match result {
  Ok(stats) => {
    println!("✓ {}", stats);
    println!("  Exported to: {}", destination.display());
  }
  Err(e) => {
    eprintln!("✗ Export failed: {}", e);
  }
}
```

## Validation

The `validate` module provides functionality to check workspace link integrity and automatically fix issues.

### Validator

The `Validator` struct checks `part_of` and `contents` references within a workspace:

```rust,ignore
use diaryx_core::validate::Validator;
use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use std::path::Path;

let fs = SyncToAsyncFs::new(RealFileSystem);
let validator = Validator::new(fs);

// Validate entire workspace starting from root index
let root_path = Path::new("./workspace/README.md");
let result = futures_lite::future::block_on(
    validator.validate_workspace(&root_path)
).unwrap();

// Or validate a single file
let file_path = Path::new("./workspace/notes/my-note.md");
let result = futures_lite::future::block_on(
    validator.validate_file(&file_path)
).unwrap();

if result.is_ok() {
    println!("✓ Validation passed ({} files checked)", result.files_checked);
} else {
    println!("Found {} errors and {} warnings",
             result.errors.len(),
             result.warnings.len());
}
```

#### Validation Errors

- `BrokenPartOf` - A file's `part_of` points to a non-existent file
- `BrokenContentsRef` - An index's `contents` references a non-existent file
- `BrokenAttachment` - A file's `attachments` references a non-existent file

#### Validation Warnings

- `OrphanFile` - A markdown file not referenced by any index
- `UnlinkedEntry` - A file/directory not in the contents hierarchy
- `UnlistedFile` - A markdown file in a directory but not in the index's contents
- `CircularReference` - Circular reference detected in workspace hierarchy
- `NonPortablePath` - A path contains absolute paths or `.`/`..` components
- `MultipleIndexes` - Multiple index files in the same directory
- `OrphanBinaryFile` - A binary file not referenced by any attachments
- `MissingPartOf` - A non-index file has no `part_of` property

### ValidationFixer

The `ValidationFixer` struct provides methods to automatically fix validation issues:

```rust,ignore
use diaryx_core::validate::{Validator, ValidationFixer};
use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use std::path::Path;

let fs = SyncToAsyncFs::new(RealFileSystem);
let validator = Validator::new(fs.clone());
let fixer = ValidationFixer::new(fs);

// Validate workspace
let root_path = Path::new("./workspace/README.md");
let result = futures_lite::future::block_on(
    validator.validate_workspace(&root_path)
).unwrap();

// Fix all issues at once
let (error_fixes, warning_fixes) = futures_lite::future::block_on(
    fixer.fix_all(&result)
);

for fix in error_fixes.iter().chain(warning_fixes.iter()) {
    if fix.success {
        println!("✓ {}", fix.message);
    } else {
        println!("✗ {}", fix.message);
    }
}

// Or fix individual issues (all methods are async)
futures_lite::future::block_on(async {
    fixer.fix_broken_part_of(Path::new("./file.md")).await;
    fixer.fix_broken_contents_ref(Path::new("./index.md"), "missing.md").await;
    fixer.fix_unlisted_file(Path::new("./index.md"), Path::new("./new-file.md")).await;
    fixer.fix_missing_part_of(Path::new("./orphan.md"), Path::new("./index.md")).await;
});
```

## Publish

## Templates

## Workspaces

## Date parsing

## Shared errors

## Configuration

## Filesystem abstraction

The `fs` module provides filesystem abstraction through two traits: `FileSystem` (synchronous) and `AsyncFileSystem` (asynchronous).

**Note:** As of the async-first refactor, all core modules (`Workspace`, `Validator`, `Exporter`, `Searcher`, `Publisher`) use `AsyncFileSystem`. For synchronous contexts (CLI, tests), wrap a sync filesystem with `SyncToAsyncFs` and use `futures_lite::future::block_on()`.

### FileSystem trait

The synchronous `FileSystem` trait provides basic implementations:

- `RealFileSystem` - Native filesystem using `std::fs` (not available on WASM)
- `InMemoryFileSystem` - In-memory implementation, useful for WASM and testing

```rust,ignore
use diaryx_core::fs::{FileSystem, InMemoryFileSystem};
use std::path::Path;

// Create an in-memory filesystem
let fs = InMemoryFileSystem::new();

// Write a file (sync)
fs.write_file(Path::new("workspace/README.md"), "# Hello").unwrap();

// Read it back
let content = fs.read_to_string(Path::new("workspace/README.md")).unwrap();
assert_eq!(content, "# Hello");
```

### AsyncFileSystem trait (Primary API)

The `AsyncFileSystem` trait is the primary API for all core modules:

- WASM environments where JavaScript APIs (like IndexedDB) are async
- Native code using async runtimes like tokio
- All workspace operations (Workspace, Validator, Exporter, etc.)

```rust,ignore
use diaryx_core::fs::{AsyncFileSystem, InMemoryFileSystem, SyncToAsyncFs};
use diaryx_core::workspace::Workspace;
use std::path::Path;

// Wrap a sync filesystem for use with async APIs
let sync_fs = InMemoryFileSystem::new();
let async_fs = SyncToAsyncFs::new(sync_fs);

// Use with Workspace (async)
let workspace = Workspace::new(async_fs);

// For sync contexts, use block_on
let tree = futures_lite::future::block_on(
    workspace.build_tree(Path::new("README.md"))
);
```

### SyncToAsyncFs adapter

The `SyncToAsyncFs` struct wraps any synchronous `FileSystem` implementation to provide an `AsyncFileSystem` interface. This is the recommended way to use the async-first API in synchronous contexts:

```rust,ignore
use diaryx_core::fs::{InMemoryFileSystem, SyncToAsyncFs, RealFileSystem};
use diaryx_core::workspace::Workspace;

// For native code
let fs = SyncToAsyncFs::new(RealFileSystem);
let workspace = Workspace::new(fs);

// For tests/WASM
let fs = SyncToAsyncFs::new(InMemoryFileSystem::new());
let workspace = Workspace::new(fs);

// Access the inner sync filesystem if needed
// let inner = async_fs.inner();
```

````


**Tool Call: List the `diaryx-core/crates/diaryx_wasm/src` directory's contents**
Status: Completed


# Files:
diaryx-core/crates/diaryx_wasm/src/async_filesystem.rs
diaryx-core/crates/diaryx_wasm/src/attachment.rs
diaryx-core/crates/diaryx_wasm/src/entry.rs
diaryx-core/crates/diaryx_wasm/src/error.rs
diaryx-core/crates/diaryx_wasm/src/export.rs
diaryx-core/crates/diaryx_wasm/src/filesystem.rs
diaryx-core/crates/diaryx_wasm/src/frontmatter.rs
diaryx-core/crates/diaryx_wasm/src/lib.rs
diaryx-core/crates/diaryx_wasm/src/search.rs
diaryx-core/crates/diaryx_wasm/src/state.rs
diaryx-core/crates/diaryx_wasm/src/template.rs
diaryx-core/crates/diaryx_wasm/src/utils.rs
diaryx-core/crates/diaryx_wasm/src/validation.rs
diaryx-core/crates/diaryx_wasm/src/workspace.rs


**Tool Call: Read file `crates/diaryx_wasm/src/lib.rs`**
Status: Completed

```diaryx-core/crates/diaryx_wasm/src/lib.rs
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

mod async_filesystem;
mod attachment;
mod entry;
mod error;
mod export;
mod filesystem;
mod frontmatter;
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

```


## Assistant

Let me look at the existing async filesystem implementation and the state module:

**Tool Call: Read file `crates/diaryx_wasm/src/state.rs`**
Status: Completed

```diaryx-core/crates/diaryx_wasm/src/state.rs
//! Global filesystem state management.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use diaryx_core::fs::{InMemoryFileSystem, SyncToAsyncFs};

thread_local! {
    static FILESYSTEM: RefCell<InMemoryFileSystem> = RefCell::new(InMemoryFileSystem::new());
}

/// Execute a closure with read access to the global filesystem.
pub fn with_fs<F, R>(f: F) -> R
where
    F: FnOnce(&InMemoryFileSystem) -> R,
{
    FILESYSTEM.with(|fs| f(&fs.borrow()))
}

/// Execute a closure with access to the global filesystem.
///
/// Note: Uses immutable borrow because `InMemoryFileSystem` uses internal
/// mutability (`RefCell<HashMap>`). The `FileSystem` trait is implemented
/// for `&InMemoryFileSystem`, not `&mut InMemoryFileSystem`.
pub fn with_fs_mut<F, R>(f: F) -> R
where
    F: FnOnce(&InMemoryFileSystem) -> R,
{
    FILESYSTEM.with(|fs| f(&fs.borrow()))
}

/// Execute a closure with an async filesystem wrapper.
///
/// This wraps the sync InMemoryFileSystem with SyncToAsyncFs for use
/// with async-first core modules (Workspace, Validator, Searcher, etc.).
pub fn with_async_fs<F, R>(f: F) -> R
where
    F: FnOnce(SyncToAsyncFs<&InMemoryFileSystem>) -> R,
{
    FILESYSTEM.with(|fs| {
        let borrowed = fs.borrow();
        let async_fs = SyncToAsyncFs::new(&*borrowed);
        f(async_fs)
    })
}

/// Simple blocking executor for running async futures in WASM.
///
/// Since InMemoryFileSystem is synchronous, futures from SyncToAsyncFs
/// complete immediately without yielding, making this safe to use.
pub fn block_on<F: Future>(f: F) -> F::Output {
    // Create a no-op waker
    const VTABLE: RawWakerVTable = RawWakerVTable::new(
        |_| RawWaker::new(std::ptr::null(), &VTABLE), // clone
        |_| {},                                       // wake
        |_| {},                                       // wake_by_ref
        |_| {},                                       // drop
    );

    let raw_waker = RawWaker::new(std::ptr::null(), &VTABLE);
    let waker = unsafe { Waker::from_raw(raw_waker) };
    let mut cx = Context::from_waker(&waker);

    let mut pinned = std::pin::pin!(f);
    loop {
        match pinned.as_mut().poll(&mut cx) {
            Poll::Ready(output) => return output,
            Poll::Pending => {
                // For sync-wrapped futures, this should never happen
                // But we handle it anyway by spinning
                std::hint::spin_loop();
            }
        }
    }
}

/// Replace the entire filesystem with a new one.
///
/// Use this for operations that need to replace the whole filesystem
/// (e.g., loading from backup, initial load).
pub fn replace_fs(new_fs: InMemoryFileSystem) {
    FILESYSTEM.with(|fs| *fs.borrow_mut() = new_fs);
}

/// Reset the filesystem to a fresh state (for testing).
#[cfg(test)]
pub fn reset_filesystem() {
    FILESYSTEM.with(|fs| *fs.borrow_mut() = InMemoryFileSystem::new());
}

```


**Tool Call: Read file `crates/diaryx_core/src/fs/async_fs.rs`**
Status: Completed

````diaryx-core/crates/diaryx_core/src/fs/async_fs.rs
//! Async filesystem abstraction module.
//!
//! This module provides the `AsyncFileSystem` trait for abstracting async filesystem operations,
//! allowing different implementations for native and WASM targets.
//!
//! This is particularly useful for:
//! - WASM environments where JavaScript APIs (like IndexedDB) are inherently async
//! - Native environments using async runtimes like tokio
//! - Code that needs to await filesystem operations
//!
//! ## Object safety
//!
//! `AsyncFileSystem` is designed to be object-safe so it can be used behind
//! `dyn AsyncFileSystem` (e.g. inside trait objects like backup targets).
//! To enable this, all methods return boxed futures.

use std::future::Future;
use std::io::Result;
use std::path::{Path, PathBuf};
use std::pin::Pin;

#[cfg(test)]
pub(crate) fn block_on_test<F: Future>(f: F) -> F::Output {
    futures_lite::future::block_on(f)
}

/// A boxed future that is Send.
/// Used for object-safe async methods and recursion.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Async abstraction over filesystem operations.
///
/// This trait mirrors `FileSystem` but with async methods, making it suitable
/// for environments where filesystem operations may be asynchronous (e.g., WASM
/// with IndexedDB, or native code using async I/O).
///
/// # Example
///
/// ```ignore
/// use diaryx_core::fs::AsyncFileSystem;
///
/// async fn example(fs: &dyn AsyncFileSystem) {
///     let content = fs.read_to_string(Path::new("file.md")).await.unwrap();
///     fs.write_file(Path::new("output.md"), &content).await.unwrap();
/// }
/// ```
pub trait AsyncFileSystem: Send + Sync {
    /// Reads the file content as a string.
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>>;

    /// Overwrites an existing file with new content.
    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>>;

    /// Creates a file ONLY if it doesn't exist.
    /// Should return an error if file exists.
    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>>;

    /// Deletes a file.
    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>>;

    /// Finds markdown files in a folder.
    fn list_md_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>>;

    /// Checks if a file or directory exists.
    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool>;

    /// Creates a directory and all parent directories.
    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>>;

    /// Checks if a path is a directory.
    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool>;

    /// Move/rename a file from `from` to `to`.
    ///
    /// Implementations should treat this as an atomic-ish move when possible,
    /// and should error if the source does not exist or if the destination already exists.
    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>>;

    // ==================== Binary File Methods ====================
    // These methods support binary files (attachments) without base64 overhead

    /// Read binary file content.
    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        Box::pin(async move {
            // Default implementation: read as string and convert to bytes
            self.read_to_string(path).await.map(|s| s.into_bytes())
        })
    }

    /// Write binary content to a file.
    fn write_binary<'a>(&'a self, _path: &'a Path, _content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Default implementation: not supported
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Binary write not supported",
            ))
        })
    }

    /// List all files in a directory (not recursive).
    fn list_files<'a>(&'a self, _dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move {
            // Default: return empty
            Ok(vec![])
        })
    }

    /// Recursively list all markdown files in a directory and its subdirectories.
    fn list_md_files_recursive<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move {
            let mut all_files = self.list_md_files(dir).await?;

            // Get subdirectories and recurse
            if let Ok(entries) = self.list_files(dir).await {
                for entry in entries {
                    if self.is_dir(&entry).await {
                        if let Ok(subdir_files) = self.list_md_files_recursive(&entry).await {
                            all_files.extend(subdir_files);
                        }
                    }
                }
            }

            Ok(all_files)
        })
    }

    /// Recursively list ALL files and directories in a directory.
    fn list_all_files_recursive<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move {
            let mut all_entries = Vec::new();

            if let Ok(entries) = self.list_files(dir).await {
                for entry in entries {
                    all_entries.push(entry.clone());
                    if self.is_dir(&entry).await {
                        if let Ok(subdir_entries) = self.list_all_files_recursive(&entry).await {
                            all_entries.extend(subdir_entries);
                        }
                    }
                }
            }

            Ok(all_entries)
        })
    }
}

// ============================================================================
// Adapter: Sync FileSystem -> AsyncFileSystem
// ============================================================================

use super::FileSystem;

/// Wrapper that adapts a synchronous `FileSystem` to `AsyncFileSystem`.
///
/// This is useful for wrapping `InMemoryFileSystem` or other sync implementations
/// to be used in async contexts. The operations complete immediately since the
/// underlying implementation is synchronous.
///
/// # Example
///
/// ```ignore
/// use diaryx_core::fs::{InMemoryFileSystem, SyncToAsyncFs, AsyncFileSystem};
///
/// let sync_fs = InMemoryFileSystem::new();
/// let async_fs = SyncToAsyncFs::new(sync_fs);
///
/// // Now you can use async_fs in async code
/// async {
///     let content = async_fs.read_to_string(Path::new("file.md")).await;
/// };
/// ```
#[derive(Clone)]
pub struct SyncToAsyncFs<F: FileSystem> {
    inner: F,
}

impl<F: FileSystem> SyncToAsyncFs<F> {
    /// Create a new async wrapper around a synchronous filesystem.
    pub fn new(fs: F) -> Self {
        Self { inner: fs }
    }

    /// Get a reference to the inner synchronous filesystem.
    pub fn inner(&self) -> &F {
        &self.inner
    }

    /// Unwrap and return the inner synchronous filesystem.
    pub fn into_inner(self) -> F {
        self.inner
    }
}

impl<F: FileSystem + Send + Sync> AsyncFileSystem for SyncToAsyncFs<F> {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move { self.inner.read_to_string(path) })
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.inner.write_file(path, content) })
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.inner.create_new(path, content) })
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.inner.delete_file(path) })
    }

    fn list_md_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move { self.inner.list_md_files(dir) })
    }

    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        Box::pin(async move { self.inner.exists(path) })
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.inner.create_dir_all(path) })
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        Box::pin(async move { self.inner.is_dir(path) })
    }

    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.inner.move_file(from, to) })
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        Box::pin(async move { self.inner.read_binary(path) })
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.inner.write_binary(path, content) })
    }

    fn list_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move { self.inner.list_files(dir) })
    }
}

// Blanket implementation for references to AsyncFileSystem
impl<T: AsyncFileSystem + ?Sized> AsyncFileSystem for &T {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        (*self).read_to_string(path)
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        (*self).write_file(path, content)
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        (*self).create_new(path, content)
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        (*self).delete_file(path)
    }

    fn list_md_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        (*self).list_md_files(dir)
    }

    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        (*self).exists(path)
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        (*self).create_dir_all(path)
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        (*self).is_dir(path)
    }

    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        (*self).move_file(from, to)
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        (*self).read_binary(path)
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        (*self).write_binary(path, content)
    }

    fn list_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        (*self).list_files(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFileSystem;

    #[test]
    fn test_sync_to_async_wrapper() {
        let sync_fs = InMemoryFileSystem::new();

        // Write a file using sync API
        sync_fs
            .write_file(Path::new("test.md"), "# Hello")
            .unwrap();

        // Wrap in async adapter
        let async_fs = SyncToAsyncFs::new(sync_fs);

        // Use futures::executor to run the async code in a test
        // Note: In real async code, you'd use an async runtime
        let content = futures_lite_test_block_on(async_fs.read_to_string(Path::new("test.md")));
        assert_eq!(content.unwrap(), "# Hello");

        let exists = futures_lite_test_block_on(async_fs.exists(Path::new("test.md")));
        assert!(exists);

        let not_exists = futures_lite_test_block_on(async_fs.exists(Path::new("nonexistent.md")));
        assert!(!not_exists);
    }

    #[test]
    fn test_async_write_and_read() {
        let sync_fs = InMemoryFileSystem::new();
        let async_fs = SyncToAsyncFs::new(sync_fs);

        // Write using async API
        let write_result =
            futures_lite_test_block_on(async_fs.write_file(Path::new("new.md"), "New content"));
        assert!(write_result.is_ok());

        // Read it back
        let content = futures_lite_test_block_on(async_fs.read_to_string(Path::new("new.md")));
        assert_eq!(content.unwrap(), "New content");
    }

    #[test]
    fn test_async_create_new() {
        let sync_fs = InMemoryFileSystem::new();
        let async_fs = SyncToAsyncFs::new(sync_fs);

        // Create new file
        let result =
            futures_lite_test_block_on(async_fs.create_new(Path::new("created.md"), "Created!"));
        assert!(result.is_ok());

        // Try to create again - should fail
        let result2 =
            futures_lite_test_block_on(async_fs.create_new(Path::new("created.md"), "Again!"));
        assert!(result2.is_err());
    }

    #[test]
    fn test_async_directory_operations() {
        let sync_fs = InMemoryFileSystem::new();
        let async_fs = SyncToAsyncFs::new(sync_fs);

        // Create directory
        let result = futures_lite_test_block_on(async_fs.create_dir_all(Path::new("a/b/c")));
        assert!(result.is_ok());

        // Check it's a directory
        let is_dir = futures_lite_test_block_on(async_fs.is_dir(Path::new("a/b/c")));
        assert!(is_dir);

        // Check parent is also a directory
        let parent_is_dir = futures_lite_test_block_on(async_fs.is_dir(Path::new("a/b")));
        assert!(parent_is_dir);
    }

    #[test]
    fn test_inner_access() {
        let sync_fs = InMemoryFileSystem::new();
        sync_fs
            .write_file(Path::new("test.md"), "content")
            .unwrap();

        let async_fs = SyncToAsyncFs::new(sync_fs);

        // Access inner
        assert!(async_fs.inner().exists(Path::new("test.md")));

        // Unwrap
        let recovered = async_fs.into_inner();
        assert!(recovered.exists(Path::new("test.md")));
    }

    /// Simple blocking executor for tests only.
    /// In production, use a proper async runtime.
    fn futures_lite_test_block_on<F: Future>(f: F) -> F::Output {
        use std::pin::pin;
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

        // Create a no-op waker
        const VTABLE: RawWakerVTable = RawWakerVTable::new(
            |_| RawWaker::new(std::ptr::null(), &VTABLE), // clone
            |_| {},                                       // wake
            |_| {},                                       // wake_by_ref
            |_| {},                                       // drop
        );

        let raw_waker = RawWaker::new(std::ptr::null(), &VTABLE);
        let waker = unsafe { Waker::from_raw(raw_waker) };
        let mut cx = Context::from_waker(&waker);

        let mut pinned = pin!(f);
        loop {
            match pinned.as_mut().poll(&mut cx) {
                Poll::Ready(output) => return output,
                Poll::Pending => {
                    // For our sync-wrapped futures, this should never happen
                    // But we handle it anyway by spinning
                    std::hint::spin_loop();
                }
            }
        }
    }
}
````


**Tool Call: Read file `crates/diaryx_wasm/src/async_filesystem.rs`**
Status: Completed

```diaryx-core/crates/diaryx_wasm/src/async_filesystem.rs
SUCCESS: File outline retrieved. This file is too large to read all at once, so the outline below shows the file's structure with line numbers.

IMPORTANT: Do NOT retry this call without line numbers - you will get the same outline.
Instead, use the line numbers below to read specific sections by calling this tool again with start_line and end_line parameters.

# File outline for /Users/adamharris/Documents/diaryx-core/crates/diaryx_wasm/src/async_filesystem.rs

struct BinaryEntry [L32-35]
 path [L33]
 data [L34]
pub struct JsAsyncBackupResult [L39-45]
 pub success [L40]
 pub files_processed [L41]
 pub text_files [L42]
 pub binary_files [L43]
 pub error [L44]
pub struct JsFileList [L49-52]
 pub files [L50]
 pub count [L51]
pub struct DiaryxAsyncFilesystem [L69]
impl DiaryxAsyncFilesystem [L72-468]
 pub fn new [L75-77]
 pub fn read_file [L83-92]
 pub fn write_file [L98-106]
 pub fn create_new [L112-120]
 pub fn delete_file [L126-134]
 pub fn file_exists [L140-145]
 pub fn is_dir [L151-156]
 pub fn create_dir_all [L162-170]
 pub fn move_file [L176-184]
 pub fn list_md_files [L190-204]
 pub fn list_files [L210-224]
 pub fn list_md_files_recursive [L230-245]
 pub fn list_all_files_recursive [L251-266]
 pub fn read_binary [L272-281]
 pub fn write_binary [L288-297]
 pub fn load_files [L308-315]
 pub fn export_files [L321-326]
 pub fn export_binary_files [L332-341]
 pub fn load_binary_files [L348-363]
 pub fn get_backup_data [L370-397]
  struct BackupData [L376-381]
   text_files [L377]
   binary_files [L378]
   text_count [L379]
   binary_count [L380]
 pub fn restore_from_backup [L403-436]
  struct BackupData [L406-409]
   text_files [L407]
   binary_files [L408]
 pub fn clear [L442-449]
 pub fn list_all_files [L455-467]
impl Default for DiaryxAsyncFilesystem [L470-474]
 fn default [L471-473]

Showing symbols 1-47 (total symbols: 47)


NEXT STEPS: To read a specific symbol's implementation, call read_file with the same path plus start_line and end_line from the outline above.
For example, to read a function shown as [L100-150], use start_line: 100 and end_line: 150.
```


**Tool Call: Read file `crates/diaryx_wasm/src/async_filesystem.rs` (lines 1-150)**
Status: Completed

````diaryx-core/crates/diaryx_wasm/src/async_filesystem.rs
//! Async filesystem operations for WASM with JavaScript Promise interop.
//!
//! This module provides an async wrapper around the in-memory filesystem
//! that works with `wasm-bindgen-futures` for proper JavaScript Promise support.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { DiaryxAsyncFilesystem } from './wasm/diaryx_wasm.js';
//!
//! const asyncFs = new DiaryxAsyncFilesystem();
//!
//! // All methods return Promises
//! const content = await asyncFs.read_file('workspace/README.md');
//! await asyncFs.write_file('workspace/new.md', '# New File');
//! const exists = await asyncFs.file_exists('workspace/new.md');
//! ```

use diaryx_core::fs::{AsyncFileSystem, FileSystem, InMemoryFileSystem, SyncToAsyncFs};
use serde::{Deserialize, Serialize};
use std::path::Path;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::state::{replace_fs, with_fs, with_fs_mut};

// ============================================================================
// Types
// ============================================================================

#[derive(Serialize, Deserialize)]
struct BinaryEntry {
    path: String,
    data: Vec<u8>,
}

/// Result of a backup operation
#[derive(Serialize)]
pub struct JsAsyncBackupResult {
    pub success: bool,
    pub files_processed: usize,
    pub text_files: usize,
    pub binary_files: usize,
    pub error: Option<String>,
}

/// Result of listing files
#[derive(Serialize)]
pub struct JsFileList {
    pub files: Vec<String>,
    pub count: usize,
}

// ============================================================================
// DiaryxAsyncFilesystem Class
// ============================================================================

/// Async filesystem operations for WASM with JavaScript Promise support.
///
/// This class provides async methods that return JavaScript Promises,
/// making it suitable for use with async/await in JavaScript.
///
/// While the underlying InMemoryFileSystem is synchronous, this wrapper
/// provides a Promise-based API that:
/// 1. Enables consistent async/await patterns in JavaScript
/// 2. Allows for future integration with truly async operations (e.g., IndexedDB)
/// 3. Works well with JavaScript's event loop
#[wasm_bindgen]
pub struct DiaryxAsyncFilesystem;

#[wasm_bindgen]
impl DiaryxAsyncFilesystem {
    /// Create a new DiaryxAsyncFilesystem instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Read a file's content (async).
    ///
    /// Returns a Promise that resolves to the file content as a string.
    #[wasm_bindgen]
    pub fn read_file(&self, path: String) -> js_sys::Promise {
        future_to_promise(async move {
            let async_fs = with_fs(|fs| SyncToAsyncFs::new(fs.clone()));
            let content = async_fs
                .read_to_string(Path::new(&path))
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(JsValue::from_str(&content))
        })
    }

    /// Write content to a file (async).
    ///
    /// Returns a Promise that resolves when the write is complete.
    #[wasm_bindgen]
    pub fn write_file(&self, path: String, content: String) -> js_sys::Promise {
        future_to_promise(async move {
            with_fs_mut(|fs| {
                fs.write_file(Path::new(&path), &content)
                    .map_err(|e| JsValue::from_str(&e.to_string()))
            })?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Create a new file (fails if exists) (async).
    ///
    /// Returns a Promise that resolves when the file is created.
    #[wasm_bindgen]
    pub fn create_new(&self, path: String, content: String) -> js_sys::Promise {
        future_to_promise(async move {
            with_fs_mut(|fs| {
                fs.create_new(Path::new(&path), &content)
                    .map_err(|e| JsValue::from_str(&e.to_string()))
            })?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Delete a file (async).
    ///
    /// Returns a Promise that resolves when the file is deleted.
    #[wasm_bindgen]
    pub fn delete_file(&self, path: String) -> js_sys::Promise {
        future_to_promise(async move {
            with_fs_mut(|fs| {
                fs.delete_file(Path::new(&path))
                    .map_err(|e| JsValue::from_str(&e.to_string()))
            })?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Check if a file exists (async).
    ///
    /// Returns a Promise that resolves to a boolean.
    #[wasm_bindgen]
    pub fn file_exists(&self, path: String) -> js_sys::Promise {
        future_to_promise(async move {
            let exists = with_fs(|fs| FileSystem::exists(fs, Path::new(&path)));
            Ok(JsValue::from_bool(exists))
        })
    }

    /// Check if a path is a directory (async).
    ///
    /// Returns a Promise that resolves to a boolean.
    #[wasm_bindgen]

````


**Tool Call: List the `diaryx-core/apps/web/src` directory's contents**
Status: Completed

# Folders:
diaryx-core/apps/web/src/lib
diaryx-core/apps/web/src/models
diaryx-core/apps/web/src/views

# Files:
diaryx-core/apps/web/src/App.svelte
diaryx-core/apps/web/src/LICENSE.md
diaryx-core/apps/web/src/app.css
diaryx-core/apps/web/src/main.ts


**Tool Call: List the `diaryx-core/apps/web/src/lib` directory's contents**
Status: Completed

# Folders:
diaryx-core/apps/web/src/lib/backend
diaryx-core/apps/web/src/lib/components
diaryx-core/apps/web/src/lib/hooks
diaryx-core/apps/web/src/lib/settings
diaryx-core/apps/web/src/lib/stores
diaryx-core/apps/web/src/lib/wasm

# Files:
diaryx-core/apps/web/src/lib/CommandPalette.svelte
diaryx-core/apps/web/src/lib/Editor.svelte
diaryx-core/apps/web/src/lib/ExportDialog.svelte
diaryx-core/apps/web/src/lib/LeftSidebar.svelte
diaryx-core/apps/web/src/lib/NewEntryModal.svelte
diaryx-core/apps/web/src/lib/RightSidebar.svelte
diaryx-core/apps/web/src/lib/SettingsDialog.svelte
diaryx-core/apps/web/src/lib/collaborationUtils.ts
diaryx-core/apps/web/src/lib/credentials.ts
diaryx-core/apps/web/src/lib/utils.ts
diaryx-core/apps/web/src/lib/workspaceCrdt.ts


**Tool Call: Read file `apps/web/README.md`**
Status: Completed

````diaryx-core/apps/web/README.md
---
title: Diaryx Web
author: adammharris
audience:
  - public
  - developers
part_of: ../README.md
---

# Diaryx Web

The Svelte web frontend for Diaryx, supporting both WebAssembly and Tauri backends.

## Getting Started

```bash
# Install dependencies (uses Bun package manager)
bun install

# Development server
bun run dev

# Build for production
bun run build
```

## Architecture

This is a plain Svelte 5 app (not SvelteKit). It uses a backend abstraction layer to support two runtime environments:

### Backend Abstraction

The `src/lib/backend/` directory contains:

| File           | Purpose                                             |
| -------------- | --------------------------------------------------- |
| `interface.ts` | TypeScript interface defining all backend methods   |
| `wasm.ts`      | WebAssembly implementation (InMemoryFS + IndexedDB) |
| `tauri.ts`     | Tauri IPC implementation (native filesystem)        |
| `index.ts`     | Runtime detection and backend export                |

```typescript
import { backend } from "$lib/backend";

// Works identically in both WASM and Tauri environments
await backend.init();
const tree = await backend.getWorkspaceTree();
const entry = await backend.getEntry("workspace/notes/my-note.md");
```

### WASM Backend

When running in a browser without Tauri:

1. Files are stored in IndexedDB
2. Loaded into `InMemoryFileSystem` on startup
3. All operations happen in memory
4. Changes persisted back to IndexedDB via `backend.persist()`

### Tauri Backend

When running inside Tauri:

1. Commands sent via Tauri IPC
2. Rust backend uses `RealFileSystem`
3. Direct native filesystem access
4. No explicit persistence needed

## Validation

Both backends support comprehensive validation and automatic fixing:

```typescript
// Validate workspace
const result = await backend.validateWorkspace();

if (result.errors.length > 0 || result.warnings.length > 0) {
  // Fix all issues automatically
  const summary = await backend.fixAll(result);
  console.log(`Fixed: ${summary.total_fixed}, Failed: ${summary.total_failed}`);
}

// Or fix individual issues
await backend.fixBrokenPartOf("workspace/broken.md");
await backend.fixUnlistedFile("workspace/index.md", "workspace/new-file.md");
```

### Validation Errors

- `BrokenPartOf` - `part_of` points to non-existent file
- `BrokenContentsRef` - `contents` references non-existent file
- `BrokenAttachment` - `attachments` references non-existent file

### Validation Warnings

- `OrphanFile` - Markdown file not in any index's contents
- `UnlinkedEntry` - File/directory not in contents hierarchy
- `UnlistedFile` - File exists but not listed in index's contents
- `CircularReference` - Circular reference in hierarchy
- `NonPortablePath` - Path contains absolute or `.`/`..` components
- `MultipleIndexes` - Multiple index files in same directory
- `OrphanBinaryFile` - Binary file not in any attachments
- `MissingPartOf` - Non-index file has no `part_of`

## Project Structure

```
src/
├── App.svelte           # Main app component
├── main.ts              # Entry point
├── app.css              # Global styles
└── lib/
    ├── backend/         # Backend abstraction layer
    │   ├── interface.ts # Backend interface definition
    │   ├── wasm.ts      # WebAssembly implementation
    │   ├── tauri.ts     # Tauri IPC implementation
    │   └── index.ts     # Runtime detection
    ├── components/      # Reusable Svelte components
    ├── stores/          # Svelte stores for state management
    ├── hooks/           # Custom Svelte hooks
    ├── wasm/            # Built WASM module (from diaryx_wasm)
    ├── Editor.svelte    # TipTap markdown editor
    ├── LeftSidebar.svelte
    ├── RightSidebar.svelte
    └── ...
```

## Building WASM

The WASM module is built from `crates/diaryx_wasm`:

```bash
cd ../../crates/diaryx_wasm
wasm-pack build --target web --out-dir ../../apps/web/src/lib/wasm
```

## Live Demo

Try the web frontend at: https://diaryx-org.github.io/diaryx/

````


**Tool Call: Read file `apps/web/src/lib/backend/wasm.ts`**
Status: Completed

```diaryx-core/apps/web/src/lib/backend/wasm.ts
SUCCESS: File outline retrieved. This file is too large to read all at once, so the outline below shows the file's structure with line numbers.

IMPORTANT: Do NOT retry this call without line numbers - you will get the same outline.
Instead, use the line numbers below to read specific sections by calling this tool again with start_line and end_line parameters.

# File outline for /Users/adamharris/Documents/diaryx-core/apps/web/src/lib/backend/wasm.ts

function normalizeEntryPathToWorkspaceRoot( ) [L21-35]
 const raw [L25]
 const root [L30]
function normalizeIndexPathToWorkspaceRoot( ) [L37-44]
function isProbablyTextFile( ) [L50-77]
 const lower [L51]
function normalizeZipPath( ) [L79-82]
async function importZipEntriesViaJszip( ) [L84-140]
 const JSZip [L92]
 const totalBytes [L94]
 const zip [L97]
 const textEntries [L99]
 const binaryEntries [L100]
 const names [L103]
 let processedBytesEstimate [L105]
 const entry [L108]
 const normalized [L111]
 const content [L123]
 const data [L127]
 path [L128]
 data [L128]
const DB_NAME [L146]
const DB_VERSION [L147]
const STORE_FILES [L148]
const STORE_BINARY_FILES [L149]
const STORE_CONFIG [L150]
interface FileEntry [L152-156]
interface BinaryFileEntry [L158-162]
class IndexedDBStorage [L164-291]
 private db [L165]
 async open() [L167-194]
  const request [L169]
  const db [L179]
  keyPath [L182]
  keyPath [L186]
  keyPath [L190]
 async loadAllFiles() [L196-207]
  const transaction [L200]
  const store [L201]
  const request [L202]
 async saveAllFiles( ) [L209-226]
  const transaction [L213]
  const store [L214]
  const now [L218]
  updatedAt [L220]
 async loadConfig() [L228-242]
  const transaction [L232]
  const store [L233]
  const request [L234]
  const result [L237]
 async saveConfig( ) [L244-255]
  const transaction [L248]
  const store [L249]
  const request [L250]
   id [L250]
   data [L250]
 async loadBinaryFiles() [L257-268]
  const transaction [L261]
  const store [L262]
  const request [L263]
 async saveBinaryFiles( ) [L270-290]
  const transaction [L276]
  const store [L277]
  const now [L281]
  data [L283]
  updatedAt [L283]
type WasmModule [L297]
let wasm [L299]
async function loadWasm() [L301-311]
 const module [L305]
class WasmBackend [L317-1184]
 private storage [L318]
 private incrementalStorage [L319]
 private config [L320]
 private wasm [L321]
 private ready [L322]
 private eventEmitter [L323]
 private _workspace [L326]
 private _entry [L327]
 private _frontmatter [L328-329]
 private _search [L330]
 private _template [L331]
 private _validation [L332-333]
 private _export [L334]
 private _attachment [L335-336]
 private _filesystem [L337-338]
 async init( ) [L340-413]
  const storageType [L364]
  const storage [L372]
  const files [L374]
  const entries [L375]
  const binaryFiles [L381]
  const binaryEntries [L383-386]
   path [L384]
   data [L385]
  default_workspace [L403]
 isReady() [L415-417]
 private requireWasm() [L419-427]
 private get workspace() [L429-433]
 private get entry() [L435-439]
 private get frontmatter() [L441-445]
 private get search() [L447-451]
 private get template() [L453-457]
 private get validation() [L459-463]
 private get exportApi() [L465-469]
 private get attachment() [L471-475]
 private get filesystem() [L477-481]
 on( ) [L487-489]
 off( ) [L491-493]
 private setupIncrementalPersistence() [L499-581]
  const handlePersistenceEvent [L501-570]
   const storage [L504]
   const path [L511]
   const content [L512]
   const content [L527]
   const content [L536]
   const oldParentContent [L541]
   const newParentContent [L549]
   const content [L561]
 async getConfig() [L587-592]
 async saveConfig( ) [L594-597]
 async getWorkspaceTree( ) [L603-609]
  const path [L607]
 async createWorkspace( ) [L611-616]
  const workspacePath [L612]
  const workspaceName [L613]
 async getFilesystemTree( ) [L618-624]
  const path [L622]
 async getEntry( ) [L630-632]
 async saveEntry( ) [L634-644]
  const frontmatter [L638]
  type [L640]
 async createEntry( ) [L646-668]
  const workspaceRoot [L650]
  const normalizedPath [L651-654]
  const newPath [L655]
  const frontmatter [L658]
  const parentPath [L659]
  type [L661]
  path [L662]
 async deleteEntry( ) [L670-687]
  let parentPath [L672]
  const frontmatter [L674]
  type [L683]
 async moveEntry( ) [L689-718]
  let oldParent [L691]
  const oldFrontmatter [L693]
  const result [L699]
  let newParent [L702]
  const newFrontmatter [L704]
  type [L711]
  path [L712]
 async attachEntryToParent( ) [L720-757]
  const workspaceRoot [L724]
  const normalizedEntryPath [L725-728]
  const normalizedParentIndexPath [L729-732]
  let oldParent [L735]
  const oldFrontmatter [L737]
  const result [L743-746]
  type [L750]
  path [L751]
  newParent [L753]
 async convertToIndex( ) [L759-761]
 async convertToLeaf( ) [L763-765]
 async createChildEntry( ) [L767-769]
 slugifyTitle( ) [L771-774]
  const wasm [L772]
 async renameEntry( ) [L776-786]
  const newPath [L777]
  type [L780]
  oldPath [L781]
 async ensureDailyEntry() [L788-790]
 async getAvailableAudiences( ) [L796-798]
 async planExport( ) [L800-805]
 async exportToMemory( ) [L807-812]
 async exportToHtml( ) [L814-819]
 async exportBinaryAttachments( ) [L821-826]
 async getAttachments( ) [L832-834]
 async uploadAttachment( ) [L836-842]
 async deleteAttachment( ) [L844-849]
 async getStorageUsage() [L851-853]
 async getAttachmentData( ) [L855-860]
 async getFrontmatter( ) [L866-868]
 async setFrontmatterProperty( ) [L870-879]
 async removeFrontmatterProperty( ) [L881-884]
 async searchWorkspace( ) [L890-905]
  const wasmOptions [L894-902]
   workspace_path [L896-897]
   search_frontmatter [L898]
   property [L899]
   case_sensitive [L900]
   workspace_path [L902]
 async listTemplates() [L911-913]
 async getTemplate( ) [L915-917]
 async saveTemplate( ) [L919-925]
 async deleteTemplate( ) [L927-929]
 async validateWorkspace( ) [L935-940]
  const path [L938]
 async validateFile( ) [L942-946]
 async fixBrokenPartOf( ) [L948-964]
  const result [L951]
  const frontmatter [L955]
  type [L957]
  path [L958]
 async fixBrokenContentsRef( ) [L966-983]
  const result [L970]
  const frontmatter [L974]
  type [L976]
  path [L977]
  contents [L978]
 async fixBrokenAttachment( ) [L985-1002]
  const result [L989]
  const frontmatter [L993]
  type [L995]
  path [L996]
 async fixNonPortablePath( ) [L1004-1028]
  const result [L1010-1015]
  const frontmatter [L1019]
  type [L1021]
  path [L1022]
 async fixUnlistedFile( ) [L1030-1053]
  const result [L1034]
  const indexFm [L1038]
  type [L1040]
  path [L1041]
  contents [L1042]
  const fileFm [L1044]
  type [L1046]
  path [L1047]
  frontmatter [L1048]
 async fixOrphanBinaryFile( ) [L1055-1072]
  const result [L1059]
  const frontmatter [L1063]
  type [L1065]
  path [L1066]
 async fixMissingPartOf( ) [L1074-1091]
  const result [L1078]
  const frontmatter [L1082]
  type [L1084]
  path [L1085]
 async fixAll( ) [L1093-1097]
 async importFromZip( ) [L1103-1168]
  const totalBytes [L1108]
  const targetRoot [L1109-1113]
  const textEntries [L1120]
  const binaryEntries [L1120]
  const prefixedText [L1129-1132]
  const prefixedBinary [L1133-1137]
   path [L1135]
   data [L1136]
  success [L1155]
  files_imported [L1156]
  error [L1157]
  const message [L1160]
  success [L1163]
  files_imported [L1164]
  error [L1165]
 async persist() [L1174-1183]

Showing symbols 1-258 (total symbols: 258)


NEXT STEPS: To read a specific symbol's implementation, call read_file with the same path plus start_line and end_line from the outline above.
For example, to read a function shown as [L100-150], use start_line: 100 and end_line: 150.
```


**Tool Call: Read file `apps/web/src/lib/backend/interface.ts`**
Status: Completed

```diaryx-core/apps/web/src/lib/backend/interface.ts
SUCCESS: File outline retrieved. This file is too large to read all at once, so the outline below shows the file's structure with line numbers.

IMPORTANT: Do NOT retry this call without line numbers - you will get the same outline.
Instead, use the line numbers below to read specific sections by calling this tool again with start_line and end_line parameters.

# File outline for /Users/adamharris/Documents/diaryx-core/apps/web/src/lib/backend/interface.ts

interface Config [L7-13]
interface TreeNode [L15-20]
interface EntryData [L22-27]
interface SearchMatch [L29-34]
interface FileSearchResult [L36-40]
interface SearchResults [L42-45]
interface ValidationError [L48-54]
interface ValidationWarning [L56-77]
interface ValidationResult [L79-83]
interface FixResult [L86-89]
interface FixSummary [L91-96]
interface ExportPlan [L99-103]
interface ExportedFile [L105-108]
interface BinaryExportFile [L110-113]
interface StorageInfo [L115-119]
interface BackupStatus [L122-127]
interface BackupData [L129-134]
interface SearchOptions [L136-141]
interface CreateEntryOptions [L143-147]
interface TemplateInfo [L149-153]
interface ImportResult [L156-160]
type BackendEventType [L170-176]
interface FileCreatedEvent [L178-183]
interface FileDeletedEvent [L185-189]
interface FileRenamedEvent [L191-195]
interface FileMovedEvent [L197-202]
interface MetadataChangedEvent [L204-208]
interface ContentsChangedEvent [L210-214]
type BackendEvent [L216-222]
type BackendEventListener [L224]
interface Backend [L237-687]
class BackendError [L693-702]
 constructor( ) [L694-701]
function isTauri() [L711-713]
function isBrowser() [L718-720]

Showing symbols 1-35 (total symbols: 35)


NEXT STEPS: To read a specific symbol's implementation, call read_file with the same path plus start_line and end_line from the outline above.
For example, to read a function shown as [L100-150], use start_line: 100 and end_line: 150.
```


## Assistant

Let me examine the WASM module usage more closely, particularly what core functions it's calling:

**Tool Call: Read file `crates/diaryx_wasm/src/validation.rs`**
Status: Completed

```diaryx-core/crates/diaryx_wasm/src/validation.rs
//! Validation operations for WASM.

use std::path::PathBuf;

use diaryx_core::validate::{ValidationFixer, Validator};
use diaryx_core::workspace::Workspace;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::error::IntoJsResult;
use crate::state::{block_on, with_async_fs};

// ============================================================================
// Types
// ============================================================================

/// Validation error returned to JavaScript
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum JsValidationError {
    BrokenPartOf { file: String, target: String },
    BrokenContentsRef { index: String, target: String },
    BrokenAttachment { file: String, attachment: String },
}

impl From<diaryx_core::validate::ValidationError> for JsValidationError {
    fn from(err: diaryx_core::validate::ValidationError) -> Self {
        use diaryx_core::validate::ValidationError;
        match err {
            ValidationError::BrokenPartOf { file, target } => JsValidationError::BrokenPartOf {
                file: file.to_string_lossy().to_string(),
                target,
            },
            ValidationError::BrokenContentsRef { index, target } => {
                JsValidationError::BrokenContentsRef {
                    index: index.to_string_lossy().to_string(),
                    target,
                }
            }
            ValidationError::BrokenAttachment { file, attachment } => {
                JsValidationError::BrokenAttachment {
                    file: file.to_string_lossy().to_string(),
                    attachment,
                }
            }
        }
    }
}

/// Validation warning returned to JavaScript
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum JsValidationWarning {
    OrphanFile {
        file: String,
    },
    CircularReference {
        files: Vec<String>,
    },
    UnlinkedEntry {
        path: String,
        is_dir: bool,
    },
    UnlistedFile {
        index: String,
        file: String,
    },
    NonPortablePath {
        file: String,
        property: String,
        value: String,
        suggested: String,
    },
    MultipleIndexes {
        directory: String,
        indexes: Vec<String>,
    },
    OrphanBinaryFile {
        file: String,
        suggested_index: Option<String>,
    },
    MissingPartOf {
        file: String,
        suggested_index: Option<String>,
    },
}

impl From<diaryx_core::validate::ValidationWarning> for JsValidationWarning {
    fn from(warn: diaryx_core::validate::ValidationWarning) -> Self {
        use diaryx_core::validate::ValidationWarning;
        match warn {
            ValidationWarning::OrphanFile { file } => JsValidationWarning::OrphanFile {
                file: file.to_string_lossy().to_string(),
            },
            ValidationWarning::CircularReference { files } => {
                JsValidationWarning::CircularReference {
                    files: files
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect(),
                }
            }
            ValidationWarning::UnlinkedEntry { path, is_dir } => {
                JsValidationWarning::UnlinkedEntry {
                    path: path.to_string_lossy().to_string(),
                    is_dir,
                }
            }
            ValidationWarning::UnlistedFile { index, file } => JsValidationWarning::UnlistedFile {
                index: index.to_string_lossy().to_string(),
                file: file.to_string_lossy().to_string(),
            },
            ValidationWarning::NonPortablePath {
                file,
                property,
                value,
                suggested,
            } => JsValidationWarning::NonPortablePath {
                file: file.to_string_lossy().to_string(),
                property,
                value,
                suggested,
            },
            ValidationWarning::MultipleIndexes { directory, indexes } => {
                JsValidationWarning::MultipleIndexes {
                    directory: directory.to_string_lossy().to_string(),
                    indexes: indexes
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect(),
                }
            }
            ValidationWarning::OrphanBinaryFile {
                file,
                suggested_index,
            } => JsValidationWarning::OrphanBinaryFile {
                file: file.to_string_lossy().to_string(),
                suggested_index: suggested_index.map(|p| p.to_string_lossy().to_string()),
            },
            ValidationWarning::MissingPartOf {
                file,
                suggested_index,
            } => JsValidationWarning::MissingPartOf {
                file: file.to_string_lossy().to_string(),
                suggested_index: suggested_index.map(|p| p.to_string_lossy().to_string()),
            },
        }
    }
}

/// Validation result returned to JavaScript
#[derive(Debug, Serialize, Deserialize)]
pub struct JsValidationResult {
    pub errors: Vec<JsValidationError>,
    pub warnings: Vec<JsValidationWarning>,
    pub files_checked: usize,
}

/// Fix result returned to JavaScript
#[derive(Debug, Serialize, Deserialize)]
pub struct JsFixResult {
    pub success: bool,
    pub message: String,
}

impl From<diaryx_core::validate::FixResult> for JsFixResult {
    fn from(result: diaryx_core::validate::FixResult) -> Self {
        JsFixResult {
            success: result.success,
            message: result.message,
        }
    }
}

/// Summary of fix operations
#[derive(Debug, Serialize, Deserialize)]
pub struct JsFixSummary {
    pub error_fixes: Vec<JsFixResult>,
    pub warning_fixes: Vec<JsFixResult>,
    pub total_fixed: usize,
    pub total_failed: usize,
}

// ============================================================================
// DiaryxValidation Class
// ============================================================================

/// Validation operations for checking workspace integrity.
#[wasm_bindgen]
pub struct DiaryxValidation;

#[wasm_bindgen]
impl DiaryxValidation {
    /// Create a new DiaryxValidation instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Validate workspace links.
    #[wasm_bindgen]
    pub fn validate(&self, workspace_path: &str) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let validator = Validator::new(fs);
            let root_path = PathBuf::from(workspace_path);

            let ws = Workspace::new(fs);
            let root_index = block_on(ws.find_root_index_in_dir(&root_path))
                .js_err()?
                .or_else(|| block_on(ws.find_any_index_in_dir(&root_path)).ok().flatten())
                .ok_or_else(|| {
                    JsValue::from_str(&format!("No workspace found at '{}'", workspace_path))
                })?;

            let result = block_on(validator.validate_workspace(&root_index)).js_err()?;

            let js_result = JsValidationResult {
                errors: result
                    .errors
                    .into_iter()
                    .map(JsValidationError::from)
                    .collect(),
                warnings: result
                    .warnings
                    .into_iter()
                    .map(JsValidationWarning::from)
                    .collect(),
                files_checked: result.files_checked,
            };

            serde_wasm_bindgen::to_value(&js_result).js_err()
        })
    }

    /// Validate a single file's links.
    #[wasm_bindgen]
    pub fn validate_file(&self, file_path: &str) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let validator = Validator::new(fs);
            let path = PathBuf::from(file_path);

            let result = block_on(validator.validate_file(&path)).js_err()?;

            let js_result = JsValidationResult {
                errors: result
                    .errors
                    .into_iter()
                    .map(JsValidationError::from)
                    .collect(),
                warnings: result
                    .warnings
                    .into_iter()
                    .map(JsValidationWarning::from)
                    .collect(),
                files_checked: result.files_checked,
            };

            serde_wasm_bindgen::to_value(&js_result).js_err()
        })
    }

    /// Fix a broken part_of reference by removing it.
    #[wasm_bindgen]
    pub fn fix_broken_part_of(&self, file_path: &str) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let fixer = ValidationFixer::new(fs);
            let path = PathBuf::from(file_path);
            let result = block_on(fixer.fix_broken_part_of(&path));
            serde_wasm_bindgen::to_value(&JsFixResult::from(result)).js_err()
        })
    }

    /// Fix a broken contents reference by removing it.
    #[wasm_bindgen]
    pub fn fix_broken_contents_ref(
        &self,
        index_path: &str,
        target: &str,
    ) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let fixer = ValidationFixer::new(fs);
            let path = PathBuf::from(index_path);
            let result = block_on(fixer.fix_broken_contents_ref(&path, target));
            serde_wasm_bindgen::to_value(&JsFixResult::from(result)).js_err()
        })
    }

    /// Fix a broken attachment reference by removing it.
    #[wasm_bindgen]
    pub fn fix_broken_attachment(
        &self,
        file_path: &str,
        attachment: &str,
    ) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let fixer = ValidationFixer::new(fs);
            let path = PathBuf::from(file_path);
            let result = block_on(fixer.fix_broken_attachment(&path, attachment));
            serde_wasm_bindgen::to_value(&JsFixResult::from(result)).js_err()
        })
    }

    /// Fix a non-portable path by normalizing it.
    #[wasm_bindgen]
    pub fn fix_non_portable_path(
        &self,
        file_path: &str,
        property: &str,
        old_value: &str,
        new_value: &str,
    ) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let fixer = ValidationFixer::new(fs);
            let path = PathBuf::from(file_path);
            let result = block_on(fixer.fix_non_portable_path(&path, property, old_value, new_value));
            serde_wasm_bindgen::to_value(&JsFixResult::from(result)).js_err()
        })
    }

    /// Add an unlisted file to an index's contents.
    #[wasm_bindgen]
    pub fn fix_unlisted_file(&self, index_path: &str, file_path: &str) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let fixer = ValidationFixer::new(fs);
            let index = PathBuf::from(index_path);
            let file = PathBuf::from(file_path);
            let result = block_on(fixer.fix_unlisted_file(&index, &file));
            serde_wasm_bindgen::to_value(&JsFixResult::from(result)).js_err()
        })
    }

    /// Add an orphan binary file to an index's attachments.
    #[wasm_bindgen]
    pub fn fix_orphan_binary_file(
        &self,
        index_path: &str,
        file_path: &str,
    ) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let fixer = ValidationFixer::new(fs);
            let index = PathBuf::from(index_path);
            let file = PathBuf::from(file_path);
            let result = block_on(fixer.fix_orphan_binary_file(&index, &file));
            serde_wasm_bindgen::to_value(&JsFixResult::from(result)).js_err()
        })
    }

    /// Fix a missing part_of by setting it to point to the given index.
    #[wasm_bindgen]
    pub fn fix_missing_part_of(
        &self,
        file_path: &str,
        index_path: &str,
    ) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let fixer = ValidationFixer::new(fs);
            let file = PathBuf::from(file_path);
            let index = PathBuf::from(index_path);
            let result = block_on(fixer.fix_missing_part_of(&file, &index));
            serde_wasm_bindgen::to_value(&JsFixResult::from(result)).js_err()
        })
    }

    /// Fix all errors and fixable warnings in a validation result.
    ///
    /// Takes a validation result (from validate or validate_file) and attempts
    /// to fix all issues.
    #[wasm_bindgen]
    pub fn fix_all(&self, validation_result: JsValue) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let fixer = ValidationFixer::new(fs);

            // Parse the JS validation result
            let js_result: JsValidationResult =
                serde_wasm_bindgen::from_value(validation_result).js_err()?;

            // Convert back to core types for fixing
            let mut error_fixes = Vec::new();
            let mut warning_fixes = Vec::new();

            // Fix errors
            for err in &js_result.errors {
                let result = match err {
                    JsValidationError::BrokenPartOf { file, target: _ } => {
                        block_on(fixer.fix_broken_part_of(&PathBuf::from(file)))
                    }
                    JsValidationError::BrokenContentsRef { index, target } => {
                        block_on(fixer.fix_broken_contents_ref(&PathBuf::from(index), target))
                    }
                    JsValidationError::BrokenAttachment { file, attachment } => {
                        block_on(fixer.fix_broken_attachment(&PathBuf::from(file), attachment))
                    }
                };
                error_fixes.push(JsFixResult::from(result));
            }

            // Fix warnings
            for warn in &js_result.warnings {
                let result = match warn {
                    JsValidationWarning::UnlistedFile { index, file } => {
                        Some(block_on(fixer.fix_unlisted_file(&PathBuf::from(index), &PathBuf::from(file))))
                    }
                    JsValidationWarning::NonPortablePath {
                        file,
                        property,
                        value,
                        suggested,
                    } => Some(block_on(fixer.fix_non_portable_path(
                        &PathBuf::from(file),
                        property,
                        value,
                        suggested,
                    ))),
                    JsValidationWarning::OrphanBinaryFile {
                        file,
                        suggested_index,
                    } => suggested_index.as_ref().map(|index| {
                        block_on(fixer.fix_orphan_binary_file(&PathBuf::from(index), &PathBuf::from(file)))
                    }),
                    JsValidationWarning::MissingPartOf {
                        file,
                        suggested_index,
                    } => suggested_index.as_ref().map(|index| {
                        block_on(fixer.fix_missing_part_of(&PathBuf::from(file), &PathBuf::from(index)))
                    }),
                    // These cannot be auto-fixed
                    JsValidationWarning::OrphanFile { .. }
                    | JsValidationWarning::UnlinkedEntry { .. }
                    | JsValidationWarning::CircularReference { .. }
                    | JsValidationWarning::MultipleIndexes { .. } => None,
                };

                if let Some(r) = result {
                    warning_fixes.push(JsFixResult::from(r));
                }
            }

            let total_fixed = error_fixes.iter().filter(|r| r.success).count()
                + warning_fixes.iter().filter(|r| r.success).count();
            let total_failed = error_fixes.iter().filter(|r| !r.success).count()
                + warning_fixes.iter().filter(|r| !r.success).count();

            let summary = JsFixSummary {
                error_fixes,
                warning_fixes,
                total_fixed,
                total_failed,
            };

            serde_wasm_bindgen::to_value(&summary).js_err()
        })
    }
}

impl Default for DiaryxValidation {
    fn default() -> Self {
        Self::new()
    }
}

```


**Tool Call: Read file `crates/diaryx_wasm/src/workspace.rs`**
Status: Completed

```diaryx-core/crates/diaryx_wasm/src/workspace.rs
//! Workspace operations for WASM.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use diaryx_core::fs::{AsyncFileSystem, FileSystem};
use diaryx_core::workspace::Workspace;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::error::IntoJsResult;
use crate::state::{block_on, with_async_fs, with_fs};

// ============================================================================
// Types
// ============================================================================

/// Tree node returned to JavaScript
#[derive(Debug, Serialize)]
pub struct JsTreeNode {
    pub name: String,
    pub description: Option<String>,
    pub path: String,
    pub children: Vec<JsTreeNode>,
}

impl From<diaryx_core::workspace::TreeNode> for JsTreeNode {
    fn from(node: diaryx_core::workspace::TreeNode) -> Self {
        JsTreeNode {
            name: node.name,
            description: node.description,
            path: node.path.to_string_lossy().to_string(),
            children: node.children.into_iter().map(JsTreeNode::from).collect(),
        }
    }
}

// ============================================================================
// DiaryxWorkspace Class
// ============================================================================

/// Workspace operations for managing workspace structure.
#[wasm_bindgen]
pub struct DiaryxWorkspace;

#[wasm_bindgen]
impl DiaryxWorkspace {
    /// Create a new DiaryxWorkspace instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Get the workspace tree structure.
    #[wasm_bindgen]
    pub fn get_tree(&self, workspace_path: &str, depth: Option<u32>) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let ws = Workspace::new(fs);
            let root_path = PathBuf::from(workspace_path);

            // Find root index in the workspace
            let root_index = block_on(ws.find_root_index_in_dir(&root_path))
                .js_err()?
                .or_else(|| block_on(ws.find_any_index_in_dir(&root_path)).ok().flatten())
                .ok_or_else(|| {
                    JsValue::from_str(&format!("No workspace found at '{}'", workspace_path))
                })?;

            let max_depth = depth.map(|d| d as usize);
            let mut visited = HashSet::new();

            let tree = block_on(ws.build_tree_with_depth(&root_index, max_depth, &mut visited))
                .js_err()?;

            let js_tree: JsTreeNode = tree.into();
            serde_wasm_bindgen::to_value(&js_tree).js_err()
        })
    }

    /// Initialize a new workspace with an index.md file.
    #[wasm_bindgen]
    pub fn create(&self, path: &str, name: &str) -> Result<(), JsValue> {
        use crate::state::with_fs_mut;

        with_fs_mut(|fs| {
            let index_path = PathBuf::from(path).join("index.md");

            if fs.exists(&index_path) {
                return Err(JsValue::from_str(&format!(
                    "Workspace already exists at '{}'",
                    path
                )));
            }

            let content = format!(
                "---\ntitle: \"{}\"\ncontents: []\n---\n\n# {}\n",
                name, name
            );

            fs.write_file(&index_path, &content).js_err()
        })
    }

    /// Get the filesystem tree structure (for "Show All Files" mode).
    #[wasm_bindgen]
    pub fn get_filesystem_tree(
        &self,
        workspace_path: &str,
        show_hidden: bool,
    ) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let root_path = PathBuf::from(workspace_path);

            fn build_tree<FS: AsyncFileSystem>(
                fs: &FS,
                path: &Path,
                show_hidden: bool,
            ) -> Result<JsTreeNode, String> {
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string());

                if !show_hidden && name.starts_with('.') {
                    return Err("hidden".to_string());
                }

                let mut children = Vec::new();

                if block_on(fs.is_dir(path)) {
                    if let Ok(entries) = block_on(fs.list_files(path)) {
                        for entry in entries {
                            if let Ok(child) = build_tree(fs, &entry, show_hidden) {
                                children.push(child);
                            }
                        }
                    }
                    children.sort_by(|a, b| {
                        let a_is_dir = !a.children.is_empty();
                        let b_is_dir = !b.children.is_empty();
                        match (a_is_dir, b_is_dir) {
                            (true, false) => std::cmp::Ordering::Less,
                            (false, true) => std::cmp::Ordering::Greater,
                            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                        }
                    });
                }

                Ok(JsTreeNode {
                    name,
                    description: None,
                    path: path.to_string_lossy().to_string(),
                    children,
                })
            }

            let tree =
                build_tree(&fs, &root_path, show_hidden).map_err(|e| JsValue::from_str(&e))?;

            serde_wasm_bindgen::to_value(&tree).js_err()
        })
    }
}

impl Default for DiaryxWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

```


**Tool Call: Thinking**
Status: Completed

Looking at the current state of the codebase:

1. **diaryx_core** is async-first - all core modules (Workspace, Validator, Exporter, Searcher, Publisher) use `AsyncFileSystem` trait.

2. **diaryx_wasm** currently uses:
   - `InMemoryFileSystem` (synchronous) wrapped with `SyncToAsyncFs` adapter
   - A custom `block_on` function to run async futures synchronously in WASM
   - The `DiaryxAsyncFilesystem` class provides Promise-based wrappers for the filesystem operations

3. The user wants to implement `AsyncFileSystem` natively in WASM with JavaScript async/await/promises to simplify the web app.

The current approach in diaryx_wasm is:
- `state.rs` has a global `InMemoryFileSystem` 
- `with_async_fs` wraps it in `SyncToAsyncFs`
- `block_on` is used to run async operations synchronously
- `DiaryxAsyncFilesystem` uses `future_to_promise` to expose Promise-based APIs to JavaScript

The problem with this approach is that it's complex and the web app (apps/web) still has to deal with the synchronous in-memory filesystem and manage persistence to IndexedDB separately.

A better approach would be to:
1. Create a native `AsyncFileSystem` implementation backed by IndexedDB directly in Rust/WASM
2. OR create a JavaScript-backed `AsyncFileSystem` implementation that calls into JS for all operations

Let me look at what options make the most sense:

Option A: IndexedDB-backed AsyncFileSystem in Rust
- Would use `idb` crate or similar
- All filesystem operations would be truly async
- Would eliminate the need for the in-memory filesystem entirely for WASM
- Complexity: Medium-High, requires learning IndexedDB crate

Option B: JavaScript-callback-backed AsyncFileSystem
- Rust would call into JavaScript for filesystem operations
- JavaScript could use IndexedDB, localStorage, or any storage backend
- Would require wasm-bindgen imports
- More flexible for the JavaScript side

Option C: Keep InMemoryFileSystem but expose all core operations via wasm-bindgen-futures Promises
- This is partially what DiaryxAsyncFilesystem does
- But currently validation, workspace, search etc. still use block_on internally

Looking at the conversation context, the user wants to simplify apps/web by having diaryx_wasm implement AsyncFileSystem natively with JS async/await/promises.

The best approach seems to be:
1. Create a JsAsyncFileSystem that implements AsyncFileSystem
2. This implementation would call into JavaScript callbacks for each operation
3. JavaScript can then use IndexedDB, localStorage, etc.

But wait - there's a simpler approach. Looking at the current code:
- `DiaryxAsyncFilesystem` already provides Promise-based APIs for basic filesystem ops
- But the higher-level operations (validate, search, export, workspace) use `block_on` internally

The simplest improvement would be:
1. Update all WASM modules (validation, workspace, search, export) to return Promises directly
2. Use `wasm_bindgen_futures::spawn_local` or `future_to_promise` instead of `block_on`

Actually, looking more carefully:
- The `block_on` in WASM is custom-built and works because `SyncToAsyncFs` futures complete immediately
- So the current approach IS working, it's just that everything is synchronous under the hood

If we want true async (for future IndexedDB integration), we'd need:
1. A proper AsyncFileSystem implementation backed by JS promises
2. Replace all `block_on` calls with `future_to_promise` or similar

Let me create a plan:

1. Create `JsAsyncFileSystem` - an AsyncFileSystem implementation that delegates to JavaScript
2. The JavaScript side will provide implementations for read/write/delete/list etc.
3. Update the WASM modules to optionally use this instead of the in-memory filesystem
4. This allows the web app to use IndexedDB directly or any other storage backend

Actually, let me re-read the user's request more carefully: "make diaryx_wasm implement the Async filesystem trait natively (with javascript async/await/promises) so we can greatly simplify apps/web"

I think what they want is:
1. The WASM layer exposes async functions that return Promises
2. The web app can just await these promises
3. The complexity of managing IndexedDB persistence is handled in the WASM layer or pushed to JS callbacks

Let me look at what the web app currently does and how it could be simplified.

Looking at wasm.ts in apps/web:
- It loads wasm module
- Loads files from IndexedDB into InMemoryFileSystem on init
- Saves files back to IndexedDB on persist()
- Has event handling for incremental persistence

The simplification would be:
1. WASM module provides async APIs that return Promises
2. WASM module internally uses a JS-backed AsyncFileSystem 
3. The JS backend (provided by web app) handles IndexedDB directly

Let me design this:

1. Create `JsAsyncFileSystem` struct that holds JS callbacks
2. Each method calls the corresponding JS callback
3. The web app provides the callbacks when creating the filesystem
4. All WASM operations become truly async, returning Promises

Let me start implementing this.


**Tool Call: Read file `apps/web/src/lib/backend/wasm.ts` (lines 340-430)**
Status: Completed

```diaryx-core/apps/web/src/lib/backend/wasm.ts
  async init(passedStorageType?: StorageType): Promise<void> {
    if (this.ready) return;

    console.log("[WasmBackend] Initializing...");

    // Load WASM module
    this.wasm = await loadWasm();

    // Initialize typed class instances
    this._workspace = new this.wasm.DiaryxWorkspace();
    this._entry = new this.wasm.DiaryxEntry();
    this._frontmatter = new this.wasm.DiaryxFrontmatter();
    this._search = new this.wasm.DiaryxSearch();
    this._template = new this.wasm.DiaryxTemplate();
    this._validation = new this.wasm.DiaryxValidation();
    this._export = new this.wasm.DiaryxExport();
    this._attachment = new this.wasm.DiaryxAttachment();
    this._filesystem = new this.wasm.DiaryxFilesystem();

    // Open IndexedDB (for config storage)
    await this.storage.open();

    // Initialize incremental storage via factory (OPFS or IndexedDB based on user selection)
    // Use passed storageType if available (essential for Workers where localStorage is missing)
    const storageType = passedStorageType ?? getStorageType();
    console.log(`[WasmBackend] Using storage type: ${storageType}`);
    this.incrementalStorage = await createStorage(storageType);

    // Set up internal event listener for immediate persistence
    this.setupIncrementalPersistence();

    // Load files from selected storage backend into WASM memory
    const storage = this.incrementalStorage as any;
    if (typeof storage.loadAllFiles === 'function') {
      const files = await storage.loadAllFiles();
      const entries: [string, string][] = files.map((f: any) => [f.path, f.content]);
      this._filesystem.load_files(entries);
      console.log(`[WasmBackend] Loaded ${files.length} text files from ${storageType}`);
      
      if (typeof storage.loadAllBinaryFiles === 'function') {
        try {
          const binaryFiles = await storage.loadAllBinaryFiles();
          if (binaryFiles.length > 0) {
            const binaryEntries = binaryFiles.map((f: any) => ({
              path: f.path,
              data: Array.from(f.data),
            }));
            this.wasm.load_binary_files(binaryEntries);
            console.log(`[WasmBackend] Loaded ${binaryFiles.length} binary files from ${storageType}`);
          }
        } catch (e) {
          console.warn('[WasmBackend] Could not load binary files:', e);
        }
      }
    } else {
      console.error(`[WasmBackend] Storage ${storageType} does not support bulk loading!`);
    }

    // Load config from IndexedDB (config is currently always in IndexedDB)
    this.config = await this.storage.loadConfig();

    // Create default config and workspace if none exists
    if (!this.config) {
      this.config = { default_workspace: "workspace" };
      await this.storage.saveConfig(this.config);

      // Create default workspace
      this._workspace.create("workspace", "My Workspace");
      await this.persist();
    }

    this.ready = true;
    console.log("[WasmBackend] Initialization complete");
  }

  isReady(): boolean {
    return this.ready;
  }

  private requireWasm(): WasmModule {
    if (!this.wasm) {
      throw new BackendError(
        "WASM module not loaded. Call init() first.",
        "NotInitialized",
      );
    }
    return this.wasm;
  }

  private get workspace() {
    if (!this._workspace)

```


**Tool Call: Read file `crates/diaryx_wasm/Cargo.toml`**
Status: Completed

```diaryx-core/crates/diaryx_wasm/Cargo.toml
[package]
name = "diaryx_wasm"
version.workspace = true
edition.workspace = true
repository.workspace = true
description = "WebAssembly bindings for Diaryx core functionality"
license-file.workspace = true
publish = false

[lib]
crate-type = ["cdylib", "rlib"]
doctest = false

[features]
default = ["console_error_panic_hook"]

[dependencies]
diaryx_core = { path = "../diaryx_core" }
wasm-bindgen = "0.2"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
serde_yaml = "0.9"
serde-wasm-bindgen = "0.6"
js-sys = "0.3"
wasm-bindgen-futures = "0.4"
regex = "1.10"
chrono = { version = "0.4", default-features = false, features = ["std", "clock", "wasmbind"] }
pathdiff = "0.2"
comrak = "0.49"

# The `console_error_panic_hook` crate provides better debugging of panics by
# logging them with `console.error`. This is great for development, but requires
# all the `std::fmt` and `std::panicking` infrastructure, so isn't great for
# code size when deploying.
console_error_panic_hook = { version = "0.1", optional = true }

[dev-dependencies]
wasm-bindgen-test = "0.3"

[profile.release]
# Tell `rustc` to optimize for small code size.
opt-level = "s"
lto = true

```


## Assistant

Now I have a good understanding of the architecture. Let me design a solution that implements `AsyncFileSystem` natively with JavaScript callbacks. This will allow the web app to provide its own storage backend (IndexedDB, OPFS, etc.) directly.

**Tool Call: crates/diaryx_wasm/src/js_async_fs.rs**
Status: Completed

Diff: crates/diaryx_wasm/src/js_async_fs.rs
```
//! JavaScript-backed AsyncFileSystem implementation.
//!
//! This module provides `JsAsyncFileSystem`, which implements the `AsyncFileSystem` trait
//! by delegating all operations to JavaScript callbacks. This allows the web frontend
//! to provide its own storage backend (IndexedDB, OPFS, localStorage, etc.) while
//! the Rust/WASM code uses the standard async filesystem interface.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { JsAsyncFileSystem } from './wasm/diaryx_wasm.js';
//!
//! // Create filesystem with JavaScript callbacks
//! const fs = new JsAsyncFileSystem({
//!   readToString: async (path) => {
//!     const data = await indexedDB.get(path);
//!     return data?.content;
//!   },
//!   writeFile: async (path, content) => {
//!     await indexedDB.put({ path, content });
//!   },
//!   deleteFile: async (path) => {
//!     await indexedDB.delete(path);
//!   },
//!   exists: async (path) => {
//!     return await indexedDB.has(path);
//!   },
//!   isDir: async (path) => {
//!     return path.endsWith('/') || await hasChildren(path);
//!   },
//!   listFiles: async (dir) => {
//!     return await indexedDB.listDir(dir);
//!   },
//!   listMdFiles: async (dir) => {
//!     const files = await indexedDB.listDir(dir);
//!     return files.filter(f => f.endsWith('.md'));
//!   },
//!   createDirAll: async (path) => {
//!     // No-op for flat storage, or create directory markers
//!   },
//!   moveFile: async (from, to) => {
//!     const content = await indexedDB.get(from);
//!     await indexedDB.put({ path: to, content: content.content });
//!     await indexedDB.delete(from);
//!   },
//!   readBinary: async (path) => {
//!     const data = await indexedDB.get(path);
//!     return new Uint8Array(data?.binary);
//!   },
//!   writeBinary: async (path, data) => {
//!     await indexedDB.put({ path, binary: Array.from(data) });
//!   },
//! });
//!
//! // Now use fs with async WASM operations
//! const workspace = new DiaryxAsyncWorkspace(fs);
//! const tree = await workspace.getTree('workspace');
//! ```

use std::cell::RefCell;
use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::rc::Rc;

use diaryx_core::fs::{AsyncFileSystem, BoxFuture};
use js_sys::{Array, Function, Promise, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

// ============================================================================
// JavaScript Callback Interface
// ============================================================================

/// JavaScript callbacks for filesystem operations.
///
/// All callbacks are optional. If a callback is not provided, the operation
/// will return an appropriate error or default value.
#[wasm_bindgen]
extern "C" {
    /// JavaScript object containing filesystem callbacks.
    #[wasm_bindgen(typescript_type = "JsFileSystemCallbacks")]
    pub type JsFileSystemCallbacks;

    #[wasm_bindgen(method, getter, js_name = "readToString")]
    fn read_to_string_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "writeFile")]
    fn write_file_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "createNew")]
    fn create_new_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "deleteFile")]
    fn delete_file_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "exists")]
    fn exists_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "isDir")]
    fn is_dir_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "listFiles")]
    fn list_files_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "listMdFiles")]
    fn list_md_files_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "createDirAll")]
    fn create_dir_all_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "moveFile")]
    fn move_file_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "readBinary")]
    fn read_binary_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "writeBinary")]
    fn write_binary_cb(this: &JsFileSystemCallbacks) -> Option<Function>;
}

// ============================================================================
// JsAsyncFileSystem Implementation
// ============================================================================

/// An `AsyncFileSystem` implementation backed by JavaScript callbacks.
///
/// This struct allows Rust code to use the async filesystem interface while
/// delegating actual storage operations to JavaScript. This is useful for:
///
/// - Using IndexedDB for persistent storage in browsers
/// - Using OPFS (Origin Private File System) for better performance
/// - Integrating with existing JavaScript storage solutions
/// - Testing with mock filesystems
///
/// ## Thread Safety
///
/// This type is designed for single-threaded WASM environments. It uses `Rc<RefCell<>>`
/// internally for shared ownership of the callbacks.
#[wasm_bindgen]
pub struct JsAsyncFileSystem {
    callbacks: Rc<RefCell<JsValue>>,
}

#[wasm_bindgen]
impl JsAsyncFileSystem {
    /// Create a new JsAsyncFileSystem with the provided callbacks.
    ///
    /// The callbacks object should implement the `JsFileSystemCallbacks` interface.
    /// All callbacks are optional - missing callbacks will cause operations to fail
    /// with appropriate errors.
    #[wasm_bindgen(constructor)]
    pub fn new(callbacks: JsValue) -> Self {
        Self {
            callbacks: Rc::new(RefCell::new(callbacks)),
        }
    }

    /// Check if the filesystem has a specific callback.
    #[wasm_bindgen]
    pub fn has_callback(&self, name: &str) -> bool {
        let callbacks = self.callbacks.borrow();
        if let Ok(obj) = js_sys::Reflect::get(&callbacks, &JsValue::from_str(name)) {
            obj.is_function()
        } else {
            false
        }
    }
}

impl Clone for JsAsyncFileSystem {
    fn clone(&self) -> Self {
        Self {
            callbacks: Rc::clone(&self.callbacks),
        }
    }
}

// Helper function to convert JsValue error to io::Error
fn js_to_io_error(err: JsValue) -> Error {
    let msg = if let Some(s) = err.as_string() {
        s
    } else if let Some(obj) = err.dyn_ref::<js_sys::Object>() {
        obj.to_string().as_string().unwrap_or_else(|| "Unknown JS error".to_string())
    } else {
        "Unknown JS error".to_string()
    };
    Error::new(ErrorKind::Other, msg)
}

// Helper function to get a callback from the callbacks object
fn get_callback(callbacks: &JsValue, name: &str) -> Option<Function> {
    js_sys::Reflect::get(callbacks, &JsValue::from_str(name))
        .ok()
        .and_then(|v| v.dyn_into::<Function>().ok())
}

// Helper to call a JS callback that returns a Promise
async fn call_async_callback(
    callbacks: &JsValue,
    name: &str,
    args: &[JsValue],
) -> std::result::Result<JsValue, Error> {
    let callback = get_callback(callbacks, name)
        .ok_or_else(|| Error::new(ErrorKind::Unsupported, format!("Callback '{}' not provided", name)))?;

    let this = JsValue::NULL;
    let result = match args.len() {
        0 => callback.call0(&this),
        1 => callback.call1(&this, &args[0]),
        2 => callback.call2(&this, &args[0], &args[1]),
        3 => callback.call3(&this, &args[0], &args[1], &args[2]),
        _ => {
            let js_args = Array::new();
            for arg in args {
                js_args.push(arg);
            }
            callback.apply(&this, &js_args)
        }
    }
    .map_err(js_to_io_error)?;

    // If result is a Promise, await it
    if result.has_type::<Promise>() {
        let promise: Promise = result.unchecked_into();
        JsFuture::from(promise).await.map_err(js_to_io_error)
    } else {
        Ok(result)
    }
}

// ============================================================================
// AsyncFileSystem Implementation
// ============================================================================

// Note: WASM is single-threaded, but the trait requires Send + Sync.
// We use unsafe impl since we know this will only be used in single-threaded WASM.
unsafe impl Send for JsAsyncFileSystem {}
unsafe impl Sync for JsAsyncFileSystem {}

impl AsyncFileSystem for JsAsyncFileSystem {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        let callbacks = Rc::clone(&self.callbacks);
        let path_str = path.to_string_lossy().to_string();

        Box::pin(async move {
            let callbacks = callbacks.borrow();
            let result = call_async_callback(
                &callbacks,
                "readToString",
                &[JsValue::from_str(&path_str)],
            )
            .await?;

            result
                .as_string()
                .ok_or_else(|| Error::new(ErrorKind::InvalidData, "readToString did not return a string"))
        })
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        let callbacks = Rc::clone(&self.callbacks);
        let path_str = path.to_string_lossy().to_string();
        let content = content.to_string();

        Box::pin(async move {
            let callbacks = callbacks.borrow();
            call_async_callback(
                &callbacks,
                "writeFile",
                &[JsValue::from_str(&path_str), JsValue::from_str(&content)],
            )
            .await?;
            Ok(())
        })
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        let callbacks = Rc::clone(&self.callbacks);
        let path_str = path.to_string_lossy().to_string();
        let content = content.to_string();

        Box::pin(async move {
            let callbacks = callbacks.borrow();
            
            // Check if createNew callback exists
            if get_callback(&callbacks, "createNew").is_some() {
                call_async_callback(
                    &callbacks,
                    "createNew",
                    &[JsValue::from_str(&path_str), JsValue::from_str(&content)],
                )
                .await?;
                Ok(())
            } else {
                // Fall back to exists + writeFile
                let exists_result = call_async_callback(
                    &callbacks,
                    "exists",
                    &[JsValue::from_str(&path_str)],
                )
                .await?;

                if exists_result.as_bool().unwrap_or(false) {
                    return Err(Error::new(ErrorKind::AlreadyExists, format!("File already exists: {}", path_str)));
                }

                call_async_callback(
                    &callbacks,
                    "writeFile",
                    &[JsValue::from_str(&path_str), JsValue::from_str(&content)],
                )
                .await?;
                Ok(())
            }
        })
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        let callbacks = Rc::clone(&self.callbacks);
        let path_str = path.to_string_lossy().to_string();

        Box::pin(async move {
            let callbacks = callbacks.borrow();
            call_async_callback(
                &callbacks,
                "deleteFile",
                &[JsValue::from_str(&path_str)],
            )
            .await?;
            Ok(())
        })
    }

    fn list_md_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        let callbacks = Rc::clone(&self.callbacks);
        let dir_str = dir.to_string_lossy().to_string();

        Box::pin(async move {
            let callbacks = callbacks.borrow();
            let result = call_async_callback(
                &callbacks,
                "listMdFiles",
                &[JsValue::from_str(&dir_str)],
            )
            .await?;

            parse_path_array(result)
        })
    }

    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        let callbacks = Rc::clone(&self.callbacks);
        let path_str = path.to_string_lossy().to_string();

        Box::pin(async move {
            let callbacks = callbacks.borrow();
            let result = call_async_callback(
                &callbacks,
                "exists",
                &[JsValue::from_str(&path_str)],
            )
            .await;

            match result {
                Ok(v) => v.as_bool().unwrap_or(false),
                Err(_) => false,
            }
        })
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        let callbacks = Rc::clone(&self.callbacks);
        let path_str = path.to_string_lossy().to_string();

        Box::pin(async move {
            let callbacks = callbacks.borrow();
            
            // createDirAll is optional - many storage backends don't need it
            if get_callback(&callbacks, "createDirAll").is_some() {
                call_async_callback(
                    &callbacks,
                    "createDirAll",
                    &[JsValue::from_str(&path_str)],
                )
                .await?;
            }
            Ok(())
        })
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        let callbacks = Rc::clone(&self.callbacks);
        let path_str = path.to_string_lossy().to_string();

        Box::pin(async move {
            let callbacks = callbacks.borrow();
            let result = call_async_callback(
                &callbacks,
                "isDir",
                &[JsValue::from_str(&path_str)],
            )
            .await;

            match result {
                Ok(v) => v.as_bool().unwrap_or(false),
                Err(_) => false,
            }
        })
    }

    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        let callbacks = Rc::clone(&self.callbacks);
        let from_str = from.to_string_lossy().to_string();
        let to_str = to.to_string_lossy().to_string();

        Box::pin(async move {
            let callbacks = callbacks.borrow();
            call_async_callback(
                &callbacks,
                "moveFile",
                &[JsValue::from_str(&from_str), JsValue::from_str(&to_str)],
            )
            .await?;
            Ok(())
        })
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        let callbacks = Rc::clone(&self.callbacks);
        let path_str = path.to_string_lossy().to_string();

        Box::pin(async move {
            let callbacks = callbacks.borrow();
            let result = call_async_callback(
                &callbacks,
                "readBinary",
                &[JsValue::from_str(&path_str)],
            )
            .await?;

            // Handle Uint8Array or Array
            if let Some(uint8_array) = result.dyn_ref::<Uint8Array>() {
                Ok(uint8_array.to_vec())
            } else if let Some(array) = result.dyn_ref::<Array>() {
                let mut bytes = Vec::with_capacity(array.length() as usize);
                for i in 0..array.length() {
                    let val = array.get(i);
                    let byte = val.as_f64().unwrap_or(0.0) as u8;
                    bytes.push(byte);
                }
                Ok(bytes)
            } else {
                Err(Error::new(ErrorKind::InvalidData, "readBinary did not return a Uint8Array or Array"))
            }
        })
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        let callbacks = Rc::clone(&self.callbacks);
        let path_str = path.to_string_lossy().to_string();
        let content = content.to_vec();

        Box::pin(async move {
            let callbacks = callbacks.borrow();
            
            // Convert bytes to Uint8Array
            let uint8_array = Uint8Array::new_with_length(content.len() as u32);
            uint8_array.copy_from(&content);

            call_async_callback(
                &callbacks,
                "writeBinary",
                &[JsValue::from_str(&path_str), uint8_array.into()],
            )
            .await?;
            Ok(())
        })
    }

    fn list_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        let callbacks = Rc::clone(&self.callbacks);
        let dir_str = dir.to_string_lossy().to_string();

        Box::pin(async move {
            let callbacks = callbacks.borrow();
            let result = call_async_callback(
                &callbacks,
                "listFiles",
                &[JsValue::from_str(&dir_str)],
            )
            .await?;

            parse_path_array(result)
        })
    }
}

// Helper function to parse a JS array of strings into Vec<PathBuf>
fn parse_path_array(value: JsValue) -> Result<Vec<PathBuf>> {
    if let Some(array) = value.dyn_ref::<Array>() {
        let mut paths = Vec::with_capacity(array.length() as usize);
        for i in 0..array.length() {
            let item = array.get(i);
            if let Some(s) = item.as_string() {
                paths.push(PathBuf::from(s));
            }
        }
        Ok(paths)
    } else {
        Err(Error::new(ErrorKind::InvalidData, "Expected array of strings"))
    }
}

// ============================================================================
// TypeScript Type Definitions
// ============================================================================

#[wasm_bindgen(typescript_custom_section)]
const TS_APPEND_CONTENT: &'static str = r#"
/**
 * Callbacks for JsAsyncFileSystem operations.
 * 
 * All callbacks should return Promises. If a callback is not provided,
 * the corresponding operation will fail with an error.
 */
export interface JsFileSystemCallbacks {
    /**
     * Read a file's content as a string.
     * @param path - The file path to read
     * @returns Promise resolving to the file content
     */
    readToString?: (path: string) => Promise<string>;
    
    /**
     * Write content to a file, creating or overwriting it.
     * @param path - The file path to write
     * @param content - The content to write
     */
    writeFile?: (path: string, content: string) => Promise<void>;
    
    /**
     * Create a new file, failing if it already exists.
     * @param path - The file path to create
     * @param content - The initial content
     */
    createNew?: (path: string, content: string) => Promise<void>;
    
    /**
     * Delete a file.
     * @param path - The file path to delete
     */
    deleteFile?: (path: string) => Promise<void>;
    
    /**
     * Check if a path exists.
     * @param path - The path to check
     * @returns Promise resolving to true if the path exists
     */
    exists?: (path: string) => Promise<boolean>;
    
    /**
     * Check if a path is a directory.
     * @param path - The path to check
     * @returns Promise resolving to true if the path is a directory
     */
    isDir?: (path: string) => Promise<boolean>;
    
    /**
     * List all files in a directory (not recursive).
     * @param dir - The directory path
     * @returns Promise resolving to array of file paths
     */
    listFiles?: (dir: string) => Promise<string[]>;
    
    /**
     * List markdown files in a directory (not recursive).
     * @param dir - The directory path
     * @returns Promise resolving to array of .md file paths
     */
    listMdFiles?: (dir: string) => Promise<string[]>;
    
    /**
     * Create a directory and all parent directories.
     * @param path - The directory path to create
     */
    createDirAll?: (path: string) => Promise<void>;
    
    /**
     * Move/rename a file.
     * @param from - The source path
     * @param to - The destination path
     */
    moveFile?: (from: string, to: string) => Promise<void>;
    
    /**
     * Read binary file content.
     * @param path - The file path to read
     * @returns Promise resolving to file content as Uint8Array
     */
    readBinary?: (path: string) => Promise<Uint8Array>;
    
    /**
     * Write binary content to a file.
     * @param path - The file path to write
     * @param data - The binary content as Uint8Array
     */
    writeBinary?: (path: string, data: Uint8Array) => Promise<void>;
}
"#;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_js_async_filesystem_creation() {
        // Just verify the struct can be created with a null JsValue
        let _fs = JsAsyncFileSystem::new(JsValue::NULL);
    }

    #[test]
    fn test_js_async_filesystem_clone() {
        let fs1 = JsAsyncFileSystem::new(JsValue::NULL);
        let fs2 = fs1.clone();
        // Both should share the same underlying callbacks
        assert!(Rc::ptr_eq(&fs1.callbacks, &fs2.callbacks));
    }
}
```


## Assistant

Now let's create the async workspace module that uses `JsAsyncFileSystem`:

**Tool Call: crates/diaryx_wasm/src/async_workspace.rs**
Status: Completed

Diff: crates/diaryx_wasm/src/async_workspace.rs
```
//! Async workspace operations for WASM with native Promise support.
//!
//! This module provides async workspace operations that work directly with
//! `JsAsyncFileSystem`, returning native JavaScript Promises. This enables
//! proper async/await patterns in the web frontend without the need for
//! synchronous wrappers or `block_on`.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { JsAsyncFileSystem, DiaryxAsyncWorkspace } from './wasm/diaryx_wasm.js';
//!
//! // Create filesystem with your storage backend callbacks
//! const fs = new JsAsyncFileSystem({ /* callbacks */ });
//!
//! // Create async workspace instance
//! const workspace = new DiaryxAsyncWorkspace(fs);
//!
//! // All methods return native Promises
//! const tree = await workspace.getTree('workspace');
//! const created = await workspace.create('new-workspace', 'My New Workspace');
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use diaryx_core::fs::AsyncFileSystem;
use diaryx_core::workspace::Workspace;
use js_sys::Promise;
use serde::Serialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::error::IntoJsResult;
use crate::js_async_fs::JsAsyncFileSystem;

// ============================================================================
// Types
// ============================================================================

/// Tree node returned to JavaScript
#[derive(Debug, Serialize)]
pub struct JsAsyncTreeNode {
    pub name: String,
    pub description: Option<String>,
    pub path: String,
    pub children: Vec<JsAsyncTreeNode>,
}

impl From<diaryx_core::workspace::TreeNode> for JsAsyncTreeNode {
    fn from(node: diaryx_core::workspace::TreeNode) -> Self {
        JsAsyncTreeNode {
            name: node.name,
            description: node.description,
            path: node.path.to_string_lossy().to_string(),
            children: node.children.into_iter().map(JsAsyncTreeNode::from).collect(),
        }
    }
}

// ============================================================================
// DiaryxAsyncWorkspace Class
// ============================================================================

/// Async workspace operations with native Promise support.
///
/// Unlike `DiaryxWorkspace` which uses `block_on` internally, this class
/// returns true JavaScript Promises that can be properly awaited. This enables:
///
/// - Proper async/await patterns in JavaScript
/// - Non-blocking operations in the browser
/// - Integration with truly async storage backends (IndexedDB, OPFS)
#[wasm_bindgen]
pub struct DiaryxAsyncWorkspace {
    fs: JsAsyncFileSystem,
}

#[wasm_bindgen]
impl DiaryxAsyncWorkspace {
    /// Create a new DiaryxAsyncWorkspace with the provided filesystem.
    #[wasm_bindgen(constructor)]
    pub fn new(fs: JsAsyncFileSystem) -> Self {
        Self { fs }
    }

    /// Get the workspace tree structure.
    ///
    /// Returns a Promise that resolves to the tree structure starting from
    /// the workspace root index.
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @param depth - Optional maximum depth to traverse (null for unlimited)
    /// @returns Promise resolving to the tree structure
    #[wasm_bindgen(js_name = "getTree")]
    pub fn get_tree(&self, workspace_path: String, depth: Option<u32>) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            // Find root index in the workspace
            let root_index = ws
                .find_root_index_in_dir(&root_path)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?
                .or_else(|| {
                    // Try to find any index if root index not found
                    // We can't use async in or_else, so we handle this differently
                    None
                });

            // If no root index found, try finding any index
            let root_index = match root_index {
                Some(idx) => idx,
                None => {
                    ws.find_any_index_in_dir(&root_path)
                        .await
                        .map_err(|e| JsValue::from_str(&e.to_string()))?
                        .ok_or_else(|| {
                            JsValue::from_str(&format!("No workspace found at '{}'", workspace_path))
                        })?
                }
            };

            let max_depth = depth.map(|d| d as usize);
            let mut visited = HashSet::new();

            let tree = ws
                .build_tree_with_depth(&root_index, max_depth, &mut visited)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_tree: JsAsyncTreeNode = tree.into();
            serde_wasm_bindgen::to_value(&js_tree).js_err()
        })
    }

    /// Create a new workspace with an index.md file.
    ///
    /// @param path - Path where the workspace should be created
    /// @param name - Name of the workspace (used in the index title)
    /// @returns Promise that resolves when the workspace is created
    #[wasm_bindgen]
    pub fn create(&self, path: String, name: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let index_path = PathBuf::from(&path).join("index.md");

            if fs.exists(&index_path).await {
                return Err(JsValue::from_str(&format!(
                    "Workspace already exists at '{}'",
                    path
                )));
            }

            let content = format!(
                "---\ntitle: \"{}\"\ncontents: []\n---\n\n# {}\n",
                name, name
            );

            fs.write_file(&index_path, &content)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            Ok(JsValue::UNDEFINED)
        })
    }

    /// Get the filesystem tree structure (for "Show All Files" mode).
    ///
    /// This returns a tree of all files and directories, not just those
    /// in the workspace hierarchy.
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @param show_hidden - Whether to include hidden files (starting with .)
    /// @returns Promise resolving to the filesystem tree structure
    #[wasm_bindgen(js_name = "getFilesystemTree")]
    pub fn get_filesystem_tree(&self, workspace_path: String, show_hidden: bool) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let root_path = PathBuf::from(&workspace_path);

            async fn build_tree(
                fs: &JsAsyncFileSystem,
                path: &Path,
                show_hidden: bool,
            ) -> Result<JsAsyncTreeNode, String> {
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string());

                if !show_hidden && name.starts_with('.') {
                    return Err("hidden".to_string());
                }

                let mut children = Vec::new();

                if fs.is_dir(path).await {
                    if let Ok(entries) = fs.list_files(path).await {
                        for entry in entries {
                            if let Ok(child) = Box::pin(build_tree(fs, &entry, show_hidden)).await {
                                children.push(child);
                            }
                        }
                    }
                    // Sort: directories first, then alphabetically
                    children.sort_by(|a, b| {
                        let a_is_dir = !a.children.is_empty();
                        let b_is_dir = !b.children.is_empty();
                        match (a_is_dir, b_is_dir) {
                            (true, false) => std::cmp::Ordering::Less,
                            (false, true) => std::cmp::Ordering::Greater,
                            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                        }
                    });
                }

                Ok(JsAsyncTreeNode {
                    name,
                    description: None,
                    path: path.to_string_lossy().to_string(),
                    children,
                })
            }

            let tree = build_tree(&fs, &root_path, show_hidden)
                .await
                .map_err(|e| JsValue::from_str(&e))?;

            serde_wasm_bindgen::to_value(&tree).js_err()
        })
    }

    /// Find the root index file in a workspace directory.
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @returns Promise resolving to the root index path, or null if not found
    #[wasm_bindgen(js_name = "findRootIndex")]
    pub fn find_root_index(&self, workspace_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            match ws.find_root_index_in_dir(&root_path).await {
                Ok(Some(index_path)) => Ok(JsValue::from_str(&index_path.to_string_lossy())),
                Ok(None) => Ok(JsValue::NULL),
                Err(e) => Err(JsValue::from_str(&e.to_string())),
            }
        })
    }

    /// Check if a path is a workspace (contains an index file).
    ///
    /// @param path - Path to check
    /// @returns Promise resolving to true if the path is a workspace
    #[wasm_bindgen(js_name = "isWorkspace")]
    pub fn is_workspace(&self, path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&fs);
            let check_path = PathBuf::from(&path);

            let is_workspace = ws
                .find_any_index_in_dir(&check_path)
                .await
                .map(|opt| opt.is_some())
                .unwrap_or(false);

            Ok(JsValue::from_bool(is_workspace))
        })
    }

    /// Build the full tree starting from a specific index file.
    ///
    /// @param index_path - Path to the index file to start from
    /// @param depth - Optional maximum depth to traverse
    /// @returns Promise resolving to the tree structure
    #[wasm_bindgen(js_name = "buildTreeFromIndex")]
    pub fn build_tree_from_index(&self, index_path: String, depth: Option<u32>) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&fs);
            let root_index = PathBuf::from(&index_path);

            let max_depth = depth.map(|d| d as usize);
            let mut visited = HashSet::new();

            let tree = ws
                .build_tree_with_depth(&root_index, max_depth, &mut visited)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_tree: JsAsyncTreeNode = tree.into();
            serde_wasm_bindgen::to_value(&js_tree).js_err()
        })
    }
}

impl Default for DiaryxAsyncWorkspace {
    fn default() -> Self {
        // Create with an empty filesystem - caller should use new() with proper fs
        Self {
            fs: JsAsyncFileSystem::new(JsValue::NULL),
        }
    }
}

// ============================================================================
// TypeScript Type Definitions
// ============================================================================

#[wasm_bindgen(typescript_custom_section)]
const TS_APPEND_CONTENT: &'static str = r#"
/**
 * Tree node representing a workspace entry.
 */
export interface AsyncTreeNode {
    /** Display name of the node */
    name: string;
    /** Optional description from frontmatter */
    description: string | null;
    /** Path to the file */
    path: string;
    /** Child nodes */
    children: AsyncTreeNode[];
}
"#;
```


## Assistant

Now let's create the async validation module:

**Tool Call: crates/diaryx_wasm/src/async_validation.rs**
Status: Completed

Diff: crates/diaryx_wasm/src/async_validation.rs
```
//! Async validation operations for WASM with native Promise support.
//!
//! This module provides async validation operations that work directly with
//! `JsAsyncFileSystem`, returning native JavaScript Promises. This enables
//! proper async/await patterns in the web frontend without the need for
//! synchronous wrappers or `block_on`.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { JsAsyncFileSystem, DiaryxAsyncValidation } from './wasm/diaryx_wasm.js';
//!
//! // Create filesystem with your storage backend callbacks
//! const fs = new JsAsyncFileSystem({ /* callbacks */ });
//!
//! // Create async validation instance
//! const validation = new DiaryxAsyncValidation(fs);
//!
//! // All methods return native Promises
//! const result = await validation.validate('workspace');
//! if (result.errors.length > 0) {
//!     const fixes = await validation.fixAll(result);
//!     console.log(`Fixed ${fixes.total_fixed} issues`);
//! }
//! ```

use std::path::PathBuf;

use diaryx_core::validate::{ValidationFixer, Validator};
use diaryx_core::workspace::Workspace;
use js_sys::Promise;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::error::IntoJsResult;
use crate::js_async_fs::JsAsyncFileSystem;

// ============================================================================
// Types (reuse from validation.rs)
// ============================================================================

/// Validation error returned to JavaScript
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum JsAsyncValidationError {
    BrokenPartOf { file: String, target: String },
    BrokenContentsRef { index: String, target: String },
    BrokenAttachment { file: String, attachment: String },
}

impl From<diaryx_core::validate::ValidationError> for JsAsyncValidationError {
    fn from(err: diaryx_core::validate::ValidationError) -> Self {
        use diaryx_core::validate::ValidationError;
        match err {
            ValidationError::BrokenPartOf { file, target } => JsAsyncValidationError::BrokenPartOf {
                file: file.to_string_lossy().to_string(),
                target,
            },
            ValidationError::BrokenContentsRef { index, target } => {
                JsAsyncValidationError::BrokenContentsRef {
                    index: index.to_string_lossy().to_string(),
                    target,
                }
            }
            ValidationError::BrokenAttachment { file, attachment } => {
                JsAsyncValidationError::BrokenAttachment {
                    file: file.to_string_lossy().to_string(),
                    attachment,
                }
            }
        }
    }
}

/// Validation warning returned to JavaScript
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum JsAsyncValidationWarning {
    OrphanFile {
        file: String,
    },
    CircularReference {
        files: Vec<String>,
    },
    UnlinkedEntry {
        path: String,
        is_dir: bool,
    },
    UnlistedFile {
        index: String,
        file: String,
    },
    NonPortablePath {
        file: String,
        property: String,
        value: String,
        suggested: String,
    },
    MultipleIndexes {
        directory: String,
        indexes: Vec<String>,
    },
    OrphanBinaryFile {
        file: String,
        suggested_index: Option<String>,
    },
    MissingPartOf {
        file: String,
        suggested_index: Option<String>,
    },
}

impl From<diaryx_core::validate::ValidationWarning> for JsAsyncValidationWarning {
    fn from(warn: diaryx_core::validate::ValidationWarning) -> Self {
        use diaryx_core::validate::ValidationWarning;
        match warn {
            ValidationWarning::OrphanFile { file } => JsAsyncValidationWarning::OrphanFile {
                file: file.to_string_lossy().to_string(),
            },
            ValidationWarning::CircularReference { files } => {
                JsAsyncValidationWarning::CircularReference {
                    files: files
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect(),
                }
            }
            ValidationWarning::UnlinkedEntry { path, is_dir } => {
                JsAsyncValidationWarning::UnlinkedEntry {
                    path: path.to_string_lossy().to_string(),
                    is_dir,
                }
            }
            ValidationWarning::UnlistedFile { index, file } => {
                JsAsyncValidationWarning::UnlistedFile {
                    index: index.to_string_lossy().to_string(),
                    file: file.to_string_lossy().to_string(),
                }
            }
            ValidationWarning::NonPortablePath {
                file,
                property,
                value,
                suggested,
            } => JsAsyncValidationWarning::NonPortablePath {
                file: file.to_string_lossy().to_string(),
                property,
                value,
                suggested,
            },
            ValidationWarning::MultipleIndexes { directory, indexes } => {
                JsAsyncValidationWarning::MultipleIndexes {
                    directory: directory.to_string_lossy().to_string(),
                    indexes: indexes
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect(),
                }
            }
            ValidationWarning::OrphanBinaryFile {
                file,
                suggested_index,
            } => JsAsyncValidationWarning::OrphanBinaryFile {
                file: file.to_string_lossy().to_string(),
                suggested_index: suggested_index.map(|p| p.to_string_lossy().to_string()),
            },
            ValidationWarning::MissingPartOf {
                file,
                suggested_index,
            } => JsAsyncValidationWarning::MissingPartOf {
                file: file.to_string_lossy().to_string(),
                suggested_index: suggested_index.map(|p| p.to_string_lossy().to_string()),
            },
        }
    }
}

/// Validation result returned to JavaScript
#[derive(Debug, Serialize, Deserialize)]
pub struct JsAsyncValidationResult {
    pub errors: Vec<JsAsyncValidationError>,
    pub warnings: Vec<JsAsyncValidationWarning>,
    pub files_checked: usize,
}

/// Fix result returned to JavaScript
#[derive(Debug, Serialize, Deserialize)]
pub struct JsAsyncFixResult {
    pub success: bool,
    pub message: String,
}

impl From<diaryx_core::validate::FixResult> for JsAsyncFixResult {
    fn from(result: diaryx_core::validate::FixResult) -> Self {
        JsAsyncFixResult {
            success: result.success,
            message: result.message,
        }
    }
}

/// Summary of fix operations
#[derive(Debug, Serialize, Deserialize)]
pub struct JsAsyncFixSummary {
    pub error_fixes: Vec<JsAsyncFixResult>,
    pub warning_fixes: Vec<JsAsyncFixResult>,
    pub total_fixed: usize,
    pub total_failed: usize,
}

// ============================================================================
// DiaryxAsyncValidation Class
// ============================================================================

/// Async validation operations with native Promise support.
///
/// Unlike `DiaryxValidation` which uses `block_on` internally, this class
/// returns true JavaScript Promises that can be properly awaited.
#[wasm_bindgen]
pub struct DiaryxAsyncValidation {
    fs: JsAsyncFileSystem,
}

#[wasm_bindgen]
impl DiaryxAsyncValidation {
    /// Create a new DiaryxAsyncValidation with the provided filesystem.
    #[wasm_bindgen(constructor)]
    pub fn new(fs: JsAsyncFileSystem) -> Self {
        Self { fs }
    }

    /// Validate workspace links.
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @returns Promise resolving to validation result with errors and warnings
    #[wasm_bindgen]
    pub fn validate(&self, workspace_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let validator = Validator::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            let ws = Workspace::new(&fs);
            
            // Find root index
            let root_index = ws
                .find_root_index_in_dir(&root_path)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let root_index = match root_index {
                Some(idx) => idx,
                None => {
                    ws.find_any_index_in_dir(&root_path)
                        .await
                        .map_err(|e| JsValue::from_str(&e.to_string()))?
                        .ok_or_else(|| {
                            JsValue::from_str(&format!("No workspace found at '{}'", workspace_path))
                        })?
                }
            };

            let result = validator
                .validate_workspace(&root_index)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_result = JsAsyncValidationResult {
                errors: result
                    .errors
                    .into_iter()
                    .map(JsAsyncValidationError::from)
                    .collect(),
                warnings: result
                    .warnings
                    .into_iter()
                    .map(JsAsyncValidationWarning::from)
                    .collect(),
                files_checked: result.files_checked,
            };

            serde_wasm_bindgen::to_value(&js_result).js_err()
        })
    }

    /// Validate a single file's links.
    ///
    /// @param file_path - Path to the file to validate
    /// @returns Promise resolving to validation result
    #[wasm_bindgen(js_name = "validateFile")]
    pub fn validate_file(&self, file_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let validator = Validator::new(&fs);
            let path = PathBuf::from(&file_path);

            let result = validator
                .validate_file(&path)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_result = JsAsyncValidationResult {
                errors: result
                    .errors
                    .into_iter()
                    .map(JsAsyncValidationError::from)
                    .collect(),
                warnings: result
                    .warnings
                    .into_iter()
                    .map(JsAsyncValidationWarning::from)
                    .collect(),
                files_checked: result.files_checked,
            };

            serde_wasm_bindgen::to_value(&js_result).js_err()
        })
    }

    /// Fix a broken part_of reference by removing it.
    ///
    /// @param file_path - Path to the file with broken part_of
    /// @returns Promise resolving to fix result
    #[wasm_bindgen(js_name = "fixBrokenPartOf")]
    pub fn fix_broken_part_of(&self, file_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = ValidationFixer::new(&fs);
            let path = PathBuf::from(&file_path);
            let result = fixer.fix_broken_part_of(&path).await;
            serde_wasm_bindgen::to_value(&JsAsyncFixResult::from(result)).js_err()
        })
    }

    /// Fix a broken contents reference by removing it.
    ///
    /// @param index_path - Path to the index file
    /// @param target - The broken target reference to remove
    /// @returns Promise resolving to fix result
    #[wasm_bindgen(js_name = "fixBrokenContentsRef")]
    pub fn fix_broken_contents_ref(&self, index_path: String, target: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = ValidationFixer::new(&fs);
            let path = PathBuf::from(&index_path);
            let result = fixer.fix_broken_contents_ref(&path, &target).await;
            serde_wasm_bindgen::to_value(&JsAsyncFixResult::from(result)).js_err()
        })
    }

    /// Fix a broken attachment reference by removing it.
    ///
    /// @param file_path - Path to the file with broken attachment
    /// @param attachment - The broken attachment reference to remove
    /// @returns Promise resolving to fix result
    #[wasm_bindgen(js_name = "fixBrokenAttachment")]
    pub fn fix_broken_attachment(&self, file_path: String, attachment: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = ValidationFixer::new(&fs);
            let path = PathBuf::from(&file_path);
            let result = fixer.fix_broken_attachment(&path, &attachment).await;
            serde_wasm_bindgen::to_value(&JsAsyncFixResult::from(result)).js_err()
        })
    }

    /// Fix a non-portable path by normalizing it.
    ///
    /// @param file_path - Path to the file
    /// @param property - The frontmatter property containing the path
    /// @param old_value - The current non-portable value
    /// @param new_value - The normalized portable value
    /// @returns Promise resolving to fix result
    #[wasm_bindgen(js_name = "fixNonPortablePath")]
    pub fn fix_non_portable_path(
        &self,
        file_path: String,
        property: String,
        old_value: String,
        new_value: String,
    ) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = ValidationFixer::new(&fs);
            let path = PathBuf::from(&file_path);
            let result = fixer
                .fix_non_portable_path(&path, &property, &old_value, &new_value)
                .await;
            serde_wasm_bindgen::to_value(&JsAsyncFixResult::from(result)).js_err()
        })
    }

    /// Add an unlisted file to an index's contents.
    ///
    /// @param index_path - Path to the index file
    /// @param file_path - Path to the file to add to contents
    /// @returns Promise resolving to fix result
    #[wasm_bindgen(js_name = "fixUnlistedFile")]
    pub fn fix_unlisted_file(&self, index_path: String, file_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = ValidationFixer::new(&fs);
            let index = PathBuf::from(&index_path);
            let file = PathBuf::from(&file_path);
            let result = fixer.fix_unlisted_file(&index, &file).await;
            serde_wasm_bindgen::to_value(&JsAsyncFixResult::from(result)).js_err()
        })
    }

    /// Add an orphan binary file to an index's attachments.
    ///
    /// @param index_path - Path to the index file
    /// @param file_path - Path to the binary file to add
    /// @returns Promise resolving to fix result
    #[wasm_bindgen(js_name = "fixOrphanBinaryFile")]
    pub fn fix_orphan_binary_file(&self, index_path: String, file_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = ValidationFixer::new(&fs);
            let index = PathBuf::from(&index_path);
            let file = PathBuf::from(&file_path);
            let result = fixer.fix_orphan_binary_file(&index, &file).await;
            serde_wasm_bindgen::to_value(&JsAsyncFixResult::from(result)).js_err()
        })
    }

    /// Fix a missing part_of by setting it to point to the given index.
    ///
    /// @param file_path - Path to the file missing part_of
    /// @param index_path - Path to the index that should be the parent
    /// @returns Promise resolving to fix result
    #[wasm_bindgen(js_name = "fixMissingPartOf")]
    pub fn fix_missing_part_of(&self, file_path: String, index_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = ValidationFixer::new(&fs);
            let file = PathBuf::from(&file_path);
            let index = PathBuf::from(&index_path);
            let result = fixer.fix_missing_part_of(&file, &index).await;
            serde_wasm_bindgen::to_value(&JsAsyncFixResult::from(result)).js_err()
        })
    }

    /// Fix all errors and fixable warnings in a validation result.
    ///
    /// @param validation_result - The validation result from validate() or validateFile()
    /// @returns Promise resolving to fix summary
    #[wasm_bindgen(js_name = "fixAll")]
    pub fn fix_all(&self, validation_result: JsValue) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = ValidationFixer::new(&fs);

            // Parse the JS validation result
            let js_result: JsAsyncValidationResult =
                serde_wasm_bindgen::from_value(validation_result)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let mut error_fixes = Vec::new();
            let mut warning_fixes = Vec::new();

            // Fix errors
            for err in &js_result.errors {
                let result = match err {
                    JsAsyncValidationError::BrokenPartOf { file, target: _ } => {
                        fixer.fix_broken_part_of(&PathBuf::from(file)).await
                    }
                    JsAsyncValidationError::BrokenContentsRef { index, target } => {
                        fixer
                            .fix_broken_contents_ref(&PathBuf::from(index), target)
                            .await
                    }
                    JsAsyncValidationError::BrokenAttachment { file, attachment } => {
                        fixer
                            .fix_broken_attachment(&PathBuf::from(file), attachment)
                            .await
                    }
                };
                error_fixes.push(JsAsyncFixResult::from(result));
            }

            // Fix warnings
            for warn in &js_result.warnings {
                let result = match warn {
                    JsAsyncValidationWarning::UnlistedFile { index, file } => {
                        Some(
                            fixer
                                .fix_unlisted_file(&PathBuf::from(index), &PathBuf::from(file))
                                .await,
                        )
                    }
                    JsAsyncValidationWarning::NonPortablePath {
                        file,
                        property,
                        value,
                        suggested,
                    } => Some(
                        fixer
                            .fix_non_portable_path(&PathBuf::from(file), property, value, suggested)
                            .await,
                    ),
                    JsAsyncValidationWarning::OrphanBinaryFile {
                        file,
                        suggested_index,
                    } => {
                        if let Some(index) = suggested_index {
                            Some(
                                fixer
                                    .fix_orphan_binary_file(
                                        &PathBuf::from(index),
                                        &PathBuf::from(file),
                                    )
                                    .await,
                            )
                        } else {
                            None
                        }
                    }
                    JsAsyncValidationWarning::MissingPartOf {
                        file,
                        suggested_index,
                    } => {
                        if let Some(index) = suggested_index {
                            Some(
                                fixer
                                    .fix_missing_part_of(
                                        &PathBuf::from(file),
                                        &PathBuf::from(index),
                                    )
                                    .await,
                            )
                        } else {
                            None
                        }
                    }
                    // These cannot be auto-fixed
                    JsAsyncValidationWarning::OrphanFile { .. }
                    | JsAsyncValidationWarning::UnlinkedEntry { .. }
                    | JsAsyncValidationWarning::CircularReference { .. }
                    | JsAsyncValidationWarning::MultipleIndexes { .. } => None,
                };

                if let Some(r) = result {
                    warning_fixes.push(JsAsyncFixResult::from(r));
                }
            }

            let total_fixed = error_fixes.iter().filter(|r| r.success).count()
                + warning_fixes.iter().filter(|r| r.success).count();
            let total_failed = error_fixes.iter().filter(|r| !r.success).count()
                + warning_fixes.iter().filter(|r| !r.success).count();

            let summary = JsAsyncFixSummary {
                error_fixes,
                warning_fixes,
                total_fixed,
                total_failed,
            };

            serde_wasm_bindgen::to_value(&summary).js_err()
        })
    }
}

impl Default for DiaryxAsyncValidation {
    fn default() -> Self {
        Self {
            fs: JsAsyncFileSystem::new(JsValue::NULL),
        }
    }
}

// ============================================================================
// TypeScript Type Definitions
// ============================================================================

#[wasm_bindgen(typescript_custom_section)]
const TS_APPEND_CONTENT: &'static str = r#"
/**
 * Validation error types.
 */
export type AsyncValidationError =
    | { type: 'BrokenPartOf'; file: string; target: string }
    | { type: 'BrokenContentsRef'; index: string; target: string }
    | { type: 'BrokenAttachment'; file: string; attachment: string };

/**
 * Validation warning types.
 */
export type AsyncValidationWarning =
    | { type: 'OrphanFile'; file: string }
    | { type: 'CircularReference'; files: string[] }
    | { type: 'UnlinkedEntry'; path: string; is_dir: boolean }
    | { type: 'UnlistedFile'; index: string; file: string }
    | { type: 'NonPortablePath'; file: string; property: string; value: string; suggested: string }
    | { type: 'MultipleIndexes'; directory: string; indexes: string[] }
    | { type: 'OrphanBinaryFile'; file: string; suggested_index: string | null }
    | { type: 'MissingPartOf'; file: string; suggested_index: string | null };

/**
 * Validation result containing errors and warnings.
 */
export interface AsyncValidationResult {
    errors: AsyncValidationError[];
    warnings: AsyncValidationWarning[];
    files_checked: number;
}

/**
 * Result of a single fix operation.
 */
export interface AsyncFixResult {
    success: boolean;
    message: string;
}

/**
 * Summary of all fix operations.
 */
export interface AsyncFixSummary {
    error_fixes: AsyncFixResult[];
    warning_fixes: AsyncFixResult[];
    total_fixed: number;
    total_failed: number;
}
"#;
```


## Assistant

Now let's create the async search module:

**Tool Call: crates/diaryx_wasm/src/async_search.rs**
Status: Completed

Diff: crates/diaryx_wasm/src/async_search.rs
```
//! Async search operations for WASM with native Promise support.
//!
//! This module provides async search operations that work directly with
//! `JsAsyncFileSystem`, returning native JavaScript Promises. This enables
//! proper async/await patterns in the web frontend without the need for
//! synchronous wrappers or `block_on`.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { JsAsyncFileSystem, DiaryxAsyncSearch } from './wasm/diaryx_wasm.js';
//!
//! // Create filesystem with your storage backend callbacks
//! const fs = new JsAsyncFileSystem({ /* callbacks */ });
//!
//! // Create async search instance
//! const search = new DiaryxAsyncSearch(fs);
//!
//! // All methods return native Promises
//! const results = await search.search('workspace', 'my query', {
//!     searchFrontmatter: true,
//!     caseSensitive: false
//! });
//! ```

use std::path::PathBuf;

use diaryx_core::search::{SearchQuery, Searcher};
use js_sys::Promise;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::error::IntoJsResult;
use crate::js_async_fs::JsAsyncFileSystem;

// ============================================================================
// Types
// ============================================================================

/// A single match within a file
#[derive(Debug, Serialize)]
pub struct JsAsyncSearchMatch {
    /// Line number (1-indexed)
    pub line: usize,
    /// The matching line content
    pub content: String,
    /// Whether this match is in frontmatter
    pub in_frontmatter: bool,
}

impl From<diaryx_core::search::SearchMatch> for JsAsyncSearchMatch {
    fn from(m: diaryx_core::search::SearchMatch) -> Self {
        JsAsyncSearchMatch {
            line: m.line,
            content: m.content,
            in_frontmatter: m.in_frontmatter,
        }
    }
}

/// Search result for a single file
#[derive(Debug, Serialize)]
pub struct JsAsyncFileSearchResult {
    /// Path to the file
    pub path: String,
    /// Title from frontmatter, if available
    pub title: Option<String>,
    /// Matches found in this file
    pub matches: Vec<JsAsyncSearchMatch>,
}

impl From<diaryx_core::search::FileSearchResult> for JsAsyncFileSearchResult {
    fn from(r: diaryx_core::search::FileSearchResult) -> Self {
        JsAsyncFileSearchResult {
            path: r.path.to_string_lossy().to_string(),
            title: r.title,
            matches: r.matches.into_iter().map(JsAsyncSearchMatch::from).collect(),
        }
    }
}

/// Complete search results
#[derive(Debug, Serialize)]
pub struct JsAsyncSearchResults {
    /// Files with matches
    pub files: Vec<JsAsyncFileSearchResult>,
    /// Total number of matches across all files
    pub total_matches: usize,
    /// Number of files searched
    pub files_searched: usize,
}

impl From<diaryx_core::search::SearchResults> for JsAsyncSearchResults {
    fn from(r: diaryx_core::search::SearchResults) -> Self {
        JsAsyncSearchResults {
            files: r.files.into_iter().map(JsAsyncFileSearchResult::from).collect(),
            total_matches: r.total_matches,
            files_searched: r.files_searched,
        }
    }
}

/// Search options from JavaScript
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JsAsyncSearchOptions {
    /// Search in frontmatter (default: true)
    #[serde(default = "default_true")]
    pub search_frontmatter: bool,
    /// Search only in a specific frontmatter property
    pub property: Option<String>,
    /// Case-sensitive search (default: false)
    #[serde(default)]
    pub case_sensitive: bool,
}

fn default_true() -> bool {
    true
}

// ============================================================================
// DiaryxAsyncSearch Class
// ============================================================================

/// Async search operations with native Promise support.
///
/// Unlike `DiaryxSearch` which uses `block_on` internally, this class
/// returns true JavaScript Promises that can be properly awaited.
#[wasm_bindgen]
pub struct DiaryxAsyncSearch {
    fs: JsAsyncFileSystem,
}

#[wasm_bindgen]
impl DiaryxAsyncSearch {
    /// Create a new DiaryxAsyncSearch with the provided filesystem.
    #[wasm_bindgen(constructor)]
    pub fn new(fs: JsAsyncFileSystem) -> Self {
        Self { fs }
    }

    /// Search for content in the workspace.
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @param pattern - The search pattern (string or regex)
    /// @param options - Optional search options
    /// @returns Promise resolving to search results
    #[wasm_bindgen]
    pub fn search(
        &self,
        workspace_path: String,
        pattern: String,
        options: JsValue,
    ) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let searcher = Searcher::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            // Parse options
            let opts: JsAsyncSearchOptions = if options.is_undefined() || options.is_null() {
                JsAsyncSearchOptions::default()
            } else {
                serde_wasm_bindgen::from_value(options)
                    .map_err(|e| JsValue::from_str(&format!("Invalid options: {}", e)))?
            };

            // Build search query
            let query = if opts.search_frontmatter {
                if let Some(prop) = opts.property {
                    SearchQuery::frontmatter(&pattern, &prop)
                } else {
                    SearchQuery::content(&pattern)
                }
            } else {
                SearchQuery::content(&pattern)
            };

            let query = if opts.case_sensitive {
                query.case_sensitive()
            } else {
                query
            };

            // Execute search
            let results = searcher
                .search(&root_path, &query)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_results: JsAsyncSearchResults = results.into();
            serde_wasm_bindgen::to_value(&js_results).js_err()
        })
    }

    /// Search for content in the workspace (content only).
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @param pattern - The search pattern
    /// @returns Promise resolving to search results
    #[wasm_bindgen(js_name = "searchContent")]
    pub fn search_content(&self, workspace_path: String, pattern: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let searcher = Searcher::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            let query = SearchQuery::content(&pattern);
            let results = searcher
                .search(&root_path, &query)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_results: JsAsyncSearchResults = results.into();
            serde_wasm_bindgen::to_value(&js_results).js_err()
        })
    }

    /// Search for a specific frontmatter property value.
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @param property - The frontmatter property to search
    /// @param pattern - The search pattern
    /// @returns Promise resolving to search results
    #[wasm_bindgen(js_name = "searchFrontmatter")]
    pub fn search_frontmatter(
        &self,
        workspace_path: String,
        property: String,
        pattern: String,
    ) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let searcher = Searcher::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            let query = SearchQuery::frontmatter(&pattern, &property);
            let results = searcher
                .search(&root_path, &query)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_results: JsAsyncSearchResults = results.into();
            serde_wasm_bindgen::to_value(&js_results).js_err()
        })
    }

    /// Search with case sensitivity.
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @param pattern - The search pattern
    /// @param case_sensitive - Whether to match case
    /// @returns Promise resolving to search results
    #[wasm_bindgen(js_name = "searchWithCase")]
    pub fn search_with_case(
        &self,
        workspace_path: String,
        pattern: String,
        case_sensitive: bool,
    ) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let searcher = Searcher::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            let query = SearchQuery::content(&pattern);
            let query = if case_sensitive {
                query.case_sensitive()
            } else {
                query
            };

            let results = searcher
                .search(&root_path, &query)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_results: JsAsyncSearchResults = results.into();
            serde_wasm_bindgen::to_value(&js_results).js_err()
        })
    }
}

impl Default for DiaryxAsyncSearch {
    fn default() -> Self {
        Self {
            fs: JsAsyncFileSystem::new(JsValue::NULL),
        }
    }
}

// ============================================================================
// TypeScript Type Definitions
// ============================================================================

#[wasm_bindgen(typescript_custom_section)]
const TS_APPEND_CONTENT: &'static str = r#"
/**
 * Options for search operations.
 */
export interface AsyncSearchOptions {
    /** Search in frontmatter (default: true) */
    searchFrontmatter?: boolean;
    /** Search only in a specific frontmatter property */
    property?: string;
    /** Case-sensitive search (default: false) */
    caseSensitive?: boolean;
}

/**
 * A single match within a file.
 */
export interface AsyncSearchMatch {
    /** Line number (1-indexed) */
    line: number;
    /** The matching line content */
    content: string;
    /** Whether this match is in frontmatter */
    in_frontmatter: boolean;
}

/**
 * Search result for a single file.
 */
export interface AsyncFileSearchResult {
    /** Path to the file */
    path: string;
    /** Title from frontmatter, if available */
    title: string | null;
    /** Matches found in this file */
    matches: AsyncSearchMatch[];
}

/**
 * Complete search results.
 */
export interface AsyncSearchResults {
    /** Files with matches */
    files: AsyncFileSearchResult[];
    /** Total number of matches across all files */
    total_matches: number;
    /** Number of files searched */
    files_searched: number;
}
"#;
```


## Assistant

Now let's create the async export module:

**Tool Call: crates/diaryx_wasm/src/async_export.rs**
Status: Completed

Diff: crates/diaryx_wasm/src/async_export.rs
```
//! Async export operations for WASM with native Promise support.
//!
//! This module provides async export operations that work directly with
//! `JsAsyncFileSystem`, returning native JavaScript Promises. This enables
//! proper async/await patterns in the web frontend without the need for
//! synchronous wrappers or `block_on`.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { JsAsyncFileSystem, DiaryxAsyncExport } from './wasm/diaryx_wasm.js';
//!
//! // Create filesystem with your storage backend callbacks
//! const fs = new JsAsyncFileSystem({ /* callbacks */ });
//!
//! // Create async export instance
//! const exporter = new DiaryxAsyncExport(fs);
//!
//! // All methods return native Promises
//! const audiences = await exporter.getAudiences('workspace');
//! const plan = await exporter.plan('workspace', 'public');
//! const files = await exporter.toMemory('workspace', 'public');
//! ```

use std::path::PathBuf;

use diaryx_core::export::Exporter;
use js_sys::Promise;
use serde::Serialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::error::IntoJsResult;
use crate::js_async_fs::JsAsyncFileSystem;

// ============================================================================
// Types
// ============================================================================

/// Export plan entry
#[derive(Debug, Serialize)]
pub struct JsAsyncExportPlanEntry {
    /// Source file path
    pub source: String,
    /// Destination file path
    pub destination: String,
    /// File title from frontmatter
    pub title: Option<String>,
}

impl From<diaryx_core::export::ExportPlanEntry> for JsAsyncExportPlanEntry {
    fn from(entry: diaryx_core::export::ExportPlanEntry) -> Self {
        JsAsyncExportPlanEntry {
            source: entry.source.to_string_lossy().to_string(),
            destination: entry.destination.to_string_lossy().to_string(),
            title: entry.title,
        }
    }
}

/// Export plan returned to JavaScript
#[derive(Debug, Serialize)]
pub struct JsAsyncExportPlan {
    /// Files to be exported
    pub files: Vec<JsAsyncExportPlanEntry>,
    /// Target audience
    pub audience: String,
    /// Total number of files
    pub total_files: usize,
}

impl From<diaryx_core::export::ExportPlan> for JsAsyncExportPlan {
    fn from(plan: diaryx_core::export::ExportPlan) -> Self {
        let total = plan.files.len();
        JsAsyncExportPlan {
            files: plan.files.into_iter().map(JsAsyncExportPlanEntry::from).collect(),
            audience: plan.audience,
            total_files: total,
        }
    }
}

/// Exported file content
#[derive(Debug, Serialize)]
pub struct JsAsyncExportedFile {
    /// File path
    pub path: String,
    /// File content
    pub content: String,
}

/// Exported binary file
#[derive(Debug, Serialize)]
pub struct JsAsyncBinaryExportFile {
    /// File path
    pub path: String,
    /// Binary content as array of bytes
    pub data: Vec<u8>,
}

/// HTML export result
#[derive(Debug, Serialize)]
pub struct JsAsyncHtmlExport {
    /// HTML files
    pub files: Vec<JsAsyncExportedFile>,
    /// Total number of files
    pub total_files: usize,
}

// ============================================================================
// DiaryxAsyncExport Class
// ============================================================================

/// Async export operations with native Promise support.
///
/// Unlike `DiaryxExport` which uses `block_on` internally, this class
/// returns true JavaScript Promises that can be properly awaited.
#[wasm_bindgen]
pub struct DiaryxAsyncExport {
    fs: JsAsyncFileSystem,
}

#[wasm_bindgen]
impl DiaryxAsyncExport {
    /// Create a new DiaryxAsyncExport with the provided filesystem.
    #[wasm_bindgen(constructor)]
    pub fn new(fs: JsAsyncFileSystem) -> Self {
        Self { fs }
    }

    /// Get available audiences from the workspace.
    ///
    /// Scans all files in the workspace and returns unique audience values
    /// found in frontmatter.
    ///
    /// @param root_path - Path to the workspace root
    /// @returns Promise resolving to array of audience names
    #[wasm_bindgen(js_name = "getAudiences")]
    pub fn get_audiences(&self, root_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            let audiences = exporter
                .get_available_audiences(&path)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let audiences_vec: Vec<String> = audiences.into_iter().collect();
            serde_wasm_bindgen::to_value(&audiences_vec).js_err()
        })
    }

    /// Plan an export operation.
    ///
    /// Returns a list of files that would be exported for the given audience.
    ///
    /// @param root_path - Path to the workspace root
    /// @param audience - Target audience to export for
    /// @returns Promise resolving to export plan
    #[wasm_bindgen]
    pub fn plan(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            // Plan export to a temporary destination (we won't actually use it)
            let dest = PathBuf::from("_export_temp");
            let plan = exporter
                .plan_export(&path, &audience, &dest)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_plan: JsAsyncExportPlan = plan.into();
            serde_wasm_bindgen::to_value(&js_plan).js_err()
        })
    }

    /// Export files to memory (returns file contents).
    ///
    /// Returns an array of files with their content, suitable for
    /// downloading or further processing.
    ///
    /// @param root_path - Path to the workspace root
    /// @param audience - Target audience to export for
    /// @returns Promise resolving to array of exported files
    #[wasm_bindgen(js_name = "toMemory")]
    pub fn to_memory(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            // Plan the export
            let dest = PathBuf::from("_export");
            let plan = exporter
                .plan_export(&path, &audience, &dest)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Read each file's content
            let mut exported_files = Vec::new();
            for entry in plan.files {
                let content = fs
                    .read_to_string(&entry.source)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                exported_files.push(JsAsyncExportedFile {
                    path: entry.destination.to_string_lossy().to_string(),
                    content,
                });
            }

            serde_wasm_bindgen::to_value(&exported_files).js_err()
        })
    }

    /// Export files to HTML format.
    ///
    /// Converts markdown files to HTML using comrak.
    ///
    /// @param root_path - Path to the workspace root
    /// @param audience - Target audience to export for
    /// @returns Promise resolving to array of HTML files
    #[wasm_bindgen(js_name = "toHtml")]
    pub fn to_html(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            // Plan the export
            let dest = PathBuf::from("_export");
            let plan = exporter
                .plan_export(&path, &audience, &dest)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Convert each file to HTML
            let mut html_files = Vec::new();
            for entry in plan.files {
                let content = fs
                    .read_to_string(&entry.source)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                // Extract body (remove frontmatter) and convert to HTML
                let body = extract_body(&content);
                let html = markdown_to_html(&body);

                // Change extension to .html
                let html_path = entry
                    .destination
                    .with_extension("html")
                    .to_string_lossy()
                    .to_string();

                html_files.push(JsAsyncExportedFile {
                    path: html_path,
                    content: html,
                });
            }

            let result = JsAsyncHtmlExport {
                total_files: html_files.len(),
                files: html_files,
            };

            serde_wasm_bindgen::to_value(&result).js_err()
        })
    }

    /// Export binary attachments for a given audience.
    ///
    /// Returns binary files (images, etc.) associated with entries
    /// that match the target audience.
    ///
    /// @param root_path - Path to the workspace root
    /// @param audience - Target audience to export for
    /// @returns Promise resolving to array of binary files
    #[wasm_bindgen(js_name = "binaryAttachments")]
    pub fn binary_attachments(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            // Plan the export to get the list of files
            let dest = PathBuf::from("_export");
            let plan = exporter
                .plan_export(&path, &audience, &dest)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Collect attachments from all exported files
            let mut binary_files = Vec::new();
            for entry in plan.files {
                // Read the file and check for attachments in frontmatter
                let content = fs
                    .read_to_string(&entry.source)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                if let Some(attachments) = get_attachments_from_frontmatter(&content) {
                    let source_dir = entry.source.parent().unwrap_or(&entry.source);

                    for attachment in attachments {
                        let attachment_path = source_dir.join(&attachment);
                        if fs.exists(&attachment_path).await {
                            match fs.read_binary(&attachment_path).await {
                                Ok(data) => {
                                    binary_files.push(JsAsyncBinaryExportFile {
                                        path: attachment,
                                        data,
                                    });
                                }
                                Err(_) => {
                                    // Skip files that can't be read
                                    continue;
                                }
                            }
                        }
                    }
                }
            }

            serde_wasm_bindgen::to_value(&binary_files).js_err()
        })
    }
}

impl Default for DiaryxAsyncExport {
    fn default() -> Self {
        Self {
            fs: JsAsyncFileSystem::new(JsValue::NULL),
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Extract body content from markdown (removes frontmatter)
fn extract_body(content: &str) -> String {
    if !content.starts_with("---") {
        return content.to_string();
    }

    // Find the closing ---
    if let Some(end_idx) = content[3..].find("\n---") {
        let after_frontmatter = &content[3 + end_idx + 4..];
        after_frontmatter.trim_start().to_string()
    } else {
        content.to_string()
    }
}

/// Convert markdown to HTML using comrak
fn markdown_to_html(markdown: &str) -> String {
    use comrak::{markdown_to_html as comrak_md_to_html, Options};
    
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.render.unsafe_ = true;

    comrak_md_to_html(markdown, &options)
}

/// Get attachments list from frontmatter
fn get_attachments_from_frontmatter(content: &str) -> Option<Vec<String>> {
    if !content.starts_with("---") {
        return None;
    }

    // Find the closing ---
    let end_idx = content[3..].find("\n---")?;
    let frontmatter = &content[3..3 + end_idx];

    // Parse YAML
    let yaml: serde_yaml::Value = serde_yaml::from_str(frontmatter).ok()?;
    let attachments = yaml.get("attachments")?;

    match attachments {
        serde_yaml::Value::Sequence(seq) => {
            Some(
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
            )
        }
        _ => None,
    }
}

// ============================================================================
// TypeScript Type Definitions
// ============================================================================

#[wasm_bindgen(typescript_custom_section)]
const TS_APPEND_CONTENT: &'static str = r#"
/**
 * Export plan entry representing a file to be exported.
 */
export interface AsyncExportPlanEntry {
    /** Source file path */
    source: string;
    /** Destination file path */
    destination: string;
    /** File title from frontmatter */
    title: string | null;
}

/**
 * Export plan containing files to be exported.
 */
export interface AsyncExportPlan {
    /** Files to be exported */
    files: AsyncExportPlanEntry[];
    /** Target audience */
    audience: string;
    /** Total number of files */
    total_files: number;
}

/**
 * Exported file with content.
 */
export interface AsyncExportedFile {
    /** File path */
    path: string;
    /** File content */
    content: string;
}

/**
 * Exported binary file.
 */
export interface AsyncBinaryExportFile {
    /** File path */
    path: string;
    /** Binary content as byte array */
    data: number[];
}

/**
 * HTML export result.
 */
export interface AsyncHtmlExport {
    /** HTML files */
    files: AsyncExportedFile[];
    /** Total number of files */
    total_files: number;
}
"#;
```


## Assistant

Now let's update the lib.rs to export all the new async modules:

**Tool Call: crates/diaryx_wasm/src/lib.rs**
Status: Completed

Diff: crates/diaryx_wasm/src/lib.rs
```
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
mod entry;
mod error;
mod export;
mod filesystem;
mod frontmatter;
mod js_async_fs;
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
pub use js_async_fs::JsAsyncFileSystem;

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

```


## Assistant

Now let's check if there are any compilation issues:

**Tool Call: `cd crates/diaryx_wasm && cargo check 2>&1 | head -100`**
Status: Completed

Terminal:
```
warning: profiles for the non root package will be ignored, specify profiles at the workspace root:
package:   /Users/adamharris/Documents/diaryx-core/crates/diaryx_wasm/Cargo.toml
workspace: /Users/adamharris/Documents/diaryx-core/Cargo.toml
    Checking diaryx_wasm v0.7.0 (/Users/adamharris/Documents/diaryx-core/crates/diaryx_wasm)
error[E0412]: cannot find type `ExportPlanEntry` in module `diaryx_core::export`
  --> crates/diaryx_wasm/src/async_export.rs:51:32
   |
51 | impl From<diaryx_core::export::ExportPlanEntry> for JsAsyncExportPlanEntry {
   |                                ^^^^^^^^^^^^^^^ help: a struct with a similar name exists: `ExportPlan`
   |
  ::: /Users/adamharris/Documents/diaryx-core/crates/diaryx_core/src/export.rs:20:1
   |
20 | pub struct ExportPlan {
   | --------------------- similarly named struct `ExportPlan` defined here

error[E0412]: cannot find type `ExportPlanEntry` in module `diaryx_core::export`
  --> crates/diaryx_wasm/src/async_export.rs:52:41
   |
52 |     fn from(entry: diaryx_core::export::ExportPlanEntry) -> Self {
   |                                         ^^^^^^^^^^^^^^^ help: a struct with a similar name exists: `ExportPlan`
   |
  ::: /Users/adamharris/Documents/diaryx-core/crates/diaryx_core/src/export.rs:20:1
   |
20 | pub struct ExportPlan {
   | --------------------- similarly named struct `ExportPlan` defined here

warning: unused import: `with_fs`
  --> crates/diaryx_wasm/src/export.rs:14:45
   |
14 | use crate::state::{block_on, with_async_fs, with_fs};
   |                                             ^^^^^^^
   |
   = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused import: `std::pin::Pin`
  --> crates/diaryx_wasm/src/js_async_fs.rs:63:5
   |
63 | use std::pin::Pin;
   |     ^^^^^^^^^^^^^

warning: unused import: `std::pin::Pin`
 --> crates/diaryx_wasm/src/state.rs:5:5
  |
5 | use std::pin::Pin;
  |     ^^^^^^^^^^^^^

warning: unused import: `with_fs`
  --> crates/diaryx_wasm/src/workspace.rs:12:45
   |
12 | use crate::state::{block_on, with_async_fs, with_fs};
   |                                             ^^^^^^^

error[E0609]: no field `files` on type `ExportPlan`
  --> crates/diaryx_wasm/src/async_export.rs:74:26
   |
74 |         let total = plan.files.len();
   |                          ^^^^^ unknown field
   |
   = note: available fields are: `included`, `excluded`, `audience`, `source_root`, `destination`

error[E0609]: no field `files` on type `ExportPlan`
  --> crates/diaryx_wasm/src/async_export.rs:76:25
   |
76 |             files: plan.files.into_iter().map(JsAsyncExportPlanEntry::from).collect(),
   |                         ^^^^^ unknown field
   |
   = note: available fields are: `included`, `excluded`, `audience`, `source_root`, `destination`

error[E0599]: no method named `get_available_audiences` found for struct `Exporter<FS>` in the current scope
   --> crates/diaryx_wasm/src/async_export.rs:147:18
    |
146 |               let audiences = exporter
    |  _____________________________-
147 | |                 .get_available_audiences(&path)
    | |                 -^^^^^^^^^^^^^^^^^^^^^^^ method not found in `Exporter<&JsAsyncFileSystem>`
    | |_________________|
    |

error[E0609]: no field `files` on type `ExportPlan`
   --> crates/diaryx_wasm/src/async_export.rs:208:31
    |
208 |             for entry in plan.files {
    |                               ^^^^^ unknown field
    |
    = note: available fields are: `included`, `excluded`, `audience`, `source_root`, `destination`

error[E0599]: no method named `read_to_string` found for struct `JsAsyncFileSystem` in the current scope
   --> crates/diaryx_wasm/src/async_export.rs:210:22
    |
209 |                   let content = fs
    |  _______________________________-
210 | |                     .read_to_string(&entry.source)
    | |                     -^^^^^^^^^^^^^^ method not found in `JsAsyncFileSystem`
    | |_____________________|
    |
    |
   ::: crates/diaryx_wasm/src/js_async_fs.rs:141:1
    |
141 |   pub struct JsAsyncFileSystem {
    |   ---------------------------- method `read_to_string` not found for this struct

```


## Assistant

Let me check the export module in diaryx_core to see the correct types:

**Tool Call: Read file `crates/diaryx_core/src/export.rs`**
Status: Completed

```diaryx-core/crates/diaryx_core/src/export.rs
SUCCESS: File outline retrieved. This file is too large to read all at once, so the outline below shows the file's structure with line numbers.

IMPORTANT: Do NOT retry this call without line numbers - you will get the same outline.
Instead, use the line numbers below to read specific sections by calling this tool again with start_line and end_line parameters.

# File outline for /Users/adamharris/Documents/diaryx-core/crates/diaryx_core/src/export.rs

pub struct ExportPlan [L20-31]
 pub included [L22]
 pub excluded [L24]
 pub audience [L26]
 pub source_root [L28]
 pub destination [L30]
pub struct ExportFile [L35-44]
 pub source_path [L37]
 pub relative_path [L39]
 pub dest_path [L41]
 pub filtered_contents [L43]
pub struct ExcludedFile [L48-53]
 pub path [L50]
 pub reason [L52]
pub enum ExclusionReason [L57-74]
 Private [L59]
 AudienceMismatch [L61-66]
  file_audience [L63]
  requested [L65]
 InheritedPrivate [L68-71]
  from [L70]
 NoAudienceDefined [L73]
impl std::fmt::Display for ExclusionReason [L76-98]
 fn fmt [L77-97]
pub struct ExportOptions [L102-107]
 pub force [L104]
 pub keep_audience [L106]
pub struct Exporter [L110-112]
 workspace [L111]
impl Exporter<FS> [L114-457]
 pub fn new [L116-120]
 pub async fn plan_export [L124-160]
 async fn plan_file_recursive [L164-243]
 fn check_visibility [L247-276]
 fn get_exclusion_reason [L279-305]
 pub async fn execute_export [L310-366]
 fn filter_contents_in_file [L369-418]
 fn remove_audience_property [L421-456]
pub struct ExportStats [L461-466]
 pub files_exported [L463]
 pub files_excluded [L465]
impl std::fmt::Display for ExportStats [L468-476]
 fn fmt [L469-475]
mod tests [L479-607]
 type TestFs [L483]
 fn make_test_fs [L485-487]
 fn test_private_file_excluded [L490-515]
 fn test_audience_inheritance [L518-543]
 fn test_no_audience_defaults_to_private [L546-567]
 fn test_filtered_contents_tracked [L570-606]

Showing symbols 1-50 (total symbols: 50)


NEXT STEPS: To read a specific symbol's implementation, call read_file with the same path plus start_line and end_line from the outline above.
For example, to read a function shown as [L100-150], use start_line: 100 and end_line: 150.
```


**Tool Call: Read file `crates/diaryx_core/src/export.rs` (lines 20-75)**
Status: Completed

```diaryx-core/crates/diaryx_core/src/export.rs
pub struct ExportPlan {
    /// Files that will be exported
    pub included: Vec<ExportFile>,
    /// Files that were filtered out (with reason)
    pub excluded: Vec<ExcludedFile>,
    /// The audience being exported for
    pub audience: String,
    /// Source workspace root
    pub source_root: PathBuf,
    /// Destination directory
    pub destination: PathBuf,
}

/// A file to be exported
#[derive(Debug, Clone, Serialize)]
pub struct ExportFile {
    /// Original path in the workspace
    pub source_path: PathBuf,
    /// Path relative to workspace root
    pub relative_path: PathBuf,
    /// Destination path
    pub dest_path: PathBuf,
    /// Contents entries that will be filtered out (if any)
    pub filtered_contents: Vec<String>,
}

/// A file that was excluded from export
#[derive(Debug, Clone, Serialize)]
pub struct ExcludedFile {
    /// Path to the excluded file
    pub path: PathBuf,
    /// Reason for exclusion
    pub reason: ExclusionReason,
}

/// Why a file was excluded
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ExclusionReason {
    /// File is marked as private
    Private,
    /// File's audience doesn't include the target audience
    AudienceMismatch {
        /// What audiences are intended to view the document
        file_audience: Vec<String>,
        /// What audiences were requested for the export
        requested: String,
    },
    /// File inherits private from parent
    InheritedPrivate {
        /// Path to the parent that was marked as `private`
        from: PathBuf,
    },
    /// File has no audience and inherits to root which has no audience (default private)
    NoAudienceDefined,
}


```


## Assistant

Now I understand the correct types. Let me fix the async_export.rs:

**Tool Call: crates/diaryx_wasm/src/async_export.rs**
Status: Completed

Diff: crates/diaryx_wasm/src/async_export.rs
```
//! Async export operations for WASM with native Promise support.
//!
//! This module provides async export operations that work directly with
//! `JsAsyncFileSystem`, returning native JavaScript Promises. This enables
//! proper async/await patterns in the web frontend without the need for
//! synchronous wrappers or `block_on`.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { JsAsyncFileSystem, DiaryxAsyncExport } from './wasm/diaryx_wasm.js';
//!
//! // Create filesystem with your storage backend callbacks
//! const fs = new JsAsyncFileSystem({ /* callbacks */ });
//!
//! // Create async export instance
//! const exporter = new DiaryxAsyncExport(fs);
//!
//! // All methods return native Promises
//! const plan = await exporter.plan('workspace', 'public');
//! const files = await exporter.toMemory('workspace', 'public');
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use diaryx_core::export::Exporter;
use diaryx_core::fs::AsyncFileSystem;
use js_sys::Promise;
use serde::Serialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::error::IntoJsResult;
use crate::js_async_fs::JsAsyncFileSystem;

// ============================================================================
// Types
// ============================================================================

/// Export plan entry representing a file to be exported
#[derive(Debug, Serialize)]
pub struct JsAsyncExportPlanEntry {
    /// Source file path
    pub source: String,
    /// Relative path in workspace
    pub relative_path: String,
    /// Destination file path
    pub destination: String,
    /// Contents that will be filtered out
    pub filtered_contents: Vec<String>,
}

impl From<diaryx_core::export::ExportFile> for JsAsyncExportPlanEntry {
    fn from(entry: diaryx_core::export::ExportFile) -> Self {
        JsAsyncExportPlanEntry {
            source: entry.source_path.to_string_lossy().to_string(),
            relative_path: entry.relative_path.to_string_lossy().to_string(),
            destination: entry.dest_path.to_string_lossy().to_string(),
            filtered_contents: entry.filtered_contents,
        }
    }
}

/// Excluded file entry
#[derive(Debug, Serialize)]
pub struct JsAsyncExcludedFile {
    /// File path
    pub path: String,
    /// Reason for exclusion
    pub reason: String,
}

impl From<diaryx_core::export::ExcludedFile> for JsAsyncExcludedFile {
    fn from(entry: diaryx_core::export::ExcludedFile) -> Self {
        JsAsyncExcludedFile {
            path: entry.path.to_string_lossy().to_string(),
            reason: entry.reason.to_string(),
        }
    }
}

/// Export plan returned to JavaScript
#[derive(Debug, Serialize)]
pub struct JsAsyncExportPlan {
    /// Files to be exported
    pub included: Vec<JsAsyncExportPlanEntry>,
    /// Files that were excluded
    pub excluded: Vec<JsAsyncExcludedFile>,
    /// Target audience
    pub audience: String,
    /// Total number of files to export
    pub total_files: usize,
    /// Total number of excluded files
    pub total_excluded: usize,
}

impl From<diaryx_core::export::ExportPlan> for JsAsyncExportPlan {
    fn from(plan: diaryx_core::export::ExportPlan) -> Self {
        let total_files = plan.included.len();
        let total_excluded = plan.excluded.len();
        JsAsyncExportPlan {
            included: plan.included.into_iter().map(JsAsyncExportPlanEntry::from).collect(),
            excluded: plan.excluded.into_iter().map(JsAsyncExcludedFile::from).collect(),
            audience: plan.audience,
            total_files,
            total_excluded,
        }
    }
}

/// Exported file content
#[derive(Debug, Serialize)]
pub struct JsAsyncExportedFile {
    /// File path
    pub path: String,
    /// File content
    pub content: String,
}

/// Exported binary file
#[derive(Debug, Serialize)]
pub struct JsAsyncBinaryExportFile {
    /// File path
    pub path: String,
    /// Binary content as array of bytes
    pub data: Vec<u8>,
}

/// HTML export result
#[derive(Debug, Serialize)]
pub struct JsAsyncHtmlExport {
    /// HTML files
    pub files: Vec<JsAsyncExportedFile>,
    /// Total number of files
    pub total_files: usize,
}

// ============================================================================
// DiaryxAsyncExport Class
// ============================================================================

/// Async export operations with native Promise support.
///
/// Unlike `DiaryxExport` which uses `block_on` internally, this class
/// returns true JavaScript Promises that can be properly awaited.
#[wasm_bindgen]
pub struct DiaryxAsyncExport {
    fs: JsAsyncFileSystem,
}

#[wasm_bindgen]
impl DiaryxAsyncExport {
    /// Create a new DiaryxAsyncExport with the provided filesystem.
    #[wasm_bindgen(constructor)]
    pub fn new(fs: JsAsyncFileSystem) -> Self {
        Self { fs }
    }

    /// Get available audiences from the workspace.
    ///
    /// Scans all files in the workspace and returns unique audience values
    /// found in frontmatter.
    ///
    /// @param root_path - Path to the workspace root
    /// @returns Promise resolving to array of audience names
    #[wasm_bindgen(js_name = "getAudiences")]
    pub fn get_audiences(&self, root_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let path = PathBuf::from(&root_path);

            // Find all markdown files and collect unique audiences
            let mut audiences = HashSet::new();
            
            let md_files = fs
                .list_md_files_recursive(&path)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            for file_path in md_files {
                let content = fs
                    .read_to_string(&file_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                if let Some(file_audiences) = get_audiences_from_frontmatter(&content) {
                    for audience in file_audiences {
                        audiences.insert(audience);
                    }
                }
            }

            let audiences_vec: Vec<String> = audiences.into_iter().collect();
            serde_wasm_bindgen::to_value(&audiences_vec).js_err()
        })
    }

    /// Plan an export operation.
    ///
    /// Returns a list of files that would be exported for the given audience.
    ///
    /// @param root_path - Path to the workspace root
    /// @param audience - Target audience to export for
    /// @returns Promise resolving to export plan
    #[wasm_bindgen]
    pub fn plan(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            // Plan export to a temporary destination (we won't actually use it)
            let dest = PathBuf::from("_export_temp");
            let plan = exporter
                .plan_export(&path, &audience, &dest)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_plan: JsAsyncExportPlan = plan.into();
            serde_wasm_bindgen::to_value(&js_plan).js_err()
        })
    }

    /// Export files to memory (returns file contents).
    ///
    /// Returns an array of files with their content, suitable for
    /// downloading or further processing.
    ///
    /// @param root_path - Path to the workspace root
    /// @param audience - Target audience to export for
    /// @returns Promise resolving to array of exported files
    #[wasm_bindgen(js_name = "toMemory")]
    pub fn to_memory(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            // Plan the export
            let dest = PathBuf::from("_export");
            let plan = exporter
                .plan_export(&path, &audience, &dest)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Read each file's content
            let mut exported_files = Vec::new();
            for entry in plan.included {
                let content = fs
                    .read_to_string(&entry.source_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                exported_files.push(JsAsyncExportedFile {
                    path: entry.relative_path.to_string_lossy().to_string(),
                    content,
                });
            }

            serde_wasm_bindgen::to_value(&exported_files).js_err()
        })
    }

    /// Export files to HTML format.
    ///
    /// Converts markdown files to HTML using comrak.
    ///
    /// @param root_path - Path to the workspace root
    /// @param audience - Target audience to export for
    /// @returns Promise resolving to array of HTML files
    #[wasm_bindgen(js_name = "toHtml")]
    pub fn to_html(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            // Plan the export
            let dest = PathBuf::from("_export");
            let plan = exporter
                .plan_export(&path, &audience, &dest)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Convert each file to HTML
            let mut html_files = Vec::new();
            for entry in plan.included {
                let content = fs
                    .read_to_string(&entry.source_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                // Extract body (remove frontmatter) and convert to HTML
                let body = extract_body(&content);
                let html = markdown_to_html(&body);

                // Change extension to .html
                let html_path = entry
                    .relative_path
                    .with_extension("html")
                    .to_string_lossy()
                    .to_string();

                html_files.push(JsAsyncExportedFile {
                    path: html_path,
                    content: html,
                });
            }

            let result = JsAsyncHtmlExport {
                total_files: html_files.len(),
                files: html_files,
            };

            serde_wasm_bindgen::to_value(&result).js_err()
        })
    }

    /// Export binary attachments for a given audience.
    ///
    /// Returns binary files (images, etc.) associated with entries
    /// that match the target audience.
    ///
    /// @param root_path - Path to the workspace root
    /// @param audience - Target audience to export for
    /// @returns Promise resolving to array of binary files
    #[wasm_bindgen(js_name = "binaryAttachments")]
    pub fn binary_attachments(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            // Plan the export to get the list of files
            let dest = PathBuf::from("_export");
            let plan = exporter
                .plan_export(&path, &audience, &dest)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Collect attachments from all exported files
            let mut binary_files = Vec::new();
            for entry in plan.included {
                // Read the file and check for attachments in frontmatter
                let content = fs
                    .read_to_string(&entry.source_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                if let Some(attachments) = get_attachments_from_frontmatter(&content) {
                    let source_dir = entry.source_path.parent().unwrap_or(&entry.source_path);

                    for attachment in attachments {
                        let attachment_path = source_dir.join(&attachment);
                        if fs.exists(&attachment_path).await {
                            match fs.read_binary(&attachment_path).await {
                                Ok(data) => {
                                    binary_files.push(JsAsyncBinaryExportFile {
                                        path: attachment,
                                        data,
                                    });
                                }
                                Err(_) => {
                                    // Skip files that can't be read
                                    continue;
                                }
                            }
                        }
                    }
                }
            }

            serde_wasm_bindgen::to_value(&binary_files).js_err()
        })
    }
}

impl Default for DiaryxAsyncExport {
    fn default() -> Self {
        Self {
            fs: JsAsyncFileSystem::new(JsValue::NULL),
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Extract body content from markdown (removes frontmatter)
fn extract_body(content: &str) -> String {
    if !content.starts_with("---") {
        return content.to_string();
    }

    // Find the closing ---
    if let Some(end_idx) = content[3..].find("\n---") {
        let after_frontmatter = &content[3 + end_idx + 4..];
        after_frontmatter.trim_start().to_string()
    } else {
        content.to_string()
    }
}

/// Convert markdown to HTML using comrak
fn markdown_to_html(markdown: &str) -> String {
    use comrak::{markdown_to_html as comrak_md_to_html, Options};
    
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.render.unsafe_ = true;

    comrak_md_to_html(markdown, &options)
}

/// Get audiences list from frontmatter
fn get_audiences_from_frontmatter(content: &str) -> Option<Vec<String>> {
    if !content.starts_with("---") {
        return None;
    }

    // Find the closing ---
    let end_idx = content[3..].find("\n---")?;
    let frontmatter = &content[3..3 + end_idx];

    // Parse YAML
    let yaml: serde_yaml::Value = serde_yaml::from_str(frontmatter).ok()?;
    let audience = yaml.get("audience")?;

    match audience {
        serde_yaml::Value::Sequence(seq) => {
            Some(
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
            )
        }
        serde_yaml::Value::String(s) => Some(vec![s.clone()]),
        _ => None,
    }
}

/// Get attachments list from frontmatter
fn get_attachments_from_frontmatter(content: &str) -> Option<Vec<String>> {
    if !content.starts_with("---") {
        return None;
    }

    // Find the closing ---
    let end_idx = content[3..].find("\n---")?;
    let frontmatter = &content[3..3 + end_idx];

    // Parse YAML
    let yaml: serde_yaml::Value = serde_yaml::from_str(frontmatter).ok()?;
    let attachments = yaml.get("attachments")?;

    match attachments {
        serde_yaml::Value::Sequence(seq) => {
            Some(
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
            )
        }
        _ => None,
    }
}

// ============================================================================
// TypeScript Type Definitions
// ============================================================================

#[wasm_bindgen(typescript_custom_section)]
const TS_APPEND_CONTENT: &'static str = r#"
/**
 * Export plan entry representing a file to be exported.
 */
export interface AsyncExportPlanEntry {
    /** Source file path */
    source: string;
    /** Relative path in workspace */
    relative_path: string;
    /** Destination file path */
    destination: string;
    /** Contents that will be filtered out */
    filtered_contents: string[];
}

/**
 * Excluded file entry.
 */
export interface AsyncExcludedFile {
    /** File path */
    path: string;
    /** Reason for exclusion */
    reason: string;
}

/**
 * Export plan containing files to be exported.
 */
export interface AsyncExportPlan {
    /** Files to be exported */
    included: AsyncExportPlanEntry[];
    /** Files that were excluded */
    excluded: AsyncExcludedFile[];
    /** Target audience */
    audience: string;
    /** Total number of files to export */
    total_files: number;
    /** Total number of excluded files */
    total_excluded: number;
}

/**
 * Exported file with content.
 */
export interface AsyncExportedFile {
    /** File path */
    path: string;
    /** File content */
    content: string;
}

/**
 * Exported binary file.
 */
export interface AsyncBinaryExportFile {
    /** File path */
    path: string;
    /** Binary content as byte array */
    data: number[];
}

/**
 * HTML export result.
 */
export interface AsyncHtmlExport {
    /** HTML files */
    files: AsyncExportedFile[];
    /** Total number of files */
    total_files: number;
}
"#;
```


## Assistant

Now let's check for more compilation issues:

**Tool Call: `cd crates/diaryx_wasm && cargo check 2>&1 | head -100`**
Status: Completed

Terminal:
```
warning: profiles for the non root package will be ignored, specify profiles at the workspace root:
package:   /Users/adamharris/Documents/diaryx-core/crates/diaryx_wasm/Cargo.toml
workspace: /Users/adamharris/Documents/diaryx-core/Cargo.toml
    Checking diaryx_wasm v0.7.0 (/Users/adamharris/Documents/diaryx-core/crates/diaryx_wasm)
warning: unused import: `with_fs`
  --> crates/diaryx_wasm/src/export.rs:14:45
   |
14 | use crate::state::{block_on, with_async_fs, with_fs};
   |                                             ^^^^^^^
   |
   = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused import: `std::pin::Pin`
  --> crates/diaryx_wasm/src/js_async_fs.rs:63:5
   |
63 | use std::pin::Pin;
   |     ^^^^^^^^^^^^^

warning: unused import: `std::pin::Pin`
 --> crates/diaryx_wasm/src/state.rs:5:5
  |
5 | use std::pin::Pin;
  |     ^^^^^^^^^^^^^

warning: unused import: `with_fs`
  --> crates/diaryx_wasm/src/workspace.rs:12:45
   |
12 | use crate::state::{block_on, with_async_fs, with_fs};
   |                                             ^^^^^^^

error[E0609]: no field `unsafe_` on type `Render`
   --> crates/diaryx_wasm/src/async_export.rs:420:20
    |
420 |     options.render.unsafe_ = true;
    |                    ^^^^^^^ unknown field
    |
help: a field with a similar name exists
    |
420 -     options.render.unsafe_ = true;
420 +     options.render.unsafe = true;
    |

error[E0609]: no field `line` on type `SearchMatch`
  --> crates/diaryx_wasm/src/async_search.rs:55:21
   |
55 |             line: m.line,
   |                     ^^^^ unknown field
   |
   = note: available fields are: `line_number`, `line_content`, `match_start`, `match_end`

error[E0609]: no field `content` on type `SearchMatch`
  --> crates/diaryx_wasm/src/async_search.rs:56:24
   |
56 |             content: m.content,
   |                        ^^^^^^^ unknown field
   |
   = note: available fields are: `line_number`, `line_content`, `match_start`, `match_end`

error[E0609]: no field `in_frontmatter` on type `SearchMatch`
  --> crates/diaryx_wasm/src/async_search.rs:57:31
   |
57 |             in_frontmatter: m.in_frontmatter,
   |                               ^^^^^^^^^^^^^^ unknown field
   |
   = note: available fields are: `line_number`, `line_content`, `match_start`, `match_end`

error[E0615]: attempted to take value of method `total_matches` on type `SearchResults`
  --> crates/diaryx_wasm/src/async_search.rs:98:30
   |
98 |             total_matches: r.total_matches,
   |                              ^^^^^^^^^^^^^ method, not a field
   |
help: use parentheses to call the method
   |
98 |             total_matches: r.total_matches(),
   |                                           ++

error[E0061]: this function takes 1 argument but 2 arguments were supplied
   --> crates/diaryx_wasm/src/async_search.rs:173:21
    |
173 |                     SearchQuery::frontmatter(&pattern, &prop)
    |                     ^^^^^^^^^^^^^^^^^^^^^^^^           ----- unexpected argument #2 of type `&std::string::String`
    |
note: associated function defined here
   --> /Users/adamharris/Documents/diaryx-core/crates/diaryx_core/src/search.rs:51:12
    |
 51 |     pub fn frontmatter(pattern: impl Into<String>) -> Self {
    |            ^^^^^^^^^^^
help: remove the extra argument
    |
173 -                     SearchQuery::frontmatter(&pattern, &prop)
173 +                     SearchQuery::frontmatter(&pattern)
    |

error[E0061]: this method takes 1 argument but 0 arguments were supplied
   --> crates/diaryx_wasm/src/async_search.rs:182:23
    |
182 |                 query.case_sensitive()
    |                       ^^^^^^^^^^^^^^-- argument #1 of type `bool` is missing
    |

```


## Assistant

Let me check the search types in diaryx_core:

**Tool Call: Read file `crates/diaryx_core/src/search.rs` (lines 1-100)**
Status: Completed

```diaryx-core/crates/diaryx_core/src/search.rs
//! Search functionality for diaryx workspaces
//!
//! Provides searching through workspace files by content or frontmatter properties.
//!
//! # Async-first Design
//!
//! This module uses `AsyncFileSystem` for all filesystem operations.
//! For synchronous contexts (CLI, tests), wrap a sync filesystem with
//! `SyncToAsyncFs` and use `futures_lite::future::block_on()`.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::fs::AsyncFileSystem;
use crate::workspace::Workspace;

/// Represents a search query configuration
#[derive(Debug, Clone, Serialize)]
pub struct SearchQuery {
    /// The pattern to search for
    pub pattern: String,
    /// Whether the search is case-sensitive
    pub case_sensitive: bool,
    /// Search mode: content, frontmatter, or specific property
    pub mode: SearchMode,
}

/// What to search in files
#[derive(Debug, Clone, Serialize)]
pub enum SearchMode {
    /// Search only the body content (after frontmatter)
    Content,
    /// Search all frontmatter properties
    Frontmatter,
    /// Search a specific frontmatter property
    Property(String),
}

impl SearchQuery {
    /// Create a new content search query
    pub fn content(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            case_sensitive: false,
            mode: SearchMode::Content,
        }
    }

    /// Create a new frontmatter search query
    pub fn frontmatter(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            case_sensitive: false,
            mode: SearchMode::Frontmatter,
        }
    }

    /// Create a search query for a specific property
    pub fn property(pattern: impl Into<String>, property_name: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            case_sensitive: false,
            mode: SearchMode::Property(property_name.into()),
        }
    }

    /// Set case sensitivity
    pub fn case_sensitive(mut self, case_sensitive: bool) -> Self {
        self.case_sensitive = case_sensitive;
        self
    }
}

/// A single match within a file
#[derive(Debug, Clone, Serialize)]
pub struct SearchMatch {
    /// Line number (1-based)
    pub line_number: usize,
    /// The full line content
    pub line_content: String,
    /// Column where match starts (0-based)
    pub match_start: usize,
    /// Column where match ends (0-based, exclusive)
    pub match_end: usize,
}

/// Search results for a single file
#[derive(Debug, Clone, Serialize)]
pub struct FileSearchResult {
    /// Path to the file
    pub path: PathBuf,
    /// Title from frontmatter (if available)
    pub title: Option<String>,
    /// All matches found in this file
    pub matches: Vec<SearchMatch>,
}

impl FileSearchResult {
    /// Returns true if this result has any matches

```


**Tool Call: Read file `crates/diaryx_core/src/search.rs` (lines 100-160)**
Status: Completed

```diaryx-core/crates/diaryx_core/src/search.rs
    /// Returns true if this result has any matches
    pub fn has_matches(&self) -> bool {
        !self.matches.is_empty()
    }

    /// Returns the number of matches
    pub fn match_count(&self) -> usize {
        self.matches.len()
    }
}

/// Aggregated search results
#[derive(Debug, Clone, Serialize)]
pub struct SearchResults {
    /// Results per file (only files with matches)
    pub files: Vec<FileSearchResult>,
    /// Total number of files searched
    pub files_searched: usize,
}

impl SearchResults {
    /// Create empty results
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            files_searched: 0,
        }
    }

    /// Total number of matches across all files
    pub fn total_matches(&self) -> usize {
        self.files.iter().map(|f| f.match_count()).sum()
    }

    /// Number of files with matches
    pub fn files_with_matches(&self) -> usize {
        self.files.len()
    }
}

impl Default for SearchResults {
    fn default() -> Self {
        Self::new()
    }
}

/// Searcher for workspace files (async-first)
pub struct Searcher<FS: AsyncFileSystem> {
    fs: FS,
}

impl<FS: AsyncFileSystem> Searcher<FS> {
    /// Create a new searcher
    pub fn new(fs: FS) -> Self {
        Self { fs }
    }

    /// Search the entire workspace starting from the root index
    pub async fn search_workspace(
        &self,
        workspace_root: &Path,

```


## Assistant

Now I understand the correct types. Let me fix the async_search.rs:

**Tool Call: crates/diaryx_wasm/src/async_search.rs**
Status: Completed

Diff: crates/diaryx_wasm/src/async_search.rs
```
//! Async search operations for WASM with native Promise support.
//!
//! This module provides async search operations that work directly with
//! `JsAsyncFileSystem`, returning native JavaScript Promises. This enables
//! proper async/await patterns in the web frontend without the need for
//! synchronous wrappers or `block_on`.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { JsAsyncFileSystem, DiaryxAsyncSearch } from './wasm/diaryx_wasm.js';
//!
//! // Create filesystem with your storage backend callbacks
//! const fs = new JsAsyncFileSystem({ /* callbacks */ });
//!
//! // Create async search instance
//! const search = new DiaryxAsyncSearch(fs);
//!
//! // All methods return native Promises
//! const results = await search.search('workspace', 'my query', {
//!     searchFrontmatter: true,
//!     caseSensitive: false
//! });
//! ```

use std::path::PathBuf;

use diaryx_core::search::{SearchQuery, Searcher};
use js_sys::Promise;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::error::IntoJsResult;
use crate::js_async_fs::JsAsyncFileSystem;

// ============================================================================
// Types
// ============================================================================

/// A single match within a file
#[derive(Debug, Serialize)]
pub struct JsAsyncSearchMatch {
    /// Line number (1-indexed)
    pub line_number: usize,
    /// The matching line content
    pub line_content: String,
    /// Column where match starts (0-based)
    pub match_start: usize,
    /// Column where match ends (0-based, exclusive)
    pub match_end: usize,
}

impl From<diaryx_core::search::SearchMatch> for JsAsyncSearchMatch {
    fn from(m: diaryx_core::search::SearchMatch) -> Self {
        JsAsyncSearchMatch {
            line_number: m.line_number,
            line_content: m.line_content,
            match_start: m.match_start,
            match_end: m.match_end,
        }
    }
}

/// Search result for a single file
#[derive(Debug, Serialize)]
pub struct JsAsyncFileSearchResult {
    /// Path to the file
    pub path: String,
    /// Title from frontmatter, if available
    pub title: Option<String>,
    /// Matches found in this file
    pub matches: Vec<JsAsyncSearchMatch>,
}

impl From<diaryx_core::search::FileSearchResult> for JsAsyncFileSearchResult {
    fn from(r: diaryx_core::search::FileSearchResult) -> Self {
        JsAsyncFileSearchResult {
            path: r.path.to_string_lossy().to_string(),
            title: r.title,
            matches: r.matches.into_iter().map(JsAsyncSearchMatch::from).collect(),
        }
    }
}

/// Complete search results
#[derive(Debug, Serialize)]
pub struct JsAsyncSearchResults {
    /// Files with matches
    pub files: Vec<JsAsyncFileSearchResult>,
    /// Total number of matches across all files
    pub total_matches: usize,
    /// Number of files searched
    pub files_searched: usize,
}

impl From<diaryx_core::search::SearchResults> for JsAsyncSearchResults {
    fn from(r: diaryx_core::search::SearchResults) -> Self {
        let total_matches = r.total_matches();
        JsAsyncSearchResults {
            files: r.files.into_iter().map(JsAsyncFileSearchResult::from).collect(),
            total_matches,
            files_searched: r.files_searched,
        }
    }
}

/// Search options from JavaScript
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JsAsyncSearchOptions {
    /// Search in frontmatter (default: false, searches content)
    #[serde(default)]
    pub search_frontmatter: bool,
    /// Search only in a specific frontmatter property
    pub property: Option<String>,
    /// Case-sensitive search (default: false)
    #[serde(default)]
    pub case_sensitive: bool,
}

// ============================================================================
// DiaryxAsyncSearch Class
// ============================================================================

/// Async search operations with native Promise support.
///
/// Unlike `DiaryxSearch` which uses `block_on` internally, this class
/// returns true JavaScript Promises that can be properly awaited.
#[wasm_bindgen]
pub struct DiaryxAsyncSearch {
    fs: JsAsyncFileSystem,
}

#[wasm_bindgen]
impl DiaryxAsyncSearch {
    /// Create a new DiaryxAsyncSearch with the provided filesystem.
    #[wasm_bindgen(constructor)]
    pub fn new(fs: JsAsyncFileSystem) -> Self {
        Self { fs }
    }

    /// Search for content in the workspace.
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @param pattern - The search pattern (string or regex)
    /// @param options - Optional search options
    /// @returns Promise resolving to search results
    #[wasm_bindgen]
    pub fn search(
        &self,
        workspace_path: String,
        pattern: String,
        options: JsValue,
    ) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let searcher = Searcher::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            // Parse options
            let opts: JsAsyncSearchOptions = if options.is_undefined() || options.is_null() {
                JsAsyncSearchOptions::default()
            } else {
                serde_wasm_bindgen::from_value(options)
                    .map_err(|e| JsValue::from_str(&format!("Invalid options: {}", e)))?
            };

            // Build search query
            let query = if let Some(prop) = opts.property {
                SearchQuery::property(&pattern, &prop)
            } else if opts.search_frontmatter {
                SearchQuery::frontmatter(&pattern)
            } else {
                SearchQuery::content(&pattern)
            };

            let query = query.case_sensitive(opts.case_sensitive);

            // Execute search
            let results = searcher
                .search(&root_path, &query)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_results: JsAsyncSearchResults = results.into();
            serde_wasm_bindgen::to_value(&js_results).js_err()
        })
    }

    /// Search for content in the workspace (content only).
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @param pattern - The search pattern
    /// @returns Promise resolving to search results
    #[wasm_bindgen(js_name = "searchContent")]
    pub fn search_content(&self, workspace_path: String, pattern: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let searcher = Searcher::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            let query = SearchQuery::content(&pattern);
            let results = searcher
                .search(&root_path, &query)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_results: JsAsyncSearchResults = results.into();
            serde_wasm_bindgen::to_value(&js_results).js_err()
        })
    }

    /// Search for a specific frontmatter property value.
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @param property - The frontmatter property to search
    /// @param pattern - The search pattern
    /// @returns Promise resolving to search results
    #[wasm_bindgen(js_name = "searchProperty")]
    pub fn search_property(
        &self,
        workspace_path: String,
        property: String,
        pattern: String,
    ) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let searcher = Searcher::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            let query = SearchQuery::property(&pattern, &property);
            let results = searcher
                .search(&root_path, &query)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_results: JsAsyncSearchResults = results.into();
            serde_wasm_bindgen::to_value(&js_results).js_err()
        })
    }

    /// Search with case sensitivity.
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @param pattern - The search pattern
    /// @param case_sensitive - Whether to match case
    /// @returns Promise resolving to search results
    #[wasm_bindgen(js_name = "searchWithCase")]
    pub fn search_with_case(
        &self,
        workspace_path: String,
        pattern: String,
        case_sensitive: bool,
    ) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let searcher = Searcher::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            let query = SearchQuery::content(&pattern).case_sensitive(case_sensitive);

            let results = searcher
                .search(&root_path, &query)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_results: JsAsyncSearchResults = results.into();
            serde_wasm_bindgen::to_value(&js_results).js_err()
        })
    }
}

impl Default for DiaryxAsyncSearch {
    fn default() -> Self {
        Self {
            fs: JsAsyncFileSystem::new(JsValue::NULL),
        }
    }
}

// ============================================================================
// TypeScript Type Definitions
// ============================================================================

#[wasm_bindgen(typescript_custom_section)]
const TS_APPEND_CONTENT: &'static str = r#"
/**
 * Options for search operations.
 */
export interface AsyncSearchOptions {
    /** Search in frontmatter instead of content (default: false) */
    searchFrontmatter?: boolean;
    /** Search only in a specific frontmatter property */
    property?: string;
    /** Case-sensitive search (default: false) */
    caseSensitive?: boolean;
}

/**
 * A single match within a file.
 */
export interface AsyncSearchMatch {
    /** Line number (1-indexed) */
    line_number: number;
    /** The matching line content */
    line_content: string;
    /** Column where match starts (0-based) */
    match_start: number;
    /** Column where match ends (0-based, exclusive) */
    match_end: number;
}

/**
 * Search result for a single file.
 */
export interface AsyncFileSearchResult {
    /** Path to the file */
    path: string;
    /** Title from frontmatter, if available */
    title: string | null;
    /** Matches found in this file */
    matches: AsyncSearchMatch[];
}

/**
 * Complete search results.
 */
export interface AsyncSearchResults {
    /** Files with matches */
    files: AsyncFileSearchResult[];
    /** Total number of matches across all files */
    total_matches: number;
    /** Number of files searched */
    files_searched: number;
}
"#;
```


## Assistant

Now let's fix the comrak unsafe field name:

**Tool Call: crates/diaryx_wasm/src/async_export.rs**
Status: Completed

Diff: crates/diaryx_wasm/src/async_export.rs
```
//! Async export operations for WASM with native Promise support.
//!
//! This module provides async export operations that work directly with
//! `JsAsyncFileSystem`, returning native JavaScript Promises. This enables
//! proper async/await patterns in the web frontend without the need for
//! synchronous wrappers or `block_on`.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { JsAsyncFileSystem, DiaryxAsyncExport } from './wasm/diaryx_wasm.js';
//!
//! // Create filesystem with your storage backend callbacks
//! const fs = new JsAsyncFileSystem({ /* callbacks */ });
//!
//! // Create async export instance
//! const exporter = new DiaryxAsyncExport(fs);
//!
//! // All methods return native Promises
//! const plan = await exporter.plan('workspace', 'public');
//! const files = await exporter.toMemory('workspace', 'public');
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use diaryx_core::export::Exporter;
use diaryx_core::fs::AsyncFileSystem;
use js_sys::Promise;
use serde::Serialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::error::IntoJsResult;
use crate::js_async_fs::JsAsyncFileSystem;

// ============================================================================
// Types
// ============================================================================

/// Export plan entry representing a file to be exported
#[derive(Debug, Serialize)]
pub struct JsAsyncExportPlanEntry {
    /// Source file path
    pub source: String,
    /// Relative path in workspace
    pub relative_path: String,
    /// Destination file path
    pub destination: String,
    /// Contents that will be filtered out
    pub filtered_contents: Vec<String>,
}

impl From<diaryx_core::export::ExportFile> for JsAsyncExportPlanEntry {
    fn from(entry: diaryx_core::export::ExportFile) -> Self {
        JsAsyncExportPlanEntry {
            source: entry.source_path.to_string_lossy().to_string(),
            relative_path: entry.relative_path.to_string_lossy().to_string(),
            destination: entry.dest_path.to_string_lossy().to_string(),
            filtered_contents: entry.filtered_contents,
        }
    }
}

/// Excluded file entry
#[derive(Debug, Serialize)]
pub struct JsAsyncExcludedFile {
    /// File path
    pub path: String,
    /// Reason for exclusion
    pub reason: String,
}

impl From<diaryx_core::export::ExcludedFile> for JsAsyncExcludedFile {
    fn from(entry: diaryx_core::export::ExcludedFile) -> Self {
        JsAsyncExcludedFile {
            path: entry.path.to_string_lossy().to_string(),
            reason: entry.reason.to_string(),
        }
    }
}

/// Export plan returned to JavaScript
#[derive(Debug, Serialize)]
pub struct JsAsyncExportPlan {
    /// Files to be exported
    pub included: Vec<JsAsyncExportPlanEntry>,
    /// Files that were excluded
    pub excluded: Vec<JsAsyncExcludedFile>,
    /// Target audience
    pub audience: String,
    /// Total number of files to export
    pub total_files: usize,
    /// Total number of excluded files
    pub total_excluded: usize,
}

impl From<diaryx_core::export::ExportPlan> for JsAsyncExportPlan {
    fn from(plan: diaryx_core::export::ExportPlan) -> Self {
        let total_files = plan.included.len();
        let total_excluded = plan.excluded.len();
        JsAsyncExportPlan {
            included: plan.included.into_iter().map(JsAsyncExportPlanEntry::from).collect(),
            excluded: plan.excluded.into_iter().map(JsAsyncExcludedFile::from).collect(),
            audience: plan.audience,
            total_files,
            total_excluded,
        }
    }
}

/// Exported file content
#[derive(Debug, Serialize)]
pub struct JsAsyncExportedFile {
    /// File path
    pub path: String,
    /// File content
    pub content: String,
}

/// Exported binary file
#[derive(Debug, Serialize)]
pub struct JsAsyncBinaryExportFile {
    /// File path
    pub path: String,
    /// Binary content as array of bytes
    pub data: Vec<u8>,
}

/// HTML export result
#[derive(Debug, Serialize)]
pub struct JsAsyncHtmlExport {
    /// HTML files
    pub files: Vec<JsAsyncExportedFile>,
    /// Total number of files
    pub total_files: usize,
}

// ============================================================================
// DiaryxAsyncExport Class
// ============================================================================

/// Async export operations with native Promise support.
///
/// Unlike `DiaryxExport` which uses `block_on` internally, this class
/// returns true JavaScript Promises that can be properly awaited.
#[wasm_bindgen]
pub struct DiaryxAsyncExport {
    fs: JsAsyncFileSystem,
}

#[wasm_bindgen]
impl DiaryxAsyncExport {
    /// Create a new DiaryxAsyncExport with the provided filesystem.
    #[wasm_bindgen(constructor)]
    pub fn new(fs: JsAsyncFileSystem) -> Self {
        Self { fs }
    }

    /// Get available audiences from the workspace.
    ///
    /// Scans all files in the workspace and returns unique audience values
    /// found in frontmatter.
    ///
    /// @param root_path - Path to the workspace root
    /// @returns Promise resolving to array of audience names
    #[wasm_bindgen(js_name = "getAudiences")]
    pub fn get_audiences(&self, root_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let path = PathBuf::from(&root_path);

            // Find all markdown files and collect unique audiences
            let mut audiences = HashSet::new();
            
            let md_files = fs
                .list_md_files_recursive(&path)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            for file_path in md_files {
                let content = fs
                    .read_to_string(&file_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                if let Some(file_audiences) = get_audiences_from_frontmatter(&content) {
                    for audience in file_audiences {
                        audiences.insert(audience);
                    }
                }
            }

            let audiences_vec: Vec<String> = audiences.into_iter().collect();
            serde_wasm_bindgen::to_value(&audiences_vec).js_err()
        })
    }

    /// Plan an export operation.
    ///
    /// Returns a list of files that would be exported for the given audience.
    ///
    /// @param root_path - Path to the workspace root
    /// @param audience - Target audience to export for
    /// @returns Promise resolving to export plan
    #[wasm_bindgen]
    pub fn plan(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            // Plan export to a temporary destination (we won't actually use it)
            let dest = PathBuf::from("_export_temp");
            let plan = exporter
                .plan_export(&path, &audience, &dest)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_plan: JsAsyncExportPlan = plan.into();
            serde_wasm_bindgen::to_value(&js_plan).js_err()
        })
    }

    /// Export files to memory (returns file contents).
    ///
    /// Returns an array of files with their content, suitable for
    /// downloading or further processing.
    ///
    /// @param root_path - Path to the workspace root
    /// @param audience - Target audience to export for
    /// @returns Promise resolving to array of exported files
    #[wasm_bindgen(js_name = "toMemory")]
    pub fn to_memory(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            // Plan the export
            let dest = PathBuf::from("_export");
            let plan = exporter
                .plan_export(&path, &audience, &dest)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Read each file's content
            let mut exported_files = Vec::new();
            for entry in plan.included {
                let content = fs
                    .read_to_string(&entry.source_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                exported_files.push(JsAsyncExportedFile {
                    path: entry.relative_path.to_string_lossy().to_string(),
                    content,
                });
            }

            serde_wasm_bindgen::to_value(&exported_files).js_err()
        })
    }

    /// Export files to HTML format.
    ///
    /// Converts markdown files to HTML using comrak.
    ///
    /// @param root_path - Path to the workspace root
    /// @param audience - Target audience to export for
    /// @returns Promise resolving to array of HTML files
    #[wasm_bindgen(js_name = "toHtml")]
    pub fn to_html(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            // Plan the export
            let dest = PathBuf::from("_export");
            let plan = exporter
                .plan_export(&path, &audience, &dest)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Convert each file to HTML
            let mut html_files = Vec::new();
            for entry in plan.included {
                let content = fs
                    .read_to_string(&entry.source_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                // Extract body (remove frontmatter) and convert to HTML
                let body = extract_body(&content);
                let html = markdown_to_html(&body);

                // Change extension to .html
                let html_path = entry
                    .relative_path
                    .with_extension("html")
                    .to_string_lossy()
                    .to_string();

                html_files.push(JsAsyncExportedFile {
                    path: html_path,
                    content: html,
                });
            }

            let result = JsAsyncHtmlExport {
                total_files: html_files.len(),
                files: html_files,
            };

            serde_wasm_bindgen::to_value(&result).js_err()
        })
    }

    /// Export binary attachments for a given audience.
    ///
    /// Returns binary files (images, etc.) associated with entries
    /// that match the target audience.
    ///
    /// @param root_path - Path to the workspace root
    /// @param audience - Target audience to export for
    /// @returns Promise resolving to array of binary files
    #[wasm_bindgen(js_name = "binaryAttachments")]
    pub fn binary_attachments(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            // Plan the export to get the list of files
            let dest = PathBuf::from("_export");
            let plan = exporter
                .plan_export(&path, &audience, &dest)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Collect attachments from all exported files
            let mut binary_files = Vec::new();
            for entry in plan.included {
                // Read the file and check for attachments in frontmatter
                let content = fs
                    .read_to_string(&entry.source_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                if let Some(attachments) = get_attachments_from_frontmatter(&content) {
                    let source_dir = entry.source_path.parent().unwrap_or(&entry.source_path);

                    for attachment in attachments {
                        let attachment_path = source_dir.join(&attachment);
                        if fs.exists(&attachment_path).await {
                            match fs.read_binary(&attachment_path).await {
                                Ok(data) => {
                                    binary_files.push(JsAsyncBinaryExportFile {
                                        path: attachment,
                                        data,
                                    });
                                }
                                Err(_) => {
                                    // Skip files that can't be read
                                    continue;
                                }
                            }
                        }
                    }
                }
            }

            serde_wasm_bindgen::to_value(&binary_files).js_err()
        })
    }
}

impl Default for DiaryxAsyncExport {
    fn default() -> Self {
        Self {
            fs: JsAsyncFileSystem::new(JsValue::NULL),
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Extract body content from markdown (removes frontmatter)
fn extract_body(content: &str) -> String {
    if !content.starts_with("---") {
        return content.to_string();
    }

    // Find the closing ---
    if let Some(end_idx) = content[3..].find("\n---") {
        let after_frontmatter = &content[3 + end_idx + 4..];
        after_frontmatter.trim_start().to_string()
    } else {
        content.to_string()
    }
}

/// Convert markdown to HTML using comrak
fn markdown_to_html(markdown: &str) -> String {
    use comrak::{markdown_to_html as comrak_md_to_html, Options};
    
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    // Note: comrak uses `unsafe_` because `unsafe` is a reserved word in Rust
    // but newer versions might use different field names
    options.render.unsafe_ = true;

    comrak_md_to_html(markdown, &options)
}

/// Get audiences list from frontmatter
fn get_audiences_from_frontmatter(content: &str) -> Option<Vec<String>> {
    if !content.starts_with("---") {
        return None;
    }

    // Find the closing ---
    let end_idx = content[3..].find("\n---")?;
    let frontmatter = &content[3..3 + end_idx];

    // Parse YAML
    let yaml: serde_yaml::Value = serde_yaml::from_str(frontmatter).ok()?;
    let audience = yaml.get("audience")?;

    match audience {
        serde_yaml::Value::Sequence(seq) => {
            Some(
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
            )
        }
        serde_yaml::Value::String(s) => Some(vec![s.clone()]),
        _ => None,
    }
}

/// Get attachments list from frontmatter
fn get_attachments_from_frontmatter(content: &str) -> Option<Vec<String>> {
    if !content.starts_with("---") {
        return None;
    }

    // Find the closing ---
    let end_idx = content[3..].find("\n---")?;
    let frontmatter = &content[3..3 + end_idx];

    // Parse YAML
    let yaml: serde_yaml::Value = serde_yaml::from_str(frontmatter).ok()?;
    let attachments = yaml.get("attachments")?;

    match attachments {
        serde_yaml::Value::Sequence(seq) => {
            Some(
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
            )
        }
        _ => None,
    }
}

// ============================================================================
// TypeScript Type Definitions
// ============================================================================

#[wasm_bindgen(typescript_custom_section)]
const TS_APPEND_CONTENT: &'static str = r#"
/**
 * Export plan entry representing a file to be exported.
 */
export interface AsyncExportPlanEntry {
    /** Source file path */
    source: string;
    /** Relative path in workspace */
    relative_path: string;
    /** Destination file path */
    destination: string;
    /** Contents that will be filtered out */
    filtered_contents: string[];
}

/**
 * Excluded file entry.
 */
export interface AsyncExcludedFile {
    /** File path */
    path: string;
    /** Reason for exclusion */
    reason: string;
}

/**
 * Export plan containing files to be exported.
 */
export interface AsyncExportPlan {
    /** Files to be exported */
    included: AsyncExportPlanEntry[];
    /** Files that were excluded */
    excluded: AsyncExcludedFile[];
    /** Target audience */
    audience: string;
    /** Total number of files to export */
    total_files: number;
    /** Total number of excluded files */
    total_excluded: number;
}

/**
 * Exported file with content.
 */
export interface AsyncExportedFile {
    /** File path */
    path: string;
    /** File content */
    content: string;
}

/**
 * Exported binary file.
 */
export interface AsyncBinaryExportFile {
    /** File path */
    path: string;
    /** Binary content as byte array */
    data: number[];
}

/**
 * HTML export result.
 */
export interface AsyncHtmlExport {
    /** HTML files */
    files: AsyncExportedFile[];
    /** Total number of files */
    total_files: number;
}
"#;
```


**Tool Call: `cd crates/diaryx_wasm && cargo check 2>&1 | head -100`**
Status: Completed

Terminal:
```
warning: profiles for the non root package will be ignored, specify profiles at the workspace root:
package:   /Users/adamharris/Documents/diaryx-core/crates/diaryx_wasm/Cargo.toml
workspace: /Users/adamharris/Documents/diaryx-core/Cargo.toml
    Checking diaryx_wasm v0.7.0 (/Users/adamharris/Documents/diaryx-core/crates/diaryx_wasm)
warning: unused import: `with_fs`
  --> crates/diaryx_wasm/src/export.rs:14:45
   |
14 | use crate::state::{block_on, with_async_fs, with_fs};
   |                                             ^^^^^^^
   |
   = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused import: `std::pin::Pin`
  --> crates/diaryx_wasm/src/js_async_fs.rs:63:5
   |
63 | use std::pin::Pin;
   |     ^^^^^^^^^^^^^

warning: unused import: `std::pin::Pin`
 --> crates/diaryx_wasm/src/state.rs:5:5
  |
5 | use std::pin::Pin;
  |     ^^^^^^^^^^^^^

warning: unused import: `with_fs`
  --> crates/diaryx_wasm/src/workspace.rs:12:45
   |
12 | use crate::state::{block_on, with_async_fs, with_fs};
   |                                             ^^^^^^^

error[E0609]: no field `unsafe_` on type `Render`
   --> crates/diaryx_wasm/src/async_export.rs:422:20
    |
422 |     options.render.unsafe_ = true;
    |                    ^^^^^^^ unknown field
    |
help: a field with a similar name exists
    |
422 -     options.render.unsafe_ = true;
422 +     options.render.unsafe = true;
    |

error[E0599]: no method named `search` found for struct `diaryx_core::search::Searcher<FS>` in the current scope
   --> crates/diaryx_wasm/src/async_search.rs:183:18
    |
182 |               let results = searcher
    |  ___________________________-
183 | |                 .search(&root_path, &query)
    | |_________________-^^^^^^
    |
help: there is a method `search_file` with a similar name
    |
183 |                 .search_file(&root_path, &query)
    |                        +++++

error[E0599]: no method named `search` found for struct `diaryx_core::search::Searcher<FS>` in the current scope
   --> crates/diaryx_wasm/src/async_search.rs:207:18
    |
206 |               let results = searcher
    |  ___________________________-
207 | |                 .search(&root_path, &query)
    | |_________________-^^^^^^
    |
help: there is a method `search_file` with a similar name
    |
207 |                 .search_file(&root_path, &query)
    |                        +++++

error[E0599]: no method named `search` found for struct `diaryx_core::search::Searcher<FS>` in the current scope
   --> crates/diaryx_wasm/src/async_search.rs:237:18
    |
236 |               let results = searcher
    |  ___________________________-
237 | |                 .search(&root_path, &query)
    | |_________________-^^^^^^
    |
help: there is a method `search_file` with a similar name
    |
237 |                 .search_file(&root_path, &query)
    |                        +++++

error[E0599]: no method named `search` found for struct `diaryx_core::search::Searcher<FS>` in the current scope
   --> crates/diaryx_wasm/src/async_search.rs:268:18
    |
267 |               let results = searcher
    |  ___________________________-
268 | |                 .search(&root_path, &query)
    | |_________________-^^^^^^
    |
help: there is a method `search_file` with a similar name
    |
268 |                 .search_file(&root_path, &query)
    |                        +++++

error[E0277]: the trait bound `InMemoryFileSystem: AsyncFileSystem` is not satisfied
  --> crates/diaryx_wasm/src/attachment.rs:44:23
   |
44 |             let app = DiaryxApp::new(fs);
   |                       ^^^^^^^^^ the trait `AsyncFileSystem` is not implemented for `InMemoryFileSystem`
   |

```


## Assistant

Let me check the search module for the correct method name, and let me also look at the comrak render options:

**Tool Call: Read file `crates/diaryx_core/src/search.rs` (lines 150-220)**
Status: Completed

```diaryx-core/crates/diaryx_core/src/search.rs

impl<FS: AsyncFileSystem> Searcher<FS> {
    /// Create a new searcher
    pub fn new(fs: FS) -> Self {
        Self { fs }
    }

    /// Search the entire workspace starting from the root index
    pub async fn search_workspace(
        &self,
        workspace_root: &Path,
        query: &SearchQuery,
    ) -> crate::error::Result<SearchResults>
    where
        FS: Clone,
    {
        let workspace = Workspace::new(self.fs.clone());
        let files = workspace.collect_workspace_files(workspace_root).await?;

        let mut results = SearchResults::new();
        results.files_searched = files.len();

        for file_path in files {
            if let Some(file_result) = self.search_file(&file_path, query).await? {
                if file_result.has_matches() {
                    results.files.push(file_result);
                }
            }
        }

        Ok(results)
    }

    /// Search a single file
    pub async fn search_file(
        &self,
        path: &Path,
        query: &SearchQuery,
    ) -> crate::error::Result<Option<FileSearchResult>> {
        let content = match self.fs.read_to_string(path).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(crate::error::DiaryxError::FileRead {
                    path: path.to_path_buf(),
                    source: e,
                });
            }
        };

        let (frontmatter_str, body, title) = self.parse_file_parts(&content);

        let matches = match &query.mode {
            SearchMode::Content => self.search_text(&body, &query.pattern, query.case_sensitive),
            SearchMode::Frontmatter => {
                self.search_text(&frontmatter_str, &query.pattern, query.case_sensitive)
            }
            SearchMode::Property(prop_name) => self.search_property(
                &frontmatter_str,
                prop_name,
                &query.pattern,
                query.case_sensitive,
            ),
        };

        Ok(Some(FileSearchResult {
            path: path.to_path_buf(),
            title,
            matches,
        }))
    }

```


## Assistant

So the method is `search_workspace`, not `search`. Let me fix the async_search.rs and async_export.rs:

**Tool Call: crates/diaryx_wasm/src/async_search.rs**
Status: Completed

Diff: crates/diaryx_wasm/src/async_search.rs
```
//! Async search operations for WASM with native Promise support.
//!
//! This module provides async search operations that work directly with
//! `JsAsyncFileSystem`, returning native JavaScript Promises. This enables
//! proper async/await patterns in the web frontend without the need for
//! synchronous wrappers or `block_on`.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { JsAsyncFileSystem, DiaryxAsyncSearch } from './wasm/diaryx_wasm.js';
//!
//! // Create filesystem with your storage backend callbacks
//! const fs = new JsAsyncFileSystem({ /* callbacks */ });
//!
//! // Create async search instance
//! const search = new DiaryxAsyncSearch(fs);
//!
//! // All methods return native Promises
//! const results = await search.search('workspace', 'my query', {
//!     searchFrontmatter: true,
//!     caseSensitive: false
//! });
//! ```

use std::path::PathBuf;

use diaryx_core::search::{SearchQuery, Searcher};
use js_sys::Promise;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::error::IntoJsResult;
use crate::js_async_fs::JsAsyncFileSystem;

// ============================================================================
// Types
// ============================================================================

/// A single match within a file
#[derive(Debug, Serialize)]
pub struct JsAsyncSearchMatch {
    /// Line number (1-indexed)
    pub line_number: usize,
    /// The matching line content
    pub line_content: String,
    /// Column where match starts (0-based)
    pub match_start: usize,
    /// Column where match ends (0-based, exclusive)
    pub match_end: usize,
}

impl From<diaryx_core::search::SearchMatch> for JsAsyncSearchMatch {
    fn from(m: diaryx_core::search::SearchMatch) -> Self {
        JsAsyncSearchMatch {
            line_number: m.line_number,
            line_content: m.line_content,
            match_start: m.match_start,
            match_end: m.match_end,
        }
    }
}

/// Search result for a single file
#[derive(Debug, Serialize)]
pub struct JsAsyncFileSearchResult {
    /// Path to the file
    pub path: String,
    /// Title from frontmatter, if available
    pub title: Option<String>,
    /// Matches found in this file
    pub matches: Vec<JsAsyncSearchMatch>,
}

impl From<diaryx_core::search::FileSearchResult> for JsAsyncFileSearchResult {
    fn from(r: diaryx_core::search::FileSearchResult) -> Self {
        JsAsyncFileSearchResult {
            path: r.path.to_string_lossy().to_string(),
            title: r.title,
            matches: r.matches.into_iter().map(JsAsyncSearchMatch::from).collect(),
        }
    }
}

/// Complete search results
#[derive(Debug, Serialize)]
pub struct JsAsyncSearchResults {
    /// Files with matches
    pub files: Vec<JsAsyncFileSearchResult>,
    /// Total number of matches across all files
    pub total_matches: usize,
    /// Number of files searched
    pub files_searched: usize,
}

impl From<diaryx_core::search::SearchResults> for JsAsyncSearchResults {
    fn from(r: diaryx_core::search::SearchResults) -> Self {
        let total_matches = r.total_matches();
        JsAsyncSearchResults {
            files: r.files.into_iter().map(JsAsyncFileSearchResult::from).collect(),
            total_matches,
            files_searched: r.files_searched,
        }
    }
}

/// Search options from JavaScript
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JsAsyncSearchOptions {
    /// Search in frontmatter (default: false, searches content)
    #[serde(default)]
    pub search_frontmatter: bool,
    /// Search only in a specific frontmatter property
    pub property: Option<String>,
    /// Case-sensitive search (default: false)
    #[serde(default)]
    pub case_sensitive: bool,
}

// ============================================================================
// DiaryxAsyncSearch Class
// ============================================================================

/// Async search operations with native Promise support.
///
/// Unlike `DiaryxSearch` which uses `block_on` internally, this class
/// returns true JavaScript Promises that can be properly awaited.
#[wasm_bindgen]
pub struct DiaryxAsyncSearch {
    fs: JsAsyncFileSystem,
}

#[wasm_bindgen]
impl DiaryxAsyncSearch {
    /// Create a new DiaryxAsyncSearch with the provided filesystem.
    #[wasm_bindgen(constructor)]
    pub fn new(fs: JsAsyncFileSystem) -> Self {
        Self { fs }
    }

    /// Search for content in the workspace.
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @param pattern - The search pattern (string or regex)
    /// @param options - Optional search options
    /// @returns Promise resolving to search results
    #[wasm_bindgen]
    pub fn search(
        &self,
        workspace_path: String,
        pattern: String,
        options: JsValue,
    ) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let searcher = Searcher::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            // Parse options
            let opts: JsAsyncSearchOptions = if options.is_undefined() || options.is_null() {
                JsAsyncSearchOptions::default()
            } else {
                serde_wasm_bindgen::from_value(options)
                    .map_err(|e| JsValue::from_str(&format!("Invalid options: {}", e)))?
            };

            // Build search query
            let query = if let Some(prop) = opts.property {
                SearchQuery::property(&pattern, &prop)
            } else if opts.search_frontmatter {
                SearchQuery::frontmatter(&pattern)
            } else {
                SearchQuery::content(&pattern)
            };

            let query = query.case_sensitive(opts.case_sensitive);

            // Execute search
            let results = searcher
                .search_workspace(&root_path, &query)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_results: JsAsyncSearchResults = results.into();
            serde_wasm_bindgen::to_value(&js_results).js_err()
        })
    }

    /// Search for content in the workspace (content only).
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @param pattern - The search pattern
    /// @returns Promise resolving to search results
    #[wasm_bindgen(js_name = "searchContent")]
    pub fn search_content(&self, workspace_path: String, pattern: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let searcher = Searcher::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            let query = SearchQuery::content(&pattern);
            let results = searcher
                .search_workspace(&root_path, &query)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_results: JsAsyncSearchResults = results.into();
            serde_wasm_bindgen::to_value(&js_results).js_err()
        })
    }

    /// Search for a specific frontmatter property value.
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @param property - The frontmatter property to search
    /// @param pattern - The search pattern
    /// @returns Promise resolving to search results
    #[wasm_bindgen(js_name = "searchProperty")]
    pub fn search_property(
        &self,
        workspace_path: String,
        property: String,
        pattern: String,
    ) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let searcher = Searcher::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            let query = SearchQuery::property(&pattern, &property);
            let results = searcher
                .search_workspace(&root_path, &query)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_results: JsAsyncSearchResults = results.into();
            serde_wasm_bindgen::to_value(&js_results).js_err()
        })
    }

    /// Search with case sensitivity.
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @param pattern - The search pattern
    /// @param case_sensitive - Whether to match case
    /// @returns Promise resolving to search results
    #[wasm_bindgen(js_name = "searchWithCase")]
    pub fn search_with_case(
        &self,
        workspace_path: String,
        pattern: String,
        case_sensitive: bool,
    ) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let searcher = Searcher::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            let query = SearchQuery::content(&pattern).case_sensitive(case_sensitive);

            let results = searcher
                .search_workspace(&root_path, &query)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_results: JsAsyncSearchResults = results.into();
            serde_wasm_bindgen::to_value(&js_results).js_err()
        })
    }
}

impl Default for DiaryxAsyncSearch {
    fn default() -> Self {
        Self {
            fs: JsAsyncFileSystem::new(JsValue::NULL),
        }
    }
}

// ============================================================================
// TypeScript Type Definitions
// ============================================================================

#[wasm_bindgen(typescript_custom_section)]
const TS_APPEND_CONTENT: &'static str = r#"
/**
 * Options for search operations.
 */
export interface AsyncSearchOptions {
    /** Search in frontmatter instead of content (default: false) */
    searchFrontmatter?: boolean;
    /** Search only in a specific frontmatter property */
    property?: string;
    /** Case-sensitive search (default: false) */
    caseSensitive?: boolean;
}

/**
 * A single match within a file.
 */
export interface AsyncSearchMatch {
    /** Line number (1-indexed) */
    line_number: number;
    /** The matching line content */
    line_content: string;
    /** Column where match starts (0-based) */
    match_start: number;
    /** Column where match ends (0-based, exclusive) */
    match_end: number;
}

/**
 * Search result for a single file.
 */
export interface AsyncFileSearchResult {
    /** Path to the file */
    path: string;
    /** Title from frontmatter, if available */
    title: string | null;
    /** Matches found in this file */
    matches: AsyncSearchMatch[];
}

/**
 * Complete search results.
 */
export interface AsyncSearchResults {
    /** Files with matches */
    files: AsyncFileSearchResult[];
    /** Total number of matches across all files */
    total_matches: number;
    /** Number of files searched */
    files_searched: number;
}
"#;
```


## Assistant

Now let me check what the comrak options look like in the current version:

**Tool Call: `grep -r "unsafe" crates/diaryx_wasm/src/ --include="*.rs" | head -10`**
Status: Completed

Terminal:
```
crates/diaryx_wasm/src/export.rs:                options.render.r#unsafe = true;
crates/diaryx_wasm/src/js_async_fs.rs:// We use unsafe impl since we know this will only be used in single-threaded WASM.
crates/diaryx_wasm/src/js_async_fs.rs:unsafe impl Send for JsAsyncFileSystem {}
crates/diaryx_wasm/src/js_async_fs.rs:unsafe impl Sync for JsAsyncFileSystem {}
crates/diaryx_wasm/src/state.rs:    let waker = unsafe { Waker::from_raw(raw_waker) };
crates/diaryx_wasm/src/async_export.rs:    // Note: comrak uses `unsafe_` because `unsafe` is a reserved word in Rust
crates/diaryx_wasm/src/async_export.rs:    options.render.unsafe_ = true;

```


## Assistant

I see - the existing code uses `r#unsafe`. Let me fix that:

**Tool Call: crates/diaryx_wasm/src/async_export.rs**
Status: Completed

Diff: crates/diaryx_wasm/src/async_export.rs
```
//! Async export operations for WASM with native Promise support.
//!
//! This module provides async export operations that work directly with
//! `JsAsyncFileSystem`, returning native JavaScript Promises. This enables
//! proper async/await patterns in the web frontend without the need for
//! synchronous wrappers or `block_on`.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { JsAsyncFileSystem, DiaryxAsyncExport } from './wasm/diaryx_wasm.js';
//!
//! // Create filesystem with your storage backend callbacks
//! const fs = new JsAsyncFileSystem({ /* callbacks */ });
//!
//! // Create async export instance
//! const exporter = new DiaryxAsyncExport(fs);
//!
//! // All methods return native Promises
//! const plan = await exporter.plan('workspace', 'public');
//! const files = await exporter.toMemory('workspace', 'public');
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use diaryx_core::export::Exporter;
use diaryx_core::fs::AsyncFileSystem;
use js_sys::Promise;
use serde::Serialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::error::IntoJsResult;
use crate::js_async_fs::JsAsyncFileSystem;

// ============================================================================
// Types
// ============================================================================

/// Export plan entry representing a file to be exported
#[derive(Debug, Serialize)]
pub struct JsAsyncExportPlanEntry {
    /// Source file path
    pub source: String,
    /// Relative path in workspace
    pub relative_path: String,
    /// Destination file path
    pub destination: String,
    /// Contents that will be filtered out
    pub filtered_contents: Vec<String>,
}

impl From<diaryx_core::export::ExportFile> for JsAsyncExportPlanEntry {
    fn from(entry: diaryx_core::export::ExportFile) -> Self {
        JsAsyncExportPlanEntry {
            source: entry.source_path.to_string_lossy().to_string(),
            relative_path: entry.relative_path.to_string_lossy().to_string(),
            destination: entry.dest_path.to_string_lossy().to_string(),
            filtered_contents: entry.filtered_contents,
        }
    }
}

/// Excluded file entry
#[derive(Debug, Serialize)]
pub struct JsAsyncExcludedFile {
    /// File path
    pub path: String,
    /// Reason for exclusion
    pub reason: String,
}

impl From<diaryx_core::export::ExcludedFile> for JsAsyncExcludedFile {
    fn from(entry: diaryx_core::export::ExcludedFile) -> Self {
        JsAsyncExcludedFile {
            path: entry.path.to_string_lossy().to_string(),
            reason: entry.reason.to_string(),
        }
    }
}

/// Export plan returned to JavaScript
#[derive(Debug, Serialize)]
pub struct JsAsyncExportPlan {
    /// Files to be exported
    pub included: Vec<JsAsyncExportPlanEntry>,
    /// Files that were excluded
    pub excluded: Vec<JsAsyncExcludedFile>,
    /// Target audience
    pub audience: String,
    /// Total number of files to export
    pub total_files: usize,
    /// Total number of excluded files
    pub total_excluded: usize,
}

impl From<diaryx_core::export::ExportPlan> for JsAsyncExportPlan {
    fn from(plan: diaryx_core::export::ExportPlan) -> Self {
        let total_files = plan.included.len();
        let total_excluded = plan.excluded.len();
        JsAsyncExportPlan {
            included: plan.included.into_iter().map(JsAsyncExportPlanEntry::from).collect(),
            excluded: plan.excluded.into_iter().map(JsAsyncExcludedFile::from).collect(),
            audience: plan.audience,
            total_files,
            total_excluded,
        }
    }
}

/// Exported file content
#[derive(Debug, Serialize)]
pub struct JsAsyncExportedFile {
    /// File path
    pub path: String,
    /// File content
    pub content: String,
}

/// Exported binary file
#[derive(Debug, Serialize)]
pub struct JsAsyncBinaryExportFile {
    /// File path
    pub path: String,
    /// Binary content as array of bytes
    pub data: Vec<u8>,
}

/// HTML export result
#[derive(Debug, Serialize)]
pub struct JsAsyncHtmlExport {
    /// HTML files
    pub files: Vec<JsAsyncExportedFile>,
    /// Total number of files
    pub total_files: usize,
}

// ============================================================================
// DiaryxAsyncExport Class
// ============================================================================

/// Async export operations with native Promise support.
///
/// Unlike `DiaryxExport` which uses `block_on` internally, this class
/// returns true JavaScript Promises that can be properly awaited.
#[wasm_bindgen]
pub struct DiaryxAsyncExport {
    fs: JsAsyncFileSystem,
}

#[wasm_bindgen]
impl DiaryxAsyncExport {
    /// Create a new DiaryxAsyncExport with the provided filesystem.
    #[wasm_bindgen(constructor)]
    pub fn new(fs: JsAsyncFileSystem) -> Self {
        Self { fs }
    }

    /// Get available audiences from the workspace.
    ///
    /// Scans all files in the workspace and returns unique audience values
    /// found in frontmatter.
    ///
    /// @param root_path - Path to the workspace root
    /// @returns Promise resolving to array of audience names
    #[wasm_bindgen(js_name = "getAudiences")]
    pub fn get_audiences(&self, root_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let path = PathBuf::from(&root_path);

            // Find all markdown files and collect unique audiences
            let mut audiences = HashSet::new();
            
            let md_files = fs
                .list_md_files_recursive(&path)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            for file_path in md_files {
                let content = fs
                    .read_to_string(&file_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                if let Some(file_audiences) = get_audiences_from_frontmatter(&content) {
                    for audience in file_audiences {
                        audiences.insert(audience);
                    }
                }
            }

            let audiences_vec: Vec<String> = audiences.into_iter().collect();
            serde_wasm_bindgen::to_value(&audiences_vec).js_err()
        })
    }

    /// Plan an export operation.
    ///
    /// Returns a list of files that would be exported for the given audience.
    ///
    /// @param root_path - Path to the workspace root
    /// @param audience - Target audience to export for
    /// @returns Promise resolving to export plan
    #[wasm_bindgen]
    pub fn plan(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            // Plan export to a temporary destination (we won't actually use it)
            let dest = PathBuf::from("_export_temp");
            let plan = exporter
                .plan_export(&path, &audience, &dest)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_plan: JsAsyncExportPlan = plan.into();
            serde_wasm_bindgen::to_value(&js_plan).js_err()
        })
    }

    /// Export files to memory (returns file contents).
    ///
    /// Returns an array of files with their content, suitable for
    /// downloading or further processing.
    ///
    /// @param root_path - Path to the workspace root
    /// @param audience - Target audience to export for
    /// @returns Promise resolving to array of exported files
    #[wasm_bindgen(js_name = "toMemory")]
    pub fn to_memory(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            // Plan the export
            let dest = PathBuf::from("_export");
            let plan = exporter
                .plan_export(&path, &audience, &dest)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Read each file's content
            let mut exported_files = Vec::new();
            for entry in plan.included {
                let content = fs
                    .read_to_string(&entry.source_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                exported_files.push(JsAsyncExportedFile {
                    path: entry.relative_path.to_string_lossy().to_string(),
                    content,
                });
            }

            serde_wasm_bindgen::to_value(&exported_files).js_err()
        })
    }

    /// Export files to HTML format.
    ///
    /// Converts markdown files to HTML using comrak.
    ///
    /// @param root_path - Path to the workspace root
    /// @param audience - Target audience to export for
    /// @returns Promise resolving to array of HTML files
    #[wasm_bindgen(js_name = "toHtml")]
    pub fn to_html(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            // Plan the export
            let dest = PathBuf::from("_export");
            let plan = exporter
                .plan_export(&path, &audience, &dest)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Convert each file to HTML
            let mut html_files = Vec::new();
            for entry in plan.included {
                let content = fs
                    .read_to_string(&entry.source_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                // Extract body (remove frontmatter) and convert to HTML
                let body = extract_body(&content);
                let html = markdown_to_html(&body);

                // Change extension to .html
                let html_path = entry
                    .relative_path
                    .with_extension("html")
                    .to_string_lossy()
                    .to_string();

                html_files.push(JsAsyncExportedFile {
                    path: html_path,
                    content: html,
                });
            }

            let result = JsAsyncHtmlExport {
                total_files: html_files.len(),
                files: html_files,
            };

            serde_wasm_bindgen::to_value(&result).js_err()
        })
    }

    /// Export binary attachments for a given audience.
    ///
    /// Returns binary files (images, etc.) associated with entries
    /// that match the target audience.
    ///
    /// @param root_path - Path to the workspace root
    /// @param audience - Target audience to export for
    /// @returns Promise resolving to array of binary files
    #[wasm_bindgen(js_name = "binaryAttachments")]
    pub fn binary_attachments(&self, root_path: String, audience: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let exporter = Exporter::new(&fs);
            let path = PathBuf::from(&root_path);

            // Plan the export to get the list of files
            let dest = PathBuf::from("_export");
            let plan = exporter
                .plan_export(&path, &audience, &dest)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Collect attachments from all exported files
            let mut binary_files = Vec::new();
            for entry in plan.included {
                // Read the file and check for attachments in frontmatter
                let content = fs
                    .read_to_string(&entry.source_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                if let Some(attachments) = get_attachments_from_frontmatter(&content) {
                    let source_dir = entry.source_path.parent().unwrap_or(&entry.source_path);

                    for attachment in attachments {
                        let attachment_path = source_dir.join(&attachment);
                        if fs.exists(&attachment_path).await {
                            match fs.read_binary(&attachment_path).await {
                                Ok(data) => {
                                    binary_files.push(JsAsyncBinaryExportFile {
                                        path: attachment,
                                        data,
                                    });
                                }
                                Err(_) => {
                                    // Skip files that can't be read
                                    continue;
                                }
                            }
                        }
                    }
                }
            }

            serde_wasm_bindgen::to_value(&binary_files).js_err()
        })
    }
}

impl Default for DiaryxAsyncExport {
    fn default() -> Self {
        Self {
            fs: JsAsyncFileSystem::new(JsValue::NULL),
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Extract body content from markdown (removes frontmatter)
fn extract_body(content: &str) -> String {
    if !content.starts_with("---") {
        return content.to_string();
    }

    // Find the closing ---
    if let Some(end_idx) = content[3..].find("\n---") {
        let after_frontmatter = &content[3 + end_idx + 4..];
        after_frontmatter.trim_start().to_string()
    } else {
        content.to_string()
    }
}

/// Convert markdown to HTML using comrak
fn markdown_to_html(markdown: &str) -> String {
    use comrak::{markdown_to_html as comrak_md_to_html, Options};
    
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    // Note: comrak uses `r#unsafe` because `unsafe` is a reserved word in Rust
    options.render.r#unsafe = true;

    comrak_md_to_html(markdown, &options)
}

/// Get audiences list from frontmatter
fn get_audiences_from_frontmatter(content: &str) -> Option<Vec<String>> {
    if !content.starts_with("---") {
        return None;
    }

    // Find the closing ---
    let end_idx = content[3..].find("\n---")?;
    let frontmatter = &content[3..3 + end_idx];

    // Parse YAML
    let yaml: serde_yaml::Value = serde_yaml::from_str(frontmatter).ok()?;
    let audience = yaml.get("audience")?;

    match audience {
        serde_yaml::Value::Sequence(seq) => {
            Some(
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
            )
        }
        serde_yaml::Value::String(s) => Some(vec![s.clone()]),
        _ => None,
    }
}

/// Get attachments list from frontmatter
fn get_attachments_from_frontmatter(content: &str) -> Option<Vec<String>> {
    if !content.starts_with("---") {
        return None;
    }

    // Find the closing ---
    let end_idx = content[3..].find("\n---")?;
    let frontmatter = &content[3..3 + end_idx];

    // Parse YAML
    let yaml: serde_yaml::Value = serde_yaml::from_str(frontmatter).ok()?;
    let attachments = yaml.get("attachments")?;

    match attachments {
        serde_yaml::Value::Sequence(seq) => {
            Some(
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
            )
        }
        _ => None,
    }
}

// ============================================================================
// TypeScript Type Definitions
// ============================================================================

#[wasm_bindgen(typescript_custom_section)]
const TS_APPEND_CONTENT: &'static str = r#"
/**
 * Export plan entry representing a file to be exported.
 */
export interface AsyncExportPlanEntry {
    /** Source file path */
    source: string;
    /** Relative path in workspace */
    relative_path: string;
    /** Destination file path */
    destination: string;
    /** Contents that will be filtered out */
    filtered_contents: string[];
}

/**
 * Excluded file entry.
 */
export interface AsyncExcludedFile {
    /** File path */
    path: string;
    /** Reason for exclusion */
    reason: string;
}

/**
 * Export plan containing files to be exported.
 */
export interface AsyncExportPlan {
    /** Files to be exported */
    included: AsyncExportPlanEntry[];
    /** Files that were excluded */
    excluded: AsyncExcludedFile[];
    /** Target audience */
    audience: string;
    /** Total number of files to export */
    total_files: number;
    /** Total number of excluded files */
    total_excluded: number;
}

/**
 * Exported file with content.
 */
export interface AsyncExportedFile {
    /** File path */
    path: string;
    /** File content */
    content: string;
}

/**
 * Exported binary file.
 */
export interface AsyncBinaryExportFile {
    /** File path */
    path: string;
    /** Binary content as byte array */
    data: number[];
}

/**
 * HTML export result.
 */
export interface AsyncHtmlExport {
    /** HTML files */
    files: AsyncExportedFile[];
    /** Total number of files */
    total_files: number;
}
"#;
```


**Tool Call: `cd crates/diaryx_wasm && cargo check 2>&1 | head -100`**
Status: Completed

Terminal:
```
warning: profiles for the non root package will be ignored, specify profiles at the workspace root:
package:   /Users/adamharris/Documents/diaryx-core/crates/diaryx_wasm/Cargo.toml
workspace: /Users/adamharris/Documents/diaryx-core/Cargo.toml
    Checking diaryx_wasm v0.7.0 (/Users/adamharris/Documents/diaryx-core/crates/diaryx_wasm)
warning: unused import: `with_fs`
  --> crates/diaryx_wasm/src/export.rs:14:45
   |
14 | use crate::state::{block_on, with_async_fs, with_fs};
   |                                             ^^^^^^^
   |
   = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused import: `std::pin::Pin`
  --> crates/diaryx_wasm/src/js_async_fs.rs:63:5
   |
63 | use std::pin::Pin;
   |     ^^^^^^^^^^^^^

warning: unused import: `std::pin::Pin`
 --> crates/diaryx_wasm/src/state.rs:5:5
  |
5 | use std::pin::Pin;
  |     ^^^^^^^^^^^^^

warning: unused import: `with_fs`
  --> crates/diaryx_wasm/src/workspace.rs:12:45
   |
12 | use crate::state::{block_on, with_async_fs, with_fs};
   |                                             ^^^^^^^

error[E0277]: the trait bound `InMemoryFileSystem: AsyncFileSystem` is not satisfied
  --> crates/diaryx_wasm/src/attachment.rs:44:23
   |
44 |             let app = DiaryxApp::new(fs);
   |                       ^^^^^^^^^ the trait `AsyncFileSystem` is not implemented for `InMemoryFileSystem`
   |
   = help: the following other types implement trait `AsyncFileSystem`:
             &T
             JsAsyncFileSystem
             SyncToAsyncFs<F>
   = note: required for `&InMemoryFileSystem` to implement `AsyncFileSystem`
note: required by a bound in `DiaryxApp`
  --> /Users/adamharris/Documents/diaryx-core/crates/diaryx_core/src/entry/mod.rs:33:26
   |
33 | pub struct DiaryxApp<FS: AsyncFileSystem> {
   |                          ^^^^^^^^^^^^^^^ required by this bound in `DiaryxApp`

error[E0277]: the trait bound `InMemoryFileSystem: AsyncFileSystem` is not satisfied
  --> crates/diaryx_wasm/src/attachment.rs:44:23
   |
44 |             let app = DiaryxApp::new(fs);
   |                       ^^^^^^^^^^^^^^^^^^ the trait `AsyncFileSystem` is not implemented for `InMemoryFileSystem`
   |
   = help: the following other types implement trait `AsyncFileSystem`:
             &T
             JsAsyncFileSystem
             SyncToAsyncFs<F>
   = note: required for `&InMemoryFileSystem` to implement `AsyncFileSystem`
note: required by a bound in `DiaryxApp`
  --> /Users/adamharris/Documents/diaryx-core/crates/diaryx_core/src/entry/mod.rs:33:26
   |
33 | pub struct DiaryxApp<FS: AsyncFileSystem> {
   |                          ^^^^^^^^^^^^^^^ required by this bound in `DiaryxApp`

error[E0599]: the method `add_attachment` exists for struct `DiaryxApp<&InMemoryFileSystem>`, but its trait bounds were not satisfied
  --> crates/diaryx_wasm/src/attachment.rs:45:17
   |
45 |             app.add_attachment(entry_path, attachment_path).js_err()
   |                 ^^^^^^^^^^^^^^ method cannot be called on `DiaryxApp<&InMemoryFileSystem>` due to unsatisfied trait bounds
   |
  ::: /Users/adamharris/Documents/diaryx-core/crates/diaryx_core/src/fs/memory.rs:16:1
   |
16 | pub struct InMemoryFileSystem {
   | ----------------------------- doesn't satisfy `InMemoryFileSystem: AsyncFileSystem`
   |
   = note: the following trait bounds were not satisfied:
           `InMemoryFileSystem: AsyncFileSystem`
           which is required by `&InMemoryFileSystem: AsyncFileSystem`

error[E0277]: the trait bound `InMemoryFileSystem: AsyncFileSystem` is not satisfied
  --> crates/diaryx_wasm/src/attachment.rs:53:23
   |
53 |             let app = DiaryxApp::new(fs);
   |                       ^^^^^^^^^ the trait `AsyncFileSystem` is not implemented for `InMemoryFileSystem`
   |
   = help: the following other types implement trait `AsyncFileSystem`:
             &T
             JsAsyncFileSystem
             SyncToAsyncFs<F>
   = note: required for `&InMemoryFileSystem` to implement `AsyncFileSystem`
note: required by a bound in `DiaryxApp`
  --> /Users/adamharris/Documents/diaryx-core/crates/diaryx_core/src/entry/mod.rs:33:26
   |
33 | pub struct DiaryxApp<FS: AsyncFileSystem> {
   |                          ^^^^^^^^^^^^^^^ required by this bound in `DiaryxApp`

error[E0277]: the trait bound `InMemoryFileSystem: AsyncFileSystem` is not satisfied
  --> crates/diaryx_wasm/src/attachment.rs:53:23
   |
53 |             let app = DiaryxApp::new(fs);

```


## Assistant

I see there are other modules that need to be updated to use `SyncToAsyncFs`. Let me check and fix the attachment module:

**Tool Call: Read file `crates/diaryx_wasm/src/attachment.rs`**
Status: Completed

```diaryx-core/crates/diaryx_wasm/src/attachment.rs
//! Attachment operations for WASM.

use std::path::Path;

use diaryx_core::entry::DiaryxApp;
use diaryx_core::fs::FileSystem;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::error::IntoJsResult;
use crate::state::{with_fs, with_fs_mut};

// ============================================================================
// Types
// ============================================================================

#[derive(Serialize)]
struct StorageInfo {
    used: u64,
    limit: u64,
    attachment_limit: u64,
}

// ============================================================================
// DiaryxAttachment Class
// ============================================================================

/// Attachment operations for managing file attachments.
#[wasm_bindgen]
pub struct DiaryxAttachment;

#[wasm_bindgen]
impl DiaryxAttachment {
    /// Create a new DiaryxAttachment instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Add an attachment path to an entry.
    #[wasm_bindgen]
    pub fn add(&self, entry_path: &str, attachment_path: &str) -> Result<(), JsValue> {
        with_fs(|fs| {
            let app = DiaryxApp::new(fs);
            app.add_attachment(entry_path, attachment_path).js_err()
        })
    }

    /// Remove an attachment path from an entry.
    #[wasm_bindgen]
    pub fn remove(&self, entry_path: &str, attachment_path: &str) -> Result<(), JsValue> {
        with_fs(|fs| {
            let app = DiaryxApp::new(fs);
            app.remove_attachment(entry_path, attachment_path).js_err()
        })
    }

    /// Get attachments for an entry.
    #[wasm_bindgen]
    pub fn list(&self, entry_path: &str) -> Result<JsValue, JsValue> {
        with_fs(|fs| {
            let app = DiaryxApp::new(fs);
            let attachments = app.get_attachments(entry_path).js_err()?;
            serde_wasm_bindgen::to_value(&attachments).js_err()
        })
    }

    /// Upload an attachment file (base64 encoded).
    #[wasm_bindgen]
    pub fn upload(
        &self,
        entry_path: &str,
        filename: &str,
        data_base64: &str,
    ) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let data = base64_decode(data_base64)
                .map_err(|e| JsValue::from_str(&format!("Base64 decode error: {}", e)))?;

            let entry_dir = Path::new(entry_path).parent().unwrap_or(Path::new("."));
            let attachments_dir = entry_dir.join("_attachments");
            let attachment_path = attachments_dir.join(filename);

            fs.create_dir_all(&attachments_dir).js_err()?;
            fs.write_binary(&attachment_path, &data).js_err()?;

            let relative_path = format!("_attachments/{}", filename);

            let app = DiaryxApp::new(fs);
            app.add_attachment(entry_path, &relative_path).js_err()?;

            Ok(relative_path)
        })
    }

    /// Delete an attachment file.
    #[wasm_bindgen]
    pub fn delete(&self, entry_path: &str, attachment_path: &str) -> Result<(), JsValue> {
        with_fs_mut(|fs| {
            let app = DiaryxApp::new(fs);
            app.remove_attachment(entry_path, attachment_path)
                .js_err()?;

            let entry_dir = Path::new(entry_path).parent().unwrap_or(Path::new("."));
            let full_path = entry_dir.join(attachment_path);
            if fs.exists(&full_path) {
                fs.delete_file(&full_path).js_err()?;
            }

            Ok(())
        })
    }

    /// Read attachment data as Uint8Array.
    #[wasm_bindgen]
    pub fn read_data(
        &self,
        entry_path: &str,
        attachment_path: &str,
    ) -> Result<js_sys::Uint8Array, JsValue> {
        with_fs(|fs| {
            let entry_dir = Path::new(entry_path).parent().unwrap_or(Path::new("."));
            let full_path = entry_dir.join(attachment_path);

            let data = fs
                .read_binary(&full_path)
                .map_err(|e| JsValue::from_str(&format!("Failed to read attachment: {}", e)))?;

            Ok(js_sys::Uint8Array::from(data.as_slice()))
        })
    }

    /// Get storage usage information.
    #[wasm_bindgen]
    pub fn get_storage_usage(&self) -> Result<JsValue, JsValue> {
        with_fs(|fs| {
            let mut total_size: u64 = 0;

            fn count_size<FS: FileSystem>(fs: &FS, dir: &Path, total: &mut u64) {
                if let Ok(entries) = fs.list_files(dir) {
                    for path in entries {
                        if fs.is_dir(&path) {
                            count_size(fs, &path, total);
                        } else if let Ok(data) = fs.read_binary(&path) {
                            *total += data.len() as u64;
                        }
                    }
                }
            }

            count_size(fs, Path::new("/"), &mut total_size);

            let info = StorageInfo {
                used: total_size,
                limit: 100 * 1024 * 1024,          // 100MB
                attachment_limit: 5 * 1024 * 1024, // 5MB
            };

            serde_wasm_bindgen::to_value(&info).js_err()
        })
    }
}

impl Default for DiaryxAttachment {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Simple base64 decoder
fn base64_decode(input: &str) -> std::result::Result<Vec<u8>, String> {
    let data = if let Some(pos) = input.find(",") {
        &input[pos + 1..]
    } else {
        input
    };

    const DECODE_TABLE: [i8; 256] = {
        let mut table = [-1i8; 256];
        let mut i = 0u8;
        while i < 26 {
            table[(b'A' + i) as usize] = i as i8;
            table[(b'a' + i) as usize] = (i + 26) as i8;
            i += 1;
        }
        let mut i = 0u8;
        while i < 10 {
            table[(b'0' + i) as usize] = (i + 52) as i8;
            i += 1;
        }
        table[b'+' as usize] = 62;
        table[b'/' as usize] = 63;
        table[b'=' as usize] = 0;
        table
    };

    let bytes: Vec<u8> = data.bytes().filter(|&b| b != b'\n' && b != b'\r').collect();
    let mut output = Vec::with_capacity(bytes.len() * 3 / 4);

    for chunk in bytes.chunks(4) {
        if chunk.len() < 4 {
            break;
        }

        let a = DECODE_TABLE[chunk[0] as usize];
        let b = DECODE_TABLE[chunk[1] as usize];
        let c = DECODE_TABLE[chunk[2] as usize];
        let d = DECODE_TABLE[chunk[3] as usize];

        if a < 0 || b < 0 {
            return Err("Invalid base64 character".to_string());
        }

        output.push(((a as u8) << 2) | ((b as u8) >> 4));
        if chunk[2] != b'=' {
            output.push(((b as u8) << 4) | ((c as u8) >> 2));
        }
        if chunk[3] != b'=' {
            output.push(((c as u8) << 6) | (d as u8));
        }
    }

    Ok(output)
}

```


## Assistant

This module uses `DiaryxApp::new(fs)` which now requires `AsyncFileSystem`. Since this uses `with_fs` which returns an `InMemoryFileSystem`, we need to update it to use `SyncToAsyncFs` and `block_on`. Let me fix this:

**Tool Call: crates/diaryx_wasm/src/attachment.rs**
Status: Completed

Diff: crates/diaryx_wasm/src/attachment.rs
```
//! Attachment operations for WASM.

use std::path::Path;

use diaryx_core::entry::DiaryxApp;
use diaryx_core::fs::{FileSystem, SyncToAsyncFs};
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::error::IntoJsResult;
use crate::state::{block_on, with_fs, with_fs_mut};

// ============================================================================
// Types
// ============================================================================

#[derive(Serialize)]
struct StorageInfo {
    used: u64,
    limit: u64,
    attachment_limit: u64,
}

// ============================================================================
// DiaryxAttachment Class
// ============================================================================

/// Attachment operations for managing file attachments.
#[wasm_bindgen]
pub struct DiaryxAttachment;

#[wasm_bindgen]
impl DiaryxAttachment {
    /// Create a new DiaryxAttachment instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Add an attachment path to an entry.
    #[wasm_bindgen]
    pub fn add(&self, entry_path: &str, attachment_path: &str) -> Result<(), JsValue> {
        with_fs(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);
            block_on(app.add_attachment(entry_path, attachment_path)).js_err()
        })
    }

    /// Remove an attachment path from an entry.
    #[wasm_bindgen]
    pub fn remove(&self, entry_path: &str, attachment_path: &str) -> Result<(), JsValue> {
        with_fs(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);
            block_on(app.remove_attachment(entry_path, attachment_path)).js_err()
        })
    }

    /// Get attachments for an entry.
    #[wasm_bindgen]
    pub fn list(&self, entry_path: &str) -> Result<JsValue, JsValue> {
        with_fs(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);
            let attachments = block_on(app.get_attachments(entry_path)).js_err()?;
            serde_wasm_bindgen::to_value(&attachments).js_err()
        })
    }

    /// Upload an attachment file (base64 encoded).
    #[wasm_bindgen]
    pub fn upload(
        &self,
        entry_path: &str,
        filename: &str,
        data_base64: &str,
    ) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let data = base64_decode(data_base64)
                .map_err(|e| JsValue::from_str(&format!("Base64 decode error: {}", e)))?;

            let entry_dir = Path::new(entry_path).parent().unwrap_or(Path::new("."));
            let attachments_dir = entry_dir.join("_attachments");
            let attachment_path = attachments_dir.join(filename);

            fs.create_dir_all(&attachments_dir).js_err()?;
            fs.write_binary(&attachment_path, &data).js_err()?;

            let relative_path = format!("_attachments/{}", filename);

            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);
            block_on(app.add_attachment(entry_path, &relative_path)).js_err()?;

            Ok(relative_path)
        })
    }

    /// Delete an attachment file.
    #[wasm_bindgen]
    pub fn delete(&self, entry_path: &str, attachment_path: &str) -> Result<(), JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);
            block_on(app.remove_attachment(entry_path, attachment_path)).js_err()?;

            let entry_dir = Path::new(entry_path).parent().unwrap_or(Path::new("."));
            let full_path = entry_dir.join(attachment_path);
            if fs.exists(&full_path) {
                fs.delete_file(&full_path).js_err()?;
            }

            Ok(())
        })
    }

    /// Read attachment data as Uint8Array.
    #[wasm_bindgen]
    pub fn read_data(
        &self,
        entry_path: &str,
        attachment_path: &str,
    ) -> Result<js_sys::Uint8Array, JsValue> {
        with_fs(|fs| {
            let entry_dir = Path::new(entry_path).parent().unwrap_or(Path::new("."));
            let full_path = entry_dir.join(attachment_path);

            let data = fs
                .read_binary(&full_path)
                .map_err(|e| JsValue::from_str(&format!("Failed to read attachment: {}", e)))?;

            Ok(js_sys::Uint8Array::from(data.as_slice()))
        })
    }

    /// Get storage usage information.
    #[wasm_bindgen]
    pub fn get_storage_usage(&self) -> Result<JsValue, JsValue> {
        with_fs(|fs| {
            let mut total_size: u64 = 0;

            fn count_size<FS: FileSystem>(fs: &FS, dir: &Path, total: &mut u64) {
                if let Ok(entries) = fs.list_files(dir) {
                    for path in entries {
                        if fs.is_dir(&path) {
                            count_size(fs, &path, total);
                        } else if let Ok(data) = fs.read_binary(&path) {
                            *total += data.len() as u64;
                        }
                    }
                }
            }

            count_size(fs, Path::new("/"), &mut total_size);

            let info = StorageInfo {
                used: total_size,
                limit: 100 * 1024 * 1024,          // 100MB
                attachment_limit: 5 * 1024 * 1024, // 5MB
            };

            serde_wasm_bindgen::to_value(&info).js_err()
        })
    }
}

impl Default for DiaryxAttachment {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Simple base64 decoder
fn base64_decode(input: &str) -> std::result::Result<Vec<u8>, String> {
    let data = if let Some(pos) = input.find(",") {
        &input[pos + 1..]
    } else {
        input
    };

    const DECODE_TABLE: [i8; 256] = {
        let mut table = [-1i8; 256];
        let mut i = 0u8;
        while i < 26 {
            table[(b'A' + i) as usize] = i as i8;
            table[(b'a' + i) as usize] = (i + 26) as i8;
            i += 1;
        }
        let mut i = 0u8;
        while i < 10 {
            table[(b'0' + i) as usize] = (i + 52) as i8;
            i += 1;
        }
        table[b'+' as usize] = 62;
        table[b'/' as usize] = 63;
        table[b'=' as usize] = 0;
        table
    };

    let bytes: Vec<u8> = data.bytes().filter(|&b| b != b'\n' && b != b'\r').collect();
    let mut output = Vec::with_capacity(bytes.len() * 3 / 4);

    for chunk in bytes.chunks(4) {
        if chunk.len() < 4 {
            break;
        }

        let a = DECODE_TABLE[chunk[0] as usize];
        let b = DECODE_TABLE[chunk[1] as usize];
        let c = DECODE_TABLE[chunk[2] as usize];
        let d = DECODE_TABLE[chunk[3] as usize];

        if a < 0 || b < 0 {
            return Err("Invalid base64 character".to_string());
        }

        output.push(((a as u8) << 2) | ((b as u8) >> 4));
        if chunk[2] != b'=' {
            output.push(((b as u8) << 4) | ((c as u8) >> 2));
        }
        if chunk[3] != b'=' {
            output.push(((c as u8) << 6) | (d as u8));
        }
    }

    Ok(output)
}
```


**Tool Call: `cd crates/diaryx_wasm && cargo check 2>&1 | head -100`**
Status: Completed

Terminal:
```
warning: profiles for the non root package will be ignored, specify profiles at the workspace root:
package:   /Users/adamharris/Documents/diaryx-core/crates/diaryx_wasm/Cargo.toml
workspace: /Users/adamharris/Documents/diaryx-core/Cargo.toml
    Checking diaryx_wasm v0.7.0 (/Users/adamharris/Documents/diaryx-core/crates/diaryx_wasm)
warning: unused import: `with_fs`
  --> crates/diaryx_wasm/src/export.rs:14:45
   |
14 | use crate::state::{block_on, with_async_fs, with_fs};
   |                                             ^^^^^^^
   |
   = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused import: `std::pin::Pin`
  --> crates/diaryx_wasm/src/js_async_fs.rs:63:5
   |
63 | use std::pin::Pin;
   |     ^^^^^^^^^^^^^

warning: unused import: `std::pin::Pin`
 --> crates/diaryx_wasm/src/state.rs:5:5
  |
5 | use std::pin::Pin;
  |     ^^^^^^^^^^^^^

warning: unused import: `with_fs`
  --> crates/diaryx_wasm/src/workspace.rs:12:45
   |
12 | use crate::state::{block_on, with_async_fs, with_fs};
   |                                             ^^^^^^^

error[E0277]: the trait bound `InMemoryFileSystem: AsyncFileSystem` is not satisfied
  --> crates/diaryx_wasm/src/entry.rs:59:23
   |
59 |             let app = DiaryxApp::new(fs);
   |                       ^^^^^^^^^ the trait `AsyncFileSystem` is not implemented for `InMemoryFileSystem`
   |
   = help: the following other types implement trait `AsyncFileSystem`:
             &T
             JsAsyncFileSystem
             SyncToAsyncFs<F>
   = note: required for `&InMemoryFileSystem` to implement `AsyncFileSystem`
note: required by a bound in `DiaryxApp`
  --> /Users/adamharris/Documents/diaryx-core/crates/diaryx_core/src/entry/mod.rs:33:26
   |
33 | pub struct DiaryxApp<FS: AsyncFileSystem> {
   |                          ^^^^^^^^^^^^^^^ required by this bound in `DiaryxApp`

error[E0277]: the trait bound `InMemoryFileSystem: AsyncFileSystem` is not satisfied
  --> crates/diaryx_wasm/src/entry.rs:59:23
   |
59 |             let app = DiaryxApp::new(fs);
   |                       ^^^^^^^^^^^^^^^^^^ the trait `AsyncFileSystem` is not implemented for `InMemoryFileSystem`
   |
   = help: the following other types implement trait `AsyncFileSystem`:
             &T
             JsAsyncFileSystem
             SyncToAsyncFs<F>
   = note: required for `&InMemoryFileSystem` to implement `AsyncFileSystem`
note: required by a bound in `DiaryxApp`
  --> /Users/adamharris/Documents/diaryx-core/crates/diaryx_core/src/entry/mod.rs:33:26
   |
33 | pub struct DiaryxApp<FS: AsyncFileSystem> {
   |                          ^^^^^^^^^^^^^^^ required by this bound in `DiaryxApp`

error[E0599]: the method `get_all_frontmatter` exists for struct `DiaryxApp<&InMemoryFileSystem>`, but its trait bounds were not satisfied
  --> crates/diaryx_wasm/src/entry.rs:61:35
   |
61 |             let frontmatter = app.get_all_frontmatter(path).js_err()?;
   |                                   ^^^^^^^^^^^^^^^^^^^ method cannot be called on `DiaryxApp<&InMemoryFileSystem>` due to unsatisfied trait bounds
   |
  ::: /Users/adamharris/Documents/diaryx-core/crates/diaryx_core/src/fs/memory.rs:16:1
   |
16 | pub struct InMemoryFileSystem {
   | ----------------------------- doesn't satisfy `InMemoryFileSystem: AsyncFileSystem`
   |
   = note: the following trait bounds were not satisfied:
           `InMemoryFileSystem: AsyncFileSystem`
           which is required by `&InMemoryFileSystem: AsyncFileSystem`

error[E0599]: the method `get_content` exists for struct `DiaryxApp<&InMemoryFileSystem>`, but its trait bounds were not satisfied
  --> crates/diaryx_wasm/src/entry.rs:75:31
   |
75 |             let content = app.get_content(path).js_err()?;
   |                               ^^^^^^^^^^^ method cannot be called on `DiaryxApp<&InMemoryFileSystem>` due to unsatisfied trait bounds
   |
  ::: /Users/adamharris/Documents/diaryx-core/crates/diaryx_core/src/fs/memory.rs:16:1
   |
16 | pub struct InMemoryFileSystem {
   | ----------------------------- doesn't satisfy `InMemoryFileSystem: AsyncFileSystem`
   |
   = note: the following trait bounds were not satisfied:
           `InMemoryFileSystem: AsyncFileSystem`
           which is required by `&InMemoryFileSystem: AsyncFileSystem`

error[E0277]: the trait bound `InMemoryFileSystem: AsyncFileSystem` is not satisfied
  --> crates/diaryx_wasm/src/entry.rs:92:23
   |
92 |             let app = DiaryxApp::new(fs);
   |                       ^^^^^^^^^ the trait `AsyncFileSystem` is not implemented for `InMemoryFileSystem`
   |

```


## Assistant

There are more places in entry.rs that need fixing. Let me look at it and fix it as well:

**Tool Call: Read file `crates/diaryx_wasm/src/entry.rs`**
Status: Completed

```diaryx-core/crates/diaryx_wasm/src/entry.rs
//! Entry operations for WASM.

use std::path::{Path, PathBuf};

use chrono::Utc;
use diaryx_core::entry::DiaryxApp;
use diaryx_core::fs::FileSystem;
use diaryx_core::template::TemplateManager;
use diaryx_core::workspace::Workspace;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::error::{IntoJsOption, IntoJsResult};
use crate::state::{with_fs, with_fs_mut};

// ============================================================================
// Types
// ============================================================================

/// Entry data returned to JavaScript
#[derive(Debug, Serialize)]
pub struct JsEntryData {
    pub path: String,
    pub title: Option<String>,
    pub frontmatter: serde_json::Map<String, serde_json::Value>,
    pub content: String,
}

/// Options for creating an entry
#[derive(Debug, Deserialize)]
pub struct CreateEntryOptions {
    pub title: Option<String>,
    pub part_of: Option<String>,
    pub template: Option<String>,
}

// ============================================================================
// DiaryxEntry Class
// ============================================================================

/// Entry operations for managing diary entries.
#[wasm_bindgen]
pub struct DiaryxEntry;

#[wasm_bindgen]
impl DiaryxEntry {
    /// Create a new DiaryxEntry instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Get an entry's content and metadata.
    #[wasm_bindgen]
    pub fn get(&self, path: &str) -> Result<JsValue, JsValue> {
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);

        with_fs(|fs| {
            let app = DiaryxApp::new(fs);

            let frontmatter = app.get_all_frontmatter(path).js_err()?;

            let mut json_frontmatter = serde_json::Map::new();
            for (key, value) in frontmatter {
                if let Ok(json_val) = serde_json::to_value(&value) {
                    json_frontmatter.insert(key, json_val);
                }
            }

            let title = json_frontmatter
                .get("title")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let content = app.get_content(path).js_err()?;

            let entry = JsEntryData {
                path: path.to_string(),
                title,
                frontmatter: json_frontmatter,
                content,
            };

            entry.serialize(&serializer).js_err()
        })
    }

    /// Save an entry's content.
    #[wasm_bindgen]
    pub fn save(&self, path: &str, content: &str) -> Result<(), JsValue> {
        with_fs_mut(|fs| {
            let app = DiaryxApp::new(fs);
            app.save_content(path, content).js_err()
        })
    }

    /// Get an entry's full raw file content.
    #[wasm_bindgen]
    pub fn read_raw(&self, path: &str) -> Result<String, JsValue> {
        with_fs(|fs| fs.read_to_string(Path::new(path)).js_err())
    }

    /// Create a new entry.
    #[wasm_bindgen]
    pub fn create(&self, path: &str, options: JsValue) -> Result<String, JsValue> {
        let options: Option<CreateEntryOptions> = if options.is_undefined() || options.is_null() {
            None
        } else {
            Some(serde_wasm_bindgen::from_value(options).js_err()?)
        };

        with_fs_mut(|fs| {
            let app = DiaryxApp::new(fs);
            let path_buf = PathBuf::from(path);

            let title = options
                .as_ref()
                .and_then(|o| o.title.clone())
                .unwrap_or_else(|| {
                    path_buf
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Untitled")
                        .to_string()
                });

            // Check if template is requested
            if let Some(ref opts) = options
                && let Some(ref template_name) = opts.template
            {
                let manager = TemplateManager::new(fs);
                if let Some(template) = manager.get(template_name) {
                    let mut context = diaryx_core::template::TemplateContext::new()
                        .with_title(&title)
                        .with_date(Utc::now().date_naive());

                    if let Some(ref part_of) = opts.part_of {
                        context = context.with_part_of(part_of);
                    }

                    let content = template.render(&context);
                    fs.create_new(&path_buf, &content).js_err()?;
                    add_to_parent_index(fs, path)?;
                    return Ok(path.to_string());
                }
            }

            // Create entry without template
            app.create_entry(path).js_err()?;

            // Set title and timestamps
            app.set_frontmatter_property(path, "title", serde_yaml::Value::String(title))
                .js_err()?;

            let now = Utc::now().to_rfc3339();
            app.set_frontmatter_property(path, "created", serde_yaml::Value::String(now.clone()))
                .js_err()?;
            app.set_frontmatter_property(path, "updated", serde_yaml::Value::String(now))
                .js_err()?;

            // Set part_of if provided
            if let Some(ref opts) = options
                && let Some(ref part_of) = opts.part_of
            {
                app.set_frontmatter_property(
                    path,
                    "part_of",
                    serde_yaml::Value::String(part_of.clone()),
                )
                .js_err()?;
            }

            add_to_parent_index(fs, path)?;
            Ok(path.to_string())
        })
    }

    /// Delete an entry.
    #[wasm_bindgen]
    pub fn delete(&self, path: &str) -> Result<(), JsValue> {
        with_fs_mut(|fs| {
            let ws = Workspace::new(fs);
            ws.delete_entry(&PathBuf::from(path)).js_err()
        })
    }

    /// Move/rename an entry.
    #[wasm_bindgen]
    pub fn move_entry(&self, from_path: &str, to_path: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let ws = Workspace::new(fs);
            ws.move_entry(&PathBuf::from(from_path), &PathBuf::from(to_path))
                .js_err()?;
            Ok(to_path.to_string())
        })
    }

    /// Attach an entry to a parent.
    #[wasm_bindgen]
    pub fn attach_to_parent(&self, entry_path: &str, parent_path: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let ws = Workspace::new(fs);
            ws.attach_and_move_entry_to_parent(
                &PathBuf::from(entry_path),
                &PathBuf::from(parent_path),
            )
            .map(|p| p.to_string_lossy().to_string())
            .js_err()
        })
    }

    /// Convert a leaf file to an index.
    #[wasm_bindgen]
    pub fn convert_to_index(&self, path: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let ws = Workspace::new(fs);
            ws.convert_to_index(&PathBuf::from(path))
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
        })
    }

    /// Convert an index back to a leaf file.
    #[wasm_bindgen]
    pub fn convert_to_leaf(&self, path: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let ws = Workspace::new(fs);
            ws.convert_to_leaf(&PathBuf::from(path))
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
        })
    }

    /// Create a new child entry under a parent.
    #[wasm_bindgen]
    pub fn create_child(&self, parent_path: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let ws = Workspace::new(fs);
            ws.create_child_entry(&PathBuf::from(parent_path), None)
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
        })
    }

    /// Rename an entry.
    #[wasm_bindgen]
    pub fn rename(&self, path: &str, new_filename: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let ws = Workspace::new(fs);
            ws.rename_entry(&PathBuf::from(path), new_filename)
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
        })
    }

    /// Ensure today's daily entry exists.
    #[wasm_bindgen]
    pub fn ensure_daily(&self) -> Result<String, JsValue> {
        use chrono::Local;
        use diaryx_core::config::Config;

        with_fs_mut(|fs| {
            let app = DiaryxApp::new(fs);
            let today = Local::now().date_naive();

            let config = Config::with_options(
                PathBuf::from("workspace"),
                Some("Daily".to_string()),
                None,
                None,
                None,
            );

            app.ensure_dated_entry(&today, &config)
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
        })
    }
}

impl Default for DiaryxEntry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Add an entry to its parent index's contents array.
fn add_to_parent_index<FS: FileSystem>(fs: &FS, entry_path: &str) -> Result<(), JsValue> {
    let path = PathBuf::from(entry_path);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .js_ok_or("Invalid path")?;

    let parent = path.parent().js_ok_or("No parent directory")?;
    let index_path = parent.join("index.md");

    if !fs.exists(&index_path) {
        return Ok(());
    }

    let app = DiaryxApp::new(fs);
    let index_path_str = index_path.to_string_lossy();

    let frontmatter = app.get_all_frontmatter(&index_path_str).js_err()?;

    let mut contents: Vec<String> = frontmatter
        .get("contents")
        .and_then(|v| {
            if let serde_yaml::Value::Sequence(seq) = v {
                Some(
                    seq.iter()
                        .filter_map(|item| item.as_str().map(String::from))
                        .collect(),
                )
            } else {
                None
            }
        })
        .unwrap_or_default();

    if !contents.contains(&file_name.to_string()) {
        contents.push(file_name.to_string());
        contents.sort();

        let yaml_contents: Vec<serde_yaml::Value> = contents
            .into_iter()
            .map(serde_yaml::Value::String)
            .collect();

        app.set_frontmatter_property(
            &index_path_str,
            "contents",
            serde_yaml::Value::Sequence(yaml_contents),
        )
        .js_err()?;
    }

    // Set part_of on entry if not present
    let entry_frontmatter = app.get_all_frontmatter(entry_path).js_err()?;

    if !entry_frontmatter.contains_key("part_of") {
        app.set_frontmatter_property(
            entry_path,
            "part_of",
            serde_yaml::Value::String("index.md".to_string()),
        )
        .js_err()?;
    }

    Ok(())
}

/// Convert a title to a kebab-case filename.
#[wasm_bindgen]
pub fn slugify_title(title: &str) -> String {
    diaryx_core::entry::slugify_title(title)
}

```


## Assistant

This is a fairly large file. Let me fix it by replacing the `with_fs` calls with proper async wrappers:

**Tool Call: crates/diaryx_wasm/src/entry.rs**
Status: Completed

Diff: crates/diaryx_wasm/src/entry.rs
```
//! Entry operations for WASM.

use std::path::{Path, PathBuf};

use chrono::Utc;
use diaryx_core::entry::DiaryxApp;
use diaryx_core::fs::{FileSystem, SyncToAsyncFs};
use diaryx_core::template::TemplateManager;
use diaryx_core::workspace::Workspace;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::error::{IntoJsOption, IntoJsResult};
use crate::state::{block_on, with_fs, with_fs_mut};

// ============================================================================
// Types
// ============================================================================

/// Entry data returned to JavaScript
#[derive(Debug, Serialize)]
pub struct JsEntryData {
    pub path: String,
    pub title: Option<String>,
    pub frontmatter: serde_json::Map<String, serde_json::Value>,
    pub content: String,
}

/// Options for creating an entry
#[derive(Debug, Deserialize)]
pub struct CreateEntryOptions {
    pub title: Option<String>,
    pub part_of: Option<String>,
    pub template: Option<String>,
}

// ============================================================================
// DiaryxEntry Class
// ============================================================================

/// Entry operations for managing diary entries.
#[wasm_bindgen]
pub struct DiaryxEntry;

#[wasm_bindgen]
impl DiaryxEntry {
    /// Create a new DiaryxEntry instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Get an entry's content and metadata.
    #[wasm_bindgen]
    pub fn get(&self, path: &str) -> Result<JsValue, JsValue> {
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);

        with_fs(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);

            let frontmatter = block_on(app.get_all_frontmatter(path)).js_err()?;

            let mut json_frontmatter = serde_json::Map::new();
            for (key, value) in frontmatter {
                if let Ok(json_val) = serde_json::to_value(&value) {
                    json_frontmatter.insert(key, json_val);
                }
            }

            let title = json_frontmatter
                .get("title")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let content = block_on(app.get_content(path)).js_err()?;

            let entry = JsEntryData {
                path: path.to_string(),
                title,
                frontmatter: json_frontmatter,
                content,
            };

            entry.serialize(&serializer).js_err()
        })
    }

    /// Save an entry's content.
    #[wasm_bindgen]
    pub fn save(&self, path: &str, content: &str) -> Result<(), JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);
            block_on(app.save_content(path, content)).js_err()
        })
    }

    /// Get an entry's full raw file content.
    #[wasm_bindgen]
    pub fn read_raw(&self, path: &str) -> Result<String, JsValue> {
        with_fs(|fs| fs.read_to_string(Path::new(path)).js_err())
    }

    /// Create a new entry.
    #[wasm_bindgen]
    pub fn create(&self, path: &str, options: JsValue) -> Result<String, JsValue> {
        let options: Option<CreateEntryOptions> = if options.is_undefined() || options.is_null() {
            None
        } else {
            Some(serde_wasm_bindgen::from_value(options).js_err()?)
        };

        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);
            let path_buf = PathBuf::from(path);

            let title = options
                .as_ref()
                .and_then(|o| o.title.clone())
                .unwrap_or_else(|| {
                    path_buf
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Untitled")
                        .to_string()
                });

            // Check if template is requested
            if let Some(ref opts) = options
                && let Some(ref template_name) = opts.template
            {
                let manager = TemplateManager::new(fs);
                if let Some(template) = manager.get(template_name) {
                    let mut context = diaryx_core::template::TemplateContext::new()
                        .with_title(&title)
                        .with_date(Utc::now().date_naive());

                    if let Some(ref part_of) = opts.part_of {
                        context = context.with_part_of(part_of);
                    }

                    let content = template.render(&context);
                    fs.create_new(&path_buf, &content).js_err()?;
                    add_to_parent_index(fs, path)?;
                    return Ok(path.to_string());
                }
            }

            // Create entry without template
            block_on(app.create_entry(path)).js_err()?;

            // Set title and timestamps
            block_on(app.set_frontmatter_property(path, "title", serde_yaml::Value::String(title)))
                .js_err()?;

            let now = Utc::now().to_rfc3339();
            block_on(app.set_frontmatter_property(path, "created", serde_yaml::Value::String(now.clone())))
                .js_err()?;
            block_on(app.set_frontmatter_property(path, "updated", serde_yaml::Value::String(now)))
                .js_err()?;

            // Set part_of if provided
            if let Some(ref opts) = options
                && let Some(ref part_of) = opts.part_of
            {
                block_on(app.set_frontmatter_property(
                    path,
                    "part_of",
                    serde_yaml::Value::String(part_of.clone()),
                ))
                .js_err()?;
            }

            add_to_parent_index(fs, path)?;
            Ok(path.to_string())
        })
    }

    /// Delete an entry.
    #[wasm_bindgen]
    pub fn delete(&self, path: &str) -> Result<(), JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.delete_entry(&PathBuf::from(path))).js_err()
        })
    }

    /// Move/rename an entry.
    #[wasm_bindgen]
    pub fn move_entry(&self, from_path: &str, to_path: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.move_entry(&PathBuf::from(from_path), &PathBuf::from(to_path)))
                .js_err()?;
            Ok(to_path.to_string())
        })
    }

    /// Attach an entry to a parent.
    #[wasm_bindgen]
    pub fn attach_to_parent(&self, entry_path: &str, parent_path: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.attach_and_move_entry_to_parent(
                &PathBuf::from(entry_path),
                &PathBuf::from(parent_path),
            ))
            .map(|p| p.to_string_lossy().to_string())
            .js_err()
        })
    }

    /// Convert a leaf file to an index.
    #[wasm_bindgen]
    pub fn convert_to_index(&self, path: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.convert_to_index(&PathBuf::from(path)))
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
        })
    }

    /// Convert an index back to a leaf file.
    #[wasm_bindgen]
    pub fn convert_to_leaf(&self, path: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.convert_to_leaf(&PathBuf::from(path)))
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
        })
    }

    /// Create a new child entry under a parent.
    #[wasm_bindgen]
    pub fn create_child(&self, parent_path: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.create_child_entry(&PathBuf::from(parent_path), None))
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
        })
    }

    /// Rename an entry.
    #[wasm_bindgen]
    pub fn rename(&self, path: &str, new_filename: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.rename_entry(&PathBuf::from(path), new_filename))
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
        })
    }

    /// Ensure today's daily entry exists.
    #[wasm_bindgen]
    pub fn ensure_daily(&self) -> Result<String, JsValue> {
        use chrono::Local;
        use diaryx_core::config::Config;

        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);
            let today = Local::now().date_naive();

            let config = Config::with_options(
                PathBuf::from("workspace"),
                Some("Daily".to_string()),
                None,
                None,
                None,
            );

            block_on(app.ensure_dated_entry(&today, &config))
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
        })
    }
}

impl Default for DiaryxEntry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Add an entry to its parent index's contents array.
fn add_to_parent_index<FS: FileSystem>(fs: &FS, entry_path: &str) -> Result<(), JsValue> {
    let path = PathBuf::from(entry_path);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .js_ok_or("Invalid path")?;

    let parent = path.parent().js_ok_or("No parent directory")?;
    let index_path = parent.join("index.md");

    if !fs.exists(&index_path) {
        return Ok(());
    }

    let async_fs = SyncToAsyncFs::new(fs.clone());
    let app = DiaryxApp::new(async_fs);
    let index_path_str = index_path.to_string_lossy();

    let frontmatter = block_on(app.get_all_frontmatter(&index_path_str)).js_err()?;

    let mut contents: Vec<String> = frontmatter
        .get("contents")
        .and_then(|v| {
            if let serde_yaml::Value::Sequence(seq) = v {
                Some(
                    seq.iter()
                        .filter_map(|item| item.as_str().map(String::from))
                        .collect(),
                )
            } else {
                None
            }
        })
        .unwrap_or_default();

    if !contents.contains(&file_name.to_string()) {
        contents.push(file_name.to_string());
        contents.sort();

        let yaml_contents: Vec<serde_yaml::Value> = contents
            .into_iter()
            .map(serde_yaml::Value::String)
            .collect();

        block_on(app.set_frontmatter_property(
            &index_path_str,
            "contents",
            serde_yaml::Value::Sequence(yaml_contents),
        ))
        .js_err()?;
    }

    // Set part_of on entry if not present
    let entry_frontmatter = block_on(app.get_all_frontmatter(entry_path)).js_err()?;

    if !entry_frontmatter.contains_key("part_of") {
        block_on(app.set_frontmatter_property(
            entry_path,
            "part_of",
            serde_yaml::Value::String("index.md".to_string()),
        ))
        .js_err()?;
    }

    Ok(())
}

/// Convert a title to a kebab-case filename.
#[wasm_bindgen]
pub fn slugify_title(title: &str) -> String {
    diaryx_core::entry::slugify_title(title)
}
```


**Tool Call: `cd crates/diaryx_wasm && cargo check 2>&1 | head -100`**
Status: Completed

Terminal:
```
warning: profiles for the non root package will be ignored, specify profiles at the workspace root:
package:   /Users/adamharris/Documents/diaryx-core/crates/diaryx_wasm/Cargo.toml
workspace: /Users/adamharris/Documents/diaryx-core/Cargo.toml
    Checking diaryx_wasm v0.7.0 (/Users/adamharris/Documents/diaryx-core/crates/diaryx_wasm)
warning: unused import: `with_fs`
  --> crates/diaryx_wasm/src/export.rs:14:45
   |
14 | use crate::state::{block_on, with_async_fs, with_fs};
   |                                             ^^^^^^^
   |
   = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused import: `std::pin::Pin`
  --> crates/diaryx_wasm/src/js_async_fs.rs:63:5
   |
63 | use std::pin::Pin;
   |     ^^^^^^^^^^^^^

warning: unused import: `std::pin::Pin`
 --> crates/diaryx_wasm/src/state.rs:5:5
  |
5 | use std::pin::Pin;
  |     ^^^^^^^^^^^^^

warning: unused import: `with_fs`
  --> crates/diaryx_wasm/src/workspace.rs:12:45
   |
12 | use crate::state::{block_on, with_async_fs, with_fs};
   |                                             ^^^^^^^

error[E0599]: no method named `ensure_dated_entry` found for struct `DiaryxApp<FS>` in the current scope
   --> crates/diaryx_wasm/src/entry.rs:285:26
    |
285 |             block_on(app.ensure_dated_entry(&today, &config))
    |                          ^^^^^^^^^^^^^^^^^^
    |
help: there is a method `create_entry` with a similar name, but with different arguments
   --> /Users/adamharris/Documents/diaryx-core/crates/diaryx_core/src/entry/mod.rs:57:5
    |
 57 |     pub async fn create_entry(&self, path: &str) -> Result<()> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0277]: the trait bound `InMemoryFileSystem: AsyncFileSystem` is not satisfied
  --> crates/diaryx_wasm/src/frontmatter.rs:32:23
   |
32 |             let app = DiaryxApp::new(fs);
   |                       ^^^^^^^^^ the trait `AsyncFileSystem` is not implemented for `InMemoryFileSystem`
   |
   = help: the following other types implement trait `AsyncFileSystem`:
             &T
             JsAsyncFileSystem
             SyncToAsyncFs<F>
   = note: required for `&InMemoryFileSystem` to implement `AsyncFileSystem`
note: required by a bound in `DiaryxApp`
  --> /Users/adamharris/Documents/diaryx-core/crates/diaryx_core/src/entry/mod.rs:33:26
   |
33 | pub struct DiaryxApp<FS: AsyncFileSystem> {
   |                          ^^^^^^^^^^^^^^^ required by this bound in `DiaryxApp`

error[E0277]: the trait bound `InMemoryFileSystem: AsyncFileSystem` is not satisfied
  --> crates/diaryx_wasm/src/frontmatter.rs:32:23
   |
32 |             let app = DiaryxApp::new(fs);
   |                       ^^^^^^^^^^^^^^^^^^ the trait `AsyncFileSystem` is not implemented for `InMemoryFileSystem`
   |
   = help: the following other types implement trait `AsyncFileSystem`:
             &T
             JsAsyncFileSystem
             SyncToAsyncFs<F>
   = note: required for `&InMemoryFileSystem` to implement `AsyncFileSystem`
note: required by a bound in `DiaryxApp`
  --> /Users/adamharris/Documents/diaryx-core/crates/diaryx_core/src/entry/mod.rs:33:26
   |
33 | pub struct DiaryxApp<FS: AsyncFileSystem> {
   |                          ^^^^^^^^^^^^^^^ required by this bound in `DiaryxApp`

error[E0599]: the method `get_all_frontmatter` exists for struct `DiaryxApp<&InMemoryFileSystem>`, but its trait bounds were not satisfied
  --> crates/diaryx_wasm/src/frontmatter.rs:34:35
   |
34 |             let frontmatter = app.get_all_frontmatter(path).js_err()?;
   |                                   ^^^^^^^^^^^^^^^^^^^ method cannot be called on `DiaryxApp<&InMemoryFileSystem>` due to unsatisfied trait bounds
   |
  ::: /Users/adamharris/Documents/diaryx-core/crates/diaryx_core/src/fs/memory.rs:16:1
   |
16 | pub struct InMemoryFileSystem {
   | ----------------------------- doesn't satisfy `InMemoryFileSystem: AsyncFileSystem`
   |
   = note: the following trait bounds were not satisfied:
           `InMemoryFileSystem: AsyncFileSystem`
           which is required by `&InMemoryFileSystem: AsyncFileSystem`

error[E0277]: the trait bound `InMemoryFileSystem: AsyncFileSystem` is not satisfied
  --> crates/diaryx_wasm/src/frontmatter.rs:51:23
   |
51 |             let app = DiaryxApp::new(fs);
   |                       ^^^^^^^^^ the trait `AsyncFileSystem` is not implemented for `InMemoryFileSystem`
   |
   = help: the following other types implement trait `AsyncFileSystem`:
             &T
             JsAsyncFileSystem

```


## Assistant

Let me check what methods `DiaryxApp` has now and also fix the frontmatter module. First let me look at the DiaryxApp:

**Tool Call: Read file `crates/diaryx_core/src/entry/mod.rs` (lines 1-120)**
Status: Completed

```diaryx-core/crates/diaryx_core/src/entry/mod.rs
//! Entry operations module.
//!
//! This module provides functionality for working with individual entries:
//! - Frontmatter manipulation (get, set, remove properties)
//! - Content operations (get, set, append, prepend)
//! - Attachment management
//! - Daily entry creation with index hierarchy
//!
//! ## Async-first refactor
//!
//! `DiaryxApp` is now async-first and uses `AsyncFileSystem`.
//! The prior sync implementation is preserved as `DiaryxAppSync`
//! (temporary compatibility during the refactor).

mod helpers;

// Re-export helper functions
pub use helpers::{prettify_filename, slugify, slugify_title};

use crate::config::Config;
use crate::date::{date_to_path, parse_date};
use crate::error::{DiaryxError, Result};
use crate::fs::{AsyncFileSystem, FileSystem};
use crate::template::{Template, TemplateContext, TemplateManager};
use chrono::{NaiveDate, Utc};
use indexmap::IndexMap;
use serde_yaml::Value;
use std::path::{Path, PathBuf};

/// Async-first Diaryx entry operations.
///
/// This is the main entry API going forward.
pub struct DiaryxApp<FS: AsyncFileSystem> {
    fs: FS,
}

/// Legacy synchronous Diaryx entry operations.
///
/// This preserves the prior `FileSystem`-based implementation during the async refactor.
/// Prefer [`DiaryxApp`].
pub struct DiaryxAppSync<FS: FileSystem> {
    fs: FS,
}

impl<FS: AsyncFileSystem> DiaryxApp<FS> {
    /// DiaryxApp constructor
    pub fn new(fs: FS) -> Self {
        Self { fs }
    }

    /// Access the underlying filesystem.
    pub fn fs(&self) -> &FS {
        &self.fs
    }

    /// Create a new entry.
    pub async fn create_entry(&self, path: &str) -> Result<()> {
        let content = format!("---\ntitle: {}\n---\n\n# {}\n\n", path, path);
        self.fs
            .create_new(std::path::Path::new(path), &content)
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: PathBuf::from(path),
                source: e,
            })?;
        Ok(())
    }

    /// Template-based entry creation is temporarily disabled for the async-first API.
    ///
    /// `TemplateManager` is still `FileSystem`-based. Once it is refactored to `AsyncFileSystem`,
    /// these methods can be re-enabled.
    #[allow(dead_code)]
    pub fn template_manager(&self, _workspace_dir: Option<&Path>) -> TemplateManager<&FS> {
        unimplemented!("TemplateManager is not yet refactored to AsyncFileSystem");
    }

    /// Template-based entry creation is temporarily disabled for the async-first API.
    #[allow(dead_code)]
    pub async fn create_entry_with_template(
        &self,
        _path: &Path,
        _template: &Template,
        _context: &TemplateContext,
    ) -> Result<()> {
        Err(DiaryxError::Unsupported(
            "Template-based entry creation is not yet supported for AsyncFileSystem".to_string(),
        ))
    }

    /// Template-based entry creation is temporarily disabled for the async-first API.
    #[allow(dead_code)]
    pub async fn create_entry_from_template(
        &self,
        _path: &Path,
        _template_name: Option<&str>,
        _title: Option<&str>,
        _workspace_dir: Option<&Path>,
    ) -> Result<()> {
        Err(DiaryxError::Unsupported(
            "Template-based entry creation is not yet supported for AsyncFileSystem".to_string(),
        ))
    }

    /// Parses a markdown file and extracts frontmatter and body.
    /// Returns an error if no frontmatter is found.
    async fn parse_file(&self, path: &str) -> Result<(IndexMap<String, Value>, String)> {
        let path_buf = PathBuf::from(path);
        let content = self
            .fs
            .read_to_string(std::path::Path::new(path))
            .await
            .map_err(|e| DiaryxError::FileRead {
                path: path_buf.clone(),
                source: e,
            })?;

        // Check if content starts with frontmatter delimiter
        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            return Err(DiaryxError::NoFrontmatter(path_buf));

```


## Assistant

Let me check more of the file to find the `ensure_dated_entry` method:

**Tool Call: Search files for regex `ensure_dated_entry`**
Status: Completed

Found 6 matches:

## Matches in diaryx-core/crates/diaryx/src/cli/entry.rs

### pub fn handle_today › L21-31
```
        Ok(date) => {
            match app.ensure_dated_entry_with_template(&date, &config, template.as_deref()) {
                Ok(path) => {
                    println!("Opening: {}", path.display());
                    if let Err(e) = launch_editor(&path, &config) {
                        eprintln!("✗ Error launching editor: {}", e);
                    }
                    // Note: touch_updated is on async DiaryxApp, not DiaryxAppSync
                    // TODO: Add touch_updated to DiaryxAppSync or migrate to async
                }
                Err(e) => eprintln!("✗ Error creating entry: {}", e),
```

2 lines remaining in ancestor node. Read the file to see all.

### pub fn handle_yesterday › L46-56
```
        Ok(date) => {
            match app.ensure_dated_entry_with_template(&date, &config, template.as_deref()) {
                Ok(path) => {
                    println!("Opening: {}", path.display());
                    if let Err(e) = launch_editor(&path, &config) {
                        eprintln!("✗ Error launching editor: {}", e);
                    }
                    // Note: touch_updated is on async DiaryxApp, not DiaryxAppSync
                    // TODO: Add touch_updated to DiaryxAppSync or migrate to async
                }
                Err(e) => eprintln!("✗ Error creating entry: {}", e),
```

2 lines remaining in ancestor node. Read the file to see all.

### pub fn handle_open › L87-97
```
        if let Ok(date) = parse_date(path_or_date) {
            match app.ensure_dated_entry(&date, &config) {
                Ok(path) => {
                    println!("Opening: {}", path.display());
                    if let Err(e) = launch_editor(&path, &config) {
                        eprintln!("✗ Error launching editor: {}", e);
                    }
                    // Note: touch_updated is on async DiaryxApp, not DiaryxAppSync
                    // TODO: Add touch_updated to DiaryxAppSync or migrate to async
                    return;
                }
```

6 lines remaining in ancestor node. Read the file to see all.

## Matches in diaryx-core/crates/diaryx_core/src/entry/mod.rs

### impl DiaryxAppSync<FS> › pub fn ensure_dated_entry › L1006-1010
```
    /// Ensure a dated entry exists, creating it if necessary.
    /// This will NEVER overwrite an existing file.
    pub fn ensure_dated_entry(&self, date: &NaiveDate, config: &Config) -> Result<PathBuf> {
        self.ensure_dated_entry_with_template(date, config, None)
    }
```

### impl DiaryxAppSync<FS> › pub fn ensure_dated_entry_with_template › L1012-1016
```
    /// Ensure a dated entry exists with an optional template.
    /// This will NEVER overwrite an existing file.
    pub fn ensure_dated_entry_with_template(
        &self,
        date: &NaiveDate,
```

## Matches in diaryx-core/crates/diaryx_wasm/src/entry.rs

### impl DiaryxEntry › pub fn ensure_daily › L283-287
```
            );

            block_on(app.ensure_dated_entry(&today, &config))
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
```


## Assistant

I see - `ensure_dated_entry` is on `DiaryxAppSync`, not the async `DiaryxApp`. Let me check what methods are available on the async version and update the entry.rs accordingly:

**Tool Call: Search files for regex `pub async fn`**
Status: Completed

Found 18 matches:

## Matches in diaryx-core/crates/diaryx_core/src/entry/mod.rs

### impl DiaryxApp<FS> › pub async fn create_entry › L55-59
```

    /// Create a new entry.
    pub async fn create_entry(&self, path: &str) -> Result<()> {
        let content = format!("---\ntitle: {}\n---\n\n# {}\n\n", path, path);
        self.fs
```

### impl DiaryxApp<FS> › pub async fn create_entry_with_template › L78-82
```
    /// Template-based entry creation is temporarily disabled for the async-first API.
    #[allow(dead_code)]
    pub async fn create_entry_with_template(
        &self,
        _path: &Path,
```

### impl DiaryxApp<FS> › pub async fn create_entry_from_template › L91-95
```
    /// Template-based entry creation is temporarily disabled for the async-first API.
    #[allow(dead_code)]
    pub async fn create_entry_from_template(
        &self,
        _path: &Path,
```

### impl DiaryxApp<FS> › pub async fn set_frontmatter_property › L212-216
```
    /// Adds or updates a frontmatter property.
    /// Creates frontmatter if none exists.
    pub async fn set_frontmatter_property(&self, path: &str, key: &str, value: Value) -> Result<()> {
        let (mut frontmatter, body) = self.parse_file_or_create_frontmatter(path).await?;
        frontmatter.insert(key.to_string(), value);
```

### impl DiaryxApp<FS> › pub async fn remove_frontmatter_property › L220-224
```
    /// Removes a frontmatter property.
    /// Does nothing if no frontmatter exists or key is not found.
    pub async fn remove_frontmatter_property(&self, path: &str, key: &str) -> Result<()> {
        match self.parse_file(path).await {
            Ok((mut frontmatter, body)) => {
```

### impl DiaryxApp<FS> › pub async fn rename_frontmatter_property › L233-237
```
    /// Renames a frontmatter property key.
    /// Returns Ok(true) if the key was found and renamed, Ok(false) if key was not found or no frontmatter.
    pub async fn rename_frontmatter_property(
        &self,
        path: &str,
```

### impl DiaryxApp<FS> › pub async fn get_frontmatter_property › L265-269
```
    /// Gets a frontmatter property value.
    /// Returns Ok(None) if no frontmatter exists or key is not found.
    pub async fn get_frontmatter_property(&self, path: &str, key: &str) -> Result<Option<Value>> {
        match self.parse_file(path).await {
            Ok((frontmatter, _)) => Ok(frontmatter.get(key).cloned()),
```

### impl DiaryxApp<FS> › pub async fn get_all_frontmatter › L275-279
```
    /// Gets all frontmatter properties.
    /// Returns empty map if no frontmatter exists.
    pub async fn get_all_frontmatter(&self, path: &str) -> Result<IndexMap<String, Value>> {
        match self.parse_file(path).await {
            Ok((frontmatter, _)) => Ok(frontmatter),
```

### impl DiaryxApp<FS> › pub async fn get_content › L286-290
```

    /// Get the content (body) of a file, excluding frontmatter.
    pub async fn get_content(&self, path: &str) -> Result<String> {
        let (_, body) = self.parse_file_or_create_frontmatter(path).await?;
        Ok(body)
```

### impl DiaryxApp<FS> › pub async fn set_content › L293-297
```
    /// Set the content (body) of a file, preserving frontmatter.
    /// Creates frontmatter if none exists.
    pub async fn set_content(&self, path: &str, content: &str) -> Result<()> {
        let (frontmatter, _) = self.parse_file_or_create_frontmatter(path).await?;
        self.reconstruct_file(path, &frontmatter, content).await
```

### impl DiaryxApp<FS> › pub async fn clear_content › L299-303
```

    /// Clear the content (body) of a file, preserving frontmatter.
    pub async fn clear_content(&self, path: &str) -> Result<()> {
        self.set_content(path, "").await
    }
```

### impl DiaryxApp<FS> › pub async fn touch_updated › L305-309
```
    /// Update the 'updated' frontmatter property with the current timestamp (RFC 3339 format).
    /// Creates frontmatter if none exists.
    pub async fn touch_updated(&self, path: &str) -> Result<()> {
        let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.set_frontmatter_property(path, "updated", Value::String(timestamp))
```

### impl DiaryxApp<FS> › pub async fn save_content › L313-317
```
    /// Save content and update the 'updated' timestamp in one operation.
    /// This is a convenience method that combines set_content and touch_updated.
    pub async fn save_content(&self, path: &str, content: &str) -> Result<()> {
        self.set_content(path, content).await?;
        self.touch_updated(path).await
```

### impl DiaryxApp<FS> › pub async fn append_content › L319-323
```

    /// Append content to the end of a file's body.
    pub async fn append_content(&self, path: &str, content: &str) -> Result<()> {
        let (frontmatter, body) = self.parse_file_or_create_frontmatter(path).await?;
        let new_body = if body.is_empty() {
```

### impl DiaryxApp<FS> › pub async fn prepend_content › L332-336
```

    /// Prepend content to the beginning of a file's body.
    pub async fn prepend_content(&self, path: &str, content: &str) -> Result<()> {
        let (frontmatter, body) = self.parse_file_or_create_frontmatter(path).await?;
        let new_body = if body.is_empty() {
```

### impl DiaryxApp<FS> › pub async fn add_attachment › L348-352
```
    /// Add an attachment path to the entry's attachments list.
    /// Creates the attachments property if it doesn't exist.
    pub async fn add_attachment(&self, path: &str, attachment_path: &str) -> Result<()> {
        let (mut frontmatter, body) = self.parse_file_or_create_frontmatter(path).await?;

```

### impl DiaryxApp<FS> › pub async fn remove_attachment › L367-371
```
    /// Remove an attachment path from the entry's attachments list.
    /// Does nothing if the attachment isn't found.
    pub async fn remove_attachment(&self, path: &str, attachment_path: &str) -> Result<()> {
        let (mut frontmatter, body) = match self.parse_file(path).await {
            Ok(result) => result,
```

### impl DiaryxApp<FS> › pub async fn get_attachments › L393-397
```

    /// Get the list of attachments directly declared in this entry.
    pub async fn get_attachments(&self, path: &str) -> Result<Vec<String>> {
        let (frontmatter, _) = match self.parse_file(path).await {
            Ok(result) => result,
```


## Assistant

I see, so `ensure_dated_entry` is only available on `DiaryxAppSync` (the legacy sync version). Let me update entry.rs to use `DiaryxAppSync` for this specific method:

**Tool Call: crates/diaryx_wasm/src/entry.rs**
Status: Completed

Diff: crates/diaryx_wasm/src/entry.rs
```
//! Entry operations for WASM.

use std::path::{Path, PathBuf};

use chrono::Utc;
use diaryx_core::entry::{DiaryxApp, DiaryxAppSync};
use diaryx_core::fs::{FileSystem, SyncToAsyncFs};
use diaryx_core::template::TemplateManager;
use diaryx_core::workspace::Workspace;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::error::{IntoJsOption, IntoJsResult};
use crate::state::{block_on, with_fs, with_fs_mut};

// ============================================================================
// Types
// ============================================================================

/// Entry data returned to JavaScript
#[derive(Debug, Serialize)]
pub struct JsEntryData {
    pub path: String,
    pub title: Option<String>,
    pub frontmatter: serde_json::Map<String, serde_json::Value>,
    pub content: String,
}

/// Options for creating an entry
#[derive(Debug, Deserialize)]
pub struct CreateEntryOptions {
    pub title: Option<String>,
    pub part_of: Option<String>,
    pub template: Option<String>,
}

// ============================================================================
// DiaryxEntry Class
// ============================================================================

/// Entry operations for managing diary entries.
#[wasm_bindgen]
pub struct DiaryxEntry;

#[wasm_bindgen]
impl DiaryxEntry {
    /// Create a new DiaryxEntry instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Get an entry's content and metadata.
    #[wasm_bindgen]
    pub fn get(&self, path: &str) -> Result<JsValue, JsValue> {
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);

        with_fs(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);

            let frontmatter = block_on(app.get_all_frontmatter(path)).js_err()?;

            let mut json_frontmatter = serde_json::Map::new();
            for (key, value) in frontmatter {
                if let Ok(json_val) = serde_json::to_value(&value) {
                    json_frontmatter.insert(key, json_val);
                }
            }

            let title = json_frontmatter
                .get("title")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let content = block_on(app.get_content(path)).js_err()?;

            let entry = JsEntryData {
                path: path.to_string(),
                title,
                frontmatter: json_frontmatter,
                content,
            };

            entry.serialize(&serializer).js_err()
        })
    }

    /// Save an entry's content.
    #[wasm_bindgen]
    pub fn save(&self, path: &str, content: &str) -> Result<(), JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);
            block_on(app.save_content(path, content)).js_err()
        })
    }

    /// Get an entry's full raw file content.
    #[wasm_bindgen]
    pub fn read_raw(&self, path: &str) -> Result<String, JsValue> {
        with_fs(|fs| fs.read_to_string(Path::new(path)).js_err())
    }

    /// Create a new entry.
    #[wasm_bindgen]
    pub fn create(&self, path: &str, options: JsValue) -> Result<String, JsValue> {
        let options: Option<CreateEntryOptions> = if options.is_undefined() || options.is_null() {
            None
        } else {
            Some(serde_wasm_bindgen::from_value(options).js_err()?)
        };

        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);
            let path_buf = PathBuf::from(path);

            let title = options
                .as_ref()
                .and_then(|o| o.title.clone())
                .unwrap_or_else(|| {
                    path_buf
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Untitled")
                        .to_string()
                });

            // Check if template is requested
            if let Some(ref opts) = options
                && let Some(ref template_name) = opts.template
            {
                let manager = TemplateManager::new(fs);
                if let Some(template) = manager.get(template_name) {
                    let mut context = diaryx_core::template::TemplateContext::new()
                        .with_title(&title)
                        .with_date(Utc::now().date_naive());

                    if let Some(ref part_of) = opts.part_of {
                        context = context.with_part_of(part_of);
                    }

                    let content = template.render(&context);
                    fs.create_new(&path_buf, &content).js_err()?;
                    add_to_parent_index(fs, path)?;
                    return Ok(path.to_string());
                }
            }

            // Create entry without template
            block_on(app.create_entry(path)).js_err()?;

            // Set title and timestamps
            block_on(app.set_frontmatter_property(path, "title", serde_yaml::Value::String(title)))
                .js_err()?;

            let now = Utc::now().to_rfc3339();
            block_on(app.set_frontmatter_property(path, "created", serde_yaml::Value::String(now.clone())))
                .js_err()?;
            block_on(app.set_frontmatter_property(path, "updated", serde_yaml::Value::String(now)))
                .js_err()?;

            // Set part_of if provided
            if let Some(ref opts) = options
                && let Some(ref part_of) = opts.part_of
            {
                block_on(app.set_frontmatter_property(
                    path,
                    "part_of",
                    serde_yaml::Value::String(part_of.clone()),
                ))
                .js_err()?;
            }

            add_to_parent_index(fs, path)?;
            Ok(path.to_string())
        })
    }

    /// Delete an entry.
    #[wasm_bindgen]
    pub fn delete(&self, path: &str) -> Result<(), JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.delete_entry(&PathBuf::from(path))).js_err()
        })
    }

    /// Move/rename an entry.
    #[wasm_bindgen]
    pub fn move_entry(&self, from_path: &str, to_path: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.move_entry(&PathBuf::from(from_path), &PathBuf::from(to_path)))
                .js_err()?;
            Ok(to_path.to_string())
        })
    }

    /// Attach an entry to a parent.
    #[wasm_bindgen]
    pub fn attach_to_parent(&self, entry_path: &str, parent_path: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.attach_and_move_entry_to_parent(
                &PathBuf::from(entry_path),
                &PathBuf::from(parent_path),
            ))
            .map(|p| p.to_string_lossy().to_string())
            .js_err()
        })
    }

    /// Convert a leaf file to an index.
    #[wasm_bindgen]
    pub fn convert_to_index(&self, path: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.convert_to_index(&PathBuf::from(path)))
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
        })
    }

    /// Convert an index back to a leaf file.
    #[wasm_bindgen]
    pub fn convert_to_leaf(&self, path: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.convert_to_leaf(&PathBuf::from(path)))
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
        })
    }

    /// Create a new child entry under a parent.
    #[wasm_bindgen]
    pub fn create_child(&self, parent_path: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.create_child_entry(&PathBuf::from(parent_path), None))
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
        })
    }

    /// Rename an entry.
    #[wasm_bindgen]
    pub fn rename(&self, path: &str, new_filename: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.rename_entry(&PathBuf::from(path), new_filename))
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
        })
    }

    /// Ensure today's daily entry exists.
    #[wasm_bindgen]
    pub fn ensure_daily(&self) -> Result<String, JsValue> {
        use chrono::Local;
        use diaryx_core::config::Config;

        with_fs_mut(|fs| {
            // Use DiaryxAppSync for ensure_dated_entry (not yet async)
            let app = DiaryxAppSync::new(fs.clone());
            let today = Local::now().date_naive();

            let config = Config::with_options(
                PathBuf::from("workspace"),
                Some("Daily".to_string()),
                None,
                None,
                None,
            );

            app.ensure_dated_entry(&today, &config)
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
        })
    }
}

impl Default for DiaryxEntry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Add an entry to its parent index's contents array.
fn add_to_parent_index<FS: FileSystem>(fs: &FS, entry_path: &str) -> Result<(), JsValue> {
    let path = PathBuf::from(entry_path);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .js_ok_or("Invalid path")?;

    let parent = path.parent().js_ok_or("No parent directory")?;
    let index_path = parent.join("index.md");

    if !fs.exists(&index_path) {
        return Ok(());
    }

    let async_fs = SyncToAsyncFs::new(fs.clone());
    let app = DiaryxApp::new(async_fs);
    let index_path_str = index_path.to_string_lossy();

    let frontmatter = block_on(app.get_all_frontmatter(&index_path_str)).js_err()?;

    let mut contents: Vec<String> = frontmatter
        .get("contents")
        .and_then(|v| {
            if let serde_yaml::Value::Sequence(seq) = v {
                Some(
                    seq.iter()
                        .filter_map(|item| item.as_str().map(String::from))
                        .collect(),
                )
            } else {
                None
            }
        })
        .unwrap_or_default();

    if !contents.contains(&file_name.to_string()) {
        contents.push(file_name.to_string());
        contents.sort();

        let yaml_contents: Vec<serde_yaml::Value> = contents
            .into_iter()
            .map(serde_yaml::Value::String)
            .collect();

        block_on(app.set_frontmatter_property(
            &index_path_str,
            "contents",
            serde_yaml::Value::Sequence(yaml_contents),
        ))
        .js_err()?;
    }

    // Set part_of on entry if not present
    let entry_frontmatter = block_on(app.get_all_frontmatter(entry_path)).js_err()?;

    if !entry_frontmatter.contains_key("part_of") {
        block_on(app.set_frontmatter_property(
            entry_path,
            "part_of",
            serde_yaml::Value::String("index.md".to_string()),
        ))
        .js_err()?;
    }

    Ok(())
}

/// Convert a title to a kebab-case filename.
#[wasm_bindgen]
pub fn slugify_title(title: &str) -> String {
    diaryx_core::entry::slugify_title(title)
}
```


## Assistant

Now let's fix the frontmatter module:

**Tool Call: Read file `crates/diaryx_wasm/src/frontmatter.rs`**
Status: Completed

```diaryx-core/crates/diaryx_wasm/src/frontmatter.rs
//! Frontmatter operations for WASM.

use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::error::IntoJsResult;
use crate::state::{with_fs, with_fs_mut};
use diaryx_core::entry::DiaryxApp;

// ============================================================================
// DiaryxFrontmatter Class
// ============================================================================

/// Frontmatter operations for managing YAML frontmatter.
#[wasm_bindgen]
pub struct DiaryxFrontmatter;

#[wasm_bindgen]
impl DiaryxFrontmatter {
    /// Create a new DiaryxFrontmatter instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Get all frontmatter for an entry.
    #[wasm_bindgen]
    pub fn get_all(&self, path: &str) -> Result<JsValue, JsValue> {
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);

        with_fs(|fs| {
            let app = DiaryxApp::new(fs);

            let frontmatter = app.get_all_frontmatter(path).js_err()?;

            let mut json_map = serde_json::Map::new();
            for (key, value) in frontmatter {
                if let Ok(json_val) = serde_json::to_value(&value) {
                    json_map.insert(key, json_val);
                }
            }

            json_map.serialize(&serializer).js_err()
        })
    }

    /// Set a frontmatter property.
    #[wasm_bindgen]
    pub fn set_property(&self, path: &str, key: &str, value: JsValue) -> Result<(), JsValue> {
        with_fs_mut(|fs| {
            let app = DiaryxApp::new(fs);

            let json_value: serde_json::Value = serde_wasm_bindgen::from_value(value).js_err()?;
            let yaml_value: serde_yaml::Value = serde_json::from_value(json_value).js_err()?;

            app.set_frontmatter_property(path, key, yaml_value).js_err()
        })
    }

    /// Remove a frontmatter property.
    #[wasm_bindgen]
    pub fn remove_property(&self, path: &str, key: &str) -> Result<(), JsValue> {
        with_fs_mut(|fs| {
            let app = DiaryxApp::new(fs);
            app.remove_frontmatter_property(path, key).js_err()
        })
    }
}

impl Default for DiaryxFrontmatter {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Parse YAML frontmatter from raw markdown content.
#[wasm_bindgen]
pub fn parse_frontmatter(content: &str) -> Result<JsValue, JsValue> {
    if !content.starts_with("---\n") {
        return serde_wasm_bindgen::to_value(&serde_json::Map::<String, serde_json::Value>::new())
            .js_err();
    }

    let rest = &content[4..];
    let end_idx = rest.find("\n---");

    let yaml_str = match end_idx {
        Some(idx) => &rest[..idx],
        None => {
            return serde_wasm_bindgen::to_value(
                &serde_json::Map::<String, serde_json::Value>::new(),
            )
            .js_err();
        }
    };

    match serde_yaml::from_str::<serde_json::Value>(yaml_str) {
        Ok(serde_json::Value::Object(map)) => serde_wasm_bindgen::to_value(&map).js_err(),
        _ => serde_wasm_bindgen::to_value(&serde_json::Map::<String, serde_json::Value>::new())
            .js_err(),
    }
}

/// Serialize a JavaScript object to YAML frontmatter format.
#[wasm_bindgen]
pub fn serialize_frontmatter(frontmatter: JsValue) -> Result<String, JsValue> {
    let map: serde_json::Map<String, serde_json::Value> =
        serde_wasm_bindgen::from_value(frontmatter).js_err()?;

    let yaml = serde_yaml::to_string(&map).js_err()?;
    let yaml = yaml.trim_end();
    Ok(format!("---\n{}\n---", yaml))
}

/// Extract the body content from raw markdown.
#[wasm_bindgen]
pub fn extract_body(content: &str) -> String {
    if !content.starts_with("---\n") {
        return content.to_string();
    }

    let rest = &content[4..];
    if let Some(end_idx) = rest.find("\n---") {
        let after_frontmatter = &rest[end_idx + 4..];
        after_frontmatter.trim_start_matches('\n').to_string()
    } else {
        content.to_string()
    }
}

```


**Tool Call: crates/diaryx_wasm/src/frontmatter.rs**
Status: Completed

Diff: crates/diaryx_wasm/src/frontmatter.rs
```
//! Frontmatter operations for WASM.

use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::error::IntoJsResult;
use crate::state::{block_on, with_fs, with_fs_mut};
use diaryx_core::entry::DiaryxApp;
use diaryx_core::fs::SyncToAsyncFs;

// ============================================================================
// DiaryxFrontmatter Class
// ============================================================================

/// Frontmatter operations for managing YAML frontmatter.
#[wasm_bindgen]
pub struct DiaryxFrontmatter;

#[wasm_bindgen]
impl DiaryxFrontmatter {
    /// Create a new DiaryxFrontmatter instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Get all frontmatter for an entry.
    #[wasm_bindgen]
    pub fn get_all(&self, path: &str) -> Result<JsValue, JsValue> {
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);

        with_fs(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);

            let frontmatter = block_on(app.get_all_frontmatter(path)).js_err()?;

            let mut json_map = serde_json::Map::new();
            for (key, value) in frontmatter {
                if let Ok(json_val) = serde_json::to_value(&value) {
                    json_map.insert(key, json_val);
                }
            }

            json_map.serialize(&serializer).js_err()
        })
    }

    /// Set a frontmatter property.
    #[wasm_bindgen]
    pub fn set_property(&self, path: &str, key: &str, value: JsValue) -> Result<(), JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);

            let json_value: serde_json::Value = serde_wasm_bindgen::from_value(value).js_err()?;
            let yaml_value: serde_yaml::Value = serde_json::from_value(json_value).js_err()?;

            block_on(app.set_frontmatter_property(path, key, yaml_value)).js_err()
        })
    }

    /// Remove a frontmatter property.
    #[wasm_bindgen]
    pub fn remove_property(&self, path: &str, key: &str) -> Result<(), JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);
            block_on(app.remove_frontmatter_property(path, key)).js_err()
        })
    }
}

impl Default for DiaryxFrontmatter {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Parse YAML frontmatter from raw markdown content.
#[wasm_bindgen]
pub fn parse_frontmatter(content: &str) -> Result<JsValue, JsValue> {
    if !content.starts_with("---\n") {
        return serde_wasm_bindgen::to_value(&serde_json::Map::<String, serde_json::Value>::new())
            .js_err();
    }

    let rest = &content[4..];
    let end_idx = rest.find("\n---");

    let yaml_str = match end_idx {
        Some(idx) => &rest[..idx],
        None => {
            return serde_wasm_bindgen::to_value(
                &serde_json::Map::<String, serde_json::Value>::new(),
            )
            .js_err();
        }
    };

    match serde_yaml::from_str::<serde_json::Value>(yaml_str) {
        Ok(serde_json::Value::Object(map)) => serde_wasm_bindgen::to_value(&map).js_err(),
        _ => serde_wasm_bindgen::to_value(&serde_json::Map::<String, serde_json::Value>::new())
            .js_err(),
    }
}

/// Serialize a JavaScript object to YAML frontmatter format.
#[wasm_bindgen]
pub fn serialize_frontmatter(frontmatter: JsValue) -> Result<String, JsValue> {
    let map: serde_json::Map<String, serde_json::Value> =
        serde_wasm_bindgen::from_value(frontmatter).js_err()?;

    let yaml = serde_yaml::to_string(&map).js_err()?;
    let yaml = yaml.trim_end();
    Ok(format!("---\n{}\n---", yaml))
}

/// Extract the body content from raw markdown.
#[wasm_bindgen]
pub fn extract_body(content: &str) -> String {
    if !content.starts_with("---\n") {
        return content.to_string();
    }

    let rest = &content[4..];
    if let Some(end_idx) = rest.find("\n---") {
        let after_frontmatter = &rest[end_idx + 4..];
        after_frontmatter.trim_start_matches('\n').to_string()
    } else {
        content.to_string()
    }
}
```


**Tool Call: `cd crates/diaryx_wasm && cargo check 2>&1 | head -100`**
Status: Completed

Terminal:
```
warning: profiles for the non root package will be ignored, specify profiles at the workspace root:
package:   /Users/adamharris/Documents/diaryx-core/crates/diaryx_wasm/Cargo.toml
workspace: /Users/adamharris/Documents/diaryx-core/Cargo.toml
    Checking diaryx_wasm v0.7.0 (/Users/adamharris/Documents/diaryx-core/crates/diaryx_wasm)
warning: unused import: `with_fs`
  --> crates/diaryx_wasm/src/export.rs:14:45
   |
14 | use crate::state::{block_on, with_async_fs, with_fs};
   |                                             ^^^^^^^
   |
   = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused import: `std::pin::Pin`
  --> crates/diaryx_wasm/src/js_async_fs.rs:63:5
   |
63 | use std::pin::Pin;
   |     ^^^^^^^^^^^^^

warning: unused import: `std::pin::Pin`
 --> crates/diaryx_wasm/src/state.rs:5:5
  |
5 | use std::pin::Pin;
  |     ^^^^^^^^^^^^^

warning: unused import: `with_fs`
  --> crates/diaryx_wasm/src/workspace.rs:12:45
   |
12 | use crate::state::{block_on, with_async_fs, with_fs};
   |                                             ^^^^^^^

warning: unused import: `FileSystem`
 --> crates/diaryx_wasm/src/export.rs:7:40
  |
7 | use diaryx_core::fs::{AsyncFileSystem, FileSystem};
  |                                        ^^^^^^^^^^

error[E0382]: borrow of moved value: `fs`
   --> crates/diaryx_wasm/src/export.rs:259:40
    |
202 |         with_async_fs(|fs| {
    |                        -- move occurs because `fs` has type `SyncToAsyncFs<&InMemoryFileSystem>`, which does not implement the `Copy` trait
...
250 |             let exporter = Exporter::new(fs);
    |                                          -- value moved here
...
259 |                 let content = block_on(fs.read_to_string(&export_file.source_path)).js_err()?;
    |                                        ^^ value borrowed here after move
    |
help: consider cloning the value if the performance cost is acceptable
    |
250 |             let exporter = Exporter::new(fs.clone());
    |                                            ++++++++

error[E0382]: borrow of moved value: `fs`
   --> crates/diaryx_wasm/src/export.rs:390:40
    |
282 |         with_async_fs(|fs| {
    |                        -- move occurs because `fs` has type `SyncToAsyncFs<&InMemoryFileSystem>`, which does not implement the `Copy` trait
...
381 |             let exporter = Exporter::new(fs);
    |                                          -- value moved here
...
390 |                 let content = block_on(fs.read_to_string(&export_file.source_path)).js_err()?;
    |                                        ^^ value borrowed here after move
    |
help: consider cloning the value if the performance cost is acceptable
    |
381 |             let exporter = Exporter::new(fs.clone());
    |                                            ++++++++

error: future cannot be sent between threads safely
   --> crates/diaryx_wasm/src/js_async_fs.rs:246:9
    |
246 | /         Box::pin(async move {
247 | |             let callbacks = callbacks.borrow();
248 | |             let result = call_async_callback(
249 | |                 &callbacks,
...   |
257 | |                 .ok_or_else(|| Error::new(ErrorKind::InvalidData, "readToString did not return a string"))
258 | |         })
    | |__________^ future created by async block is not `Send`
    |
    = help: within `{async block@crates/diaryx_wasm/src/js_async_fs.rs:246:18: 246:28}`, the trait `Send` is not implemented for `Rc<RefCell<wasm_bindgen::JsValue>>`
note: captured value is not `Send`
   --> crates/diaryx_wasm/src/js_async_fs.rs:247:29
    |
247 |             let callbacks = callbacks.borrow();
    |                             ^^^^^^^^^ has type `Rc<RefCell<wasm_bindgen::JsValue>>` which is not `Send`
    = note: required for the cast from `Pin<Box<{async block@crates/diaryx_wasm/src/js_async_fs.rs:246:18: 246:28}>>` to `Pin<Box<dyn Future<Output = Result<std::string::String, std::io::Error>> + Send>>`

error: future cannot be sent between threads safely
   --> crates/diaryx_wasm/src/js_async_fs.rs:246:9
    |
246 | /         Box::pin(async move {
247 | |             let callbacks = callbacks.borrow();
248 | |             let result = call_async_callback(
249 | |                 &callbacks,
...   |
257 | |                 .ok_or_else(|| Error::new(ErrorKind::InvalidData, "readToString did not return a string"))
258 | |         })

```


## Assistant

There are two issues:
1. The borrow issue in export.rs - need to clone `fs`
2. The `Send` issue in `JsAsyncFileSystem` - we need a different approach for WASM since `Rc<RefCell>` is not `Send`

For WASM, we don't actually need `Send` since it's single-threaded, but the `BoxFuture` type requires it. Let me fix this by changing the approach:

**Tool Call: Read file `crates/diaryx_core/src/fs/async_fs.rs` (lines 1-40)**
Status: Completed

````diaryx-core/crates/diaryx_core/src/fs/async_fs.rs
//! Async filesystem abstraction module.
//!
//! This module provides the `AsyncFileSystem` trait for abstracting async filesystem operations,
//! allowing different implementations for native and WASM targets.
//!
//! This is particularly useful for:
//! - WASM environments where JavaScript APIs (like IndexedDB) are inherently async
//! - Native environments using async runtimes like tokio
//! - Code that needs to await filesystem operations
//!
//! ## Object safety
//!
//! `AsyncFileSystem` is designed to be object-safe so it can be used behind
//! `dyn AsyncFileSystem` (e.g. inside trait objects like backup targets).
//! To enable this, all methods return boxed futures.

use std::future::Future;
use std::io::Result;
use std::path::{Path, PathBuf};
use std::pin::Pin;

#[cfg(test)]
pub(crate) fn block_on_test<F: Future>(f: F) -> F::Output {
    futures_lite::future::block_on(f)
}

/// A boxed future that is Send.
/// Used for object-safe async methods and recursion.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Async abstraction over filesystem operations.
///
/// This trait mirrors `FileSystem` but with async methods, making it suitable
/// for environments where filesystem operations may be asynchronous (e.g., WASM
/// with IndexedDB, or native code using async I/O).
///
/// # Example
///
/// ```ignore
/// use diaryx_core::fs::AsyncFileSystem;

````


## Assistant

I see - the `BoxFuture` requires `Send`. For WASM, we need to deal with this differently. Since WASM is single-threaded, we can use a separate non-Send future type. However, that would require major changes to the core library.

A simpler approach is to not use `Rc<RefCell>` but instead clone the callbacks reference directly. Let me reconsider the design.

Actually, the simplest fix for now is to make `JsAsyncFileSystem` store the callbacks in a way that is `Send` (by storing them in the async block itself). Let me rewrite `JsAsyncFileSystem`:

**Tool Call: crates/diaryx_wasm/src/js_async_fs.rs**
Status: Completed

Diff: crates/diaryx_wasm/src/js_async_fs.rs
```
//! JavaScript-backed AsyncFileSystem implementation.
//!
//! This module provides `JsAsyncFileSystem`, which implements the `AsyncFileSystem` trait
//! by delegating all operations to JavaScript callbacks. This allows the web frontend
//! to provide its own storage backend (IndexedDB, OPFS, localStorage, etc.) while
//! the Rust/WASM code uses the standard async filesystem interface.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { JsAsyncFileSystem } from './wasm/diaryx_wasm.js';
//!
//! // Create filesystem with JavaScript callbacks
//! const fs = new JsAsyncFileSystem({
//!   readToString: async (path) => {
//!     const data = await indexedDB.get(path);
//!     return data?.content;
//!   },
//!   writeFile: async (path, content) => {
//!     await indexedDB.put({ path, content });
//!   },
//!   deleteFile: async (path) => {
//!     await indexedDB.delete(path);
//!   },
//!   exists: async (path) => {
//!     return await indexedDB.has(path);
//!   },
//!   isDir: async (path) => {
//!     return path.endsWith('/') || await hasChildren(path);
//!   },
//!   listFiles: async (dir) => {
//!     return await indexedDB.listDir(dir);
//!   },
//!   listMdFiles: async (dir) => {
//!     const files = await indexedDB.listDir(dir);
//!     return files.filter(f => f.endsWith('.md'));
//!   },
//!   createDirAll: async (path) => {
//!     // No-op for flat storage, or create directory markers
//!   },
//!   moveFile: async (from, to) => {
//!     const content = await indexedDB.get(from);
//!     await indexedDB.put({ path: to, content: content.content });
//!     await indexedDB.delete(from);
//!   },
//!   readBinary: async (path) => {
//!     const data = await indexedDB.get(path);
//!     return new Uint8Array(data?.binary);
//!   },
//!   writeBinary: async (path, data) => {
//!     await indexedDB.put({ path, binary: Array.from(data) });
//!   },
//! });
//!
//! // Now use fs with async WASM operations
//! const workspace = new DiaryxAsyncWorkspace(fs);
//! const tree = await workspace.getTree('workspace');
//! ```

use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

use diaryx_core::fs::{AsyncFileSystem, BoxFuture};
use js_sys::{Array, Function, Promise, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

// ============================================================================
// JavaScript Callback Interface
// ============================================================================

/// JavaScript callbacks for filesystem operations.
///
/// All callbacks are optional. If a callback is not provided, the operation
/// will return an appropriate error or default value.
#[wasm_bindgen]
extern "C" {
    /// JavaScript object containing filesystem callbacks.
    #[wasm_bindgen(typescript_type = "JsFileSystemCallbacks")]
    pub type JsFileSystemCallbacks;

    #[wasm_bindgen(method, getter, js_name = "readToString")]
    fn read_to_string_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "writeFile")]
    fn write_file_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "createNew")]
    fn create_new_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "deleteFile")]
    fn delete_file_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "exists")]
    fn exists_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "isDir")]
    fn is_dir_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "listFiles")]
    fn list_files_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "listMdFiles")]
    fn list_md_files_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "createDirAll")]
    fn create_dir_all_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "moveFile")]
    fn move_file_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "readBinary")]
    fn read_binary_cb(this: &JsFileSystemCallbacks) -> Option<Function>;

    #[wasm_bindgen(method, getter, js_name = "writeBinary")]
    fn write_binary_cb(this: &JsFileSystemCallbacks) -> Option<Function>;
}

// ============================================================================
// JsAsyncFileSystem Implementation
// ============================================================================

/// An `AsyncFileSystem` implementation backed by JavaScript callbacks.
///
/// This struct allows Rust code to use the async filesystem interface while
/// delegating actual storage operations to JavaScript. This is useful for:
///
/// - Using IndexedDB for persistent storage in browsers
/// - Using OPFS (Origin Private File System) for better performance
/// - Integrating with existing JavaScript storage solutions
/// - Testing with mock filesystems
///
/// ## Thread Safety
///
/// This type is designed for single-threaded WASM environments. The callbacks
/// JsValue is cloned into each async operation to satisfy Send requirements,
/// but actual execution remains single-threaded.
#[wasm_bindgen]
#[derive(Clone)]
pub struct JsAsyncFileSystem {
    // We store the callbacks as a JsValue which can be cloned
    // Each async operation will clone this and work with its own copy
    callbacks: JsValue,
}

#[wasm_bindgen]
impl JsAsyncFileSystem {
    /// Create a new JsAsyncFileSystem with the provided callbacks.
    ///
    /// The callbacks object should implement the `JsFileSystemCallbacks` interface.
    /// All callbacks are optional - missing callbacks will cause operations to fail
    /// with appropriate errors.
    #[wasm_bindgen(constructor)]
    pub fn new(callbacks: JsValue) -> Self {
        Self { callbacks }
    }

    /// Check if the filesystem has a specific callback.
    #[wasm_bindgen]
    pub fn has_callback(&self, name: &str) -> bool {
        if let Ok(obj) = js_sys::Reflect::get(&self.callbacks, &JsValue::from_str(name)) {
            obj.is_function()
        } else {
            false
        }
    }
}

// Helper function to convert JsValue error to io::Error
fn js_to_io_error(err: JsValue) -> Error {
    let msg = if let Some(s) = err.as_string() {
        s
    } else if let Some(obj) = err.dyn_ref::<js_sys::Object>() {
        obj.to_string().as_string().unwrap_or_else(|| "Unknown JS error".to_string())
    } else {
        "Unknown JS error".to_string()
    };
    Error::new(ErrorKind::Other, msg)
}

// Helper function to get a callback from the callbacks object
fn get_callback(callbacks: &JsValue, name: &str) -> Option<Function> {
    js_sys::Reflect::get(callbacks, &JsValue::from_str(name))
        .ok()
        .and_then(|v| v.dyn_into::<Function>().ok())
}

// Helper to call a JS callback that returns a Promise
async fn call_async_callback(
    callbacks: &JsValue,
    name: &str,
    args: &[JsValue],
) -> std::result::Result<JsValue, Error> {
    let callback = get_callback(callbacks, name)
        .ok_or_else(|| Error::new(ErrorKind::Unsupported, format!("Callback '{}' not provided", name)))?;

    let this = JsValue::NULL;
    let result = match args.len() {
        0 => callback.call0(&this),
        1 => callback.call1(&this, &args[0]),
        2 => callback.call2(&this, &args[0], &args[1]),
        3 => callback.call3(&this, &args[0], &args[1], &args[2]),
        _ => {
            let js_args = Array::new();
            for arg in args {
                js_args.push(arg);
            }
            callback.apply(&this, &js_args)
        }
    }
    .map_err(js_to_io_error)?;

    // If result is a Promise, await it
    if result.has_type::<Promise>() {
        let promise: Promise = result.unchecked_into();
        JsFuture::from(promise).await.map_err(js_to_io_error)
    } else {
        Ok(result)
    }
}

// ============================================================================
// AsyncFileSystem Implementation
// ============================================================================

// Note: WASM is single-threaded, but the trait requires Send + Sync.
// We use unsafe impl since we know this will only be used in single-threaded WASM.
// The JsValue is cloned into each async block, so there's no actual sharing.
unsafe impl Send for JsAsyncFileSystem {}
unsafe impl Sync for JsAsyncFileSystem {}

impl AsyncFileSystem for JsAsyncFileSystem {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();

        Box::pin(async move {
            let result = call_async_callback(
                &callbacks,
                "readToString",
                &[JsValue::from_str(&path_str)],
            )
            .await?;

            result
                .as_string()
                .ok_or_else(|| Error::new(ErrorKind::InvalidData, "readToString did not return a string"))
        })
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();
        let content = content.to_string();

        Box::pin(async move {
            call_async_callback(
                &callbacks,
                "writeFile",
                &[JsValue::from_str(&path_str), JsValue::from_str(&content)],
            )
            .await?;
            Ok(())
        })
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();
        let content = content.to_string();

        Box::pin(async move {
            // Check if createNew callback exists
            if get_callback(&callbacks, "createNew").is_some() {
                call_async_callback(
                    &callbacks,
                    "createNew",
                    &[JsValue::from_str(&path_str), JsValue::from_str(&content)],
                )
                .await?;
                Ok(())
            } else {
                // Fall back to exists + writeFile
                let exists_result = call_async_callback(
                    &callbacks,
                    "exists",
                    &[JsValue::from_str(&path_str)],
                )
                .await?;

                if exists_result.as_bool().unwrap_or(false) {
                    return Err(Error::new(ErrorKind::AlreadyExists, format!("File already exists: {}", path_str)));
                }

                call_async_callback(
                    &callbacks,
                    "writeFile",
                    &[JsValue::from_str(&path_str), JsValue::from_str(&content)],
                )
                .await?;
                Ok(())
            }
        })
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();

        Box::pin(async move {
            call_async_callback(
                &callbacks,
                "deleteFile",
                &[JsValue::from_str(&path_str)],
            )
            .await?;
            Ok(())
        })
    }

    fn list_md_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        let callbacks = self.callbacks.clone();
        let dir_str = dir.to_string_lossy().to_string();

        Box::pin(async move {
            let result = call_async_callback(
                &callbacks,
                "listMdFiles",
                &[JsValue::from_str(&dir_str)],
            )
            .await?;

            parse_path_array(result)
        })
    }

    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();

        Box::pin(async move {
            let result = call_async_callback(
                &callbacks,
                "exists",
                &[JsValue::from_str(&path_str)],
            )
            .await;

            match result {
                Ok(v) => v.as_bool().unwrap_or(false),
                Err(_) => false,
            }
        })
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();

        Box::pin(async move {
            // createDirAll is optional - many storage backends don't need it
            if get_callback(&callbacks, "createDirAll").is_some() {
                call_async_callback(
                    &callbacks,
                    "createDirAll",
                    &[JsValue::from_str(&path_str)],
                )
                .await?;
            }
            Ok(())
        })
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();

        Box::pin(async move {
            let result = call_async_callback(
                &callbacks,
                "isDir",
                &[JsValue::from_str(&path_str)],
            )
            .await;

            match result {
                Ok(v) => v.as_bool().unwrap_or(false),
                Err(_) => false,
            }
        })
    }

    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        let callbacks = self.callbacks.clone();
        let from_str = from.to_string_lossy().to_string();
        let to_str = to.to_string_lossy().to_string();

        Box::pin(async move {
            call_async_callback(
                &callbacks,
                "moveFile",
                &[JsValue::from_str(&from_str), JsValue::from_str(&to_str)],
            )
            .await?;
            Ok(())
        })
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();

        Box::pin(async move {
            let result = call_async_callback(
                &callbacks,
                "readBinary",
                &[JsValue::from_str(&path_str)],
            )
            .await?;

            // Handle Uint8Array or Array
            if let Some(uint8_array) = result.dyn_ref::<Uint8Array>() {
                Ok(uint8_array.to_vec())
            } else if let Some(array) = result.dyn_ref::<Array>() {
                let mut bytes = Vec::with_capacity(array.length() as usize);
                for i in 0..array.length() {
                    let val = array.get(i);
                    let byte = val.as_f64().unwrap_or(0.0) as u8;
                    bytes.push(byte);
                }
                Ok(bytes)
            } else {
                Err(Error::new(ErrorKind::InvalidData, "readBinary did not return a Uint8Array or Array"))
            }
        })
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();
        let content = content.to_vec();

        Box::pin(async move {
            // Convert bytes to Uint8Array
            let uint8_array = Uint8Array::new_with_length(content.len() as u32);
            uint8_array.copy_from(&content);

            call_async_callback(
                &callbacks,
                "writeBinary",
                &[JsValue::from_str(&path_str), uint8_array.into()],
            )
            .await?;
            Ok(())
        })
    }

    fn list_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        let callbacks = self.callbacks.clone();
        let dir_str = dir.to_string_lossy().to_string();

        Box::pin(async move {
            let result = call_async_callback(
                &callbacks,
                "listFiles",
                &[JsValue::from_str(&dir_str)],
            )
            .await?;

            parse_path_array(result)
        })
    }
}

// Helper function to parse a JS array of strings into Vec<PathBuf>
fn parse_path_array(value: JsValue) -> Result<Vec<PathBuf>> {
    if let Some(array) = value.dyn_ref::<Array>() {
        let mut paths = Vec::with_capacity(array.length() as usize);
        for i in 0..array.length() {
            let item = array.get(i);
            if let Some(s) = item.as_string() {
                paths.push(PathBuf::from(s));
            }
        }
        Ok(paths)
    } else {
        Err(Error::new(ErrorKind::InvalidData, "Expected array of strings"))
    }
}

// ============================================================================
// TypeScript Type Definitions
// ============================================================================

#[wasm_bindgen(typescript_custom_section)]
const TS_APPEND_CONTENT: &'static str = r#"
/**
 * Callbacks for JsAsyncFileSystem operations.
 * 
 * All callbacks should return Promises. If a callback is not provided,
 * the corresponding operation will fail with an error.
 */
export interface JsFileSystemCallbacks {
    /**
     * Read a file's content as a string.
     * @param path - The file path to read
     * @returns Promise resolving to the file content
     */
    readToString?: (path: string) => Promise<string>;
    
    /**
     * Write content to a file, creating or overwriting it.
     * @param path - The file path to write
     * @param content - The content to write
     */
    writeFile?: (path: string, content: string) => Promise<void>;
    
    /**
     * Create a new file, failing if it already exists.
     * @param path - The file path to create
     * @param content - The initial content
     */
    createNew?: (path: string, content: string) => Promise<void>;
    
    /**
     * Delete a file.
     * @param path - The file path to delete
     */
    deleteFile?: (path: string) => Promise<void>;
    
    /**
     * Check if a path exists.
     * @param path - The path to check
     * @returns Promise resolving to true if the path exists
     */
    exists?: (path: string) => Promise<boolean>;
    
    /**
     * Check if a path is a directory.
     * @param path - The path to check
     * @returns Promise resolving to true if the path is a directory
     */
    isDir?: (path: string) => Promise<boolean>;
    
    /**
     * List all files in a directory (not recursive).
     * @param dir - The directory path
     * @returns Promise resolving to array of file paths
     */
    listFiles?: (dir: string) => Promise<string[]>;
    
    /**
     * List markdown files in a directory (not recursive).
     * @param dir - The directory path
     * @returns Promise resolving to array of .md file paths
     */
    listMdFiles?: (dir: string) => Promise<string[]>;
    
    /**
     * Create a directory and all parent directories.
     * @param path - The directory path to create
     */
    createDirAll?: (path: string) => Promise<void>;
    
    /**
     * Move/rename a file.
     * @param from - The source path
     * @param to - The destination path
     */
    moveFile?: (from: string, to: string) => Promise<void>;
    
    /**
     * Read binary file content.
     * @param path - The file path to read
     * @returns Promise resolving to file content as Uint8Array
     */
    readBinary?: (path: string) => Promise<Uint8Array>;
    
    /**
     * Write binary content to a file.
     * @param path - The file path to write
     * @param data - The binary content as Uint8Array
     */
    writeBinary?: (path: string, data: Uint8Array) => Promise<void>;
}
"#;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_js_async_filesystem_creation() {
        // Just verify the struct can be created with a null JsValue
        let _fs = JsAsyncFileSystem::new(JsValue::NULL);
    }

    #[test]
    fn test_js_async_filesystem_clone() {
        let fs1 = JsAsyncFileSystem::new(JsValue::NULL);
        let fs2 = fs1.clone();
        // Both should be independent clones
        assert!(!fs1.has_callback("test"));
        assert!(!fs2.has_callback("test"));
    }
}
```


## Assistant

Now let's fix the export.rs borrow issues:

**Tool Call: Read file `crates/diaryx_wasm/src/export.rs` (lines 200-300)**
Status: Completed

```diaryx-core/crates/diaryx_wasm/src/export.rs
    #[wasm_bindgen]
    pub fn to_memory(&self, root_path: &str, audience: &str) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            if audience == "*" {
                let ws = Workspace::new(fs);
                let mut files: Vec<ExportedFile> = Vec::new();
                let root = Path::new(root_path);
                let root_dir = root.parent().unwrap_or(root);

                fn collect_all<FS: AsyncFileSystem>(
                    ws: &Workspace<FS>,
                    path: &Path,
                    root_dir: &Path,
                    files: &mut Vec<ExportedFile>,
                    visited: &mut HashSet<PathBuf>,
                ) {
                    if visited.contains(path) {
                        return;
                    }
                    visited.insert(path.to_path_buf());

                    if let Ok(index) = block_on(ws.parse_index(path)) {
                        let relative_path = pathdiff::diff_paths(path, root_dir)
                            .unwrap_or_else(|| path.to_path_buf());

                        if let Ok(content) = block_on(ws.fs_ref().read_to_string(path)) {
                            let processed = remove_audience_from_content(&content);
                            files.push(ExportedFile {
                                path: relative_path.to_string_lossy().to_string(),
                                content: processed,
                            });
                        }

                        if index.frontmatter.is_index() {
                            for child_rel in index.frontmatter.contents_list() {
                                let child_path = index.resolve_path(child_rel);
                                if block_on(ws.fs_ref().exists(&child_path)) {
                                    collect_all(ws, &child_path, root_dir, files, visited);
                                }
                            }
                        }
                    }
                }

                let mut visited = HashSet::new();
                collect_all(&ws, root, root_dir, &mut files, &mut visited);

                return serde_wasm_bindgen::to_value(&files).js_err();
            }

            let exporter = Exporter::new(fs);

            let plan = block_on(exporter
                .plan_export(Path::new(root_path), audience, Path::new("/export")))
                .js_err()?;

            let mut files: Vec<ExportedFile> = Vec::new();

            for export_file in &plan.included {
                let content = block_on(fs.read_to_string(&export_file.source_path)).js_err()?;

                let processed = if !export_file.filtered_contents.is_empty() {
                    filter_contents_and_audience(&content, &export_file.filtered_contents)
                } else {
                    remove_audience_from_content(&content)
                };

                files.push(ExportedFile {
                    path: export_file.relative_path.to_string_lossy().to_string(),
                    content: processed,
                });
            }

            serde_wasm_bindgen::to_value(&files).js_err()
        })
    }

    /// Export files to memory as HTML.
    #[wasm_bindgen]
    pub fn to_html(&self, root_path: &str, audience: &str) -> Result<JsValue, JsValue> {
        use comrak::{Options, markdown_to_html};

        with_async_fs(|fs| {
            fn convert_md_to_html(markdown: &str) -> String {
                let mut options = Options::default();
                options.extension.strikethrough = true;
                options.extension.table = true;
                options.extension.autolink = true;
                options.extension.tasklist = true;
                options.render.r#unsafe = true;

                let html_body = markdown_to_html(markdown, &options);

                format!(
                    r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; max-width: 800px; margin: 0 auto; padding: 2rem; line-height: 1.6; }}

```


## Assistant

I see - the issue is that `Exporter::new(fs)` moves `fs`. Let me look at that section and fix it:

**Tool Call: Read file `crates/diaryx_wasm/src/export.rs` (lines 245-280)**
Status: Completed

```diaryx-core/crates/diaryx_wasm/src/export.rs
                collect_all(&ws, root, root_dir, &mut files, &mut visited);

                return serde_wasm_bindgen::to_value(&files).js_err();
            }

            let exporter = Exporter::new(fs);

            let plan = block_on(exporter
                .plan_export(Path::new(root_path), audience, Path::new("/export")))
                .js_err()?;

            let mut files: Vec<ExportedFile> = Vec::new();

            for export_file in &plan.included {
                let content = block_on(fs.read_to_string(&export_file.source_path)).js_err()?;

                let processed = if !export_file.filtered_contents.is_empty() {
                    filter_contents_and_audience(&content, &export_file.filtered_contents)
                } else {
                    remove_audience_from_content(&content)
                };

                files.push(ExportedFile {
                    path: export_file.relative_path.to_string_lossy().to_string(),
                    content: processed,
                });
            }

            serde_wasm_bindgen::to_value(&files).js_err()
        })
    }

    /// Export files to memory as HTML.
    #[wasm_bindgen]
    pub fn to_html(&self, root_path: &str, audience: &str) -> Result<JsValue, JsValue> {
        use comrak::{Options, markdown_to_html};

```


## Assistant

I need to look at the full export.rs file to understand the structure and fix the borrowing issue:

**Tool Call: Read file `crates/diaryx_wasm/src/export.rs` (lines 1-60)**
Status: Completed

```diaryx-core/crates/diaryx_wasm/src/export.rs
//! Export operations for WASM.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use diaryx_core::export::Exporter;
use diaryx_core::fs::{AsyncFileSystem, FileSystem};
use diaryx_core::workspace::Workspace;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::error::IntoJsResult;
use crate::frontmatter::extract_body;
use crate::state::{block_on, with_async_fs, with_fs};

// ============================================================================
// Types
// ============================================================================

#[derive(Serialize)]
struct ExportPlanJs {
    included: Vec<IncludedFileJs>,
    excluded: Vec<ExcludedFileJs>,
    audience: String,
}

#[derive(Serialize)]
struct IncludedFileJs {
    path: String,
    relative_path: String,
}

#[derive(Serialize)]
struct ExcludedFileJs {
    path: String,
    reason: String,
}

#[derive(Serialize)]
struct ExportedFile {
    path: String,
    content: String,
}

#[derive(Serialize)]
struct BinaryExportFile {
    path: String,
    data: Vec<u8>,
}

// ============================================================================
// DiaryxExport Class
// ============================================================================

/// Export operations for exporting workspace content.
#[wasm_bindgen]
pub struct DiaryxExport;

#[wasm_bindgen]
impl DiaryxExport {

```


## Assistant

Let me look at the issue locations and fix them. The problem is that `Exporter::new(fs)` consumes `fs`, but we need to use `fs` later. The fix is to clone `fs` or use references properly:

**Tool Call: crates/diaryx_wasm/src/export.rs**
Status: Completed

Diff: crates/diaryx_wasm/src/export.rs
```
//! Export operations for WASM.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use diaryx_core::export::Exporter;
use diaryx_core::fs::{AsyncFileSystem, FileSystem};
use diaryx_core::workspace::Workspace;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::error::IntoJsResult;
use crate::frontmatter::extract_body;
use crate::state::{block_on, with_async_fs, with_fs};

// ============================================================================
// Types
// ============================================================================

#[derive(Serialize)]
struct ExportPlanJs {
    included: Vec<IncludedFileJs>,
    excluded: Vec<ExcludedFileJs>,
    audience: String,
}

#[derive(Serialize)]
struct IncludedFileJs {
    path: String,
    relative_path: String,
}

#[derive(Serialize)]
struct ExcludedFileJs {
    path: String,
    reason: String,
}

#[derive(Serialize)]
struct ExportedFile {
    path: String,
    content: String,
}

#[derive(Serialize)]
struct BinaryExportFile {
    path: String,
    data: Vec<u8>,
}

// ============================================================================
// DiaryxExport Class
// ============================================================================

/// Export operations for exporting workspace content.
#[wasm_bindgen]
pub struct DiaryxExport;

#[wasm_bindgen]
impl DiaryxExport {
    /// Create a new DiaryxExport instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Get all available audience tags from the workspace.
    #[wasm_bindgen]
    pub fn get_audiences(&self, root_path: &str) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let ws = Workspace::new(fs);
            let mut audiences: HashSet<String> = HashSet::new();

            fn collect_audiences<FS: AsyncFileSystem>(
                ws: &Workspace<FS>,
                path: &Path,
                audiences: &mut HashSet<String>,
                visited: &mut HashSet<PathBuf>,
            ) {
                if visited.contains(path) {
                    return;
                }
                visited.insert(path.to_path_buf());

                if let Ok(index) = block_on(ws.parse_index(path)) {
                    if let Some(file_audiences) = &index.frontmatter.audience {
                        for a in file_audiences {
                            if a.to_lowercase() != "private" {
                                audiences.insert(a.clone());
                            }
                        }
                    }

                    if index.frontmatter.is_index() {
                        for child_rel in index.frontmatter.contents_list() {
                            let child_path = index.resolve_path(child_rel);
                            if block_on(ws.fs_ref().exists(&child_path)) {
                                collect_audiences(ws, &child_path, audiences, visited);
                            }
                        }
                    }
                }
            }

            let mut visited = HashSet::new();
            collect_audiences(&ws, Path::new(root_path), &mut audiences, &mut visited);

            let mut result: Vec<String> = audiences.into_iter().collect();
            result.sort();

            serde_wasm_bindgen::to_value(&result).js_err()
        })
    }

    /// Plan an export operation.
    #[wasm_bindgen]
    pub fn plan(&self, root_path: &str, audience: &str) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            if audience == "*" {
                let ws = Workspace::new(fs);
                let mut included = Vec::new();
                let root = Path::new(root_path);
                let root_dir = root.parent().unwrap_or(root);

                fn collect_all<FS: AsyncFileSystem>(
                    ws: &Workspace<FS>,
                    path: &Path,
                    root_dir: &Path,
                    included: &mut Vec<IncludedFileJs>,
                    visited: &mut HashSet<PathBuf>,
                ) {
                    if visited.contains(path) {
                        return;
                    }
                    visited.insert(path.to_path_buf());

                    if let Ok(index) = block_on(ws.parse_index(path)) {
                        let relative_path = pathdiff::diff_paths(path, root_dir)
                            .unwrap_or_else(|| path.to_path_buf());

                        included.push(IncludedFileJs {
                            path: path.to_string_lossy().to_string(),
                            relative_path: relative_path.to_string_lossy().to_string(),
                        });

                        if index.frontmatter.is_index() {
                            for child_rel in index.frontmatter.contents_list() {
                                let child_path = index.resolve_path(child_rel);
                                if block_on(ws.fs_ref().exists(&child_path)) {
                                    collect_all(ws, &child_path, root_dir, included, visited);
                                }
                            }
                        }
                    }
                }

                let mut visited = HashSet::new();
                collect_all(&ws, root, root_dir, &mut included, &mut visited);

                let result = ExportPlanJs {
                    included,
                    excluded: vec![],
                    audience: "*".to_string(),
                };

                return serde_wasm_bindgen::to_value(&result).js_err();
            }

            let exporter = Exporter::new(fs);

            let plan = block_on(exporter
                .plan_export(Path::new(root_path), audience, Path::new("/export")))
                .js_err()?;

            let result = ExportPlanJs {
                included: plan
                    .included
                    .iter()
                    .map(|f| IncludedFileJs {
                        path: f.source_path.to_string_lossy().to_string(),
                        relative_path: f.relative_path.to_string_lossy().to_string(),
                    })
                    .collect(),
                excluded: plan
                    .excluded
                    .iter()
                    .map(|f| ExcludedFileJs {
                        path: f.path.to_string_lossy().to_string(),
                        reason: f.reason.to_string(),
                    })
                    .collect(),
                audience: plan.audience,
            };

            serde_wasm_bindgen::to_value(&result).js_err()
        })
    }

    /// Export files to memory as markdown.
    #[wasm_bindgen]
    pub fn to_memory(&self, root_path: &str, audience: &str) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            if audience == "*" {
                let ws = Workspace::new(fs);
                let mut files: Vec<ExportedFile> = Vec::new();
                let root = Path::new(root_path);
                let root_dir = root.parent().unwrap_or(root);

                fn collect_all<FS: AsyncFileSystem>(
                    ws: &Workspace<FS>,
                    path: &Path,
                    root_dir: &Path,
                    files: &mut Vec<ExportedFile>,
                    visited: &mut HashSet<PathBuf>,
                ) {
                    if visited.contains(path) {
                        return;
                    }
                    visited.insert(path.to_path_buf());

                    if let Ok(index) = block_on(ws.parse_index(path)) {
                        let relative_path = pathdiff::diff_paths(path, root_dir)
                            .unwrap_or_else(|| path.to_path_buf());

                        if let Ok(content) = block_on(ws.fs_ref().read_to_string(path)) {
                            let processed = remove_audience_from_content(&content);
                            files.push(ExportedFile {
                                path: relative_path.to_string_lossy().to_string(),
                                content: processed,
                            });
                        }

                        if index.frontmatter.is_index() {
                            for child_rel in index.frontmatter.contents_list() {
                                let child_path = index.resolve_path(child_rel);
                                if block_on(ws.fs_ref().exists(&child_path)) {
                                    collect_all(ws, &child_path, root_dir, files, visited);
                                }
                            }
                        }
                    }
                }

                let mut visited = HashSet::new();
                collect_all(&ws, root, root_dir, &mut files, &mut visited);

                return serde_wasm_bindgen::to_value(&files).js_err();
            }

            let exporter = Exporter::new(fs.clone());

            let plan = block_on(exporter
                .plan_export(Path::new(root_path), audience, Path::new("/export")))
                .js_err()?;

            let mut files: Vec<ExportedFile> = Vec::new();

            for export_file in &plan.included {
                let content = block_on(fs.clone().read_to_string(&export_file.source_path)).js_err()?;

                let processed = if !export_file.filtered_contents.is_empty() {
                    filter_contents_and_audience(&content, &export_file.filtered_contents)
                } else {
                    remove_audience_from_content(&content)
                };

                files.push(ExportedFile {
                    path: export_file.relative_path.to_string_lossy().to_string(),
                    content: processed,
                });
            }

            serde_wasm_bindgen::to_value(&files).js_err()
        })
    }

    /// Export files to memory as HTML.
    #[wasm_bindgen]
    pub fn to_html(&self, root_path: &str, audience: &str) -> Result<JsValue, JsValue> {
        use comrak::{Options, markdown_to_html};

        with_async_fs(|fs| {
            fn convert_md_to_html(markdown: &str) -> String {
                let mut options = Options::default();
                options.extension.strikethrough = true;
                options.extension.table = true;
                options.extension.autolink = true;
                options.extension.tasklist = true;
                options.render.r#unsafe = true;

                let html_body = markdown_to_html(markdown, &options);

                format!(
                    r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; max-width: 800px; margin: 0 auto; padding: 2rem; line-height: 1.6; }}
        pre {{ background: #f4f4f4; padding: 1rem; overflow-x: auto; }}
        code {{ background: #f4f4f4; padding: 0.2rem 0.4rem; }}
        img {{ max-width: 100%; }}
        table {{ border-collapse: collapse; width: 100%; }}
        th, td {{ border: 1px solid #ddd; padding: 0.5rem; text-align: left; }}
    </style>
</head>
<body>
{}
</body>
</html>"#,
                    html_body
                )
            }

            if audience == "*" {
                let ws = Workspace::new(fs);
                let mut files: Vec<ExportedFile> = Vec::new();
                let root = Path::new(root_path);
                let root_dir = root.parent().unwrap_or(root);

                fn collect_all<FS: AsyncFileSystem>(
                    ws: &Workspace<FS>,
                    path: &Path,
                    root_dir: &Path,
                    files: &mut Vec<ExportedFile>,
                    visited: &mut HashSet<PathBuf>,
                    convert_fn: &dyn Fn(&str) -> String,
                ) {
                    if visited.contains(path) {
                        return;
                    }
                    visited.insert(path.to_path_buf());

                    if let Ok(index) = block_on(ws.parse_index(path)) {
                        let relative_path = pathdiff::diff_paths(path, root_dir)
                            .unwrap_or_else(|| path.to_path_buf());

                        if let Ok(content) = block_on(ws.fs_ref().read_to_string(path)) {
                            let body = extract_body(&content);
                            let html = convert_fn(&body);
                            let html_path = relative_path.to_string_lossy().replace(".md", ".html");

                            files.push(ExportedFile {
                                path: html_path,
                                content: html,
                            });
                        }

                        if index.frontmatter.is_index() {
                            for child_rel in index.frontmatter.contents_list() {
                                let child_path = index.resolve_path(child_rel);
                                if block_on(ws.fs_ref().exists(&child_path)) {
                                    collect_all(
                                        ws,
                                        &child_path,
                                        root_dir,
                                        files,
                                        visited,
                                        convert_fn,
                                    );
                                }
                            }
                        }
                    }
                }

                let mut visited = HashSet::new();
                collect_all(
                    &ws,
                    root,
                    root_dir,
                    &mut files,
                    &mut visited,
                    &convert_md_to_html,
                );

                return serde_wasm_bindgen::to_value(&files).js_err();
            }

            let exporter = Exporter::new(fs.clone());

            let plan = block_on(exporter
                .plan_export(Path::new(root_path), audience, Path::new("/export")))
                .js_err()?;

            let mut files: Vec<ExportedFile> = Vec::new();

            for export_file in &plan.included {
                let content = block_on(fs.clone().read_to_string(&export_file.source_path)).js_err()?;
                let body = extract_body(&content);
                let html = convert_md_to_html(&body);
                let html_path = export_file
                    .relative_path
                    .to_string_lossy()
                    .replace(".md", ".html");

                files.push(ExportedFile {
                    path: html_path,
                    content: html,
                });
            }

            serde_wasm_bindgen::to_value(&files).js_err()
        })
    }

    /// Export binary attachment files.
    #[wasm_bindgen]
    pub fn binary_attachments(&self, root_path: &str, _audience: &str) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let ws = Workspace::new(fs);
            let root = Path::new(root_path);
            let root_dir = root.parent().unwrap_or(root);
            let mut binary_files: Vec<BinaryExportFile> = Vec::new();
            let mut visited_entries: HashSet<PathBuf> = HashSet::new();
            let mut visited_attachment_dirs: HashSet<PathBuf> = HashSet::new();

            fn collect_attachments<FS: AsyncFileSystem>(
                ws: &Workspace<FS>,
                entry_path: &Path,
                root_dir: &Path,
                binary_files: &mut Vec<BinaryExportFile>,
                visited_entries: &mut HashSet<PathBuf>,
                visited_attachment_dirs: &mut HashSet<PathBuf>,
            ) {
                if visited_entries.contains(entry_path) {
                    return;
                }
                visited_entries.insert(entry_path.to_path_buf());

                if let Ok(index) = block_on(ws.parse_index(entry_path)) {
                    let entry_dir = entry_path.parent().unwrap_or(Path::new("."));
                    let attachments_dir = entry_dir.join("_attachments");

                    if block_on(ws.fs_ref().is_dir(&attachments_dir))
                        && !visited_attachment_dirs.contains(&attachments_dir)
                    {
                        visited_attachment_dirs.insert(attachments_dir.clone());

                        if let Ok(files) = block_on(ws.fs_ref().list_files(&attachments_dir)) {
                            for file_path in files {
                                if !block_on(ws.fs_ref().is_dir(&file_path))
                                    && let Ok(data) = block_on(ws.fs_ref().read_binary(&file_path))
                                {
                                    let relative_path = pathdiff::diff_paths(&file_path, root_dir)
                                        .unwrap_or_else(|| file_path.clone());

                                    binary_files.push(BinaryExportFile {
                                        path: relative_path.to_string_lossy().to_string(),
                                        data,
                                    });
                                }
                            }
                        }
                    }

                    if index.frontmatter.is_index() {
                        for child_rel in index.frontmatter.contents_list() {
                            let child_path = index.resolve_path(child_rel);
                            if block_on(ws.fs_ref().exists(&child_path)) {
                                collect_attachments(
                                    ws,
                                    &child_path,
                                    root_dir,
                                    binary_files,
                                    visited_entries,
                                    visited_attachment_dirs,
                                );
                            }
                        }
                    }
                }
            }

            collect_attachments(
                &ws,
                root,
                root_dir,
                &mut binary_files,
                &mut visited_entries,
                &mut visited_attachment_dirs,
            );

            serde_wasm_bindgen::to_value(&binary_files).js_err()
        })
    }
}

impl Default for DiaryxExport {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Remove audience property from content.
pub fn remove_audience_from_content(content: &str) -> String {
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        return content.to_string();
    }

    let rest = &content[4..];
    let Some(end_idx) = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n")) else {
        return content.to_string();
    };

    let frontmatter_str = &rest[..end_idx];
    let body = &rest[end_idx + 5..];

    if let Ok(mut frontmatter) = serde_yaml::from_str::<serde_yaml::Value>(frontmatter_str)
        && let Some(map) = frontmatter.as_mapping_mut()
        && map
            .remove(serde_yaml::Value::String("audience".to_string()))
            .is_some()
        && let Ok(new_fm) = serde_yaml::to_string(&frontmatter)
    {
        return format!("---\n{}---\n{}", new_fm, body);
    }

    content.to_string()
}

/// Filter contents array and remove audience.
pub fn filter_contents_and_audience(content: &str, filtered: &[String]) -> String {
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        return content.to_string();
    }

    let rest = &content[4..];
    let Some(end_idx) = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n")) else {
        return content.to_string();
    };

    let frontmatter_str = &rest[..end_idx];
    let body = &rest[end_idx + 5..];

    if let Ok(mut frontmatter) = serde_yaml::from_str::<serde_yaml::Value>(frontmatter_str) {
        if let Some(map) = frontmatter.as_mapping_mut() {
            map.remove(serde_yaml::Value::String("audience".to_string()));

            if let Some(contents) = map.get_mut(serde_yaml::Value::String("contents".to_string()))
                && let Some(arr) = contents.as_sequence_mut()
            {
                arr.retain(|item| {
                    if let Some(s) = item.as_str() {
                        !filtered.iter().any(|f| f == s)
                    } else {
                        true
                    }
                });
            }
        }

        if let Ok(new_fm) = serde_yaml::to_string(&frontmatter) {
            return format!("---\n{}---\n{}", new_fm, body);
        }
    }

    content.to_string()
}

```


