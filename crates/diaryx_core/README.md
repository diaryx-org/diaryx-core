---
title: Diaryx Core Library

author: adammharris

audience:
  - public

part_of: ../../README.md
---

# Diaryx Core Library

This is the `diaryx_core` library! It contains shared code for the Diaryx clients.

NOTE: this README is not finished yet!

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
    │   ├── memory.rs (In-memory filesystem, used by WASM/web client)
    │   ├── mod.rs
    │   └── native.rs (Actual filesystem [std::fs] used by Tauri/CLI)
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

Note: writing after this point may be outdated.

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

```rust
use diaryx_core::export::{ExportOptions, ExportPlan, Exporter};
use diaryx_core::fs::RealFileSystem;
use std::path::Path;

let workspace_root = Path::new("./workspace");
let audience = "public";
let destination = Path::new("./export");
let fs = RealFileSystem;
let exporter = Exporter::new(fs);
let plan = match exporter.plan_export(&workspace_root, audience, destination) {
  Ok(plan) => plan,
  Err(e) => {
    eprintln!("✗ Failed to plan export: {}", e);
    return;
  }
};

let force = false;
let keep_audience = false;
let options = ExportOptions {
    force,
    keep_audience,
};

match exporter.execute_export(&plan, &options) {
  Ok(stats) => {
    println!("✓ {}", stats);
    println!("  Exported to: {}", destination.display());
  }
  Err(e) => {
    eprintln!("✗ Export failed: {}", e);
    if !force && destination.exists() {
      eprintln!("  (use --force to overwrite existing destination)");
    }
  }
}
```

## Validation

The `validate` module provides functionality to check workspace link integrity and automatically fix issues.

### Validator

The `Validator` struct checks `part_of` and `contents` references within a workspace:

```rust,no_run
use diaryx_core::validate::Validator;
use diaryx_core::fs::RealFileSystem;
use std::path::Path;

let validator = Validator::new(RealFileSystem);

// Validate entire workspace starting from root index
let root_path = Path::new("./workspace/README.md");
let result = validator.validate_workspace(&root_path).unwrap();

// Or validate a single file
let file_path = Path::new("./workspace/notes/my-note.md");
let result = validator.validate_file(&file_path).unwrap();

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

```rust,no_run
use diaryx_core::validate::{Validator, ValidationFixer};
use diaryx_core::fs::RealFileSystem;
use std::path::Path;

let validator = Validator::new(RealFileSystem);
let fixer = ValidationFixer::new(RealFileSystem);

// Validate workspace
let root_path = Path::new("./workspace/README.md");
let result = validator.validate_workspace(&root_path).unwrap();

// Fix all issues at once
let (error_fixes, warning_fixes) = fixer.fix_all(&result);

for fix in error_fixes.iter().chain(warning_fixes.iter()) {
    if fix.success {
        println!("✓ {}", fix.message);
    } else {
        println!("✗ {}", fix.message);
    }
}

// Or fix individual issues
fixer.fix_broken_part_of(Path::new("./file.md"));
fixer.fix_broken_contents_ref(Path::new("./index.md"), "missing.md");
fixer.fix_unlisted_file(Path::new("./index.md"), Path::new("./new-file.md"));
fixer.fix_missing_part_of(Path::new("./orphan.md"), Path::new("./index.md"));
```

## Publish

## Templates

## Workspaces

## Date parsing

## Shared errors

## Configuration

## Filesystem abstraction
