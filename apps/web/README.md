---
title: web
description: Svelte + Tiptap frontend for Diaryx
author: adammharris
audience:
- public
- developers
part_of: '[README](/apps/README.md)'
contents:
- '[Tiptap Custom Extensions](/apps/web/docs/tiptap-custom-extensions.md)'
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

## Testing

The web app includes comprehensive unit tests (Vitest) and E2E tests (Playwright).

### Running Tests

```bash
# Run unit tests
bun run test

# Run tests with coverage
bun run test:coverage

# Run tests with UI
bun run test:ui

# Run E2E tests
bun run test:e2e

# Run E2E tests with UI
bun run test:e2e:ui
```

### Test Structure

```
src/
├── test/
│   └── setup.ts                    # Test setup and mocks
├── models/
│   ├── services/
│   │   ├── attachmentService.test.ts
│   │   ├── shareService.test.ts
│   │   ├── toastService.test.ts
│   │   └── workspaceCrdtService.test.ts
│   └── stores/
│       ├── workspaceStore.test.ts
│       ├── entryStore.test.ts
│       ├── collaborationStore.test.ts
│       └── uiStore.test.ts
├── lib/
│   ├── backend/
│   │   └── api.test.ts
│   ├── crdt/
│   │   ├── workspaceCrdtBridge.test.ts
│   │   └── collaborationBridge.test.ts
│   └── components/
│       ├── AttachmentPicker.test.ts
│       └── Editor.test.ts
e2e/
├── workspace.spec.ts               # Workspace navigation tests
├── editor.spec.ts                  # Editor functionality tests
├── attachments.spec.ts             # Attachment handling tests
└── share.spec.ts                   # Share session tests
```

### Configuration

- `vitest.config.ts` - Vitest configuration with Svelte support and jsdom environment
- `playwright.config.ts` - Playwright configuration for E2E testing

## Building WASM

The WASM module is built from `crates/diaryx_wasm`:

```bash
cd ../../crates/diaryx_wasm
wasm-pack build --target web --out-dir ../../apps/web/src/lib/wasm
```

## Developer Guides

| Guide | Description |
| ----- | ----------- |
| [TipTap Custom Extensions](docs/tiptap-custom-extensions.md) | Creating custom TipTap extensions with markdown support |

## Live Demo

Try the web frontend at: https://diaryx-org.github.io/diaryx/
