---
title: Diaryx Core Library

author: adammharris

audience:
  - public

part_of: ../../README.md
---

# Diaryx Core Library

This is the `diaryx_core` library! It contains shared code for the Diaryx clients.

There are three Diaryx clients right now:

1. Command-line (`diaryx`)
2. Web (via `diaryx_wasm`)
3. Tauri (via Tauri backend)

Diaryx is an opinionated journaling method that makes careful use of frontmatter
so that journal entries are queryable and useable well into the future.

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

## Publish

## Templates

## Workspaces

## Date parsing

## Shared errors

## Configuration

## Filesystem abstraction
