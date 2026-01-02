---
title: Diaryx
description: Repository for the Diaryx project
author: adammharris
version: v0.6.0
updated: 2026-01-02T13:06:25-05:00
contents:
  - crates/diaryx/README.md
  - crates/diaryx_core/README.md
  - crates/diaryx_wasm/README.md
  - apps/README.md
  - LICENSE.md
  - roadmap.md
  - AGENTS.md
audience:
  - public
  - developers
---

# Diaryx

Diaryx is software for personal writing, designed to embed standardized metadata into markdown files, eliminating the need to index a vault and introducing greater portability across applications.

This repo uses Diaryx for its own documentation. The "root index file" is [README.md](README.md). In the frontmatter, it has a `contents` property that includes a list of markdown files considered part of the documentation. Each of these files has a `part_of` property set to `README.md`, allowing for bidirectional traversal. Markdown files are considered "leaf" files if they do not have a `contents` property, or "index" files if they do. A root index file is a file that has a `contents` property but no `part_of` property. 

Another important feature of Diaryx enabled by this structure is "audience filtering." Files may have an `audience` property, which defines access groups for the file and its children. Thus, Diaryx may export different subsets of the documentation to different audiences.

All of this logic is defined in the `diaryx_core` Rust crate, and is used by the `diaryx` CLI, `diaryx_wasm`, and `apps/tauri`. Please refer to the links below for more information on these specific projects.

## Codebase organization

- [`crates/diaryx_core`](crates/diaryx_core/README.md): Core logic for all Diaryx apps.
- [`crates/diaryx`](crates/diaryx/README.md): CLI frontend for Diaryx.
- [`crates/diaryx_wasm`](crates/diaryx_wasm/README.md): WebAssembly bindings for `diaryx_core`, used in `apps/web`.
- [`apps/web`](apps/web/README.md): Svelte + TipTap frontend for Diaryx.
- [`apps/tauri`](apps/tauri/README.md): Tauri frontend for Diaryx. Uses `apps/web` as its frontend, but calls the functions through the Tauri backend instead of through WebAssembly, allowing for native filesystem access.

## Installation

You can try a live demo of the Diaryx web frontend at <https://diaryx-org.github.io/diaryx/>.

You can also go into GitHub releases and use the Diaryx app as you please, "except for providing any product that competes with the software or any product the licensor or any of its affiliates provides using the software." (See [the license](./LICENSE.md)).

## Roadmap

See the project-level roadmap [here](roadmap.md).

## License

PolyForm Shield 1.0. Read it [here](LICENSE.md).