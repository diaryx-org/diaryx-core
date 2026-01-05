---
title: Diaryx Tauri
author: adammharris
audience:
  - public
  - developers
part_of: ../README.md
---

# Diaryx Tauri

The Tauri backend for Diaryx, providing native filesystem access for the web frontend.

## Architecture

The Tauri app shares the same Svelte frontend as the web app (`apps/web`), but instead of using WebAssembly with an in-memory filesystem, it uses Tauri IPC to communicate with a Rust backend that accesses the real filesystem.

```
┌─────────────────────────────────────────┐
│           Svelte Frontend               │
│         (apps/web/src/lib)              │
└─────────────────┬───────────────────────┘
                  │
        ┌─────────┴─────────┐
        │                   │
        ▼                   ▼
┌───────────────┐   ┌───────────────┐
│  WasmBackend  │   │ TauriBackend  │
│ (IndexedDB +  │   │ (Tauri IPC)   │
│  InMemoryFS)  │   │               │
└───────────────┘   └───────┬───────┘
                            │
                            ▼
                    ┌───────────────┐
                    │ commands.rs   │
                    │ (diaryx_core) │
                    └───────┬───────┘
                            │
                            ▼
                    ┌───────────────┐
                    │ RealFileSystem│
                    └───────────────┘
```

## Building

```bash
# Development
cd apps/tauri
bun install
bun run tauri dev

# Production build
bun run tauri build
```

## Tauri Commands

All IPC commands are defined in `src-tauri/src/commands.rs` and registered in `src-tauri/src/lib.rs`.

### Validation Commands

The validation system checks workspace link integrity and can automatically fix issues.

| Command                     | Description                            |
| --------------------------- | -------------------------------------- |
| `validate_workspace`        | Validate entire workspace from root    |
| `validate_file`             | Validate a single file's links         |
| `fix_broken_part_of`        | Remove broken `part_of` reference      |
| `fix_broken_contents_ref`   | Remove broken `contents` reference     |
| `fix_broken_attachment`     | Remove broken `attachments` reference  |
| `fix_non_portable_path`     | Normalize non-portable paths           |
| `fix_unlisted_file`         | Add file to index's contents           |
| `fix_orphan_binary_file`    | Add binary file to index's attachments |
| `fix_missing_part_of`       | Set missing `part_of` property         |
| `fix_all_validation_issues` | Fix all errors and fixable warnings    |

### Other Commands

| Category       | Commands                                                                          |
| -------------- | --------------------------------------------------------------------------------- |
| Workspace      | `get_workspace_tree`, `get_filesystem_tree`, `create_workspace`                   |
| Entries        | `get_entry`, `save_entry`, `create_entry`, `delete_entry`, `move_entry`           |
| Entries (cont) | `attach_entry_to_parent`, `convert_to_index`, `convert_to_leaf`                   |
| Entries (cont) | `create_child_entry`, `rename_entry`, `ensure_daily_entry`                        |
| Frontmatter    | `get_frontmatter`, `set_frontmatter_property`, `remove_frontmatter_property`      |
| Attachments    | `get_attachments`, `upload_attachment`, `delete_attachment`                       |
| Search         | `search_workspace`                                                                |
| Export         | `get_available_audiences`, `plan_export`, `export_to_memory`, `export_to_html`    |
| Backup         | `backup_workspace`, `restore_workspace`, `backup_to_s3`, `backup_to_google_drive` |
| Import         | `import_from_zip`, `pick_and_import_zip`                                          |

## Platform Support

- macOS (Intel and Apple Silicon)
- Windows
- Linux
- iOS (via Tauri mobile)
- Android (via Tauri mobile)

Mobile platforms use platform-appropriate paths within app sandboxes.
