---
title: Scripts
part_of: '[README](/README.md)'
attachments:
- build-wasm.sh
- sync-versions.sh
- test-sync.sh
---

# Scripts

This folder contains three scripts:

- `build-wasm.sh`: Builds crates/diaryx_wasm for the web app. Used in apps/web/package.json's build script.
- `sync-versions.sh`: Using the root README.md as a source of truth, updates every version number in the repository.
- `test-sync.sh`: Opens a tmux window with the diaryx_sync_server and two web app dev servers.
