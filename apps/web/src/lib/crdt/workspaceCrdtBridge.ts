/**
 * Workspace CRDT Bridge - replaces workspaceCrdt.ts with Rust CRDT backend.
 *
 * This module provides the same API surface as the original workspaceCrdt.ts
 * but delegates all operations to the Rust CRDT via RustCrdtApi.
 *
 * Supports both:
 * - Hocuspocus server-based sync
 * - P2P sync via y-webrtc (no server required)
 */

import * as Y from 'yjs';
import type { RustCrdtApi } from './rustCrdtApi';
import { HocuspocusBridge, createHocuspocusBridge } from './hocuspocusBridge';
import type { FileMetadata, BinaryRef } from '../backend/generated';
import {
  isP2PEnabled,
  createP2PProvider,
  destroyP2PProvider,
} from './p2pSyncBridge';
import type { WebrtcProvider } from 'y-webrtc';

// State
let rustApi: RustCrdtApi | null = null;
let syncBridge: HocuspocusBridge | null = null;
let p2pProvider: WebrtcProvider | null = null;
let workspaceYDoc: Y.Doc | null = null;
let serverUrl: string | null = null;
let _workspaceId: string | null = null;
let initialized = false;
let _initializing = false;

// Track last state vector for incremental sync
let lastStateVector: Uint8Array | null = null;

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
type FileChangeCallback = (path: string, metadata: FileMetadata | null) => void;
const fileChangeCallbacks = new Set<FileChangeCallback>();

// ===========================================================================
// Configuration
// ===========================================================================

/**
 * Set the Hocuspocus server URL for workspace sync.
 * Creates and connects a HocuspocusBridge if the URL is set.
 */
export async function setWorkspaceServer(url: string | null): Promise<void> {
  const previousUrl = serverUrl;
  serverUrl = url;

  // Skip if URL hasn't changed or not initialized
  if (previousUrl === url || !initialized || !rustApi) {
    return;
  }

  // Disconnect existing bridge if any
  if (syncBridge) {
    console.log('[WorkspaceCrdtBridge] Disconnecting existing Hocuspocus bridge');
    syncBridge.destroy();
    syncBridge = null;
  }

  // Create new bridge if URL is set
  if (url) {
    console.log('[WorkspaceCrdtBridge] Creating Hocuspocus bridge for workspace:', url);

    syncBridge = createHocuspocusBridge({
      url,
      docName: 'workspace',
      rustApi,
      onStatusChange: (connected) => {
        console.log('[WorkspaceCrdtBridge] Hocuspocus connection status:', connected);
      },
      onSynced: () => {
        console.log('[WorkspaceCrdtBridge] Hocuspocus sync complete');
      },
      onUpdate: async (update) => {
        // Remote update received from server - notify file change callbacks
        console.log('[WorkspaceCrdtBridge] Remote update received from Hocuspocus:', update.length, 'bytes');
        // Refresh file list to pick up any changes
        try {
          const files = await rustApi!.listFiles(false);
          for (const [path, metadata] of files) {
            notifyFileChange(path, metadata);
          }
        } catch (error) {
          console.error('[WorkspaceCrdtBridge] Failed to refresh files after remote update:', error);
        }
      },
    });

    await syncBridge.connect();
  }
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
  _workspaceId = id;
}

/**
 * Get the current workspace ID.
 */
