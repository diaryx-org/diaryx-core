/**
 * Stub module for WASM when building Tauri (which uses native Rust backend).
 * This prevents Vite from failing when the WASM files don't exist.
 */

export default function init() {
  throw new Error('WASM backend is not available in Tauri builds. Use the native backend instead.');
}

export class DiaryxBackend {
  static createOpfs() {
    throw new Error('WASM backend is not available in Tauri builds.');
  }
  static createIndexedDb() {
    throw new Error('WASM backend is not available in Tauri builds.');
  }
  static createFromDirectoryHandle() {
    throw new Error('WASM backend is not available in Tauri builds.');
  }
}
