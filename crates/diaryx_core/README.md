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
