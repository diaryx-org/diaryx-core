/**
 * JavaScript bridge for Rust WASM to call SqliteStorage.
 *
 * This file provides synchronous wrapper functions that Rust can import
 * via wasm-bindgen. The functions operate on the global SqliteStorage instance.
 *
 * IMPORTANT: SqliteStorage must be initialized BEFORE Rust code calls these functions.
 * The web app should call `initializeSqliteStorage()` during startup.
 */

import {
  getSqliteStorageSync,
  getSqliteStorage,
  flushSqliteStorage,
} from "./sqliteStorage.js";

// ============================================================================
// Initialization
// ============================================================================

/**
 * Initialize the SQLite storage. Must be called before any other functions.
 * Returns a promise that resolves when storage is ready.
 * @returns {Promise<void>}
 */
export async function initializeSqliteStorage() {
  await getSqliteStorage();
  console.log("[SqliteStorageBridge] Storage initialized");
}

/**
 * Check if storage is initialized.
 * @returns {boolean}
 */
export function isStorageReady() {
  return getSqliteStorageSync() !== null;
}

// ============================================================================
// CrdtStorage trait methods (synchronous - called from Rust)
// ============================================================================

/**
 * Load a document's state.
 * @param {string} name - Document name
 * @returns {Uint8Array | null}
 */
export function crdt_load_doc(name) {
  const storage = getSqliteStorageSync();
  if (!storage) {
    console.error("[SqliteStorageBridge] Storage not initialized");
    return null;
  }
  return storage.loadDoc(name);
}

/**
 * Save a document's state.
 * @param {string} name - Document name
 * @param {Uint8Array} state - Document state
 * @param {Uint8Array} stateVector - State vector for the document
 */
export function crdt_save_doc(name, state, stateVector) {
  const storage = getSqliteStorageSync();
  if (!storage) {
    console.error("[SqliteStorageBridge] Storage not initialized");
    return;
  }
  storage.saveDoc(name, state, stateVector);
}

/**
 * Delete a document and all its updates.
 * @param {string} name - Document name
 */
export function crdt_delete_doc(name) {
  const storage = getSqliteStorageSync();
  if (!storage) {
    console.error("[SqliteStorageBridge] Storage not initialized");
    return;
  }
  storage.deleteDoc(name);
}

/**
 * List all document names.
 * @returns {string[]}
 */
export function crdt_list_docs() {
  const storage = getSqliteStorageSync();
  if (!storage) {
    console.error("[SqliteStorageBridge] Storage not initialized");
    return [];
  }
  return storage.listDocs();
}

/**
 * Append an update to the log.
 * @param {string} name - Document name
 * @param {Uint8Array} update - Update data
 * @param {string} origin - Update origin ("local", "remote", "sync")
 * @param {string | null} deviceId - Device ID
 * @param {string | null} deviceName - Device name
 * @returns {number} - Update ID
 */
export function crdt_append_update(name, update, origin, deviceId, deviceName) {
  const storage = getSqliteStorageSync();
  if (!storage) {
    console.error("[SqliteStorageBridge] Storage not initialized");
    return -1;
  }
  return storage.appendUpdate(name, update, origin, deviceId, deviceName);
}

/**
 * Get updates since a given ID.
 * @param {string} name - Document name
 * @param {number} sinceId - Update ID to start from
 * @returns {Array<{updateId: number, docName: string, data: Uint8Array, timestamp: number, origin: string, deviceId: string | null, deviceName: string | null}>}
 */
export function crdt_get_updates_since(name, sinceId) {
  const storage = getSqliteStorageSync();
  if (!storage) {
    console.error("[SqliteStorageBridge] Storage not initialized");
    return [];
  }
  return storage.getUpdatesSince(name, sinceId);
}

/**
 * Get all updates for a document.
 * @param {string} name - Document name
 * @returns {Array<{updateId: number, docName: string, data: Uint8Array, timestamp: number, origin: string, deviceId: string | null, deviceName: string | null}>}
 */
export function crdt_get_all_updates(name) {
  const storage = getSqliteStorageSync();
  if (!storage) {
    console.error("[SqliteStorageBridge] Storage not initialized");
    return [];
  }
  return storage.getAllUpdates(name);
}

/**
 * Get the latest update ID for a document.
 * @param {string} name - Document name
 * @returns {number}
 */
export function crdt_get_latest_update_id(name) {
  const storage = getSqliteStorageSync();
  if (!storage) {
    console.error("[SqliteStorageBridge] Storage not initialized");
    return 0;
  }
  return storage.getLatestUpdateId(name);
}

/**
 * Compact old updates.
 * @param {string} name - Document name
 * @param {number} keepUpdates - Number of updates to keep
 */
export function crdt_compact(name, keepUpdates) {
  const storage = getSqliteStorageSync();
  if (!storage) {
    console.error("[SqliteStorageBridge] Storage not initialized");
    return;
  }
  storage.compact(name, keepUpdates);
}

// ============================================================================
// File index methods
// ============================================================================

/**
 * Update the file index.
 * @param {string} path - File path
 * @param {string | null} title - File title
 * @param {string | null} partOf - Parent file path
 * @param {boolean} deleted - Whether file is deleted
 * @param {number} modifiedAt - Modification timestamp
 */
export function crdt_update_file_index(
  path,
  title,
  partOf,
  deleted,
  modifiedAt
) {
  const storage = getSqliteStorageSync();
  if (!storage) {
    console.error("[SqliteStorageBridge] Storage not initialized");
    return;
  }
  storage.updateFileIndex(path, title, partOf, deleted, modifiedAt);
}

/**
 * Query active files from the index.
 * @returns {Array<{path: string, title: string | null, partOf: string | null}>}
 */
export function crdt_query_active_files() {
  const storage = getSqliteStorageSync();
  if (!storage) {
    console.error("[SqliteStorageBridge] Storage not initialized");
    return [];
  }
  return storage.queryActiveFiles();
}

/**
 * Remove a file from the index.
 * @param {string} path - File path
 */
export function crdt_remove_from_file_index(path) {
  const storage = getSqliteStorageSync();
  if (!storage) {
    console.error("[SqliteStorageBridge] Storage not initialized");
    return;
  }
  storage.removeFromFileIndex(path);
}

// ============================================================================
// Persistence
// ============================================================================

/**
 * Flush storage to OPFS (call before page unload).
 * @returns {Promise<void>}
 */
export async function flushStorage() {
  await flushSqliteStorage();
}

// Register beforeunload handler to flush on page close (only in Window context)
if (typeof globalThis !== "undefined" && typeof globalThis.addEventListener === "function") {
  // Check if we're in a Window context (has beforeunload)
  try {
    globalThis.addEventListener("beforeunload", () => {
      flushSqliteStorage().catch(console.error);
    });
  } catch {
    // Not in a Window context (probably a Worker), skip beforeunload
  }
}
