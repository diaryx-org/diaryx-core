/**
 * SQLite-based CRDT storage using sql.js with OPFS persistence.
 *
 * This provides persistent storage for CRDT documents and updates in the web app,
 * matching the schema used by the native SqliteStorage in diaryx_core.
 *
 * ## Architecture
 * - Uses sql.js (SQLite compiled to WASM) for in-memory database
 * - Persists database file to OPFS for durability across page reloads
 * - Provides synchronous read/write operations (required by CrdtStorage trait)
 * - Background async persistence to OPFS on changes
 */

import initSqlJs, { type Database } from "sql.js";

// OPFS path for the database file
const DB_OPFS_PATH = ".diaryx/crdt.db";

// Debounce delay for OPFS persistence (ms)
const PERSIST_DEBOUNCE_MS = 1000;

/**
 * Represents a CRDT update stored in the database.
 */
export interface CrdtUpdate {
  updateId: number;
  docName: string;
  data: Uint8Array;
  timestamp: number;
  origin: string;
  deviceId: string | null;
  deviceName: string | null;
}

/**
 * SQLite-based CRDT storage with OPFS persistence.
 */
export class SqliteStorage {
  private db: Database;
  private persistTimer: ReturnType<typeof setTimeout> | null = null;
  private dirty = false;

  private constructor(db: Database) {
    this.db = db;
  }

  /**
   * Create and initialize a new SqliteStorage instance.
   * Loads existing database from OPFS if available.
   */
  static async create(): Promise<SqliteStorage> {
    // Initialize sql.js with the WASM file
    const SQL = await initSqlJs({
      // Use CDN for the WASM file
      locateFile: (file: string) =>
        `https://sql.js.org/dist/${file}`,
    });

    // Try to load existing database from OPFS
    let dbData: Uint8Array | null = null;
    try {
      dbData = await loadFromOpfs(DB_OPFS_PATH);
    } catch (e) {
      // No existing database, will create new one
      console.log("[SqliteStorage] No existing database found, creating new");
    }

    // Create database (with existing data if available)
    const db = dbData ? new SQL.Database(dbData) : new SQL.Database();

    const storage = new SqliteStorage(db);
    storage.initSchema();

    // If this is a new database, persist it immediately
    if (!dbData) {
      await storage.persistToOpfs();
    }

    return storage;
  }

  /**
   * Initialize the database schema.
   * Creates tables if they don't exist (matches native SqliteStorage schema).
   */
  private initSchema(): void {
    this.db.run(`
      -- Document snapshots (compacted state)
      CREATE TABLE IF NOT EXISTS documents (
        name TEXT PRIMARY KEY,
        state BLOB NOT NULL,
        state_vector BLOB NOT NULL,
        updated_at INTEGER NOT NULL
      );

      -- Incremental updates (for history)
      CREATE TABLE IF NOT EXISTS updates (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        doc_name TEXT NOT NULL,
        data BLOB NOT NULL,
        origin TEXT NOT NULL,
        timestamp INTEGER NOT NULL,
        device_id TEXT,
        device_name TEXT
      );

      -- Index for efficient sync queries
      CREATE INDEX IF NOT EXISTS idx_updates_doc_id ON updates(doc_name, id);

      -- Metadata for workspace files (queryable without loading CRDT)
      CREATE TABLE IF NOT EXISTS file_index (
        path TEXT PRIMARY KEY,
        title TEXT,
        part_of TEXT,
        deleted INTEGER NOT NULL DEFAULT 0,
        modified_at INTEGER NOT NULL
      );

      -- Index for querying non-deleted files
      CREATE INDEX IF NOT EXISTS idx_file_index_deleted ON file_index(deleted);
    `);
  }

  // =========================================================================
  // CrdtStorage trait methods (synchronous)
  // =========================================================================

  /**
   * Load the full document state as a binary blob.
   * Returns null if the document doesn't exist.
   */
  loadDoc(name: string): Uint8Array | null {
    const stmt = this.db.prepare("SELECT state FROM documents WHERE name = ?");
    stmt.bind([name]);

    if (stmt.step()) {
      const row = stmt.get();
      stmt.free();
      return row[0] as Uint8Array;
    }

    stmt.free();
    return null;
  }

  /**
   * Save the full document state.
   * Overwrites any existing state for the document.
   */
  saveDoc(name: string, state: Uint8Array, stateVector: Uint8Array): void {
    const now = Date.now();
    this.db.run(
      "INSERT OR REPLACE INTO documents (name, state, state_vector, updated_at) VALUES (?, ?, ?, ?)",
      [name, state, stateVector, now]
    );
    this.markDirty();
  }