export function getWorkspaceId(): string | null {
  return _workspaceId;
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
  /** Hocuspocus bridge (optional, for sync) */
  syncBridge?: HocuspocusBridge;
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
    }
    _workspaceId = options.workspaceId ?? null;

    if (options.onFileChange) {
      fileChangeCallbacks.add(options.onFileChange);
    }

    // Connect sync bridge if provided
    if (syncBridge) {
      await syncBridge.connect();
    } else if (serverUrl && rustApi) {
      // Create Hocuspocus bridge if serverUrl was set (either from options or prior setWorkspaceServer call)
      const workspaceDocName = _workspaceId ? `${_workspaceId}:workspace` : 'workspace';
      console.log('[WorkspaceCrdtBridge] Creating Hocuspocus bridge during init:', serverUrl, 'docName:', workspaceDocName);
      syncBridge = createHocuspocusBridge({
        url: serverUrl,
        docName: workspaceDocName,
        rustApi,
        onStatusChange: (connected) => {
          console.log('[WorkspaceCrdtBridge] Hocuspocus connection status:', connected);
        },
        onSynced: () => {
          console.log('[WorkspaceCrdtBridge] Hocuspocus sync complete');
        },
        onUpdate: async (update) => {
          console.log('[WorkspaceCrdtBridge] Remote update received from Hocuspocus:', update.length, 'bytes');
          try {
            const files = await rustApi!.listFiles(false);
            for (const [path, metadata] of files) {
              notifyFileChange(path, metadata);
            }
          } catch (error) {
            console.error('[WorkspaceCrdtBridge] Failed to refresh files after remote update:', error);
          }
        },
      });
      await syncBridge.connect();
    }

    // Initialize P2P if enabled
    await initP2PWorkspaceSync();

    initialized = true;
    options.onReady?.();
  } finally {
    _initializing = false;
  }
}

/**
 * Initialize P2P sync for workspace.
 * Creates a Y.Doc that syncs workspace state via WebRTC.
 */
async function initP2PWorkspaceSync(): Promise<void> {
  if (!isP2PEnabled() || !rustApi) {
    return;
  }

  // Create workspace Y.Doc for P2P sync
  workspaceYDoc = new Y.Doc();

  // Load initial state from Rust CRDT
  try {
    const fullState = await rustApi.getFullState('workspace');
    console.log('[WorkspaceCrdtBridge] Loading initial state from Rust, bytes:', fullState.length);
    if (fullState.length > 0) {
      Y.applyUpdate(workspaceYDoc, fullState, 'rust');
      // Log what's in the Y.Doc after loading
      const filesMap = workspaceYDoc.getMap('files');
      console.log('[WorkspaceCrdtBridge] Loaded Y.Doc has', filesMap.size, 'files');
    }
    // Store initial state vector for incremental sync
    lastStateVector = await rustApi.getSyncState('workspace');
    console.log('[WorkspaceCrdtBridge] Initial state vector stored, bytes:', lastStateVector.length);
  } catch (error) {
    console.warn('[WorkspaceCrdtBridge] Failed to load workspace state for P2P:', error);
  }

  // Create P2P provider
  // For P2P sync, use just 'workspace' as docName - the sync code already uniquely identifies the room.
  // Including workspace ID would prevent sync since each device has a different workspace ID.
  p2pProvider = createP2PProvider(workspaceYDoc, 'workspace');

  if (p2pProvider) {
    console.log('[WorkspaceCrdtBridge] P2P provider created for workspace');
    console.log('[WorkspaceCrdtBridge] Provider connected:', p2pProvider.connected);
    console.log('[WorkspaceCrdtBridge] Provider synced:', (p2pProvider as any).synced);

    // Log when provider syncs
    p2pProvider.on('synced', (event: { synced: boolean }) => {
      console.log('[WorkspaceCrdtBridge] Provider synced event:', event);
    });

    // Sync Y.Doc changes back to Rust CRDT
    workspaceYDoc.on('update', async (update: Uint8Array, origin: unknown) => {
      // Log ALL updates with their origin to see if peer updates arrive
      const originStr = origin === p2pProvider ? 'p2p-provider' : String(origin);
      console.log('[WorkspaceCrdtBridge] Y.Doc update received, origin:', originStr, 'bytes:', update.length);
      if (origin === 'rust' || !rustApi || !workspaceYDoc) return;

      try {
        console.log('[WorkspaceCrdtBridge] Applying remote update to Rust CRDT...');
        await rustApi.applyRemoteUpdate(update, 'workspace');
        console.log('[WorkspaceCrdtBridge] Remote update applied successfully');
        // Log what's in the Y.Doc after update
        const filesMap = workspaceYDoc.getMap('files');
        console.log('[WorkspaceCrdtBridge] Y.Doc now has', filesMap.size, 'files');
      } catch (error) {
        console.error('[WorkspaceCrdtBridge] Failed to sync P2P update to Rust:', error);
      }
    });
  }
}

/**
 * Refresh P2P sync for workspace.
 * Call this after enabling/disabling P2P.
 */
