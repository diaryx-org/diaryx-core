/**
 * Storage module for Diaryx web app.
 *
 * This module provides persistent CRDT storage using SQLite (via sql.js)
 * with OPFS for durability.
 */

export {
  SqliteStorage,
  getSqliteStorage,
  getSqliteStorageSync,
  flushSqliteStorage,
  type CrdtUpdate,
} from "./sqliteStorage.js";

export {
  initializeSqliteStorage,
  isStorageReady,
  flushStorage,
} from "./sqliteStorageBridge.js";

/**
 * Set up the global bridge that Rust WASM code will use to access storage.
 *
 * Call this BEFORE creating DiaryxBackend to enable persistent CRDT storage.
 * Works in both Window and Worker contexts using globalThis.
 *
 * @example
 * ```typescript
 * import { setupCrdtStorageBridge } from './lib/storage';
 * await setupCrdtStorageBridge();
 * const backend = await DiaryxBackend.createOpfs();
 * ```
 */
export async function setupCrdtStorageBridge(): Promise<void> {
  // Dynamically import to avoid circular dependencies
  const bridge = await import("./sqliteStorageBridge.js");

  // Initialize the storage
  await bridge.initializeSqliteStorage();

  // Set up the global bridge object that Rust will access
  // Use globalThis for compatibility with both Window and Worker contexts
  (globalThis as any).__diaryx_crdt_storage = {
    crdt_load_doc: bridge.crdt_load_doc,
    crdt_save_doc: bridge.crdt_save_doc,
    crdt_delete_doc: bridge.crdt_delete_doc,
    crdt_list_docs: bridge.crdt_list_docs,
    crdt_append_update: bridge.crdt_append_update,
    crdt_get_updates_since: bridge.crdt_get_updates_since,
    crdt_get_all_updates: bridge.crdt_get_all_updates,
    crdt_get_latest_update_id: bridge.crdt_get_latest_update_id,
    crdt_compact: bridge.crdt_compact,
    crdt_rename_doc: bridge.crdt_rename_doc,
    crdt_update_file_index: bridge.crdt_update_file_index,
    crdt_query_active_files: bridge.crdt_query_active_files,
    crdt_remove_from_file_index: bridge.crdt_remove_from_file_index,
  };

  console.log("[Storage] CRDT storage bridge initialized");
}
