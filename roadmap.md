---
title: Roadmap
description: The plan for future Diaryx features
author: adammharris
created: 2025-12-05T12:06:55-07:00
updated: 2025-12-25T16:43:14-07:00
audience:
  - public
part_of: README.md
---

# Roadmap

## v0.6.0

- Workspace import
  - Import from Obsidian (add all part_of/contents properties + index files)
  - Better validation
- Sync/backup backends
  - Google Drive, file/folders, and so forth
  - Configurable intervals/sync behavior
  - Possibly live edit/CRDT?
- Links between files
  - Click to seamlessly navigate to other files

## Future considerations

## Better documentation

We have just one README file right now.

### Sync/Backup

Probably add a trait similar to Filesystem trait.

A similar system already exists in apps/web for the WebAssembly/IndexedDB backend. It uses an InMemoryFilesystem and regularly "persists" to IndexedDB. Similarly, the user could set up multiple "backends" and have Diaryx persist to certain ones at certain intervals.

Questions:
- Should we have separate traits for syncing and backup?

### Undo/redo

I would like `diaryx undo` and `diaryx redo` commands to undo/redo any command that was previously done, because it is easy to make mistakes.