  /**
   * Delete a document and all its updates.
   */
  deleteDoc(name: string): void {
    this.db.run("DELETE FROM updates WHERE doc_name = ?", [name]);
    this.db.run("DELETE FROM documents WHERE name = ?", [name]);
    this.markDirty();
  }

  /**
   * List all document names in storage.
   */
  listDocs(): string[] {
    const stmt = this.db.prepare("SELECT name FROM documents ORDER BY name");
    const names: string[] = [];

    while (stmt.step()) {
      const row = stmt.get();
      names.push(row[0] as string);
    }

    stmt.free();
    return names;
  }

  /**
   * Append an incremental update to the update log.
   * Returns the ID of the newly created update record.
   */
  appendUpdate(
    name: string,
    update: Uint8Array,
    origin: string,
    deviceId: string | null = null,
    deviceName: string | null = null
  ): number {
    const now = Date.now();
    this.db.run(
      "INSERT INTO updates (doc_name, data, origin, timestamp, device_id, device_name) VALUES (?, ?, ?, ?, ?, ?)",
      [name, update, origin, now, deviceId, deviceName]
    );

    // Get the last inserted row ID
    const stmt = this.db.prepare("SELECT last_insert_rowid()");
    stmt.step();
    const row = stmt.get();
    stmt.free();

    this.markDirty();
    return row[0] as number;
  }

  /**
   * Get all updates for a document since a given update ID.
   */
  getUpdatesSince(name: string, sinceId: number): CrdtUpdate[] {
    const stmt = this.db.prepare(
      "SELECT id, data, origin, timestamp, device_id, device_name FROM updates WHERE doc_name = ? AND id > ? ORDER BY id ASC"
    );
    stmt.bind([name, sinceId]);

    const updates: CrdtUpdate[] = [];
    while (stmt.step()) {
      const row = stmt.get();
      updates.push({
        updateId: row[0] as number,
        docName: name,
        data: row[1] as Uint8Array,
        timestamp: row[3] as number,
        origin: row[2] as string,
        deviceId: row[4] as string | null,
        deviceName: row[5] as string | null,
      });
    }

    stmt.free();
    return updates;
  }

  /**
   * Get all updates for a document.
   */
  getAllUpdates(name: string): CrdtUpdate[] {
    return this.getUpdatesSince(name, 0);
  }

  /**
   * Get the latest update ID for a document.
   * Returns 0 if no updates exist.
   */
  getLatestUpdateId(name: string): number {
    const stmt = this.db.prepare(
      "SELECT id FROM updates WHERE doc_name = ? ORDER BY id DESC LIMIT 1"
    );
    stmt.bind([name]);

    if (stmt.step()) {
      const row = stmt.get();
      stmt.free();
      return row[0] as number;
    }

    stmt.free();
    return 0;
  }

  /**
   * Compact old updates into the document snapshot.
   * Keeps only the most recent `keepUpdates` in the log.
   */
  compact(name: string, keepUpdates: number): void {
    // Count updates
    const countStmt = this.db.prepare(
      "SELECT COUNT(*) FROM updates WHERE doc_name = ?"
    );
    countStmt.bind([name]);
    countStmt.step();
    const count = countStmt.get()[0] as number;
    countStmt.free();

    if (count <= keepUpdates) {
      return;
    }

    // Find the cutoff ID
    const cutoffStmt = this.db.prepare(
      "SELECT id FROM updates WHERE doc_name = ? ORDER BY id DESC LIMIT 1 OFFSET ?"
    );
    cutoffStmt.bind([name, keepUpdates - 1]);
    cutoffStmt.step();
    const cutoffId = cutoffStmt.get()[0] as number;
    cutoffStmt.free();

    // Delete old updates
    this.db.run("DELETE FROM updates WHERE doc_name = ? AND id < ?", [
      name,
      cutoffId,
    ]);
    this.markDirty();
  }

  // =========================================================================
  // File index methods (for queryable file metadata)
  // =========================================================================

  /**
   * Update the file index from decoded FileMetadata.
   */
  updateFileIndex(
    path: string,
    title: string | null,
    partOf: string | null,
    deleted: boolean,
    modifiedAt: number
  ): void {
    this.db.run(
      "INSERT OR REPLACE INTO file_index (path, title, part_of, deleted, modified_at) VALUES (?, ?, ?, ?, ?)",
      [path, title, partOf, deleted ? 1 : 0, modifiedAt]
    );
    this.markDirty();
  }

