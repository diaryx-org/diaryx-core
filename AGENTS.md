---
title: AGENTS
description: Instructions for AI Agents
author: adammharris
updated: 2026-01-31T23:46:08Z
part_of: '[README](/README.md)'
---

# Instructions for AI agents

## Workspace Overview

<!-- BEGIN:WORKSPACE_INDEX -->
README - Repository for the Diaryx project
├── crates - Cargo crates for Diaryx
│   ├── diaryx - CLI frontend
│   ├── diaryx_core - Core library shared by Diaryx clients
│   │   ├── CRDT Synchronization - Conflict-free replicated data types for real-time collaboration
│   │   └── Cloud Sync - Bidirectional file synchronization with cloud storage
│   ├── diaryx_wasm - WASM bindings for diaryx_core
│   └── diaryx_sync_server - Sync server used by frontends
├── apps - GUI frontends for Diaryx
│   ├── web - Svelte + Tiptap frontend for Diaryx
│   │   └── TipTap Custom Extensions - Guide to creating custom TipTap extensions with markdown support
│   └── tauri - Web app + native backend
├── LICENSE - PolyForm Shield License 1.0.0
├── ROADMAP - The plan for future Diaryx features
├── AGENTS - Instructions for AI Agents
├── CONTRIBUTING - A guide for making contributions in the Diaryx repo
└── Scripts
<!-- END:WORKSPACE_INDEX -->

## Entry Points

Read the root README.md first. For specific projects, use these entry points:

| Project | Entry point |
|---------|-------------|
| Entire workspace | README.md |
| Core library | crates/diaryx_core/README.md |
| CLI | crates/diaryx/README.md |
| Web app | apps/web/README.md |
| Tauri app | apps/tauri/README.md |
| WASM bindings | crates/diaryx_wasm/README.md |
| Sync server | crates/diaryx_sync_server/README.md |

## Commands

```bash
# Build all crates
cargo build

# Run tests
cargo test

# Run CLI
cargo run --bin diaryx -- <args>

# Build WASM
./scripts/build-wasm.sh
# Or, since this script is included in package.json...
cd apps/web && bun run build

# Web dev server
cd apps/web && bun run dev

# Tauri dev
cd apps/tauri && cargo tauri dev
```

## Not Documented

Read these files directly when needed:
- CI/workflows: `.github/workflows/*.yml`
- Pre-commit config: `.pre-commit-config.yaml`
