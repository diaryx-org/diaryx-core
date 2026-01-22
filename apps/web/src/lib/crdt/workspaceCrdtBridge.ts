/**
 * Workspace CRDT Bridge - replaces workspaceCrdt.ts with Rust CRDT backend.
 *
 * This module provides the same API surface as the original workspaceCrdt.ts
 * but delegates all operations to the Rust CRDT via RustCrdtApi.
 *
 * Supports Hocuspocus server-based sync for device-to-device synchronization.
 */

import * as Y from 'yjs';
import type { RustCrdtApi } from './rustCrdtApi';
import { RustSyncBridge, createRustSyncBridge } from './rustSyncBridge';
import { BodySyncBridge, createBodySyncBridge } from './bodySyncBridge';
import type { FileMetadata, BinaryRef } from '../backend/generated';
import type { Backend, FileSystemEvent } from '../backend/interface';
import { crdt_update_file_index, isStorageReady } from '$lib/storage/sqliteStorageBridge.js';
import type { Api } from '../backend/api';
import { shareSessionStore } from '@/models/stores/shareSessionStore.svelte';
import { getClientOwnerId } from '@/models/services/shareService';
import { getToken } from '$lib/auth/authStore.svelte';

/**
 * Convert an HTTP URL to a WebSocket URL for sync.
 * SimpleSyncBridge expects WebSocket URLs (wss:// or ws://), not HTTP URLs.
 */