  /**
   * Query active (non-deleted) files from the index.
   */
  queryActiveFiles(): Array<{
    path: string;
    title: string | null;
    partOf: string | null;
  }> {
    const stmt = this.db.prepare(
      "SELECT path, title, part_of FROM file_index WHERE deleted = 0 ORDER BY path"
    );
    const files: Array<{
      path: string;
      title: string | null;
      partOf: string | null;
    }> = [];

    while (stmt.step()) {
      const row = stmt.get();
      files.push({
        path: row[0] as string,
        title: row[1] as string | null,
        partOf: row[2] as string | null,
      });
    }

    stmt.free();
    return files;
  }

  /**
   * Remove a file from the index entirely.
   */
  removeFromFileIndex(path: string): void {
    this.db.run("DELETE FROM file_index WHERE path = ?", [path]);
    this.markDirty();
  }

  // =========================================================================
  // OPFS persistence
  // =========================================================================

  /**
   * Mark the database as dirty and schedule persistence.
   */
  private markDirty(): void {
    this.dirty = true;
    this.schedulePersist();
  }

  /**
   * Schedule a debounced persist to OPFS.
   */
  private schedulePersist(): void {
    if (this.persistTimer) {
      clearTimeout(this.persistTimer);
    }

    this.persistTimer = setTimeout(async () => {
      if (this.dirty) {
        await this.persistToOpfs();
        this.dirty = false;
      }
    }, PERSIST_DEBOUNCE_MS);
  }

  /**
   * Persist the database to OPFS immediately.
   */
  async persistToOpfs(): Promise<void> {
    try {
      const data = this.db.export();
      await saveToOpfs(DB_OPFS_PATH, data);
    } catch (e) {
      console.error("[SqliteStorage] Failed to persist to OPFS:", e);
    }
  }

  /**
   * Force immediate persistence (call before page unload).
   */
  async flush(): Promise<void> {
    if (this.persistTimer) {
      clearTimeout(this.persistTimer);
      this.persistTimer = null;
    }
    if (this.dirty) {
      await this.persistToOpfs();
      this.dirty = false;
    }
  }

  /**
   * Close the database and flush to OPFS.
   */
  async close(): Promise<void> {
    await this.flush();
    this.db.close();
  }

  /**
   * Export the database as a Uint8Array (for debugging/backup).
   */
  export(): Uint8Array {
    return this.db.export();
  }
}

// ============================================================================
// OPFS Helper Functions
// ============================================================================

/**
 * Get or create nested directories in OPFS.
 */
async function getOrCreateDir(
  root: FileSystemDirectoryHandle,
  path: string[]
): Promise<FileSystemDirectoryHandle> {
  let current = root;
  for (const segment of path) {
    current = await current.getDirectoryHandle(segment, { create: true });
  }
  return current;
}

/**
 * Load a file from OPFS.
 */
async function loadFromOpfs(path: string): Promise<Uint8Array> {
  const root = await navigator.storage.getDirectory();
  const segments = path.split("/").filter((s) => s.length > 0);
  const fileName = segments.pop()!;
  const dir = await getOrCreateDir(root, segments);

  const fileHandle = await dir.getFileHandle(fileName);
  const file = await fileHandle.getFile();
  const buffer = await file.arrayBuffer();
  return new Uint8Array(buffer);
}

/**
 * Save a file to OPFS.
 */
async function saveToOpfs(path: string, data: Uint8Array): Promise<void> {
  const root = await navigator.storage.getDirectory();
  const segments = path.split("/").filter((s) => s.length > 0);
  const fileName = segments.pop()!;
  const dir = await getOrCreateDir(root, segments);

  const fileHandle = await dir.getFileHandle(fileName, { create: true });
  const writable = await fileHandle.createWritable();
  // Write using ArrayBuffer - cast to avoid SharedArrayBuffer type issue
  await writable.write(data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength) as ArrayBuffer);
  await writable.close();
}

// ============================================================================
// Global instance management
// ============================================================================

let globalStorage: SqliteStorage | null = null;
let initPromise: Promise<SqliteStorage> | null = null;

/**
 * Get the global SqliteStorage instance.
 * Creates one if it doesn't exist.
 */
export async function getSqliteStorage(): Promise<SqliteStorage> {
  if (globalStorage) {
    return globalStorage;
  }

  if (initPromise) {
    return initPromise;
  }

  initPromise = SqliteStorage.create().then((storage) => {
    globalStorage = storage;
    return storage;
  });

  return initPromise;
}

/**
 * Get the global SqliteStorage instance if already initialized.
 * Returns null if not yet initialized.
 */
export function getSqliteStorageSync(): SqliteStorage | null {
  return globalStorage;
}

/**
 * Flush the global storage to OPFS (call before page unload).
 */
export async function flushSqliteStorage(): Promise<void> {
  if (globalStorage) {
    await globalStorage.flush();
  }
}
