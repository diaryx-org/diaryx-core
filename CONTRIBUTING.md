# Contributing to Diaryx

Welcome to the Diaryx project! This document will help you understand the codebase structure, identify areas for improvement, and find good first issues to work on.

## Repository Structure

Diaryx is organized as a Rust workspace with multiple crates:

```
diaryx-core/
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

| Module         | Purpose                                                                             |
| -------------- | ----------------------------------------------------------------------------------- |
| `config.rs`    | Configuration management (workspace paths, editor settings)                         |
| `date.rs`      | Natural language date parsing and path generation                                   |
| `entry.rs`     | Main `DiaryxApp` struct with entry CRUD operations                                  |
| `error.rs`     | Unified error types (`DiaryxError`)                                                 |
| `export.rs`    | Audience-based export functionality                                                 |
| `fs.rs`        | Filesystem abstraction (`FileSystem` trait, `RealFileSystem`, `InMemoryFileSystem`) |
| `publish.rs`   | HTML publishing with navigation                                                     |
| `search.rs`    | Full-text and frontmatter search                                                    |
| `template.rs`  | Template engine with variable substitution                                          |
| `workspace.rs` | Workspace tree building and index management                                        |

#### `diaryx` - CLI Application

Command-line interface built on top of `diaryx_core`.

| Module             | Purpose                                       |
| ------------------ | --------------------------------------------- |
| `main.rs`          | Entry point                                   |
| `editor.rs`        | System editor integration                     |
| `cli/args.rs`      | Clap argument definitions                     |
| `cli/mod.rs`       | Command dispatcher                            |
| `cli/entry.rs`     | today, yesterday, open, create commands       |
| `cli/workspace.rs` | workspace subcommands (add, mv, create, etc.) |
| `cli/property.rs`  | Frontmatter property manipulation             |
| `cli/content.rs`   | Body content manipulation                     |
| `cli/search.rs`    | Search command handler                        |
| `cli/template.rs`  | Template management                           |
| `cli/util.rs`      | Shared CLI utilities                          |

#### `diaryx_wasm` - WebAssembly Bindings

WASM bindings that expose `diaryx_core` functionality to JavaScript. Uses an in-memory filesystem that syncs with IndexedDB.

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

Thank you for contributing to Diaryx! 🎉
