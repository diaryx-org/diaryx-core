---
title: CRDT Synchronization
description: Conflict-free replicated data types for real-time collaboration
part_of: '[README](/crates/diaryx_core/README.md)'
audience:
- developers
---

# CRDT Synchronization

This module provides conflict-free replicated data types (CRDTs) for real-time
collaboration, built on [yrs](https://docs.rs/yrs) (the Rust port of Yjs).

## Feature Flags

This module requires the `crdt` feature:

```toml
[dependencies]
diaryx_core = { version = "...", features = ["crdt"] }

# For SQLite-based persistent storage (native only)
diaryx_core = { version = "...", features = ["crdt", "crdt-sqlite"] }
```

## Architecture

The CRDT system has several layers, from low-level to high-level:

```text
                    +-----------------+
                    |  SyncProtocol   |  Y-sync for Hocuspocus server
                    +--------+--------+
                             |
          +------------------+------------------+
          |                                     |
+---------v----------+             +-----------v---------+
|   WorkspaceCrdt    |             |    BodyDocManager   |
| (file hierarchy)   |             | (document content)  |
+---------+----------+             +-----------+---------+
          |                                     |
          |              +-------------+        |
          +------------->| CrdtStorage |<-------+
                         +------+------+
                                |
               +----------------+----------------+
               |                                 |
      +--------v--------+              +---------v--------+
      |  MemoryStorage  |              |  SqliteStorage   |
      +-----------------+              +------------------+
```

1. **Types** (`types.rs`): Core data structures like `FileMetadata` and `BinaryRef`
2. **Storage** (`storage.rs`): `CrdtStorage` trait for persisting CRDT state
3. **WorkspaceCrdt** (`workspace_doc.rs`): Y.Doc for workspace file hierarchy
4. **BodyDoc** (`body_doc.rs`): Per-file Y.Doc for document content
5. **BodyDocManager** (`body_doc_manager.rs`): Manages multiple BodyDocs
6. **SyncProtocol** (`sync.rs`): Y-sync protocol for Hocuspocus server
7. **HistoryManager** (`history.rs`): Version history and time travel

## WorkspaceCrdt

Manages the workspace file hierarchy as a CRDT. Files are keyed by stable
document IDs (UUIDs), making renames and moves trivial property updates.

### Doc-ID Based Architecture

```rust,ignore
use diaryx_core::crdt::{WorkspaceCrdt, MemoryStorage, FileMetadata};
use std::sync::Arc;

let storage = Arc::new(MemoryStorage::new());
let workspace = WorkspaceCrdt::new(storage);

// Create a file with auto-generated UUID
let metadata = FileMetadata::with_filename(
    "my-note.md".to_string(),
    Some("My Note".to_string())
);
let doc_id = workspace.create_file(metadata).unwrap();

// Derive filesystem path from doc_id (walks parent chain)
let path = workspace.get_path(&doc_id); // Some("my-note.md")

// Find doc_id by path
let found_id = workspace.find_by_path(Path::new("my-note.md"));

// Renames and moves are trivial - doc_id is stable!
workspace.rename_file(&doc_id, "new-name.md").unwrap();
workspace.move_file(&doc_id, Some(&parent_doc_id)).unwrap();
```

### Legacy Path-Based API

For backward compatibility, path-based operations are still supported:

```rust,ignore
workspace.set_file("notes/my-note.md", metadata);
let meta = workspace.get_file("notes/my-note.md");
workspace.remove_file("notes/my-note.md");
```

### Migration

Workspaces using the legacy path-based format can be migrated:

```rust,ignore
if workspace.needs_migration() {
    let count = workspace.migrate_to_doc_ids().unwrap();
    println!("Migrated {} files", count);
}
```

## BodyDoc

Manages individual document content with collaborative editing support:

```rust,ignore
use diaryx_core::crdt::{BodyDoc, MemoryStorage};
use std::sync::Arc;

let storage = Arc::new(MemoryStorage::new());
let doc = BodyDoc::new("notes/my-note.md", storage);

// Body content operations
doc.set_body("# Hello World\n\nThis is my note.");
let content = doc.get_body();

// Collaborative editing
doc.insert_at(0, "Prefix: ");
doc.delete_range(0, 8);

// Frontmatter operations
doc.set_frontmatter("title", "My Note");
let title = doc.get_frontmatter("title");
doc.remove_frontmatter("audience");
```

## BodyDocManager

Manages multiple BodyDocs with lazy loading:

```rust,ignore
use diaryx_core::crdt::{BodyDocManager, MemoryStorage};
use std::sync::Arc;

let storage = Arc::new(MemoryStorage::new());
let manager = BodyDocManager::new(storage);

// Get or create a BodyDoc for a file
let doc = manager.get_or_create("notes/my-note.md");
doc.set_body("Content here");

// Check if a doc exists
if manager.has_doc("notes/my-note.md") {
    // ...
}

// Remove a doc from the manager
manager.remove_doc("notes/my-note.md");
```

## Sync Protocol

The sync module implements Y-sync protocol for real-time collaboration with
Hocuspocus or other Y.js-compatible servers:

```rust,ignore
use diaryx_core::crdt::{WorkspaceCrdt, MemoryStorage};
use std::sync::Arc;

let storage = Arc::new(MemoryStorage::new());
let workspace = WorkspaceCrdt::new("workspace", storage);

// Get sync state for initial handshake
let state_vector = workspace.get_sync_state();

// Apply remote update from server
let remote_update: Vec<u8> = /* from WebSocket */;
workspace.apply_update(&remote_update);

// Encode state for sending to server
let full_state = workspace.encode_state();

// Encode incremental update since a state vector
let diff = workspace.encode_state_as_update(&remote_state_vector);
```

## Version History

All local changes are automatically recorded, enabling version history and
time travel:

```rust,ignore
use diaryx_core::crdt::{WorkspaceCrdt, MemoryStorage, HistoryEntry};
use std::sync::Arc;

let storage = Arc::new(MemoryStorage::new());
let workspace = WorkspaceCrdt::new("workspace", storage.clone());

// Make some changes
workspace.set_file("file1.md", metadata1);
workspace.set_file("file2.md", metadata2);

// Get version history
let history: Vec<HistoryEntry> = storage.get_all_updates("workspace").unwrap();
for entry in &history {
    println!("Version {} at {:?}: {} bytes",
             entry.version, entry.timestamp, entry.update.len());
}

// Time travel to a specific version
workspace.restore_to_version(1);
```

## Storage Backends

### MemoryStorage

In-memory storage for WASM/web and testing:

```rust,ignore
use diaryx_core::crdt::MemoryStorage;
use std::sync::Arc;

let storage = Arc::new(MemoryStorage::new());
```

### SqliteStorage

Persistent storage using SQLite (requires `crdt-sqlite` feature, native only):

```rust,ignore
use diaryx_core::crdt::SqliteStorage;
use std::sync::Arc;

let storage = Arc::new(SqliteStorage::open("crdt.db").unwrap());
```

## Integration with Command API

CRDT operations are available through the unified command API for WASM/Tauri:

```rust,ignore
use diaryx_core::{Diaryx, Command, CommandResult};

let diaryx = Diaryx::with_crdt(fs, crdt_storage);

// Execute CRDT commands
let result = diaryx.execute(Command::GetSyncState {
    doc_type: "workspace".to_string(),
    doc_name: None,
});

let result = diaryx.execute(Command::SetFileMetadata {
    path: "notes/my-note.md".to_string(),
    metadata: file_metadata,
});

let result = diaryx.execute(Command::GetHistory {
    doc_type: "workspace".to_string(),
    doc_name: None,
});
```

## Relationship to Cloud Sync

The CRDT module handles **real-time collaboration** (character-by-character edits),
while the [`sync`](../sync/README.md) module handles **file-level cloud sync**
(S3, Google Drive). They work together:

- CRDT tracks fine-grained changes within documents
- Cloud sync uploads/downloads whole files to/from storage providers
- Both use the same `WorkspaceCrdt` metadata for consistency
