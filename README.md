---
title: Diaryx
description: Repository for the Diaryx project
author: adammharris
contents:
  - LICENSE.md
  - roadmap.md
  - crates/diaryx_cli/README.md
  - crates/diaryx_core/README.md
  - apps/README.md
version: v0.4.3
---

# Diaryx

A monorepo for the Diaryx project, consisting of:

1. `crates/diaryx_core`: Core logic for all Diaryx apps
2. `crates/diaryx_cli`: An opinionated command line tool for keeping a journal. Emphasizes Markdown frontmatter, daily journal keeping, ease of use, and hierarchal connections between files.
3. `crates/diaryx_wasm`: WebAssembly bindings for `diaryx_core`, used in the web app below.
4. `apps/web`: A web app using the same Rust core under the surface as a WebAssembly module.
5. `apps/tauri`: A Tauri app using the same web frontend, but calling the functions through the Tauri backend instead of through WebAssembly, allowing for native filesystem access.

## Installation

Click [here](crates/diaryx_cli/README.md) for information about the `diaryx` command-line tool.

Click [here](apps/tauri/README.md) for information about the Diaryx app (Tauri).

Also, you can try a live demo at <https://diaryx-org.github.io/diaryx-core/>.

You can also go into GitHub releases and use the app as you please, "except for providing any product that competes with the software or any product the licensor or any of its affiliates provides using the software." (See [the license](./LICENSE.md)).

## roadmap

See [the roadmap document here](roadmap.md).

## License

PolyForm Shield 1.0. Read it [here](LICENSE.md).

## Development

First, clone the repository.

```bash
git clone https://github.com/diaryx-org/diaryx-core.git
cd /path/to/diaryx-core
```

To install the CLI:

```bash
cargo install --path crates/diaryx_cli .
```

To build the Tauri app:

```bash
cd apps/tauri
bun tauri dev
```

To run the website:

```bash
wasm-pack build crates/diaryx_wasm --target web --out-dir apps/web/src/lib/wasm
cd apps/web
bun run dev
```