export async function refreshWorkspaceP2P(): Promise<void> {
  // Cleanup existing P2P
  if (p2pProvider) {
    destroyP2PProvider('workspace');
    p2pProvider = null;
  }
  if (workspaceYDoc) {
    workspaceYDoc.destroy();
    workspaceYDoc = null;
  }
  lastStateVector = null;

  // Reinitialize if P2P is enabled
  if (isP2PEnabled() && initialized) {
    await initP2PWorkspaceSync();
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
  syncBridge?.destroy();
  syncBridge = null;

  // Cleanup P2P
  if (p2pProvider) {
    destroyP2PProvider('workspace');
    p2pProvider = null;
  }
  if (workspaceYDoc) {
    workspaceYDoc.destroy();
    workspaceYDoc = null;
  }
  lastStateVector = null;

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
 * Check if workspace is connected (via server or P2P).
 */
export function isWorkspaceConnected(): boolean {
  // Check Hocuspocus connection
  if (syncBridge?.isSynced()) {
    return true;
  }
  // Check P2P connection
  if (p2pProvider?.connected) {
    return true;
  }
  return false;
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
 * Set file metadata in the CRDT.
 */
export async function setFileMetadata(path: string, metadata: FileMetadata): Promise<void> {
  console.log('[WorkspaceCrdtBridge] setFileMetadata called:', path);
  if (!rustApi) {
    throw new Error('Workspace not initialized');
  }
  await rustApi.setFile(path, metadata);
  console.log('[WorkspaceCrdtBridge] Rust setFile complete, now syncing to P2P...');
  // Sync to P2P Y.Doc so peers receive the update
  await syncRustChangesToP2P();
  console.log('[WorkspaceCrdtBridge] setFileMetadata complete:', path);
  notifyFileChange(path, metadata);
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
    // Sync to P2P Y.Doc so peers receive the update
    await syncRustChangesToP2P();
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
  // Sync to P2P Y.Doc so peers receive the update
  await syncRustChangesToP2P();
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
      // Sync to P2P Y.Doc so peers receive the update
      await syncRustChangesToP2P();
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
      // Sync to P2P Y.Doc so peers receive the update
      await syncRustChangesToP2P();
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
    // Sync to P2P Y.Doc so peers receive the update
    await syncRustChangesToP2P();
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
    // Sync to P2P Y.Doc so peers receive the update
    await syncRustChangesToP2P();
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

// Private helpers

/**
 * Sync Rust CRDT changes to P2P Y.Doc.
 * Call this after any operation that modifies the Rust workspace CRDT.
 * This ensures P2P peers receive the update.
 */
async function syncRustChangesToP2P(): Promise<void> {
  console.log('[WorkspaceCrdtBridge] syncRustChangesToP2P called, hasYDoc:', !!workspaceYDoc, 'hasProvider:', !!p2pProvider);

  if (!workspaceYDoc || !rustApi || !p2pProvider) {
    console.log('[WorkspaceCrdtBridge] syncRustChangesToP2P skipped: missing dependencies');
    return;
  }

  try {
    // Get updates since last known state
    if (lastStateVector) {
      console.log('[WorkspaceCrdtBridge] Getting missing updates, lastStateVector bytes:', lastStateVector.length);
      const missingUpdates = await rustApi.getMissingUpdates(lastStateVector, 'workspace');
      console.log('[WorkspaceCrdtBridge] getMissingUpdates returned', missingUpdates.length, 'bytes');
      if (missingUpdates.length > 0) {
        console.log('[WorkspaceCrdtBridge] Syncing', missingUpdates.length, 'bytes to P2P Y.Doc');
        // Apply with 'rust' origin to prevent the update listener from syncing back
        Y.applyUpdate(workspaceYDoc, missingUpdates, 'rust');
        const filesMap = workspaceYDoc.getMap('files');
        console.log('[WorkspaceCrdtBridge] P2P Y.Doc now has', filesMap.size, 'files');
      } else {
        console.log('[WorkspaceCrdtBridge] No new updates from Rust CRDT');
      }
    } else {
      // No previous state, get full state
      console.log('[WorkspaceCrdtBridge] No lastStateVector, getting full state');
      const fullState = await rustApi.getFullState('workspace');
      console.log('[WorkspaceCrdtBridge] getFullState returned', fullState.length, 'bytes');
      if (fullState.length > 0) {
        console.log('[WorkspaceCrdtBridge] Syncing full state to P2P Y.Doc, bytes:', fullState.length);
        Y.applyUpdate(workspaceYDoc, fullState, 'rust');
        const filesMap = workspaceYDoc.getMap('files');
        console.log('[WorkspaceCrdtBridge] P2P Y.Doc now has', filesMap.size, 'files');
      }
    }

    // Update state vector for next incremental sync
    const newStateVector = await rustApi.getSyncState('workspace');
    console.log('[WorkspaceCrdtBridge] Updated lastStateVector, bytes:', newStateVector.length);
    lastStateVector = newStateVector;
  } catch (error) {
    console.error('[WorkspaceCrdtBridge] Failed to sync Rust changes to P2P:', error);
  }
}

function notifyFileChange(path: string, metadata: FileMetadata | null): void {
  for (const callback of fileChangeCallbacks) {
    try {
      callback(path, metadata);
    } catch (error) {
      console.error('[WorkspaceCrdtBridge] File change callback error:', error);
    }
  }
}

// ===========================================================================
// Debug
// ===========================================================================

/**
 * Debug function to check P2P sync state.
 * Call this from browser console: window.debugP2PSync()
 */
export async function debugP2PSync(): Promise<void> {
  console.log('=== P2P Sync Debug ===');
  console.log('workspaceYDoc:', workspaceYDoc ? 'exists' : 'null');
  console.log('p2pProvider:', p2pProvider ? 'exists' : 'null');
  console.log('p2pProvider.connected:', p2pProvider?.connected);
  console.log('p2pProvider.synced:', (p2pProvider as any)?.synced);
  console.log('rustApi:', rustApi ? 'exists' : 'null');
  console.log('lastStateVector:', lastStateVector ? `${lastStateVector.length} bytes` : 'null');

  if (p2pProvider) {
    // Access internal state for debugging
    const providerAny = p2pProvider as any;
    console.log('p2pProvider.room:', providerAny.room ? 'exists' : 'null');
    if (providerAny.room) {
      console.log('p2pProvider.room.webrtcConns size:', providerAny.room.webrtcConns?.size);
      console.log('p2pProvider.room.bcConns size:', providerAny.room.bcConns?.size);
    }
  }

  if (workspaceYDoc) {
    const filesMap = workspaceYDoc.getMap('files');
    console.log('workspaceYDoc files count:', filesMap.size);
    console.log('workspaceYDoc files:', Array.from(filesMap.keys()));

    // Log Y.Doc state
    const stateVector = Y.encodeStateVector(workspaceYDoc);
    console.log('workspaceYDoc stateVector:', stateVector.length, 'bytes');
  }

  if (rustApi) {
    try {
      const fullState = await rustApi.getFullState('workspace');
      console.log('Rust CRDT full state:', fullState.length, 'bytes');
      const files = await rustApi.listFiles(false);
      console.log('Rust CRDT files count:', files.length);
      console.log('Rust CRDT files:', files.map(([path]) => path));
    } catch (e) {
      console.error('Error getting Rust state:', e);
    }
  }
  console.log('=== End Debug ===');
}

/**
 * Debug function to check Hocuspocus sync state.
 * Call this from browser console: window.debugHocuspocusSync()
 */
export function debugHocuspocusSync(): void {
  console.log('=== Hocuspocus Sync Debug ===');
  console.log('serverUrl:', serverUrl);
  console.log('syncBridge:', syncBridge ? 'exists' : 'null');
  console.log('syncBridge.status:', syncBridge?.getStatus());
  console.log('syncBridge.synced:', syncBridge?.isSynced());
  console.log('initialized:', initialized);
  console.log('rustApi:', rustApi ? 'exists' : 'null');
  console.log('=== End Debug ===');
}

// Expose debug functions globally for browser console
if (typeof window !== 'undefined') {
  (window as any).debugP2PSync = debugP2PSync;
  (window as any).debugHocuspocusSync = debugHocuspocusSync;
}

// Re-export types
export type { FileMetadata, BinaryRef };
