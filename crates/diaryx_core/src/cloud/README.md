---
title: Cloud Sync
description: Bidirectional file synchronization with cloud storage
part_of: '[README](/crates/diaryx_core/README.md)'
audience:
- developers
---

# Cloud Sync

This module provides bidirectional file synchronization with cloud storage
providers (S3, Google Drive, etc.) while integrating with the CRDT system
for conflict resolution.

## Architecture

```text
Cloud Storage (S3/GDrive)
        ^
        |
        v
+---------------+
| SyncEngine    |  Orchestrates sync operations
+-------+-------+
        |
        v
+---------------+
| SyncManifest  |  Tracks file state (hashes, timestamps)
+-------+-------+
        |
        v
+---------------+
| AsyncFileSystem + CRDT
+---------------+
```

## Key Components

### SyncManifest

Tracks the synchronization state for each file:

```rust,ignore
use diaryx_core::cloud::{SyncManifest, FileSyncState};
use std::path::Path;

let mut manifest = SyncManifest::new();

// Record a file's sync state
manifest.set_state("notes/my-note.md", FileSyncState {
    local_hash: Some("abc123".to_string()),
    remote_hash: Some("abc123".to_string()),
    local_modified: Some(timestamp),
    remote_modified: Some(timestamp),
    etag: Some("etag-value".to_string()),
});

// Check if a file needs syncing
let state = manifest.get_state("notes/my-note.md");
```

### Change Detection

The module detects local and remote changes:

```rust,ignore
use diaryx_core::cloud::{LocalChange, RemoteChange, SyncDirection};

// Local changes detected by comparing filesystem to manifest
let local_changes: Vec<LocalChange> = detect_local_changes(&fs, &manifest).await?;

// Remote changes detected by comparing provider listing to manifest
let remote_changes: Vec<RemoteChange> = detect_remote_changes(&provider, &manifest).await?;
```

Change types include:
- **Created**: New file that doesn't exist on the other side
- **Modified**: File content changed since last sync
- **Deleted**: File removed since last sync

### Conflict Resolution

When both local and remote have changed, conflicts are detected:

```rust,ignore
use diaryx_core::cloud::{ConflictInfo, ConflictResolution};

// Conflict detected when both sides changed
let conflict = ConflictInfo {
    path: "notes/my-note.md".to_string(),
    local_modified: local_timestamp,
    remote_modified: remote_timestamp,
    local_hash: "abc123".to_string(),
    remote_hash: "def456".to_string(),
};

// Resolution strategies
let resolution = ConflictResolution::KeepLocal;  // Overwrite remote
let resolution = ConflictResolution::KeepRemote; // Overwrite local
let resolution = ConflictResolution::KeepBoth;   // Create duplicate
```

### CloudSyncProvider Trait

Implement this trait for custom storage backends:

```rust,ignore
use diaryx_core::cloud::{CloudSyncProvider, RemoteFileInfo};
use async_trait::async_trait;

#[async_trait]
impl CloudSyncProvider for MyStorageBackend {
    async fn list_files(&self, prefix: &str) -> Result<Vec<RemoteFileInfo>>;
    async fn download(&self, path: &str) -> Result<Vec<u8>>;
    async fn upload(&self, path: &str, content: &[u8]) -> Result<()>;
    async fn delete(&self, path: &str) -> Result<()>;
    async fn get_file_info(&self, path: &str) -> Result<Option<RemoteFileInfo>>;
}
```

### SyncEngine

Orchestrates the entire sync process:

```rust,ignore
use diaryx_core::cloud::{SyncEngine, CloudSyncResult, SyncProgress};

let engine = SyncEngine::new(fs, provider, manifest);

// Perform sync with progress callback
let result: CloudSyncResult = engine.sync(|progress: SyncProgress| {
    println!("{}: {}/{}", progress.stage.description(),
             progress.current, progress.total);
}).await?;

// Check results
if result.success {
    println!("Uploaded: {}, Downloaded: {}, Deleted: {}",
             result.files_uploaded,
             result.files_downloaded,
             result.files_deleted);
} else if !result.conflicts.is_empty() {
    // Handle conflicts
    for conflict in result.conflicts {
        println!("Conflict: {}", conflict.path);
    }
}
```

## Sync Workflow

1. **Detect local changes**: Compare filesystem to manifest
2. **Detect remote changes**: Compare provider listing to manifest
3. **Identify conflicts**: Files changed on both sides
4. **Upload**: Send local-only changes to remote
5. **Download**: Fetch remote-only changes to local
6. **Delete**: Remove files deleted from authoritative side
7. **Update manifest**: Record new sync state

## Progress Tracking

The `SyncProgress` struct provides detailed progress information:

```rust,ignore
use diaryx_core::cloud::{SyncProgress, SyncStage};

fn handle_progress(progress: SyncProgress) {
    match progress.stage {
        SyncStage::DetectingLocal => println!("Scanning local files..."),
        SyncStage::DetectingRemote => println!("Fetching remote list..."),
        SyncStage::Uploading => {
            println!("Uploading {}/{}: {}",
                     progress.current, progress.total,
                     progress.message.unwrap_or_default());
        }
        SyncStage::Downloading => println!("Downloading..."),
        SyncStage::Complete => println!("Sync complete!"),
        SyncStage::Error => println!("Sync failed!"),
        _ => {}
    }
}
```

## Relationship to CRDT Sync

This module handles **file-level cloud sync** (whole files to/from storage),
while the [`crdt`](../crdt/README.md) module handles **real-time collaboration**
(character-by-character edits via WebSocket).

| Aspect | Cloud Sync (this module) | CRDT Sync |
|--------|--------------------------|-----------|
| Granularity | Whole files | Characters/operations |
| Protocol | HTTP (S3/REST) | WebSocket (Y-sync) |
| Latency | Minutes | Milliseconds |
| Offline | Batch sync | Automatic merge |
| Use case | Backup, cross-device | Live collaboration |

Both systems share the `WorkspaceCrdt` metadata for consistency.