function toWebSocketUrl(httpUrl: string): string {
  let wsUrl = httpUrl
    .replace(/^https:\/\//, 'wss://')
    .replace(/^http:\/\//, 'ws://');

  // Only append /sync if not already present
  if (!wsUrl.endsWith('/sync')) {
    wsUrl += '/sync';
  }

  return wsUrl;
}

// State
let rustApi: RustCrdtApi | null = null;
let syncBridge: RustSyncBridge | null = null;
let backendApi: Api | null = null;

// Workspace server sync - re-enabled after fixing incremental update issue.
// The previous issue was caused by re-encoding full state instead of applying incremental updates.
const WORKSPACE_SERVER_SYNC_ENABLED = true;
let workspaceYDoc: Y.Doc | null = null;
let serverUrl: string | null = null;
let _workspaceId: string | null = null;
let initialized = false;
let _initializing = false;

// Initial sync tracking - allows waiting for first sync to complete
let _initialSyncComplete = false;
let _initialSyncResolvers: Array<() => void> = [];

// Per-file body sync bridges (file_path -> BodySyncBridge)
const bodyBridges = new Map<string, BodySyncBridge>();

// Cached server URL for body bridges
let _serverUrl: string | null = null;

// Per-file mutex to prevent race conditions on concurrent updates
// Map of path -> Promise that resolves when the lock is released
const fileLocks = new Map<string, Promise<void>>();

// Track pending intervals/timeouts for proper cleanup
const pendingIntervals: Set<ReturnType<typeof setInterval>> = new Set();
const pendingTimeouts: Set<ReturnType<typeof setTimeout>> = new Set();

/**
 * Acquire a lock for a specific file path.
 * Returns a release function to call when done.
 * This prevents concurrent read-modify-write races on the same file.
 */
async function acquireFileLock(path: string): Promise<() => void> {
  // Wait for any existing lock to be released
  while (fileLocks.has(path)) {
    await fileLocks.get(path);
  }

  // Create a new lock
  let releaseLock: () => void;
  const lockPromise = new Promise<void>((resolve) => {
    releaseLock = resolve;
  });
  fileLocks.set(path, lockPromise);

  // Return the release function
  return () => {
    fileLocks.delete(path);
    releaseLock!();
  };
}

// Callbacks
type FileChangeCallback = (path: string | null, metadata: FileMetadata | null) => void;
const fileChangeCallbacks = new Set<FileChangeCallback>();

// Session sync callbacks - called when session data is received and synced
type SessionSyncCallback = () => void;
const sessionSyncCallbacks = new Set<SessionSyncCallback>();

// Body change callbacks - called when a file's body content changes remotely
type BodyChangeCallback = (path: string, body: string) => void;
const bodyChangeCallbacks = new Set<BodyChangeCallback>();

// Sync progress callbacks - called to report sync progress
type SyncProgressCallback = (completed: number, total: number) => void;
const syncProgressCallbacks = new Set<SyncProgressCallback>();

// Sync status callbacks - called when sync status changes
type SyncStatusCallback = (status: 'idle' | 'connecting' | 'syncing' | 'synced' | 'error', error?: string) => void;
const syncStatusCallbacks = new Set<SyncStatusCallback>();

// ===========================================================================
// Configuration
// ===========================================================================

/**
 * Set the server URL for workspace sync.
 * Creates and connects a RustSyncBridge if the URL is set.
 *
 * NOTE: Workspace server sync is currently disabled (WORKSPACE_SERVER_SYNC_ENABLED = false).
 * Per-document sync works fine, but workspace sync causes content duplication issues.
 */
export async function setWorkspaceServer(url: string | null): Promise<void> {
  const previousUrl = serverUrl;
  serverUrl = url;
  _serverUrl = url ? toWebSocketUrl(url) : null;

  console.log('[WorkspaceCrdtBridge] setWorkspaceServer:', {
    inputUrl: url,
    resolvedServerUrl: _serverUrl,
    previousUrl,
    initialized,
    hasRustApi: !!rustApi,
  });

  // Skip if URL hasn't changed or not initialized
  if (previousUrl === url || !initialized || !rustApi) {
    return;
  }

  // Disconnect existing bridge if any
  if (syncBridge) {
    console.log('[WorkspaceCrdtBridge] Disconnecting existing sync bridge');
    syncBridge.destroy();
    syncBridge = null;
  }

  // Workspace server sync is disabled for now
  if (!WORKSPACE_SERVER_SYNC_ENABLED) {
    console.log('[WorkspaceCrdtBridge] Workspace server sync disabled, skipping bridge creation');
    return;
  }

  // Create new bridge if URL is set
  if (url) {
    const workspaceDocName = _workspaceId ? `${_workspaceId}:workspace` : 'workspace';
    const wsUrl = toWebSocketUrl(url);
    console.log('[WorkspaceCrdtBridge] Creating RustSyncBridge for workspace:', wsUrl, 'docName:', workspaceDocName);

    syncBridge = createRustSyncBridge({
      serverUrl: wsUrl,
      docName: workspaceDocName,
      rustApi: rustApi,
      authToken: getToken() ?? undefined,
      onStatusChange: (connected) => {
        console.log('[WorkspaceCrdtBridge] Connection status:', connected);
        notifySyncStatus(connected ? 'syncing' : 'idle');
      },
      onSynced: async () => {
        console.log('[WorkspaceCrdtBridge] Initial sync complete');
        notifySyncStatus('synced');
        // With RustSyncBridge, the Rust CRDT is already updated by the sync.
        // Notify UI to refresh from Rust CRDT.
        notifyFileChange(null, null);

        // Update file index from Rust CRDT for persistence
        await updateFileIndexFromCrdt();
      },
      onRemoteUpdate: async () => {
        // Remote update received - write metadata to disk for device sync
        await processRemoteMetadataUpdates();
        // Notify UI to refresh tree (FileSystemEvents don't fire for sync-originated changes)
        notifyFileChange(null, null);
      },
    });

    // Notify connecting status
    notifySyncStatus('connecting');

    await syncBridge.connect();
  }
}



/**
 * Write a file to disk with frontmatter and body content.
 * Delegates to the Rust backend which handles YAML frontmatter generation.
 */
async function writeFileWithFrontmatter(path: string, metadata: FileMetadata, body: string): Promise<void> {
  if (!backendApi) return;
  // Delegate to Rust backend - it handles YAML frontmatter generation
  await backendApi.writeFileWithMetadata(path, metadata as unknown as import('../backend/generated/serde_json/JsonValue').JsonValue, body);
}

/**
 * Get the current workspace server URL.
 */
export function getWorkspaceServer(): string | null {
  return serverUrl;
}

/**
 * Set the initializing state (for UI feedback).
 */
export function setInitializing(value: boolean): void {
  _initializing = value;
}

/**
 * Set the workspace ID for room naming.
 */
export function setWorkspaceId(id: string | null): void {
  console.log('[WorkspaceCrdtBridge] setWorkspaceId:', { id, previousId: _workspaceId });
  _workspaceId = id;
}

/**
 * Set the backend API for file operations.
 * This is used to write synced file content to disk for guests.
 */
export function setBackendApi(api: Api): void {
  backendApi = api;
}

/**
 * Get the storage path for a file in guest mode.
 *
 * For guests using in-memory storage (web): Returns the original path (no prefix needed).
 * For guests using OPFS (Tauri, future): Prefixes with guest/{joinCode}/... to isolate storage.
 * For hosts: Returns the original path.
 */
function getGuestStoragePath(originalPath: string): string {
  const isGuest = shareSessionStore.isGuest;
  const joinCode = shareSessionStore.joinCode;
  const usesInMemory = shareSessionStore.usesInMemoryStorage;

  console.log('[WorkspaceCrdtBridge] getGuestStoragePath:', {
    originalPath,
    isGuest,
    joinCode,
    usesInMemory,
    mode: shareSessionStore.mode
  });

  // Hosts don't need path prefixing
  if (!isGuest || !joinCode) {
    return originalPath;
  }

  // Guests using in-memory storage don't need path prefixing
  // (they have their own isolated filesystem)
  if (usesInMemory) {
    console.log('[WorkspaceCrdtBridge] Using original path (in-memory storage):', originalPath);
    return originalPath;
  }

  // Guests using OPFS need path prefixing to isolate their storage
  const guestPath = `guest/${joinCode}/${originalPath}`;
  console.log('[WorkspaceCrdtBridge] Using guest path (OPFS):', guestPath);
  return guestPath;
}

/**
 * Convert a guest storage path back to the canonical path.
 * This strips the guest/{joinCode}/ prefix if present.
 * Used when syncing to Y.Doc to ensure consistent keys across host and guest.
 *
 * For guests using in-memory storage: paths are already canonical (no prefix).
 * For guests using OPFS: strips the guest/{joinCode}/ prefix.
 */
function getCanonicalPath(storagePath: string): string {
  const isGuest = shareSessionStore.isGuest;
  const joinCode = shareSessionStore.joinCode;
  const usesInMemory = shareSessionStore.usesInMemoryStorage;

  if (!isGuest || !joinCode) {
    return storagePath;
  }

  // Guests using in-memory storage don't have prefixes
  if (usesInMemory) {
    return storagePath;
  }

  // Strip guest/{joinCode}/ prefix if present (for OPFS guests)
  const guestPrefix = `guest/${joinCode}/`;
  if (storagePath.startsWith(guestPrefix)) {
    const canonicalPath = storagePath.slice(guestPrefix.length);
    console.log('[WorkspaceCrdtBridge] Converted guest path to canonical:', storagePath, '->', canonicalPath);
    return canonicalPath;
  }

  return storagePath;
}

// Session code for share sessions
let _sessionCode: string | null = null;

/**
 * Process remote updates for guests.
 * Writes all files from Rust CRDT to disk, and deletes files marked as deleted.
 */
async function processRemoteUpdateForGuests(): Promise<void> {
  const isGuest = shareSessionStore.isGuest;
  if (!isGuest || !rustApi || !backendApi) return;

  console.log('[WorkspaceCrdtBridge] Processing remote update for guest');

  // Get ALL files from Rust CRDT including deleted ones
  const allFiles = await rustApi.listFiles(true); // true = include deleted
  for (const [path, metadata] of allFiles) {
    const storagePath = getGuestStoragePath(path);

    if (metadata.deleted) {
      // File was deleted - remove from guest storage
      try {
        const exists = await backendApi.fileExists(storagePath);
        if (exists) {
          console.log('[WorkspaceCrdtBridge] Guest: Deleting file:', storagePath);
          await backendApi.deleteEntry(storagePath);
        }
      } catch (err) {
        console.warn('[WorkspaceCrdtBridge] Guest: Failed to delete file:', storagePath, err);
      }
    } else {
      // File exists - write to disk
      const body = (metadata.extra?.['_body'] as string) ?? '';
      await writeFileWithFrontmatter(storagePath, metadata, body);

      // Notify body change callbacks so editor can reload if needed
      if (body) {
        notifyBodyChange(path, body);
      }
    }
  }

  // Note: UI notifications (notifySessionSync, notifyFileChange) are now handled
  // by FileSystemEvents from the Rust CRDT, avoiding duplicate notifications
}

// Track last known body content to detect echo (our own changes coming back)
const lastKnownBodyContent = new Map<string, string>();

/**
 * Process remote metadata updates for device sync (non-guest hosts).
 *
 * SIMPLIFIED: CRDT is the single source of truth.
 * Flow: CRDT → Files → UI (one direction only)
 *
 * With the BodyDoc architecture, body content is synced via per-file BodySyncBridges.
 * This function handles workspace metadata sync (title, part_of, contents, etc.)
 * and writes updated files to disk.
 */
async function processRemoteMetadataUpdates(): Promise<void> {
  if (!rustApi || !backendApi) return;

  // Skip for guests (handled by processRemoteUpdateForGuests)
  if (shareSessionStore.isGuest) return;

  console.log('[WorkspaceCrdtBridge] Processing remote metadata updates for device sync');

  // Get ALL files from Rust CRDT including deleted ones
  // We need to process deletions as well as updates
  const allFiles = await rustApi.listFiles(true); // true = include deleted
  let updatedCount = 0;
  let deletedCount = 0;

  for (const [path, metadata] of allFiles) {
    if (metadata.deleted) {
      // File was deleted - remove from filesystem
      try {
        const exists = await backendApi.fileExists(path);
        if (exists) {
          console.log('[WorkspaceCrdtBridge] Deleting file from disk:', path);
          await backendApi.deleteEntry(path);
          deletedCount++;
        }
      } catch (err) {
        console.warn('[WorkspaceCrdtBridge] Failed to delete file:', path, err);
      }
    } else {
      // File exists - write to disk with CRDT metadata and body
      let body = '';
      try {
        body = await rustApi.getBodyContent(path);
      } catch {
        body = '';
      }

      await writeFileWithFrontmatter(path, metadata, body);
      updatedCount++;
    }
  }

  if (updatedCount > 0 || deletedCount > 0) {
    console.log(`[WorkspaceCrdtBridge] Synced ${updatedCount} files, deleted ${deletedCount} files`);
  }
}

/**
 * Notify all body change callbacks.
 */
function notifyBodyChange(path: string, body: string): void {
  console.log('[WorkspaceCrdtBridge] Notifying body change:', path, 'callbacks:', bodyChangeCallbacks.size);
  for (const callback of bodyChangeCallbacks) {
    try {
      callback(path, body);
    } catch (error) {
      console.error('[WorkspaceCrdtBridge] Body change callback error:', error);
    }
  }
}

/**
 * Start syncing with a share session.
 * This will connect to the server with the session code.
 * Returns a Promise that resolves when initial sync is complete.
 * @param isHost - If true, sends initial state to server (for session hosts)
 */
export async function startSessionSync(sessionServerUrl: string, sessionCode: string, isHost: boolean = false): Promise<void> {
  console.log('[WorkspaceCrdtBridge] Starting session sync:', sessionCode, 'isHost:', isHost);

  _sessionCode = sessionCode;

  // Disconnect existing bridge if any
  if (syncBridge) {
    syncBridge.destroy();
    syncBridge = null;
  }

  if (!rustApi) {
    console.error('[WorkspaceCrdtBridge] RustCrdtApi not initialized for session');
    return;
  }

  const workspaceDocName = 'workspace';
  console.log('[WorkspaceCrdtBridge] Creating RustSyncBridge for session:', sessionServerUrl, 'doc:', workspaceDocName, 'session:', sessionCode);

  // Create a promise that resolves when initial sync completes
  let syncResolve: () => void;
  const syncPromise = new Promise<void>((resolve) => {
    syncResolve = resolve;
  });

  syncBridge = createRustSyncBridge({
    serverUrl: sessionServerUrl,
    docName: workspaceDocName,
    rustApi: rustApi,
    sessionCode: sessionCode,
    sendInitialState: isHost, // Host sends their state to server
    ownerId: isHost ? getClientOwnerId() : undefined, // Pass ownerId for hosts (read-only enforcement)
    onStatusChange: (connected) => {
      console.log('[WorkspaceCrdtBridge] Session sync status:', connected);
      notifySyncStatus(connected ? 'syncing' : 'idle');
    },
    onSynced: async () => {
      console.log('[WorkspaceCrdtBridge] Session sync complete, isHost:', isHost);
      notifySyncStatus('synced');
      // For guests, write files to disk after initial sync
      if (!isHost) {
        await processRemoteUpdateForGuests();
        // Notify UI to refresh tree (triggers App.svelte to build tree from CRDT)
        notifySessionSync();
      }
      // Resolve the sync promise to signal completion
      syncResolve();
    },
    onRemoteUpdate: async () => {
      // Remote update received - handle based on host/guest mode
      if (shareSessionStore.isGuest) {
        // Guests: write files to disk (in-memory storage)
        await processRemoteUpdateForGuests();
        // Notify UI to refresh tree (triggers App.svelte to rebuild)
        notifySessionSync();
      } else {
        // Hosts: write metadata to disk for device sync
        await processRemoteMetadataUpdates();
        // Notify UI to refresh tree (FileSystemEvents don't fire for sync-originated changes)
        notifyFileChange(null, null);
      }
    },
  });

  await syncBridge.connect();

  // Wait for initial sync to complete (with timeout)
  const timeoutPromise = new Promise<void>((_, reject) => {
    setTimeout(() => reject(new Error('Session sync timeout')), 15000);
  });

  try {
    await Promise.race([syncPromise, timeoutPromise]);
    console.log('[WorkspaceCrdtBridge] Session sync fully complete');
  } catch (error) {
    console.warn('[WorkspaceCrdtBridge] Session sync did not complete in time, continuing anyway');
  }
}

/**
 * Stop syncing with a share session.
 */
export function stopSessionSync(): void {
  console.log('[WorkspaceCrdtBridge] Stopping session sync');

  _sessionCode = null;

  if (syncBridge) {
    syncBridge.destroy();
    syncBridge = null;
  }
}

/**
 * Get the current session code.
 */
export function getSessionCode(): string | null {
  return _sessionCode;
}

/**
 * Get the current workspace ID.
 */
export function getWorkspaceId(): string | null {
  return _workspaceId;
}

/**
 * Check if device-to-device sync is active.
 * This is when we have a syncBridge connected but no live share session.
 */
export function isDeviceSyncActive(): boolean {
  return syncBridge !== null && syncBridge.isSynced();
}

/**
 * Check if workspace is currently initializing.
 */
export function isInitializing(): boolean {
  return _initializing;
}

// ===========================================================================
// Initialization
// ===========================================================================

export interface WorkspaceInitOptions {
  /** Rust CRDT API instance */
  rustApi: RustCrdtApi;
  /** Sync bridge (optional, for sync) */
  syncBridge?: RustSyncBridge;
  /** Server URL (optional) */
  serverUrl?: string;
  /** Workspace ID for room naming */
  workspaceId?: string;
  /** Called when initialization completes */
  onReady?: () => void;
  /** Called when file metadata changes */
  onFileChange?: FileChangeCallback;
}

/**
 * Initialize the workspace CRDT.
 */
export async function initWorkspace(options: WorkspaceInitOptions): Promise<void> {
  if (initialized) {
    console.warn('[WorkspaceCrdtBridge] Already initialized');
    return;
  }

  _initializing = true;

  try {
    rustApi = options.rustApi;
    syncBridge = options.syncBridge ?? null;
    // Keep existing serverUrl if set (from setWorkspaceServer called before init)
    if (options.serverUrl) {
      serverUrl = options.serverUrl;
      // IMPORTANT: Also set _serverUrl which is used by getOrCreateBodyBridge
      _serverUrl = toWebSocketUrl(options.serverUrl);
    }
    _workspaceId = options.workspaceId ?? null;

    console.log('[WorkspaceCrdtBridge] initWorkspace:', {
      workspaceId: _workspaceId,
      serverUrl: serverUrl,
      resolvedServerUrl: _serverUrl,
      hasRustApi: !!rustApi,
    });

    if (options.onFileChange) {
      fileChangeCallbacks.add(options.onFileChange);
    }

    // Connect sync bridge if provided (and workspace server sync is enabled)
    // Only connect if we have a workspaceId (authenticated mode, not local-only)
    if (WORKSPACE_SERVER_SYNC_ENABLED && _workspaceId) {
      if (syncBridge) {
        await syncBridge.connect();
      } else if (serverUrl && rustApi) {
        const workspaceDocName = _workspaceId ? `${_workspaceId}:workspace` : 'workspace';
        const wsUrl = toWebSocketUrl(serverUrl);
        console.log('[WorkspaceCrdtBridge] Creating RustSyncBridge during init:', wsUrl, 'docName:', workspaceDocName);

        syncBridge = createRustSyncBridge({
          serverUrl: wsUrl,
          docName: workspaceDocName,
          rustApi: rustApi,
          authToken: getToken() ?? undefined,
          onStatusChange: (connected) => {
            console.log('[WorkspaceCrdtBridge] Connection status:', connected);
          },
          onSynced: async () => {
            console.log('[WorkspaceCrdtBridge] Initial sync complete');

            // Log files in CRDT after sync for debugging
            try {
              const files = await rustApi!.listFiles(false);
              console.log(`[WorkspaceCrdtBridge] Files in CRDT after sync: ${files.length}`);
              for (const [path] of files) {
                console.log(`[WorkspaceCrdtBridge]   - ${path}`);
              }
            } catch (e) {
              console.warn('[WorkspaceCrdtBridge] Could not list files after sync:', e);
            }

            // With RustSyncBridge, the Rust CRDT is already updated by the sync.
            notifyFileChange(null, null);

            // Update file index from Rust CRDT for persistence
            await updateFileIndexFromCrdt();

            // Mark initial sync complete and notify waiters
            markInitialSyncComplete();
          },
          onRemoteUpdate: async () => {
            // Remote update received - write metadata to disk for device sync
            // UI notifications are handled by FileSystemEvents from the Rust CRDT
            await processRemoteMetadataUpdates();
          },
        });

        await syncBridge.connect();
      }
    } else {
      if (!_workspaceId) {
        console.log('[WorkspaceCrdtBridge] Sync skipped: local-only mode (no workspaceId)');
      } else {
        console.log('[WorkspaceCrdtBridge] Workspace server sync disabled during init');
      }
      // No sync needed - mark as complete immediately
      markInitialSyncComplete();
    }

    initialized = true;
    options.onReady?.();
  } finally {
    _initializing = false;
  }
}


/**
 * Disconnect the workspace sync.
 */
export function disconnectWorkspace(): void {
  syncBridge?.disconnect();
}

/**
 * Reconnect the workspace sync.
 */
export function reconnectWorkspace(): void {
  syncBridge?.connect();
}

/**
 * Destroy the workspace and cleanup.
 */
export async function destroyWorkspace(): Promise<void> {
  // Close all body sync bridges first
  closeAllBodyBridges();

  syncBridge?.destroy();
  syncBridge = null;
  if (workspaceYDoc) {
    workspaceYDoc.destroy();
    workspaceYDoc = null;
  }

  // Clear pending intervals/timeouts to prevent memory leaks
  for (const interval of pendingIntervals) {
    clearInterval(interval);
  }
  pendingIntervals.clear();

  for (const timeout of pendingTimeouts) {
    clearTimeout(timeout);
  }
  pendingTimeouts.clear();

  // Clear file locks
  fileLocks.clear();

  rustApi = null;
  initialized = false;
  fileChangeCallbacks.clear();
}

/**
 * Check if workspace is initialized.
 */
export function isWorkspaceInitialized(): boolean {
  return initialized;
}

/**
 * Check if workspace is connected to server.
 */
export function isWorkspaceConnected(): boolean {
  return syncBridge?.isSynced() ?? false;
}

// ===========================================================================
// File Operations
// ===========================================================================

/**
 * Get file metadata from the CRDT.
 */
export async function getFileMetadata(path: string): Promise<FileMetadata | null> {
  if (!rustApi) return null;
  return rustApi.getFile(path);
}

/**
 * Get all files (excluding deleted).
 */
export async function getAllFiles(): Promise<Map<string, FileMetadata>> {
  if (!rustApi) return new Map();
  const files = await rustApi.listFiles(false);
  return new Map(files);
}

/**
 * Get all files including deleted.
 */
export async function getAllFilesIncludingDeleted(): Promise<Map<string, FileMetadata>> {
  if (!rustApi) return new Map();
  const files = await rustApi.listFiles(true);
  return new Map(files);
}

/**
 * TreeNode interface (matching the backend interface)
 */
interface TreeNode {
  name: string;
  description: string | null;
  path: string;
  children: TreeNode[];
}

/**
 * Populate the CRDT with file metadata.
 * Used before creating a share session to ensure all files are in the CRDT.
 *
 * @param files - Files to add to the CRDT
 * @param externalRustApi - Optional RustCrdtApi to use (for pre-init population)
 */
export async function populateCrdtFromFiles(
  files: Array<{ path: string; metadata: Partial<FileMetadata> }>,
  externalRustApi?: RustCrdtApi
): Promise<void> {
  const api = externalRustApi ?? rustApi;
  if (!api) {
    console.error('[WorkspaceCrdtBridge] Cannot populate CRDT: not initialized');
    return;
  }

  console.log('[WorkspaceCrdtBridge] Populating CRDT with', files.length, 'files');

  for (const { path, metadata } of files) {
    const fullMetadata: FileMetadata = {
      title: metadata.title ?? null,
      part_of: metadata.part_of ?? null,
      contents: metadata.contents ?? null,
      attachments: metadata.attachments ?? [],
      deleted: metadata.deleted ?? false,
      audience: metadata.audience ?? null,
      description: metadata.description ?? null,
      extra: metadata.extra ?? {},
      modified_at: metadata.modified_at ?? BigInt(Date.now()),
    };

    await api.setFile(path, fullMetadata);
  }

  console.log('[WorkspaceCrdtBridge] CRDT population complete with', files.length, 'files');
}

/**
 * Build a tree from CRDT file metadata.
 * This is used for guests who don't have files on disk but have synced metadata.
 *
 * Both hosts and guests read from Rust CRDT - the sync mechanism (RustSyncBridge)
 * updates the Rust CRDT directly, so it contains the synced data for both.
 */
export async function getTreeFromCrdt(): Promise<TreeNode | null> {
  // Both hosts and guests read from Rust CRDT
  // (sync updates Rust CRDT directly via RustSyncBridge)
  if (!rustApi) return null;

  const files = await rustApi.listFiles(false);

  if (files.length === 0) return null;

  const fileMap = new Map(files);
  console.log('[WorkspaceCrdtBridge] Building tree from CRDT, files:', files.map(([p]) => p));

  // Find root files (files with no part_of, or part_of pointing to non-existent file)
  const rootFiles: string[] = [];
  for (const [path, metadata] of fileMap) {
    if (!metadata.part_of || !fileMap.has(metadata.part_of)) {
      rootFiles.push(path);
    }
  }

  console.log('[WorkspaceCrdtBridge] Root files:', rootFiles);

  // If no clear root, use the first file as root
  if (rootFiles.length === 0 && files.length > 0) {
    rootFiles.push(files[0][0]);
  }

  // Build tree recursively
  // For guests, paths are prefixed with guest/{joinCode}/ to point to isolated storage
  function buildNode(originalPath: string): TreeNode {
    const metadata = fileMap.get(originalPath);
    const name = originalPath.split('/').pop()?.replace(/\.md$/, '') || originalPath;

    // For guests, use prefixed path so file opens work correctly
    const storagePath = getGuestStoragePath(originalPath);

    const children: TreeNode[] = [];
    if (metadata?.contents) {
      for (const childPath of metadata.contents) {
        if (fileMap.has(childPath)) {
          children.push(buildNode(childPath));
        }
      }
    }

    return {
      name: metadata?.title || name,
      description: metadata?.description || null,
      path: storagePath,  // Use storage path (prefixed for guests)
      children,
    };
  }

  // If single root, return it; otherwise create a virtual root
  if (rootFiles.length === 1) {
    return buildNode(rootFiles[0]);
  } else {
    // Multiple roots - create a virtual workspace root
    const virtualRootPath = getGuestStoragePath('workspace');
    return {
      name: 'Shared Workspace',
      description: 'Files shared in this session',
      path: virtualRootPath,
      children: rootFiles.map(buildNode),
    };
  }
}

/**
 * Set file metadata in the CRDT.
 */
export async function setFileMetadata(path: string, metadata: FileMetadata): Promise<void> {
  console.log('[WorkspaceCrdtBridge] setFileMetadata called:', path);
  if (!rustApi) {
    throw new Error('Workspace not initialized');
  }

  // Block updates in read-only mode for guests
  if (isReadOnlyBlocked()) {
    console.log('[WorkspaceCrdtBridge] Blocked setFileMetadata in read-only session:', path);
    return;
  }

  await rustApi.setFile(path, metadata);
  console.log('[WorkspaceCrdtBridge] Rust setFile complete, now syncing...');

  // Send changes to server via RustSyncBridge
  // Note: sendLocalChanges() internally checks connection state
  if (syncBridge) {
    await syncBridge.sendLocalChanges();
  }

  console.log('[WorkspaceCrdtBridge] setFileMetadata complete:', path);
  notifyFileChange(path, metadata);
}

/**
 * Sync a single file to the session Y.Doc.
 * This will trigger the SimpleSyncBridge to send the update to the server.
 */
function syncFileToYDoc(path: string, metadata: FileMetadata): void {
  if (!workspaceYDoc || !syncBridge) {
    return; // No session active
  }

  console.log('[WorkspaceCrdtBridge] Syncing file to Y.Doc:', path);
  const filesMap = workspaceYDoc.getMap('files');
  const metadataObj: Record<string, unknown> = {
    title: metadata.title,
    part_of: metadata.part_of,
    contents: metadata.contents,
    attachments: metadata.attachments,
    deleted: metadata.deleted,
    audience: metadata.audience,
    description: metadata.description,
    extra: metadata.extra,
    modified_at: metadata.modified_at ? Number(metadata.modified_at) : null,
  };
  filesMap.set(path, metadataObj);
  console.log('[WorkspaceCrdtBridge] File synced to Y.Doc, total files:', filesMap.size);
}

/**
 * Rename a file in the Y.Doc (remove old path, add new path).
 * This is used when a file is renamed to sync the change to other devices.
 * @param oldPath - The original file path
 * @param newPath - The new file path after rename
 * @param metadata - The file metadata to store at the new path
 */
export function renameFileInYDoc(oldPath: string, newPath: string, metadata: FileMetadata): void {
  if (!workspaceYDoc || !syncBridge) {
    return; // No session active
  }

  console.log('[WorkspaceCrdtBridge] Renaming file in Y.Doc:', oldPath, '->', newPath);
  const filesMap = workspaceYDoc.getMap('files');

  // Remove old path
  filesMap.delete(oldPath);

  // Add new path with metadata
  syncFileToYDoc(newPath, metadata);
}

/**
 * Delete a file in the Y.Doc (mark as deleted with tombstone).
 * This syncs the delete operation to other devices.
 */
export function deleteFileInYDoc(path: string): void {
  if (!workspaceYDoc || !syncBridge) {
    return; // No session active
  }

  console.log('[WorkspaceCrdtBridge] Deleting file in Y.Doc:', path);
  const filesMap = workspaceYDoc.getMap('files');

  // Get existing metadata and mark as deleted (tombstone)
  const existing = filesMap.get(path) as Record<string, unknown> | undefined;
  if (existing) {
    filesMap.set(path, { ...existing, deleted: true, modified_at: Date.now() });
  } else {
    // File not in Y.Doc - create tombstone
    filesMap.set(path, { deleted: true, modified_at: Date.now() });
  }
}

/**
 * Check if updates should be blocked due to read-only mode.
 * Returns true if the user is a guest in a read-only session.
 */
function isReadOnlyBlocked(): boolean {
  return shareSessionStore.isGuest && shareSessionStore.readOnly;
}

/**
 * Update file body content in the workspace Y.Doc.
 * This allows live sync of file content changes during:
 * - Live share sessions (when _sessionCode is set)
 * - Device-to-device sync (when syncBridge is active)
 * Called when the editor saves content.
 */
export function updateFileBodyInYDoc(path: string, body: string): void {
  // Check if we have an active sync mechanism (either session or device sync)
  const hasActiveSync = _sessionCode !== null || (syncBridge !== null && syncBridge.isSynced());

  if (!workspaceYDoc || !hasActiveSync) {
    return; // No sync active
  }

  // Block updates in read-only mode for guests
  if (isReadOnlyBlocked()) {
    console.log('[WorkspaceCrdtBridge] Blocked body update in read-only session:', path);
    return;
  }

  // Convert guest storage path to canonical path for Y.Doc sync
  // This ensures host and guest use the same keys in the files map
  const canonicalPath = getCanonicalPath(path);
  console.log('[WorkspaceCrdtBridge] Updating file body in Y.Doc:', canonicalPath, 'length:', body.length);
  const filesMap = workspaceYDoc.getMap('files');
  const existing = filesMap.get(canonicalPath) as Record<string, unknown> | undefined;

  if (existing) {
    // Preserve existing metadata and update body in extra
    const existingExtra = (existing.extra as Record<string, unknown>) || {};
    const updated = {
      ...existing,
      extra: {
        ...existingExtra,
        _body: body,
      },
      modified_at: Date.now(),
    };
    filesMap.set(canonicalPath, updated);
    console.log('[WorkspaceCrdtBridge] File body updated in Y.Doc');
  } else {
    // File doesn't exist in Y.Doc yet - create minimal entry with body
    console.log('[WorkspaceCrdtBridge] Creating new file entry in Y.Doc with body:', canonicalPath);
    const metadataObj: Record<string, unknown> = {
      title: null,
      part_of: null,
      contents: null,
      attachments: [],
      deleted: false,
      audience: null,
      description: null,
      extra: { _body: body },
      modified_at: Date.now(),
    };
    filesMap.set(canonicalPath, metadataObj);
  }

  // After updating Y.Doc, send changes to sync server
  if (syncBridge) {
    syncBridge.sendLocalChanges().catch((err) => {
      console.error('[WorkspaceCrdtBridge] Failed to send body sync:', err);
    });
  }
}

/**
 * Get or create a body sync bridge for a specific file.
 * Body bridges handle per-file body content sync separately from workspace metadata.
 */
async function getOrCreateBodyBridge(filePath: string): Promise<BodySyncBridge | null> {
  if (!rustApi || !_serverUrl || !_workspaceId) {
    // Missing config - caller should handle local-only mode before calling this
    return null;
  }

  const canonicalPath = getCanonicalPath(filePath);

  // Return existing bridge if available
  const existing = bodyBridges.get(canonicalPath);
  if (existing?.isConnected()) {
    return existing;
  }

  // Destroy disconnected bridge if any
  if (existing) {
    existing.destroy();
    bodyBridges.delete(canonicalPath);
  }

  // CRITICAL: Load body content from disk into CRDT BEFORE connecting sync bridge.
  // Without this, the body CRDT is empty when sync starts, and the server's
  // (possibly empty) state would overwrite local content.
  try {
    const existingBodyContent = await rustApi.getBodyContent(canonicalPath);
    if (existingBodyContent && existingBodyContent.length > 0) {
      // Body CRDT already has content - just track for echo detection
      lastKnownBodyContent.set(canonicalPath, existingBodyContent);
      console.log(`[BodySyncBridge] Body CRDT already has ${existingBodyContent.length} chars for ${canonicalPath}`);
    } else if (backendApi && !shareSessionStore.isGuest) {
      // Body CRDT is empty - try to load content from disk (hosts only)
      // Guests don't have files on disk, so skip this for them
      try {
        console.log(`[BodySyncBridge] Body CRDT empty for ${canonicalPath}, loading from disk...`);
        const entry = await backendApi.getEntry(canonicalPath);
        if (entry && entry.content && entry.content.length > 0) {
          console.log(`[BodySyncBridge] Loading ${entry.content.length} chars from disk into CRDT for ${canonicalPath}`);
          await rustApi.setBodyContent(canonicalPath, entry.content);
          lastKnownBodyContent.set(canonicalPath, entry.content);
        }
      } catch (diskErr) {
        // File might not exist on disk (e.g., remote-only file) - that's OK
        console.log(`[BodySyncBridge] Could not load from disk for ${canonicalPath}:`, diskErr);
      }
    }
  } catch (err) {
    console.warn(`[BodySyncBridge] Could not get body content for ${canonicalPath}:`, err);
  }

  // Create new bridge
  const bridge = createBodySyncBridge({
    serverUrl: _serverUrl,
    workspaceId: _workspaceId,
    filePath: canonicalPath,
    rustApi: rustApi,
    sessionCode: shareSessionStore.joinCode ?? undefined,
    guestId: shareSessionStore.isGuest ? getClientOwnerId() : undefined,
    authToken: getToken() ?? undefined,
    onStatusChange: (connected) => {
      console.log(`[BodySyncBridge] ${canonicalPath} connection:`, connected);
    },
    onSynced: () => {
      console.log(`[BodySyncBridge] ${canonicalPath} synced`);
    },
    onBodyChange: async (content) => {
      // Skip if this matches our last known content (avoid processing our own echoed changes)
      const lastKnown = lastKnownBodyContent.get(canonicalPath);
      if (lastKnown === content) {
        console.log(`[BodySyncBridge] ${canonicalPath} skipping unchanged content`);
        return;
      }

      console.log(`[BodySyncBridge] ${canonicalPath} received remote body update, length:`, content.length);
      // Track and notify UI
      lastKnownBodyContent.set(canonicalPath, content);
      notifyBodyChange(canonicalPath, content);

      // Write body to disk (for non-guests)
      if (!shareSessionStore.isGuest && backendApi) {
        const metadata = await rustApi!.getFile(canonicalPath);
        if (metadata) {
          await writeFileWithFrontmatter(canonicalPath, metadata, content);
        }
      }
    },
  });

  bodyBridges.set(canonicalPath, bridge);
  await bridge.connect();

  return bridge;
}

/**
 * Close body sync for a specific file.
 * Call when the file is no longer being actively edited.
 */
export function closeBodySync(filePath: string): void {
  const canonicalPath = getCanonicalPath(filePath);
  const bridge = bodyBridges.get(canonicalPath);
  if (bridge) {
    console.log(`[BodySyncBridge] Closing sync for: ${canonicalPath}`);
    bridge.destroy();
    bodyBridges.delete(canonicalPath);
  }
}

/**
 * Ensure body sync bridge is connected for a file.
 * Call this when opening a file to receive remote body updates.
 *
 * This eagerly creates the body bridge so that remote body updates
 * are received even before the user starts editing. Without this,
 * files opened from sync would appear empty because the body bridge
 * wasn't created yet.
 */
export async function ensureBodySync(filePath: string): Promise<void> {
  if (!_workspaceId || !_serverUrl || !rustApi) {
    console.log('[WorkspaceCrdtBridge] ensureBodySync skipped - not in sync mode:', {
      hasWorkspaceId: !!_workspaceId,
      hasServerUrl: !!_serverUrl,
      hasRustApi: !!rustApi,
    });
    return;
  }
  const canonicalPath = getCanonicalPath(filePath);
  console.log('[WorkspaceCrdtBridge] ensureBodySync for:', canonicalPath);
  await getOrCreateBodyBridge(canonicalPath);
}

/**
 * Proactively sync body docs for multiple files.
 * Call this after the tree loads to pre-fetch body content for all files,
 * so they're ready when the user opens them.
 *
 * @param filePaths Array of file paths to sync bodies for
 * @param concurrency How many body syncs to run in parallel (default 3)
 */
export async function proactivelySyncBodies(filePaths: string[], concurrency = 3): Promise<void> {
  if (!_workspaceId || !_serverUrl || !rustApi) {
    console.log('[WorkspaceCrdtBridge] proactivelySyncBodies skipped - not in sync mode');
    return;
  }

  console.log(`[WorkspaceCrdtBridge] Proactively syncing ${filePaths.length} body docs with concurrency ${concurrency}`);

  // Process in batches to avoid overwhelming the server
  for (let i = 0; i < filePaths.length; i += concurrency) {
    const batch = filePaths.slice(i, i + concurrency);
    await Promise.all(
      batch.map(async (path) => {
        try {
          const canonicalPath = getCanonicalPath(path);
          // Only sync if not already connected
          if (!bodyBridges.has(canonicalPath)) {
            await getOrCreateBodyBridge(canonicalPath);
          }
        } catch (e) {
          console.warn(`[WorkspaceCrdtBridge] Failed to sync body for ${path}:`, e);
        }
      })
    );
  }

  console.log(`[WorkspaceCrdtBridge] Proactive body sync complete for ${filePaths.length} files`);
}

/**
 * Close all body sync bridges.
 * Call during cleanup/disconnect.
 */
function closeAllBodyBridges(): void {
  console.log(`[BodySyncBridge] Closing all ${bodyBridges.size} bridges`);
  for (const bridge of bodyBridges.values()) {
    bridge.destroy();
  }
  bodyBridges.clear();
}

/**
 * Sync body content through a per-file body sync bridge.
 *
 * This function uses a separate WebSocket connection and Y.Doc per file,
 * preventing large file bodies from bloating the workspace CRDT.
 *
 * @param path - The file path (will be converted to canonical path for sync)
 * @param content - The body content to sync
 */
export async function syncBodyContent(path: string, content: string): Promise<void> {
  if (!rustApi) {
    console.warn('[WorkspaceCrdtBridge] Cannot sync body - rustApi not initialized');
    return;
  }

  // Block updates in read-only mode for guests
  if (isReadOnlyBlocked()) {
    console.log('[WorkspaceCrdtBridge] Blocked body sync in read-only session:', path);
    return;
  }

  const canonicalPath = getCanonicalPath(path);

  try {
    // Track the content we're syncing
    lastKnownBodyContent.set(canonicalPath, content);

    // In local-only mode (no workspaceId), skip server sync entirely
    if (!_workspaceId) {
      await rustApi.setBodyContent(canonicalPath, content);
      return;
    }

    console.log('[WorkspaceCrdtBridge] Syncing body content:', canonicalPath, 'length:', content.length);

    // Get or create body sync bridge for this file
    const bridge = await getOrCreateBodyBridge(canonicalPath);

    if (bridge) {
      // Send body update via dedicated body sync bridge
      await bridge.sendBodyUpdate(content);
      console.log('[WorkspaceCrdtBridge] Body sync via bridge complete:', canonicalPath);
    } else {
      // Fallback: update local body doc without server sync
      await rustApi.setBodyContent(canonicalPath, content);
      console.log('[WorkspaceCrdtBridge] Body stored locally (no server):', canonicalPath);
    }
  } catch (err) {
    console.error('[WorkspaceCrdtBridge] Failed to sync body content:', err);
  }
}

/**
 * Update specific fields of file metadata.
 * Uses a per-file lock to prevent race conditions on concurrent updates.
 */
export async function updateFileMetadata(
  path: string,
  updates: Partial<FileMetadata>
): Promise<void> {
  console.log('[WorkspaceCrdtBridge] updateFileMetadata called:', path, updates);
  if (!rustApi) {
    throw new Error('Workspace not initialized');
  }

  // Block updates in read-only mode for guests
  if (isReadOnlyBlocked()) {
    console.log('[WorkspaceCrdtBridge] Blocked updateFileMetadata in read-only session:', path);
    return;
  }

  // Acquire lock to prevent concurrent read-modify-write races
  const releaseLock = await acquireFileLock(path);

  try {
    const existing = await rustApi.getFile(path);
    const updated: FileMetadata = {
      title: updates.title ?? existing?.title ?? null,
      part_of: updates.part_of ?? existing?.part_of ?? null,
      contents: updates.contents ?? existing?.contents ?? null,
      attachments: updates.attachments ?? existing?.attachments ?? [],
      deleted: updates.deleted ?? existing?.deleted ?? false,
      audience: updates.audience ?? existing?.audience ?? null,
      description: updates.description ?? existing?.description ?? null,
      extra: updates.extra ?? existing?.extra ?? {},
      modified_at: BigInt(Date.now()),
    };

    console.log('[WorkspaceCrdtBridge] Updating file metadata:', path, updated);
    await rustApi.setFile(path, updated);
    console.log('[WorkspaceCrdtBridge] File metadata updated successfully:', path);

    // Send changes to server via RustSyncBridge
    // Note: sendLocalChanges() internally checks connection state
    if (syncBridge) {
      await syncBridge.sendLocalChanges();
    }


    notifyFileChange(path, updated);
  } finally {
    releaseLock();
  }
}

/**
 * Delete a file (soft delete via tombstone).
 */
export async function deleteFile(path: string): Promise<void> {
  await updateFileMetadata(path, { deleted: true });
}

/**
 * Restore a deleted file.
 */
export async function restoreFile(path: string): Promise<void> {
  await updateFileMetadata(path, { deleted: false });
}

/**
 * Permanently remove a file from the CRDT.
 * Note: This sets all fields to null/empty, as CRDTs don't support true deletion.
 */
export async function purgeFile(path: string): Promise<void> {
  if (!rustApi) {
    throw new Error('Workspace not initialized');
  }

  const metadata: FileMetadata = {
    title: null,
    part_of: null,
    contents: null,
    attachments: [],
    deleted: true,
    audience: null,
    description: null,
    extra: {},
    modified_at: BigInt(Date.now()),
  };

  await rustApi.setFile(path, metadata);

  notifyFileChange(path, null);
}

// ===========================================================================
// Hierarchy Operations
// ===========================================================================

/**
 * Add a child to a parent's contents array.
 * Uses locking to prevent race conditions with concurrent modifications.
 */
export async function addToContents(parentPath: string, childPath: string): Promise<void> {
  if (!rustApi) return;

  const releaseLock = await acquireFileLock(parentPath);
  try {
    const parent = await rustApi.getFile(parentPath);
    if (!parent) return;

    const contents = parent.contents ?? [];
    if (!contents.includes(childPath)) {
      contents.push(childPath);
      const updated: FileMetadata = {
        ...parent,
        contents,
        modified_at: BigInt(Date.now()),
      };
      await rustApi.setFile(parentPath, updated);

      // Send changes to server via RustSyncBridge
      if (syncBridge?.isConnected()) {
        await syncBridge.sendLocalChanges();
      }


      notifyFileChange(parentPath, updated);
    }
  } finally {
    releaseLock();
  }
}

/**
 * Remove a child from a parent's contents array.
 * Uses locking to prevent race conditions with concurrent modifications.
 */
export async function removeFromContents(parentPath: string, childPath: string): Promise<void> {
  if (!rustApi) return;

  const releaseLock = await acquireFileLock(parentPath);
  try {
    const parent = await rustApi.getFile(parentPath);
    if (!parent) return;

    const contents = parent.contents ?? [];
    const index = contents.indexOf(childPath);
    if (index !== -1) {
      contents.splice(index, 1);
      const updated: FileMetadata = {
        ...parent,
        contents: contents.length > 0 ? contents : null,
        modified_at: BigInt(Date.now()),
      };
      await rustApi.setFile(parentPath, updated);

      // Send changes to server via RustSyncBridge
      if (syncBridge?.isConnected()) {
        await syncBridge.sendLocalChanges();
      }


      notifyFileChange(parentPath, updated);
    }
  } finally {
    releaseLock();
  }
}

/**
 * Set the part_of (parent) for a file.
 */
export async function setPartOf(childPath: string, parentPath: string | null): Promise<void> {
  await updateFileMetadata(childPath, { part_of: parentPath });
}

/**
 * Move a file to a new parent.
 */
export async function moveFile(
  path: string,
  newParentPath: string,
  newPath: string
): Promise<void> {
  const file = await getFileMetadata(path);
  if (!file) return;

  // Remove from old parent
  if (file.part_of) {
    await removeFromContents(file.part_of, path);
  }

  // Add to new parent
  await addToContents(newParentPath, newPath);

  // Update the file's part_of
  if (path !== newPath) {
    // If path changed, create new entry and delete old
    await setFileMetadata(newPath, { ...file, part_of: newParentPath });
    await purgeFile(path);
  } else {
    await updateFileMetadata(path, { part_of: newParentPath });
  }
}

/**
 * Rename a file (change its path).
 */
export async function renameFile(oldPath: string, newPath: string): Promise<void> {
  const file = await getFileMetadata(oldPath);
  if (!file) return;

  // Create new entry with new path
  await setFileMetadata(newPath, { ...file, modified_at: BigInt(Date.now()) });

  // Update parent's contents
  if (file.part_of) {
    const parent = await getFileMetadata(file.part_of);
    if (parent?.contents) {
      const contents = parent.contents.map((c) => (c === oldPath ? newPath : c));
      await updateFileMetadata(file.part_of, { contents });
    }
  }

  // Delete old entry
  await purgeFile(oldPath);
}

// ===========================================================================
// Attachment Operations
// ===========================================================================

/**
 * Add an attachment to a file.
 * Uses locking to prevent race conditions with concurrent modifications.
 */
export async function addAttachment(filePath: string, attachment: BinaryRef): Promise<void> {
  if (!rustApi) return;

  const releaseLock = await acquireFileLock(filePath);
  try {
    const file = await rustApi.getFile(filePath);
    if (!file) return;

    const attachments = [...file.attachments, attachment];
    const updated: FileMetadata = {
      ...file,
      attachments,
      modified_at: BigInt(Date.now()),
    };
    await rustApi.setFile(filePath, updated);

    // Send changes to server via RustSyncBridge
    if (syncBridge?.isConnected()) {
      await syncBridge.sendLocalChanges();
    }


    notifyFileChange(filePath, updated);
  } finally {
    releaseLock();
  }
}

/**
 * Remove an attachment from a file.
 * Uses locking to prevent race conditions with concurrent modifications.
 */
export async function removeAttachment(filePath: string, attachmentPath: string): Promise<void> {
  if (!rustApi) return;

  const releaseLock = await acquireFileLock(filePath);
  try {
    const file = await rustApi.getFile(filePath);
    if (!file) return;

    const attachments = file.attachments.filter((a) => a.path !== attachmentPath);
    const updated: FileMetadata = {
      ...file,
      attachments,
      modified_at: BigInt(Date.now()),
    };
    await rustApi.setFile(filePath, updated);

    // Send changes to server via RustSyncBridge
    if (syncBridge?.isConnected()) {
      await syncBridge.sendLocalChanges();
    }


    notifyFileChange(filePath, updated);
  } finally {
    releaseLock();
  }
}

/**
 * Get attachments for a file.
 */
export async function getAttachments(filePath: string): Promise<BinaryRef[]> {
  const file = await getFileMetadata(filePath);
  return file?.attachments ?? [];
}

// ===========================================================================
// Sync Operations
// ===========================================================================

/**
 * Save CRDT state to storage.
 */
export async function saveCrdtState(): Promise<void> {
  if (!rustApi) return;
  await rustApi.saveCrdtState('workspace');
}

/**
 * Wait for initial sync to complete.
 *
 * This should be called after initWorkspace() to ensure synced data
 * is available before building the UI tree. Returns immediately if:
 * - Sync is already complete
 * - Sync is not enabled (local-only mode)
 *
 * @param timeoutMs Maximum time to wait for sync (default 10 seconds)
 * @returns true if sync completed, false if timed out or not applicable
 */
export function waitForInitialSync(timeoutMs = 10000): Promise<boolean> {
  return new Promise((resolve) => {
    // Already synced
    if (_initialSyncComplete) {
      resolve(true);
      return;
    }

    // No sync in progress (local-only mode or sync disabled)
    if (!syncBridge && initialized) {
      console.log('[WorkspaceCrdtBridge] waitForInitialSync: no sync bridge, resolving immediately');
      _initialSyncComplete = true;
      resolve(true);
      return;
    }

    // Not initialized yet - wait for it
    if (!initialized && !_initializing) {
      console.log('[WorkspaceCrdtBridge] waitForInitialSync: not initialized, resolving false');
      resolve(false);
      return;
    }

    // Set up timeout
    const timeout = setTimeout(() => {
      console.warn('[WorkspaceCrdtBridge] waitForInitialSync: timed out after', timeoutMs, 'ms');
      // Remove our resolver from the list
      const idx = _initialSyncResolvers.indexOf(resolveSync);
      if (idx >= 0) {
        _initialSyncResolvers.splice(idx, 1);
      }
      resolve(false);
    }, timeoutMs);

    // Add resolver to be called when sync completes
    const resolveSync = () => {
      clearTimeout(timeout);
      resolve(true);
    };
    _initialSyncResolvers.push(resolveSync);
  });
}

/**
 * Mark initial sync as complete and notify any waiters.
 * Called internally when the sync bridge's onSynced callback fires.
 */
function markInitialSyncComplete(): void {
  if (_initialSyncComplete) return;

  console.log('[WorkspaceCrdtBridge] Initial sync complete, notifying', _initialSyncResolvers.length, 'waiters');
  _initialSyncComplete = true;

  // Resolve all waiting promises
  for (const resolve of _initialSyncResolvers) {
    resolve();
  }
  _initialSyncResolvers = [];
}

/**
 * Wait for sync to complete (with timeout).
 */
export function waitForSync(timeoutMs = 5000): Promise<boolean> {
  return new Promise((resolve) => {
    if (isWorkspaceConnected()) {
      resolve(true);
      return;
    }

    const cleanup = () => {
      clearInterval(checkInterval);
      clearTimeout(timeout);
      pendingIntervals.delete(checkInterval);
      pendingTimeouts.delete(timeout);
    };

    const timeout = setTimeout(() => {
      cleanup();
      resolve(false);
    }, timeoutMs);
    pendingTimeouts.add(timeout);

    const checkInterval = setInterval(() => {
      if (isWorkspaceConnected()) {
        cleanup();
        resolve(true);
      }
    }, 100);
    pendingIntervals.add(checkInterval);
  });
}

// ===========================================================================
// Statistics
// ===========================================================================

/**
 * Get workspace statistics.
 */
export async function getWorkspaceStats(): Promise<{
  totalFiles: number;
  activeFiles: number;
  deletedFiles: number;
}> {
  const allFiles = await getAllFilesIncludingDeleted();
  const activeFiles = await getAllFiles();

  return {
    totalFiles: allFiles.size,
    activeFiles: activeFiles.size,
    deletedFiles: allFiles.size - activeFiles.size,
  };
}

// ===========================================================================
// Callbacks
// ===========================================================================

/**
 * Subscribe to file changes.
 */
export function onFileChange(callback: FileChangeCallback): () => void {
  fileChangeCallbacks.add(callback);
  return () => fileChangeCallbacks.delete(callback);
}

/**
 * Subscribe to session sync events.
 * Called when session data is received and synced to Rust.
 * Use this to trigger UI refreshes after receiving data from a share session.
 */
export function onSessionSync(callback: SessionSyncCallback): () => void {
  sessionSyncCallbacks.add(callback);
  return () => sessionSyncCallbacks.delete(callback);
}

/**
 * Subscribe to file body changes.
 * Called when a file's body content changes remotely (from another session participant).
 * Use this to reload the editor when the current file's content changes.
 *
 * @param callback - Receives the canonical path and new body content
 * @returns Unsubscribe function
 */
export function onBodyChange(callback: BodyChangeCallback): () => void {
  bodyChangeCallbacks.add(callback);
  return () => bodyChangeCallbacks.delete(callback);
}

/**
 * Subscribe to sync progress updates.
 * Called when files are being synced to report progress.
 *
 * @param callback - Receives (completed, total) file counts
 * @returns Unsubscribe function
 */
export function onSyncProgress(callback: SyncProgressCallback): () => void {
  syncProgressCallbacks.add(callback);
  return () => syncProgressCallbacks.delete(callback);
}

/**
 * Subscribe to sync status changes.
 * Called when sync status changes (idle, connecting, syncing, synced, error).
 *
 * @param callback - Receives the new status and optional error message
 * @returns Unsubscribe function
 */
export function onSyncStatus(callback: SyncStatusCallback): () => void {
  syncStatusCallbacks.add(callback);
  return () => syncStatusCallbacks.delete(callback);
}

/**
 * Notify all sync status callbacks.
 */
function notifySyncStatus(status: 'idle' | 'connecting' | 'syncing' | 'synced' | 'error', error?: string): void {
  console.log('[WorkspaceCrdtBridge] Notifying sync status:', status, error ? `(${error})` : '');
  for (const callback of syncStatusCallbacks) {
    try {
      callback(status, error);
    } catch (err) {
      console.error('[WorkspaceCrdtBridge] Sync status callback error:', err);
    }
  }
}

/**
 * Notify all session sync callbacks.
 */
function notifySessionSync(): void {
  console.log('[WorkspaceCrdtBridge] Notifying session sync callbacks, count:', sessionSyncCallbacks.size);
  for (const callback of sessionSyncCallbacks) {
    try {
      callback();
    } catch (error) {
      console.error('[WorkspaceCrdtBridge] Session sync callback error:', error);
    }
  }
}

// Private helpers

function notifyFileChange(path: string | null, metadata: FileMetadata | null): void {
  for (const callback of fileChangeCallbacks) {
    try {
      callback(path, metadata);
    } catch (error) {
      console.error('[WorkspaceCrdtBridge] File change callback error:', error);
    }
  }
}

/**
 * Update the file index from Rust CRDT state.
 * Called after remote sync updates to keep the SQLite index in sync.
 */
/**
 * Update the SQLite file index from the CRDT state.
 * Includes retry logic for when storage isn't ready yet.
 *
 * @param retryCount Number of retries remaining (default 3)
 * @param retryDelayMs Delay between retries in ms (default 500, doubles each retry)
 */
async function updateFileIndexFromCrdt(retryCount = 3, retryDelayMs = 500): Promise<void> {
  if (!rustApi) return;

  // Check if SQLite storage is initialized
  if (!isStorageReady()) {
    if (retryCount > 0) {
      console.log(`[WorkspaceCrdtBridge] Storage not ready, retrying file index update in ${retryDelayMs}ms (${retryCount} retries left)`);
      setTimeout(() => {
        updateFileIndexFromCrdt(retryCount - 1, retryDelayMs * 2);
      }, retryDelayMs);
    } else {
      console.warn('[WorkspaceCrdtBridge] Storage not ready after retries, skipping file index update');
    }
    return;
  }

  try {
    const files = await rustApi.listFiles(true); // Include deleted to update tombstones
    console.log(`[WorkspaceCrdtBridge] Updating file index with ${files.length} files`);
    for (const [path, metadata] of files) {
      if (metadata) {
        crdt_update_file_index(
          path,
          metadata.title ?? null,
          metadata.part_of ?? null,
          metadata.deleted ?? false,
          Number(metadata.modified_at ?? Date.now())
        );
      }
    }
  } catch (err) {
    console.error('[WorkspaceCrdtBridge] Failed to update file index:', err);
  }
}

// ===========================================================================
// Filesystem Event Subscription
// ===========================================================================

// Active filesystem event subscription ID
let fsEventSubscriptionId: number | null = null;

/**
 * Initialize filesystem event subscription from the Rust backend.
 *
 * This subscribes to events emitted by the decorated filesystem layer
 * (EventEmittingFs/CrdtFs) and uses them to update the UI and CRDT state.
 *
 * Call this after the backend is initialized to enable automatic
 * UI updates when filesystem operations occur.
 *
 * @param backend The backend instance to subscribe to
 * @returns Cleanup function to unsubscribe
 */
export function initEventSubscription(backend: Backend): () => void {
  // Skip if backend doesn't support filesystem events
  if (!backend.onFileSystemEvent) {
    console.log('[WorkspaceCrdtBridge] Backend does not support filesystem events');
    return () => {};
  }

  // Unsubscribe from any existing subscription
  if (fsEventSubscriptionId !== null && backend.offFileSystemEvent) {
    backend.offFileSystemEvent(fsEventSubscriptionId);
  }

  // Subscribe to filesystem events
  fsEventSubscriptionId = backend.onFileSystemEvent((event: FileSystemEvent) => {
    handleFileSystemEvent(event);
  });

  console.log('[WorkspaceCrdtBridge] Subscribed to filesystem events, id:', fsEventSubscriptionId);

  // Return cleanup function
  return () => {
    if (fsEventSubscriptionId !== null && backend.offFileSystemEvent) {
      backend.offFileSystemEvent(fsEventSubscriptionId);
      fsEventSubscriptionId = null;
      console.log('[WorkspaceCrdtBridge] Unsubscribed from filesystem events');
    }
  };
}

/**
 * Handle a filesystem event from the Rust backend.
 *
 * This function processes events and triggers appropriate UI updates
 * and CRDT synchronization.
 */
function handleFileSystemEvent(event: FileSystemEvent): void {
  console.log('[WorkspaceCrdtBridge] Received filesystem event:', event.type, event);

  switch (event.type) {
    case 'FileCreated':
      // New file created - notify UI
      notifyFileChange(event.path, event.frontmatter ? (event.frontmatter as FileMetadata) : null);
      // Trigger tree refresh for all users (guests via session sync, hosts via file change with null path)
      if (shareSessionStore.isGuest) {
        notifySessionSync();
      } else {
        // For hosts: trigger tree refresh by calling with null path
        notifyFileChange(null, null);
      }
      break;

    case 'FileDeleted':
      // File deleted - notify UI with null metadata
      notifyFileChange(event.path, null);
      // Trigger tree refresh for all users
      if (shareSessionStore.isGuest) {
        notifySessionSync();
      } else {
        notifyFileChange(null, null);
      }
      break;

    case 'FileRenamed':
      // File renamed - notify both old and new paths
      notifyFileChange(event.old_path, null);
      notifyFileChange(event.new_path, null);
      // Trigger tree refresh for all users
      if (shareSessionStore.isGuest) {
        notifySessionSync();
      } else {
        notifyFileChange(null, null);
      }
      break;

    case 'FileMoved':
      // File moved - notify the new path
      notifyFileChange(event.path, null);
      // Trigger tree refresh for all users
      if (shareSessionStore.isGuest) {
        notifySessionSync();
      } else {
        notifyFileChange(null, null);
      }
      break;

    case 'MetadataChanged':
      // Metadata changed - notify with new frontmatter
      notifyFileChange(event.path, event.frontmatter as FileMetadata);
      break;

    case 'ContentsChanged':
      // Body content changed - notify body change callbacks
      notifyBodyChange(event.path, event.body);
      break;
  }
}

/**
 * Check if filesystem event subscription is active.
 */
export function isEventSubscriptionActive(): boolean {
  return fsEventSubscriptionId !== null;
}

// ===========================================================================
// Debug
// ===========================================================================

/**
 * Debug function to check Hocuspocus sync state.
 * Call this from browser console: window.debugSync()
 */
export function debugSync(): void {
  console.log('=== Sync Debug ===');
  console.log('serverUrl:', serverUrl);
  console.log('syncBridge:', syncBridge ? 'exists' : 'null');
  console.log('syncBridge.synced:', syncBridge?.isSynced());
  console.log('initialized:', initialized);
  console.log('rustApi:', rustApi ? 'exists' : 'null');
  console.log('workspaceYDoc:', workspaceYDoc ? 'exists' : 'null');

  if (workspaceYDoc) {
    const filesMap = workspaceYDoc.getMap('files');
    console.log('workspaceYDoc files count:', filesMap.size);
  }

  if (rustApi) {
    console.log('Fetching Rust CRDT state...');
    rustApi.getFullState('workspace').then(fullState => {
      console.log('Rust CRDT full state:', fullState.length, 'bytes');
      return rustApi!.listFiles(false);
    }).then(files => {
      console.log('Rust CRDT files count:', files.length);
      console.log('Rust CRDT files:', files.map(([path]) => path));
    }).catch(e => {
      console.error('Error getting Rust state:', e);
    });
  }
  console.log('=== End Debug ===');
}

// Legacy alias for backwards compatibility
export const debugHocuspocusSync = debugSync;

// Expose debug function globally for browser console
if (typeof window !== 'undefined') {
  (window as any).debugSync = debugSync;
}

// Re-export types
export type { FileMetadata, BinaryRef };
