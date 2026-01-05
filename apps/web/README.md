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
