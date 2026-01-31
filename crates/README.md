---
title: crates
description: Cargo crates for Diaryx
author: adammharris
contents:
- '[README](/crates/diaryx/README.md)'
- '[README](/crates/diaryx_core/README.md)'
- '[README](/crates/diaryx_wasm/README.md)'
- '[README](/crates/diaryx_sync_server/README.md)'
attachments:
- LICENSE.md
part_of: '[README](/README.md)'
---

This folder contains three crates for Diaryx.

- [`diaryx`](diaryx/README.md): CLI interface
- [`diaryx_core`](diaryx_core/README.md): Core functions shared across all Diaryx clients
- [`diaryx_wasm`](diaryx_wasm/README.md): WASM version of `diaryx_core` to be used in the web client at [`../apps/web`](../apps/web/README.md)
- [`diaryx_sync_server`](diaryx_sync_server/README.md): Sync server to enable live sync/multi-device sync (soon publishing as well).

Cargo also copies the LICENSE.md file here for publishing crates. It is has the same content as `../LICENSE.md`.
