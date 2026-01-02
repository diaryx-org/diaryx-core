---
title: diaryx_wasm overview
part_of: ../../README.md
---

# diaryx_wasm overview

This crate provides WebAssembly bindings for `diaryx_core`, used in `apps/web`.

To build the WebAssembly module, run:

```bash
wasm-pack build --target web --out-dir ../../apps/web/src/lib/wasm
```
