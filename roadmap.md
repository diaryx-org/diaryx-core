---
title: Roadmap
description: The plan for future Diaryx features
author: adammharris
created: 2025-12-05T12:06:55-07:00
updated: 2025-12-28T13:38:14-05:00
audience:
  - public
part_of: README.md
---

# Roadmap

## v0.6.0

### Sync/Backup

A similar system already exists in apps/web for the WebAssembly/IndexedDB backend. It uses an InMemoryFilesystem and regularly "persists" to IndexedDB. Similarly, the user could set up multiple "backends" and have Diaryx persist to certain ones at certain intervals.

- Google Drive, file/folders, and so forth
- Configurable intervals/sync behavior
- Possibly live edit/CRDT?

```mermaid
sequenceDiagram
participant App as Journal App
participant Working as Working Memory<br/>(Real/InMemory FS)
participant Persist as Persistence Layer
participant Storage as Storage<br/>(IndexedDB/Cloud/Drive)

Note over App,Storage: User writes journal entry

App->>Working: write(entry)
Working-->>App: success

Note over Working,Storage: Variable frequency persistence

loop Configurable intervals
    Working->>Persist: persist()
    Persist->>Storage: save/sync/backup
    Storage-->>Persist: complete
    Persist-->>Working: complete
end

Note over App,Storage: User reads journal entry

App->>Working: read(entryId)
Working-->>App: entry content
```

Here is the class diagram for the current FileSystem trait and a potential PersistTarget:

```mermaid
classDiagram
class FileSystem {
<<trait>>
+read_to_string(path: Path) Result~String~
+write_file(path: Path, content: str) Result
+create_new(path: Path, content: str) Result
+delete_file(path: Path) Result
+list_md_files(dir: Path) Result~Vec~PathBuf~~
+exists(path: Path) bool
+create_dir_all(path: Path) Result
+is_dir(path: Path) bool
+move_file(from: Path, to: Path) Result
+read_binary(path: Path) Result~Vec~u8~~
+write_binary(path: Path, content: bytes) Result
+list_files(dir: Path) Result~Vec~PathBuf~~
+list_md_files_recursive(dir: Path) Result~Vec~PathBuf~~
+list_all_files_recursive(dir: Path) Result~Vec~PathBuf~~
}

class RealFileSystem {
    +read_to_string(path: Path) Result~String~
    +write_file(path: Path, content: str) Result
    +create_new(path: Path, content: str) Result
    +delete_file(path: Path) Result
    +list_md_files(dir: Path) Result~Vec~PathBuf~~
    +exists(path: Path) bool
    +create_dir_all(path: Path) Result
    +is_dir(path: Path) bool
    +move_file(from: Path, to: Path) Result
}

class InMemoryFileSystem {
    -files: HashMap~PathBuf, Vec~u8~~
    +read_to_string(path: Path) Result~String~
    +write_file(path: Path, content: str) Result
    +create_new(path: Path, content: str) Result
    +delete_file(path: Path) Result
    +list_md_files(dir: Path) Result~Vec~PathBuf~~
    +exists(path: Path) bool
    +create_dir_all(path: Path) Result
    +is_dir(path: Path) bool
    +move_file(from: Path, to: Path) Result
}

class PersistTarget {
    <<trait>>
    +sync_frequency() Duration
    +persist(filesystem: FileSystem) Result
    +restore(filesystem: FileSystem) Result
    +is_available() bool
    +get_last_sync() Option~Timestamp~
}

class IndexedDBTarget {
    -db_name: String
    -frequency: Duration
    +sync_frequency() Duration
    +persist(filesystem: FileSystem) Result
    +restore(filesystem: FileSystem) Result
    +is_available() bool
    +get_last_sync() Option~Timestamp~
}

class RemoteTarget {
    -server_url: String
    -auth_token: String
    -frequency: Duration
    +sync_frequency() Duration
    +persist(filesystem: FileSystem) Result
    +restore(filesystem: FileSystem) Result
    +is_available() bool
    +get_last_sync() Option~Timestamp~
}

class CloudStorageTarget {
    -provider: CloudProvider
    -credentials: Credentials
    -frequency: Duration
    +sync_frequency() Duration
    +persist(filesystem: FileSystem) Result
    +restore(filesystem: FileSystem) Result
    +is_available() bool
    +get_last_sync() Option~Timestamp~
}

class LocalDriveTarget {
    -backup_path: PathBuf
    -frequency: Duration
    +sync_frequency() Duration
    +persist(filesystem: FileSystem) Result
    +restore(filesystem: FileSystem) Result
    +is_available() bool
    +get_last_sync() Option~Timestamp~
}

class PersistenceManager {
    -filesystem: Box~FileSystem~
    -targets: Vec~Box~PersistTarget~~
    +new(filesystem: FileSystem) PersistenceManager
    +add_target(target: PersistTarget)
    +sync_all() Result
    +sync_due_targets() Result
    +restore_from(target: PersistTarget) Result
}

FileSystem <|.. RealFileSystem : implements
FileSystem <|.. InMemoryFileSystem : implements
PersistTarget <|.. IndexedDBTarget : implements
PersistTarget <|.. RemoteTarget : implements
PersistTarget <|.. CloudStorageTarget : implements
PersistTarget <|.. LocalDriveTarget : implements
PersistenceManager o-- FileSystem : uses
PersistenceManager o-- PersistTarget : manages
```
 
### Workspace import

Import from Obsidian (add all part_of/contents properties + index files)

Better validation
 
### Links between files

Click to seamlessly navigate to other files
Currently tries to open path in domain (treats like literal link)

## Future considerations

### Better documentation

We have just one README file right now.

### Undo/redo

I would like `diaryx undo` and `diaryx redo` commands to undo/redo any command that was previously done, because it is easy to make mistakes.

### Encryption

Ideally hot-swappable similar to backup backends. Maybe Cryptomator?

### Dark mode

Save eyeballs!

### Math/diagrams

TipTap has an extension for LaTeX, but I would like to support Mermaid diagrams and Typst syntax as well. Maybe there is a way to swap parsers and return an image?