---
title: CONTRIBUTING
description: A guide for making contributions in the Diaryx repo
part_of: '[README](/README.md)'
---

# Contributing to Diaryx

Welcome to the Diaryx project! This document will help you understand the codebase structure, identify areas for improvement, and find good first issues to work on.

Note that much of the documentation in this repo is NOT complete, though it is mostly up-to-date.

## Repository Structure

Diaryx is organized as a Rust workspace with multiple crates:

```
diaryx/
├── crates/
│   ├── diaryx_core/     # Core library - shared logic for all frontends
│   ├── diaryx/          # CLI application
│   └── diaryx_wasm/     # WebAssembly bindings for web frontend
├── apps/
│   ├── tauri/           # Desktop application (Tauri)
│   └── web/             # Web application
└── Cargo.toml           # Workspace configuration
```

### Crate Overview

#### `diaryx_core` - Core Library

The heart of the project. Contains all business logic that should be shared across frontends.

See [more information here](crates/diaryx_core/README.md).

#### `diaryx` - CLI Application

Command-line interface built on top of `diaryx_core`.

See [more information here](crates/diaryx/README.md).

#### `diaryx_wasm` - WebAssembly Bindings

WASM bindings that expose `diaryx_core` functionality to JavaScript. Uses an in-memory filesystem that syncs with IndexedDB.

See [more information here](crates/diaryx_wasm/README.md).

---

## Development Setup

```bash
# Clone the repository
git clone https://github.com/diaryx-org/diaryx-core.git
cd diaryx-core

# Build all crates
cargo build

# Run tests
cargo test

# Install the CLI locally
cargo install --path crates/diaryx

# Build WASM (requires wasm-pack)
wasm-pack build crates/diaryx_wasm --target web
```

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy` and address warnings
- Add tests for new functionality
- Document public APIs with rustdoc

## Pull Request Guidelines

1. **One issue per PR** - Keep changes focused
2. **Include tests** - Especially for bug fixes
3. **Update documentation** - If behavior changes
4. **Reference the issue** - Use "Fixes #123" in PR description

---

## Architecture Goals

The long-term vision for `diaryx_core`:

```
┌─────────────────────────────────────────────────────────────┐
│                     diaryx_core                             │
├─────────────────────────────────────────────────────────────┤
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │    Entry     │  │  Workspace   │  │   Search     │       │
│  │  Operations  │  │  Management  │  │   Engine     │       │
│  └──────────────┘  └──────────────┘  └──────────────┘       │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │   Template   │  │    Export    │  │   Publish    │       │
│  │    Engine    │  │   (Filter)   │  │   (HTML)     │       │
│  └──────────────┘  └──────────────┘  └──────────────┘       │
├─────────────────────────────────────────────────────────────┤
│  FileSystem Trait (RealFileSystem | InMemoryFileSystem)     │
└─────────────────────────────────────────────────────────────┘
           │                    │                    │
           ▼                    ▼                    ▼
    ┌──────────┐        ┌──────────────┐      ┌──────────┐
    │   CLI    │        │    WASM      │      │  Tauri   │
    │ (diaryx) │        │ (diaryx_wasm)│      │  Backend │
    └──────────┘        └──────────────┘      └──────────┘
```

All business logic should live in `diaryx_core`. Frontends should be thin wrappers that:

- Handle I/O (filesystem, user input, HTTP)
- Convert types for their environment
- Call core functions

---

## Current Issues

- Remove sync filesystem from diaryx_core
- Update Tauri and CLI to use an async filesystem natively

Thank you for contributing to Diaryx!
