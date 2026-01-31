---
title: AGENTS
description: Instructions for AI Agents
author: adammharris
updated: 2026-01-02T13:06:25-05:00
part_of: '[README](/README.md)'
---

# Instructions for AI agents

## Entry points

Here is your "table of contents" for this repository. You should always read the root README.md first before doing anything in this codebase. For specific projects, you must read their respective "entry point" files. If you make changes, update these files to reflect the changes you make.

| Project | Entry point |
|---------|-------------|
| Entire workspace | README.md |
| diaryx_core (core logic) | crates/diaryx_core/README.md |
| Apps (GUI frontend for Diaryx) | apps/README.md |
| Web app | apps/web/README.md |
| Tauri app | apps/tauri/README.md |
| CLI | crates/diaryx/README.md |
| WASM build (backend for web app) | crates/diaryx_wasm/README.md |
| Sync server | crates/diaryx_sync_server/README.md |

Not documented:
- CI/workflows (read .yml files in .github/workflows if necessary)
- Pre-commit files. Read `.pre-commit-config.yaml` if necessary.
