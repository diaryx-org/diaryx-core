/**
 * Workspace CRDT Bridge - replaces workspaceCrdt.ts with Rust CRDT backend.
 *
 * This module provides the same API surface as the original workspaceCrdt.ts
 * but delegates all operations to the Rust CRDT via RustCrdtApi.
 *
 * ## Doc-ID Based Architecture
 *
 * Files are keyed by stable document IDs (UUIDs) rather than file paths.
 * This makes renames trivial property updates rather than delete+create operations.
 *
 * Key changes:
 * - `bodyBridges` are keyed by doc_id (stable across renames)
 * - Use `getPathForDocId()` to derive filesystem paths
 * - Use `findDocIdByPath()` to look up doc_ids from paths
 * - The `filename` field in FileMetadata contains the filename on disk
 *
 * Supports Hocuspocus server-based sync for device-to-device synchronization.
 */

import type { RustCrdtApi } from './rustCrdtApi';
import { SyncTransport, createWorkspaceSyncTransport } from './syncTransport';
import { MultiplexedBodySync } from './multiplexedBodySync';
import type { WasmSyncBridge } from './wasmSyncBridge';
import { UnifiedSyncTransport, createUnifiedSyncTransport } from './unifiedSyncTransport';
import type { FileMetadata, BinaryRef } from '../backend/generated';
import type { Backend, FileSystemEvent, SyncEvent } from '../backend/interface';
import { crdt_update_file_index, isStorageReady } from '$lib/storage/sqliteStorageBridge.js';
import type { Api } from '../backend/api';
import { shareSessionStore } from '@/models/stores/shareSessionStore.svelte';
import { collaborationStore } from '@/models/stores/collaborationStore.svelte';
import { getToken } from '$lib/auth/authStore.svelte';
// New Rust sync helpers - progressively replacing TypeScript implementations
import * as syncHelpers from './syncHelpers';

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

/**
 * Convert an HTTP URL to v2 WebSocket URL (/sync2).
 */
function toWebSocketUrlV2(httpUrl: string): string {
  return httpUrl
    .replace(/^https:\/\//, 'wss://')
    .replace(/^http:\/\//, 'ws://')
    .replace(/\/sync$/, '')
    .replace(/\/$/, '')
    + '/sync2';
}

/**
 * Check if a path refers to a temporary file that should not be synced.
 */
function isTempFile(path: string): boolean {
  return path.endsWith('.tmp') || path.endsWith('.bak') || path.endsWith('.swap');
}

// Cross-module singleton support.
// Vite dev server may create separate module instances when the same file is
// imported via different paths (e.g., "$lib/crdt/workspaceCrdtBridge" from app
// code vs "/src/lib/crdt/workspaceCrdtBridge" from page.evaluate in tests).
// We register key functions on globalThis so unconfigured module instances
// can delegate to the configured one.
const _g = globalThis as any;
if (!_g.__diaryx_bridge) {
  _g.__diaryx_bridge = {} as Record<string, Function>;
}

/** Register this module instance as the configured bridge on globalThis. */
function registerBridgeOnGlobal(): void {
  _g.__diaryx_bridge.ensureBodySync = _ensureBodySyncImpl;
  _g.__diaryx_bridge.getBodyContentFromCrdt = _getBodyContentFromCrdtImpl;
  _g.__diaryx_bridge.getFileMetadata = _getFileMetadataImpl;
}

// State
let rustApi: RustCrdtApi | null = null;
let syncBridge: SyncTransport | null = null;
let backendApi: Api | null = null;
let _backend: Backend | null = null;

// Native sync state (Tauri only)
let _nativeSyncActive = false;
let _nativeSyncUnsubscribe: (() => void) | null = null;

// WASM unified sync bridge (new architecture)
let wasmSyncBridge: WasmSyncBridge | null = null;

let serverUrl: string | null = null;
let _workspaceId: string | null = null;
let initialized = false;
let _initializing = false;

// Initial sync tracking - allows waiting for first sync to complete
let _initialSyncComplete = false;
let _initialSyncResolvers: Array<() => void> = [];

// Multiplexed body sync transport (single WebSocket for all body syncs)
let multiplexedBodySync: MultiplexedBodySync | null = null;

// Unified sync v2 transport (single WebSocket for workspace + body)
let unifiedSyncTransport: UnifiedSyncTransport | null = null;

// Legacy per-file body sync bridges (kept for fallback/migration)
// Will be removed once multiplexed sync is stable
const bodyBridges = new Map<string, SyncTransport>();

// Lock for pending body bridge creations to prevent race conditions
const bodyBridgePendingCreation = new Map<string, Promise<void>>();

// Cached server URL for body bridges
let _serverUrl: string | null = null;

// Pending body sync requests when sync config isn't ready yet
const pendingBodySync = new Set<string>();

// Flag: true when this client loaded from server (load_server mode).
// When set, body CRDTs are cleared before sync to prevent duplication
// (importFromZip populates body CRDT locally, then server sends the same content).
let _freshFromServerLoad = false;

export function setFreshFromServerLoad(value: boolean): void {
  _freshFromServerLoad = value;
}

export function isFreshFromServerLoad(): boolean {
  return _freshFromServerLoad;
}

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

async function flushPendingBodySync(reason: string): Promise<void> {
  if (pendingBodySync.size === 0) return;
  if (!_backend || _backend?.hasNativeSync?.()) return;
  if (!_workspaceId || !_serverUrl || !rustApi) return;

  const files = Array.from(pendingBodySync);
  pendingBodySync.clear();
  console.log(
    `[WorkspaceCrdtBridge] Flushing ${files.length} pending body sync request(s) (${reason})`,
  );

  for (const filePath of files) {
    try {
      await getOrCreateBodyBridge(filePath, true);
    } catch (err) {
      console.warn(
        `[WorkspaceCrdtBridge] Failed to flush pending body sync for ${filePath}:`,
        err,
      );
      pendingBodySync.add(filePath);
    }
  }
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
 * For Tauri: Uses native Rust sync client for better performance.
 * For WASM/web: Creates and connects a SyncTransport.
 *
 * IMPORTANT: setBackend() must be called before this function.
 * If backend is null, sync operations will fail silently or throw.
 */
export async function setWorkspaceServer(url: string | null): Promise<void> {
  const previousUrl = serverUrl;
  serverUrl = url;
  _serverUrl = url ? toWebSocketUrl(url) : null;

  console.log('[WorkspaceCrdtBridge] setWorkspaceServer:', url ? 'connecting' : 'disconnecting');

  // Skip if URL hasn't changed or not initialized
  if (previousUrl === url || !initialized || !rustApi) {
    return;
  }

  // Validate backend is initialized before proceeding with sync setup
  if (url && !_backend) {
    console.error('[WorkspaceCrdtBridge] CRITICAL: setWorkspaceServer called with URL but _backend is null!');
    console.error('[WorkspaceCrdtBridge] Call setBackend() before setWorkspaceServer() to avoid silent failures.');
    // Don't throw to avoid breaking existing code, but log prominently
    notifySyncStatus('error', 'Sync initialization failed: backend not configured');
    return;
  }

  // Disconnect existing sync (native or browser-based)
  await disconnectExistingSync();

  // Create new sync if URL is set
  if (url && _backend) {
    // Reset initial sync tracking since we're starting a new sync connection
    _initialSyncComplete = false;
    _initialSyncResolvers = [];

    const workspaceDocName = _workspaceId ? `${_workspaceId}:workspace` : 'workspace';

    // Check if backend supports native sync (Tauri)
    if (_backend.hasNativeSync?.() && _backend.startSync) {
      // Use _serverUrl (WebSocket URL) for native sync - Rust client expects wss:// scheme
      console.log('[WorkspaceCrdtBridge] Using native sync (Tauri)');

      // Set up event listener for native sync events
      if (_backend.onSyncEvent) {
        _nativeSyncUnsubscribe = _backend.onSyncEvent((event: SyncEvent) => {
          handleNativeSyncEvent(event);
        });
      }

      // Notify connecting status
      notifySyncStatus('connecting');

      try {
        _nativeSyncActive = true;
        await _backend.startSync(_serverUrl!, workspaceDocName, getToken() ?? undefined);
        console.log('[WorkspaceCrdtBridge] Native sync started successfully');
      } catch (e) {
        console.error('[WorkspaceCrdtBridge] Native sync failed to start:', e);
        notifySyncStatus('error', e instanceof Error ? e.message : String(e));
        _nativeSyncActive = false;
      }
    } else {
      // Use browser WebSocket (WASM/web)
      console.log('[WorkspaceCrdtBridge] Using browser sync (v2)');

      // Use v2 UnifiedSyncTransport (single WebSocket for workspace + body via /sync2)
      // This replaces the previous WasmSyncBridge (v1 /sync) path which caused
      // "before handshake complete" warnings due to protocol mismatch.
      {
        const v2Url = toWebSocketUrlV2(url);

        unifiedSyncTransport = createUnifiedSyncTransport({
          serverUrl: v2Url,
          workspaceId: _workspaceId!,
          backend: _backend,
          writeToDisk: true,
          authToken: getToken() ?? undefined,
          onStatusChange: (connected) => {
            notifySyncStatus(connected ? 'syncing' : 'idle');
            if (!connected) {
              collaborationStore.setBodySyncStatus('idle');
            }
          },
          onWorkspaceSynced: async () => {
            notifySyncStatus('synced');
            notifyFileChange(null, null);
            await updateFileIndexFromCrdt();
            markInitialSyncComplete();

            // Proactively subscribe body sync for all files
            let filePaths: string[] = [];
            try {
              const allFiles = await getAllFiles();
              filePaths = Array.from(allFiles.keys());
              if (filePaths.length > 0) {
                console.log(`[UnifiedSync] Subscribing body sync for ${filePaths.length} files`);
                // Subscribe in batches with concurrency limit
                const concurrency = 5;
                for (let i = 0; i < filePaths.length; i += concurrency) {
                  const batch = filePaths.slice(i, i + concurrency);
                  await Promise.all(batch.map(fp => getOrCreateBodyBridge(fp)));
                }
                console.log(`[UnifiedSync] All body subscriptions complete`);
              }
            } catch (e) {
              console.warn('[UnifiedSync] Failed to start body sync:', e);
            }

            // Wait for all body syncs to complete (SyncStep2 responses) before marking synced.
            // Without this, body CRDTs may not yet have the server's content when we mark "synced",
            // which can cause edits to overwrite the original content.
            if (unifiedSyncTransport && filePaths.length > 0) {
              console.log(`[UnifiedSync] Waiting for ${filePaths.length} body syncs to complete...`);
              const waitPromises = filePaths.map(fp =>
                unifiedSyncTransport!.waitForBodySync(fp, 15000).catch(() => false)
              );
              await Promise.all(waitPromises);
              console.log(`[UnifiedSync] All body syncs complete`);
            }

            _freshFromServerLoad = false; // Reset after body sync is truly complete
            collaborationStore.setBodySyncStatus('synced');
          },
          onSyncComplete: (filesSynced) => {
            console.log(`[UnifiedSync] Sync complete: ${filesSynced} files synced`);
            collaborationStore.setBodySyncStatus('synced');
          },
          onFilesChanged: async (changedFiles) => {
            notifyFileChange(null, null);
            // Subscribe body sync for any new/changed files
            for (const filePath of changedFiles) {
              try {
                await getOrCreateBodyBridge(filePath);
              } catch (err) {
                console.warn(`[UnifiedSync] Failed to subscribe body for changed file ${filePath}:`, err);
              }
            }
          },
          onProgress: (completed, total) => {
            notifySyncProgress(completed, total);
          },
        });

        // Notify connecting status
        notifySyncStatus('connecting');

        await unifiedSyncTransport.connect();
      }
    }

    registerBridgeOnGlobal();
    await flushPendingBodySync('setWorkspaceServer');
  }
}

/**
 * Handle native sync events from Tauri.
 */
function handleNativeSyncEvent(event: SyncEvent): void {

  switch (event.type) {
    case 'status-changed':
      // Map status to our internal status type
      // Track metadata and body status separately for accurate UI representation
      const metadataConnected = event.status.metadata === 'connected';
      const bodyConnected = event.status.body === 'connected';
      const isConnecting = event.status.metadata === 'connecting' || event.status.body === 'connecting';

      // Update body sync status based on native sync state
      if (bodyConnected) {
        collaborationStore.setBodySyncStatus('synced');
      } else if (event.status.body === 'connecting') {
        collaborationStore.setBodySyncStatus('syncing');
      } else {
        collaborationStore.setBodySyncStatus('idle');
      }

      // Update metadata sync status
      if (metadataConnected && bodyConnected) {
        notifySyncStatus('synced');
      } else if (isConnecting) {
        notifySyncStatus('connecting');
      } else if (metadataConnected) {
        // Metadata connected but body not yet - show syncing
        notifySyncStatus('syncing');
      } else {
        notifySyncStatus('idle');
      }
      break;

    case 'files-changed':
      console.log('[WorkspaceCrdtBridge] Native sync: files changed:', event.paths);
      // Mark initial sync complete when we receive first files-changed event
      if (!_initialSyncComplete) {
        markInitialSyncComplete();
        updateFileIndexFromCrdt();
        // Auto-sync body content in background
        autoSyncBodiesInBackground();
      }
      notifyFileChange(null, null);
      break;

    case 'body-changed':
      console.log('[WorkspaceCrdtBridge] Native sync: body changed:', event.path);
      // The body content is already in the CRDT, fetch it and notify
      if (rustApi) {
        rustApi.getBodyContent(event.path).then(content => {
          if (content) {
            notifyBodyChange(event.path, content);
          }
        }).catch(e => {
          console.warn('[WorkspaceCrdtBridge] Failed to get body content:', e);
        });
      }
      break;

    case 'progress':
      notifySyncProgress(event.completed, event.total);
      break;

    case 'error':
      console.error('[WorkspaceCrdtBridge] Native sync error:', event.message);
      notifySyncStatus('error', event.message);
      break;
  }
}

/**
 * Disconnect existing sync (native or browser-based).
 */
async function disconnectExistingSync(): Promise<void> {
  // Stop native sync if active
  if (_nativeSyncActive && _backend?.stopSync) {
    console.log('[WorkspaceCrdtBridge] Stopping native sync');
    try {
      await _backend.stopSync();
    } catch (e) {
      console.warn('[WorkspaceCrdtBridge] Error stopping native sync:', e);
    }
    _nativeSyncActive = false;
  }

  // Unsubscribe from native sync events
  if (_nativeSyncUnsubscribe) {
    _nativeSyncUnsubscribe();
    _nativeSyncUnsubscribe = null;
  }

  // Close all body sync bridges (multiplexed and legacy)
  closeAllBodyBridges();

  // Reset body sync status since we're disconnecting
  collaborationStore.resetBodySyncStatus();

  // Disconnect new WASM sync bridge if any
  if (wasmSyncBridge) {
    console.log('[WorkspaceCrdtBridge] Disconnecting WasmSyncBridge');
    wasmSyncBridge.disconnect();
    wasmSyncBridge = null;
  }

  // Disconnect v2 unified sync transport if any
  if (unifiedSyncTransport) {
    console.log('[WorkspaceCrdtBridge] Disconnecting v2 UnifiedSyncTransport');
    unifiedSyncTransport.destroy();
    unifiedSyncTransport = null;
  }

  // Disconnect legacy browser sync bridge if any
  if (syncBridge) {
    console.log('[WorkspaceCrdtBridge] Disconnecting legacy browser sync bridge');
    syncBridge.destroy();
    syncBridge = null;
  }
}

/**
 * Auto-sync body content for all files in background.
 *
 * NOTE: When backend has native sync capability (Tauri), this is a no-op because
 * the native SyncClient handles body sync internally.
 */
async function autoSyncBodiesInBackground(): Promise<void> {
  // Skip if backend supports native sync - it handles body sync internally
  // We check hasNativeSync() (capability) not _nativeSyncActive (state) to prevent
  // fallback to TypeScript sync even if native sync fails to start
  if (_backend?.hasNativeSync?.()) {
    console.log('[WorkspaceCrdtBridge] autoSyncBodiesInBackground skipped (native sync capable)');
    return;
  }

  // Skip if using new WasmSyncBridge - it handles body sync automatically via subscribe_all_bodies()
  if (wasmSyncBridge) {
    console.log('[WorkspaceCrdtBridge] autoSyncBodiesInBackground skipped (using WasmSyncBridge)');
    return;
  }

  // Skip if using v2 UnifiedSyncTransport - body sync is multiplexed on the same connection
  if (unifiedSyncTransport) {
    console.log('[WorkspaceCrdtBridge] autoSyncBodiesInBackground skipped (using v2 UnifiedSyncTransport)');
    return;
  }

  try {
    const allFiles = await getAllFiles();
    const filePaths = Array.from(allFiles.keys());
    if (filePaths.length > 0) {
      console.log(`[WorkspaceCrdtBridge] Auto-syncing ${filePaths.length} body docs in background`);
      // Don't wait for completion since this runs in background
      proactivelySyncBodies(filePaths, { concurrency: 5, waitForComplete: false }).catch(e => {
        console.warn('[WorkspaceCrdtBridge] Background body sync error:', e);
      });
    }
  } catch (e) {
    console.warn('[WorkspaceCrdtBridge] Failed to start background body sync:', e);
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
 * If server URL is already set and ID changes, reconnect workspace sync.
 */
export async function setWorkspaceId(id: string | null): Promise<void> {
  console.log('[WorkspaceCrdtBridge] setWorkspaceId:', { id, previousId: _workspaceId });
  const previousId = _workspaceId;
  _workspaceId = id;

  // If we have a server URL and the ID changed, reconnect with the new doc name
  if (serverUrl && id && id !== previousId) {
    console.log('[WorkspaceCrdtBridge] Workspace ID changed, reconnecting workspace sync');
    // Disconnect existing sync bridge
    if (syncBridge) {
      syncBridge.destroy();
      syncBridge = null;
    }
    // Force reconnect by temporarily clearing serverUrl
    const savedUrl = serverUrl;
    serverUrl = null;
    await setWorkspaceServer(savedUrl);
  }

  await flushPendingBodySync('setWorkspaceId');
}

/**
 * Set the backend API for file operations.
 * This is used to write synced file content to disk for guests.
 */
export function setBackendApi(api: Api): void {
  backendApi = api;
}

/**
 * Set the backend for sync operations.
 * This is used for Rust-backed sync helpers that need direct backend access.
 */
export function setBackend(backend: Backend): void {
  _backend = backend;
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
export function getCanonicalPath(storagePath: string): string {
  let path = storagePath;

  // Strip leading "./" for consistent path format (server uses paths without ./ prefix)
  if (path.startsWith('./')) {
    path = path.slice(2);
  }

  const isGuest = shareSessionStore.isGuest;
  const joinCode = shareSessionStore.joinCode;
  const usesInMemory = shareSessionStore.usesInMemoryStorage;

  if (!isGuest || !joinCode) {
    return path;
  }

  // Guests using in-memory storage don't have prefixes
  if (usesInMemory) {
    return path;
  }

  // Strip guest/{joinCode}/ prefix if present (for OPFS guests)
  const guestPrefix = `guest/${joinCode}/`;
  if (path.startsWith(guestPrefix)) {
    return path.slice(guestPrefix.length);
  }

  return path;
}

// Session code for share sessions
let _sessionCode: string | null = null;

/**
 * Notify all body change callbacks.
 */
function notifyBodyChange(path: string, body: string): void {
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

  // IMPORTANT: Set module-level server URL and workspace ID for body sync bridges
  // For guests, these aren't set via initWorkspace, so we set them here
  if (!isHost) {
    _serverUrl = toWebSocketUrl(sessionServerUrl);
    // Use session code as workspace ID for body sync (server routes based on this)
    _workspaceId = sessionCode;
    console.log('[WorkspaceCrdtBridge] Guest: set _serverUrl and _workspaceId for body sync:', {
      _serverUrl,
      _workspaceId,
    });
  }

  // Disconnect existing bridge if any
  if (syncBridge) {
    syncBridge.destroy();
    syncBridge = null;
  }

  if (!rustApi) {
    console.error('[WorkspaceCrdtBridge] RustCrdtApi not initialized for session');
    return;
  }

  // Configure sync handler for guests (path prefixing, etc.)
  if (!isHost && _backend) {
    const usesOpfs = !shareSessionStore.usesInMemoryStorage;
    console.log('[WorkspaceCrdtBridge] Configuring sync handler for guest:', { sessionCode, usesOpfs });
    await syncHelpers.configureSyncHandler(_backend, sessionCode, usesOpfs);
  }

  const workspaceDocName = 'workspace';
  console.log('[WorkspaceCrdtBridge] Creating SyncTransport for session:', sessionServerUrl, 'doc:', workspaceDocName, 'session:', sessionCode);

  if (!_backend) {
    console.error('[WorkspaceCrdtBridge] Backend not initialized for session sync');
    return;
  }

  // Create a promise that resolves when initial sync completes
  let syncResolve: () => void;
  const syncPromise = new Promise<void>((resolve) => {
    syncResolve = resolve;
  });

  syncBridge = createWorkspaceSyncTransport({
    serverUrl: sessionServerUrl,
    docName: workspaceDocName,
    workspaceId: _workspaceId ?? undefined,
    backend: _backend,
    sessionCode: sessionCode,
    writeToDisk: true, // Rust handles disk writes automatically (with guest path prefixing if configured)
    onStatusChange: (connected) => {
      console.log('[WorkspaceCrdtBridge] Session sync status:', connected);
      notifySyncStatus(connected ? 'syncing' : 'idle');
    },
    onSynced: async () => {
      console.log('[WorkspaceCrdtBridge] Session sync complete, isHost:', isHost);
      notifySyncStatus('synced');
      if (!isHost) notifySessionSync();
      syncResolve();
    },
    onFilesChanged: () => {
      shareSessionStore.isGuest ? notifySessionSync() : notifyFileChange(null, null);
    },
    onProgress: (completed, total) => {
      console.log('[WorkspaceCrdtBridge] Session sync progress:', completed, '/', total);
      notifySyncProgress(completed, total);
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
export async function stopSessionSync(): Promise<void> {
  console.log('[WorkspaceCrdtBridge] Stopping session sync');

  _sessionCode = null;

  // Clear sync handler guest configuration
  if (_backend) {
    await syncHelpers.configureSyncHandler(_backend, null, false);
  }

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
  return syncBridge !== null && syncBridge.isSynced;
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
  syncBridge?: SyncTransport;
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

    // Connect sync bridge if provided
    // Only connect if we have a workspaceId (authenticated mode, not local-only)
    if (_workspaceId) {
      if (syncBridge) {
        await syncBridge.connect();
      } else if (serverUrl && rustApi && _backend) {
        const workspaceDocName = _workspaceId ? `${_workspaceId}:workspace` : 'workspace';

        // Check if backend supports native sync (Tauri)
        if (_backend.hasNativeSync?.() && _backend.startSync) {
          // Use _serverUrl (WebSocket URL) for native sync - Rust client expects wss:// scheme
          console.log('[WorkspaceCrdtBridge] Using native sync during init (Tauri):', _serverUrl, 'docName:', workspaceDocName);

          // Set up event listener for native sync events
          if (_backend.onSyncEvent) {
            _nativeSyncUnsubscribe = _backend.onSyncEvent((event: SyncEvent) => {
              handleNativeSyncEvent(event);
            });
          }

          try {
            _nativeSyncActive = true;
            await _backend.startSync(_serverUrl!, workspaceDocName, getToken() ?? undefined);
            console.log('[WorkspaceCrdtBridge] Native sync started successfully (init)');
          } catch (e) {
            console.error('[WorkspaceCrdtBridge] Native sync failed to start (init):', e);
            _nativeSyncActive = false;
          }
        } else {
          // Use v2 UnifiedSyncTransport (single WebSocket for workspace + body via /sync2)
          console.log('[WorkspaceCrdtBridge] Using v2 UnifiedSyncTransport during init');

          const v2Url = toWebSocketUrlV2(serverUrl);

          unifiedSyncTransport = createUnifiedSyncTransport({
            serverUrl: v2Url,
            workspaceId: _workspaceId!,
            backend: _backend,
            writeToDisk: true,
            authToken: getToken() ?? undefined,
            onStatusChange: (connected) => {
              notifySyncStatus(connected ? 'syncing' : 'idle');
              if (!connected) {
                collaborationStore.setBodySyncStatus('idle');
              }
            },
            onWorkspaceSynced: async () => {
              notifySyncStatus('synced');
              notifyFileChange(null, null);
              await updateFileIndexFromCrdt();
              markInitialSyncComplete();

              // Proactively subscribe body sync for all files
              let filePaths: string[] = [];
              try {
                const allFiles = await getAllFiles();
                filePaths = Array.from(allFiles.keys());
                if (filePaths.length > 0) {
                  console.log(`[UnifiedSync] Subscribing body sync for ${filePaths.length} files (init)`);
                  const concurrency = 5;
                  for (let i = 0; i < filePaths.length; i += concurrency) {
                    const batch = filePaths.slice(i, i + concurrency);
                    await Promise.all(batch.map(fp => getOrCreateBodyBridge(fp)));
                  }
                  console.log(`[UnifiedSync] All body subscriptions complete (init)`);
                }
              } catch (e) {
                console.warn('[UnifiedSync] Failed to start body sync (init):', e);
              }

              // Wait for all body syncs to complete (SyncStep2 responses) before marking synced.
              if (unifiedSyncTransport && filePaths.length > 0) {
                console.log(`[UnifiedSync] Waiting for ${filePaths.length} body syncs to complete (init)...`);
                const waitPromises = filePaths.map(fp =>
                  unifiedSyncTransport!.waitForBodySync(fp, 15000).catch(() => false)
                );
                await Promise.all(waitPromises);
                console.log(`[UnifiedSync] All body syncs complete (init)`);
              }

              _freshFromServerLoad = false;
              collaborationStore.setBodySyncStatus('synced');
            },
            onSyncComplete: (filesSynced) => {
              console.log(`[UnifiedSync] Sync complete (init): ${filesSynced} files synced`);
              collaborationStore.setBodySyncStatus('synced');
            },
            onFilesChanged: async (changedFiles) => {
              notifyFileChange(null, null);
              // Subscribe body sync for any new/changed files
              for (const filePath of changedFiles) {
                try {
                  await getOrCreateBodyBridge(filePath);
                } catch (err) {
                  console.warn(`[UnifiedSync] Failed to subscribe body for changed file ${filePath}:`, err);
                }
              }
            },
            onProgress: (completed, total) => {
              notifySyncProgress(completed, total);
            },
          });

          notifySyncStatus('connecting');
          await unifiedSyncTransport.connect();
        }
      }
    } else {
      console.log('[WorkspaceCrdtBridge] Sync skipped: local-only mode (no workspaceId)');
      // No sync needed - mark as complete immediately
      markInitialSyncComplete();
    }

    initialized = true;
    registerBridgeOnGlobal();
    await flushPendingBodySync('initWorkspace');
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

  // Disconnect existing sync (native or browser-based)
  await disconnectExistingSync();

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
  // Check native sync first (Tauri)
  if (_nativeSyncActive) {
    return true;
  }
  // Fall back to browser sync bridge
  return syncBridge?.isSynced ?? false;
}

// ===========================================================================
// File Operations
// ===========================================================================

/**
 * Get file metadata from the CRDT.
 */
export async function getFileMetadata(path: string): Promise<FileMetadata | null> {
  // Delegate to configured module instance if this one isn't configured
  if (!rustApi && _g.__diaryx_bridge?.getFileMetadata) {
    return _g.__diaryx_bridge.getFileMetadata(path);
  }
  return _getFileMetadataImpl(path);
}

async function _getFileMetadataImpl(path: string): Promise<FileMetadata | null> {
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
 * Mark all CRDT files as deleted (tombstone).
 * Used when "Load from server" is selected to ensure local files don't
 * persist after sync with server.
 *
 * Note: This sets deleted=true on all files but preserves other metadata
 * as required by CRDT semantics.
 */
export async function markAllCrdtFilesAsDeleted(): Promise<number> {
  if (!rustApi) {
    console.warn('[WorkspaceCrdtBridge] Cannot mark files as deleted: not initialized');
    return 0;
  }

  const files = await rustApi.listFiles(false); // Get non-deleted files
  const filePaths = files.map(([path]) => path);

  console.log(`[WorkspaceCrdtBridge] Marking ${filePaths.length} CRDT files as deleted`);

  let deleted = 0;
  for (const path of filePaths) {
    try {
      const existing = await rustApi.getFile(path);
      if (existing && !existing.deleted) {
        const updated: FileMetadata = {
          ...existing,
          deleted: true,
          modified_at: BigInt(Date.now()),
        };
        await rustApi.setFile(path, updated);
        deleted++;
      }
    } catch (e) {
      console.warn(`[WorkspaceCrdtBridge] Failed to mark ${path} as deleted:`, e);
    }
  }

  console.log(`[WorkspaceCrdtBridge] Marked ${deleted} CRDT files as deleted`);
  return deleted;
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
      filename: path.split('/').pop() ?? '',
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

  // Helper to resolve relative path against a base file's directory
  function resolveRelativePath(basePath: string, relativePath: string): string {
    // Get the directory of the base path
    const lastSlash = basePath.lastIndexOf('/');
    const baseDir = lastSlash >= 0 ? basePath.substring(0, lastSlash) : '';

    // Join and normalize the path
    const parts = baseDir ? baseDir.split('/') : [];
    for (const segment of relativePath.split('/')) {
      if (segment === '..') {
        parts.pop();
      } else if (segment !== '.' && segment !== '') {
        parts.push(segment);
      }
    }
    return parts.join('/');
  }

  // Helper to check if a part_of reference is valid (resolving relative paths)
  function hasValidPartOf(path: string, partOf: string | null): boolean {
    if (!partOf) return false;
    // Try as-is first (absolute path)
    if (fileMap.has(partOf)) return true;
    // Try resolving as relative path
    const resolved = resolveRelativePath(path, partOf);
    return fileMap.has(resolved);
  }

  // Find root files (files with no part_of, or part_of pointing to non-existent file)
  const rootFiles: string[] = [];
  for (const [path, metadata] of fileMap) {
    if (!hasValidPartOf(path, metadata.part_of)) {
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
        // Contents may be relative paths - resolve against parent's directory
        const absoluteChildPath = resolveRelativePath(originalPath, childPath);
        if (fileMap.has(absoluteChildPath)) {
          children.push(buildNode(absoluteChildPath));
        } else if (fileMap.has(childPath)) {
          // Fallback: try as-is (in case it's already absolute)
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

  // Skip temporary files - they should never enter the CRDT
  if (isTempFile(path)) {
    console.log('[WorkspaceCrdtBridge] Skipping setFileMetadata for temp file:', path);
    return;
  }

  // Block updates in read-only mode for guests
  if (isReadOnlyBlocked()) {
    console.log('[WorkspaceCrdtBridge] Blocked setFileMetadata in read-only session:', path);
    return;
  }

  await rustApi.setFile(path, metadata);
  // Sync happens automatically via SendSyncMessage events from Rust

  console.log('[WorkspaceCrdtBridge] setFileMetadata complete:', path);
  notifyFileChange(path, metadata);
}

/**
 * Check if updates should be blocked due to read-only mode.
 * Returns true if the user is a guest in a read-only session.
 */
function isReadOnlyBlocked(): boolean {
  return shareSessionStore.isGuest && shareSessionStore.readOnly;
}

/**
 * Get or create a body sync subscription for a specific file.
 *
 * Uses multiplexed body sync - a single WebSocket connection for all files.
 * This dramatically reduces CPU usage compared to per-file connections.
 *
 * Uses locking to prevent race conditions when multiple callers
 * (e.g., proactive sync and entry opening) try to subscribe to
 * the same file simultaneously.
 *
 * NOTE: When native sync is active (Tauri), this is a no-op because
 * the native SyncClient handles body sync internally.
 */
async function getOrCreateBodyBridge(filePath: string, sendFocus: boolean = false): Promise<void> {
  // Skip temporary files - they should never be synced
  if (isTempFile(filePath)) {
    return;
  }

  // Skip if backend supports native sync - it handles body sync internally
  // We check hasNativeSync() (capability) not _nativeSyncActive (state) to prevent
  // fallback to TypeScript sync even if native sync fails to start
  if (_backend?.hasNativeSync?.()) {
    console.log('[WorkspaceCrdtBridge] getOrCreateBodyBridge skipped (native sync capable):', filePath);
    return;
  }

  if (!rustApi || !_serverUrl || !_workspaceId || !_backend) {
    // Missing config - caller should handle local-only mode before calling this
    return;
  }

  const canonicalPath = getCanonicalPath(filePath);

  // Use v2 unified transport if active
  if (unifiedSyncTransport) {
    // Already subscribed? Skip (prevents re-subscribe loop from SendSyncMessage events).
    if (unifiedSyncTransport.isBodySubscribed(canonicalPath)) {
      return;
    }

    // Load body content from disk into CRDT BEFORE syncing (same as v1 path).
    // This ensures local content is present for the Y-CRDT protocol to send.
    try {
      const existingBodyContent = await rustApi.getBodyContent(canonicalPath);
      if (existingBodyContent && existingBodyContent.length > 0) {
        if (_freshFromServerLoad) {
          // Reset the body doc to a fresh empty Y.Doc to prevent duplication with server content.
          // importFromZip writes files to disk which populates body CRDTs (local actor).
          // When the server sends its Y-CRDT state (original actor), Y-CRDT merges
          // both independent inserts → text appears twice.
          //
          // CRITICAL: We use resetBodyDoc() instead of setBodyContent('', ...) because
          // setBodyContent creates Y-CRDT DELETE operations. These DELETEs would be sent
          // to the server during the Y-sync handshake response, propagating to other clients
          // and emptying their editors. resetBodyDoc() creates a fresh Y.Doc with NO operations.
          console.log(`[UnifiedSync] Resetting body doc for ${canonicalPath} (freshFromServerLoad)`);
          await rustApi.resetBodyDoc(canonicalPath);
        } else {
          await syncHelpers.trackContent(_backend!, canonicalPath, existingBodyContent);
          console.log(`[UnifiedSync] Body CRDT already has ${existingBodyContent.length} chars for ${canonicalPath}`);
        }
      } else if (backendApi && !shareSessionStore.isGuest) {
        // Body CRDT is empty but we're about to sync — let the server deliver content.
        // Loading from disk here creates NEW operations (fresh actor ID after reload)
        // which duplicate with the server's original operations.
        console.log(`[UnifiedSync] Body CRDT empty for ${canonicalPath}, letting sync deliver content`);
      }
    } catch (err) {
      console.warn(`[UnifiedSync] Could not get body content for ${canonicalPath}:`, err);
    }

    await unifiedSyncTransport.subscribeBody(
      canonicalPath,
      async (message) => {
        // Handle incoming body sync message via Rust
        const result = await syncHelpers.handleBodySyncMessage(_backend!, canonicalPath, message, true);
        // Send response back if Rust returns one
        if (result.response && result.response.length > 0) {
          unifiedSyncTransport!.sendBodyMessage(canonicalPath, new Uint8Array(result.response));
        }
        // Note: we do NOT call notifyBodyChange here — the Rust side emits a
        // ContentsChanged event (handled in the event dispatcher) for the same
        // update. Relying on that single path avoids double-notification which
        // can cause editor rendering glitches.
      },
      async () => {
        console.log(`[WorkspaceCrdtBridge] v2 body synced: ${canonicalPath}`);
        // If CRDT is still empty after sync, load from disk (offline-created file)
        if (rustApi && backendApi && !shareSessionStore.isGuest) {
          try {
            const contentAfterSync = await rustApi.getBodyContent(canonicalPath);
            if (!contentAfterSync || contentAfterSync.length === 0) {
              const entry = await backendApi.getEntry(canonicalPath);
              if (entry?.content && entry.content.length > 0) {
                console.log(`[UnifiedSync] Post-sync: loading ${entry.content.length} chars from disk for ${canonicalPath}`);
                await rustApi.setBodyContent(canonicalPath, entry.content);
                const update = await syncHelpers.createBodyUpdate(_backend!, canonicalPath, entry.content);
                if (update && update.length > 0) {
                  unifiedSyncTransport!.sendBodyMessage(canonicalPath, new Uint8Array(update));
                }
              }
            }
          } catch (e) {
            console.warn(`[UnifiedSync] Post-sync disk load failed for ${canonicalPath}:`, e);
          }
        }
      }
    );
    return;
  }

  // If there's already a pending subscription for this file, wait for it
  const pendingCreation = bodyBridgePendingCreation.get(canonicalPath);
  if (pendingCreation) {
    await pendingCreation;
    return;
  }

  // Already subscribed via multiplexed sync?
  if (multiplexedBodySync?.isSubscribed(canonicalPath)) {
    return;
  }

  // Create the subscription with a lock to prevent concurrent creation
  const createPromise = subscribeToBodyViaMultiplexed(canonicalPath, sendFocus);
  bodyBridgePendingCreation.set(canonicalPath, createPromise);

  try {
    await createPromise;
  } finally {
    bodyBridgePendingCreation.delete(canonicalPath);
  }
}

/**
 * Ensure the multiplexed body sync connection exists and subscribe to a file.
 */
async function subscribeToBodyViaMultiplexed(canonicalPath: string, sendFocus: boolean = false): Promise<void> {
  if (!rustApi || !_serverUrl || !_workspaceId || !_backend) {
    return;
  }

  // Create multiplexed transport if not exists
  if (!multiplexedBodySync) {
    multiplexedBodySync = new MultiplexedBodySync({
      serverUrl: _serverUrl,
      workspaceId: _workspaceId,
      backend: _backend,
      authToken: getToken() ?? undefined,
      sessionCode: shareSessionStore.joinCode ?? undefined,
      onStatusChange: (connected) => {
        console.log('[MultiplexedBodySync] Status:', connected ? 'connected' : 'disconnected');
        // Body sync status is separate from metadata sync status
        if (!connected) {
          collaborationStore.setBodySyncStatus('idle');
        }
      },
      onProgress: (completed, total) => {
        // Update body sync progress separately
        collaborationStore.setBodySyncProgress({ completed, total });
        notifySyncProgress(completed, total);
      },
      onSyncComplete: (filesSynced) => {
        console.log(`[MultiplexedBodySync] Sync complete: ${filesSynced} files`);
        // Mark body sync as complete
        collaborationStore.setBodySyncStatus('synced');
      },
      // Handle focus list changes from server
      // When other clients focus on files, we should subscribe to sync for those files
      onFocusListChanged: async (files) => {
        console.log(`[MultiplexedBodySync] Focus list changed: ${files.length} files`);

        // Subscribe to any newly focused files that we're not already subscribed to
        for (const filePath of files) {
          const canonicalPath = getCanonicalPath(filePath);
          if (!multiplexedBodySync?.isSubscribed(canonicalPath)) {
            console.log(`[MultiplexedBodySync] Auto-subscribing to focused file: ${canonicalPath}`);
            // Create body sync subscription for this file (sendFocus=false to avoid re-broadcasting)
            try {
              await getOrCreateBodyBridge(canonicalPath);
            } catch (err) {
              console.warn(`[MultiplexedBodySync] Failed to subscribe to focused file ${canonicalPath}:`, err);
            }
          }
        }
      },
      // Handle messages for files we're not actively subscribed to
      // This ensures updates from other clients (e.g., Tauri) are applied even if the file isn't open
      onUnsubscribedMessage: async (filePath: string, message: Uint8Array) => {
        if (!_backend) return;

        console.log(`[MultiplexedBodySync] Processing unsubscribed message for ${filePath}, ${message.length} bytes`);

        try {
          // Apply the update to the Rust CRDT without disk writes (we're not actively editing this file)
          const response = await _backend.execute({
            type: 'HandleBodySyncMessage' as any,
            params: {
              doc_name: filePath,
              message: Array.from(message),
              write_to_disk: !shareSessionStore.isGuest, // Write to disk for hosts
            },
          } as any);

          if ((response.type as string) === 'BodySyncResult') {
            const result = response as any;
            // Send response if Rust returns one (e.g., SyncStep2 in response to SyncStep1)
            if (result.data?.response && result.data.response.length > 0) {
              multiplexedBodySync?.send(filePath, new Uint8Array(result.data.response));
            }
            // Note: We don't notify body change callbacks here since the file isn't open
            // The change will be visible when the user opens the file
            if (result.data?.content && !result.data?.is_echo) {
              console.log(`[MultiplexedBodySync] Applied remote update for unopened file ${filePath}, ${result.data.content.length} chars`);
            }
          }
        } catch (err) {
          console.warn(`[MultiplexedBodySync] Failed to handle unsubscribed message for ${filePath}:`, err);
        }
      },
    });
    await multiplexedBodySync.connect();
  }

  // IMPORTANT: Load body content from disk into CRDT BEFORE syncing.
  // This ensures our local content is in the CRDT when sync starts, so the
  // Y-CRDT protocol can send it to the server (for uploads) or merge it
  // with server content (for bidirectional sync).
  try {
    const existingBodyContent = await rustApi.getBodyContent(canonicalPath);
    if (existingBodyContent && existingBodyContent.length > 0) {
      // CRDT already has content - just track for echo detection
      await syncHelpers.trackContent(_backend!, canonicalPath, existingBodyContent);
      console.log(`[MultiplexedBodySync] Body CRDT already has ${existingBodyContent.length} chars for ${canonicalPath}`);
    } else if (backendApi && !shareSessionStore.isGuest) {
      // CRDT is empty and we're a host - load from disk BEFORE syncing
      // This is critical for uploads: content must be in CRDT before sync protocol runs
      try {
        const entry = await backendApi.getEntry(canonicalPath);
        if (entry && entry.content && entry.content.length > 0) {
          console.log(`[MultiplexedBodySync] Loading ${entry.content.length} chars from disk into CRDT BEFORE sync for ${canonicalPath}`);
          await syncHelpers.trackContent(_backend!, canonicalPath, entry.content);
          await rustApi.setBodyContent(canonicalPath, entry.content);
        }
      } catch (diskErr) {
        console.log(`[MultiplexedBodySync] Could not load from disk for ${canonicalPath}:`, diskErr);
      }
    }
  } catch (err) {
    console.warn(`[MultiplexedBodySync] Could not get body content for ${canonicalPath}:`, err);
  }

  // Subscribe to this file via the multiplexed connection
  await multiplexedBodySync.subscribe(
    canonicalPath,
    async (message: Uint8Array) => {
      // Handle incoming sync message via Rust
      console.log(`[BodySync] Calling HandleBodySyncMessage for ${canonicalPath}, write_to_disk=${!shareSessionStore.isGuest}, message_len=${message.length}`);

      const response = await _backend!.execute({
        type: 'HandleBodySyncMessage' as any,
        params: {
          doc_name: canonicalPath,
          message: Array.from(message),
          write_to_disk: !shareSessionStore.isGuest,
        },
      } as any);

      console.log(`[BodySync] HandleBodySyncMessage response for ${canonicalPath}:`, {
        responseType: response.type,
        hasData: !!(response as any).data,
      });

      if ((response.type as string) === 'BodySyncResult') {
        const result = response as any;
        console.log(`[BodySync] BodySyncResult for ${canonicalPath}:`, {
          hasContent: !!result.data?.content,
          contentLen: result.data?.content?.length ?? 0,
          isEcho: result.data?.is_echo,
          hasResponse: !!result.data?.response?.length,
          responseLen: result.data?.response?.length ?? 0,
        });

        // Send response if Rust returns one
        if (result.data?.response && result.data.response.length > 0) {
          multiplexedBodySync?.send(canonicalPath, new Uint8Array(result.data.response));
        }

        // Handle content change (if not an echo)
        // Note: Rust HandleBodySyncMessage already writes to disk when write_to_disk=true,
        // so we only need to notify the UI here (no saveEntry call to avoid sync loop)
        if (result.data?.content && !result.data?.is_echo) {
          // Skip if this matches our last known content
          if (_backend && await syncHelpers.isEcho(_backend, canonicalPath, result.data.content)) {
            console.log(`[MultiplexedBodySync] ${canonicalPath} skipping unchanged content`);
            return;
          }

          console.log(`[MultiplexedBodySync] ${canonicalPath} received remote body update, length:`, result.data.content.length);
          if (_backend) {
            await syncHelpers.trackContent(_backend, canonicalPath, result.data.content);
          }

          notifyBodyChange(canonicalPath, result.data.content);
        }
      }
    },
    async () => {
      // onSynced callback
      console.log(`[MultiplexedBodySync] ${canonicalPath} synced`);

      // For hosts: handle uploading scenario (CRDT empty, disk has content)
      // Note: Downloading scenario (CRDT has content) is already handled by Rust
      // HandleBodySyncMessage with write_to_disk=true, so no saveEntry needed here
      if (backendApi && !shareSessionStore.isGuest && rustApi && _backend) {
        try {
          const crdtContent = await rustApi.getBodyContent(canonicalPath);

          if (crdtContent && crdtContent.length > 0) {
            // CRDT has content - Rust already wrote to disk, just notify UI
            console.log(`[MultiplexedBodySync] ${canonicalPath} synced with ${crdtContent.length} chars`);
            await syncHelpers.trackContent(_backend, canonicalPath, crdtContent);
            notifyBodyChange(canonicalPath, crdtContent);
          } else {
            // CRDT is empty - try loading from disk (uploading scenario)
            const entry = await backendApi.getEntry(canonicalPath).catch(() => null);
            const diskContent = entry?.content || '';

            if (diskContent && diskContent.length > 0) {
              console.log(`[MultiplexedBodySync] Loading ${diskContent.length} chars from disk into CRDT for ${canonicalPath}`);
              await syncHelpers.trackContent(_backend, canonicalPath, diskContent);
              await rustApi.setBodyContent(canonicalPath, diskContent);
            }
          }
        } catch (err) {
          console.warn(`[MultiplexedBodySync] Error syncing body for ${canonicalPath}:`, err);
        }
      }

      // For guests, read body content from CRDT and notify UI
      if (shareSessionStore.isGuest && rustApi) {
        try {
          const bodyContent = await rustApi.getBodyContent(canonicalPath);
          if (bodyContent && bodyContent.length > 0) {
            console.log(`[MultiplexedBodySync] Guest: Notifying UI of synced body for ${canonicalPath}, length: ${bodyContent.length}`);
            notifyBodyChange(canonicalPath, bodyContent);
          } else {
            console.log(`[MultiplexedBodySync] Guest: No body content in CRDT for ${canonicalPath}`);
          }
        } catch (err) {
          console.warn(`[MultiplexedBodySync] Guest: Failed to read body from CRDT for ${canonicalPath}:`, err);
        }
      }
    }
  );

  console.log(`[MultiplexedBodySync] Subscribed to ${canonicalPath}`);

  // Send focus message only when explicitly requested (user opened file in editor)
  // Don't send focus for background sync or when auto-subscribing to other clients' focus list
  if (sendFocus) {
    multiplexedBodySync.focus([canonicalPath]);
    console.log(`[MultiplexedBodySync] Focused on ${canonicalPath}`);
  }
}

/**
 * Close body sync for a specific file.
 * Call when the file is no longer being actively edited.
 *
 * Note: With multiplexed sync, we just unsubscribe from the file but keep
 * the WebSocket connection open for other files.
 */
export function closeBodySync(filePath: string): void {
  const canonicalPath = getCanonicalPath(filePath);

  // Unsubscribe from multiplexed sync
  if (multiplexedBodySync?.isSubscribed(canonicalPath)) {
    console.log(`[MultiplexedBodySync] Unsubscribing from: ${canonicalPath}`);
    multiplexedBodySync.unsubscribe(canonicalPath);

    // Send unfocus message to server
    multiplexedBodySync.unfocus([canonicalPath]);
  }

  // Also clean up any legacy per-file bridges
  const bridge = bodyBridges.get(canonicalPath);
  if (bridge) {
    console.log(`[BodySyncBridge] Closing legacy sync for: ${canonicalPath}`);
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
 *
 * NOTE: When backend has native sync capability (Tauri), this is a no-op because
 * the native SyncClient handles body sync internally.
 */
export async function ensureBodySync(filePath: string): Promise<void> {
  // Delegate to configured module instance if this one isn't configured
  if (!rustApi && _g.__diaryx_bridge?.ensureBodySync) {
    return _g.__diaryx_bridge.ensureBodySync(filePath);
  }
  return _ensureBodySyncImpl(filePath);
}

async function _ensureBodySyncImpl(filePath: string): Promise<void> {
  // Skip if backend supports native sync - it handles body sync internally
  // We check hasNativeSync() (capability) not _nativeSyncActive (state) to prevent
  // fallback to TypeScript sync even if native sync fails to start
  if (_backend?.hasNativeSync?.()) {
    console.log('[WorkspaceCrdtBridge] ensureBodySync skipped (native sync capable):', filePath);
    return;
  }

  const canonicalPath = getCanonicalPath(filePath);

  if (!_workspaceId || !_serverUrl || !rustApi) {
    console.log('[WorkspaceCrdtBridge] ensureBodySync deferred - not in sync mode yet:', {
      hasWorkspaceId: !!_workspaceId,
      hasServerUrl: !!_serverUrl,
      hasRustApi: !!rustApi,
      queuedPath: canonicalPath,
    });
    pendingBodySync.add(canonicalPath);
    return;
  }

  console.log('[WorkspaceCrdtBridge] ensureBodySync for:', canonicalPath);
  // Pass sendFocus=true since this is called when user opens a file in the editor
  await getOrCreateBodyBridge(canonicalPath, true);
}

/**
 * Get body content from the CRDT.
 * This is useful for guests who don't have files on disk but need to read
 * body content that was synced into the CRDT.
 *
 * @param filePath - The file path (can be storage path - will be converted to canonical)
 * @returns The body content, or null if not available
 */
export async function getBodyContentFromCrdt(filePath: string): Promise<string | null> {
  // Delegate to configured module instance if this one isn't configured
  if (!rustApi && _g.__diaryx_bridge?.getBodyContentFromCrdt) {
    return _g.__diaryx_bridge.getBodyContentFromCrdt(filePath);
  }
  return _getBodyContentFromCrdtImpl(filePath);
}

async function _getBodyContentFromCrdtImpl(filePath: string): Promise<string | null> {
  if (!rustApi) {
    return null;
  }
  const canonicalPath = getCanonicalPath(filePath);
  try {
    const content = await rustApi.getBodyContent(canonicalPath);
    return content || null;
  } catch (err) {
    console.warn('[WorkspaceCrdtBridge] Failed to get body content from CRDT:', err);
    return null;
  }
}

/**
 * Options for proactive body sync.
 */
export interface ProactiveSyncOptions {
  /** How many body syncs to run in parallel (default 3) */
  concurrency?: number;
  /** Callback for progress updates during subscription phase */
  onProgress?: (completed: number, total: number) => void;
  /** Whether to wait for sync_complete from server (default true) */
  waitForComplete?: boolean;
  /** Timeout for waiting for sync_complete in ms (default 120000 = 2 minutes) */
  syncTimeout?: number;
}

/**
 * Proactively sync body docs for multiple files.
 * Call this after the tree loads to pre-fetch body content for all files,
 * so they're ready when the user opens them.
 *
 * NOTE: When backend has native sync capability (Tauri), this is a no-op because
 * the native SyncClient handles body sync internally.
 *
 * @param filePaths Array of file paths to sync bodies for
 * @param optionsOrConcurrency Options object, or concurrency number for backward compatibility
 */
export async function proactivelySyncBodies(
  filePaths: string[],
  optionsOrConcurrency?: number | ProactiveSyncOptions
): Promise<void> {
  // Skip if backend supports native sync - it handles body sync internally
  // We check hasNativeSync() (capability) not _nativeSyncActive (state) to prevent
  // fallback to TypeScript sync even if native sync fails to start
  if (_backend?.hasNativeSync?.()) {
    console.log('[WorkspaceCrdtBridge] proactivelySyncBodies skipped (native sync capable)');
    return;
  }

  if (!_workspaceId || !_serverUrl || !rustApi) {
    console.log('[WorkspaceCrdtBridge] proactivelySyncBodies skipped - not in sync mode');
    return;
  }

  // Handle backward compatibility: number arg means concurrency
  const options = typeof optionsOrConcurrency === 'number'
    ? { concurrency: optionsOrConcurrency }
    : optionsOrConcurrency ?? {};
  const concurrency = options.concurrency ?? 3;
  const onProgress = options.onProgress;
  const waitForComplete = options.waitForComplete ?? true;
  const syncTimeout = options.syncTimeout ?? 120000; // 2 minutes default

  console.log(`[WorkspaceCrdtBridge] Proactively syncing ${filePaths.length} body docs with concurrency ${concurrency}`);

  // Mark body sync as in progress
  collaborationStore.setBodySyncStatus('syncing');

  let completed = 0;
  const total = filePaths.length;

  // Report initial progress
  collaborationStore.setBodySyncProgress({ completed, total });
  onProgress?.(completed, total);

  // Process in batches to avoid overwhelming the server
  for (let i = 0; i < filePaths.length; i += concurrency) {
    const batch = filePaths.slice(i, i + concurrency);
    await Promise.all(
      batch.map(async (path) => {
        try {
          const canonicalPath = getCanonicalPath(path);
          // Only sync if not already subscribed via multiplexed sync or legacy bridges
          if (!multiplexedBodySync?.isSubscribed(canonicalPath) && !bodyBridges.has(canonicalPath)) {
            await getOrCreateBodyBridge(canonicalPath);
          }
        } catch (e) {
          console.warn(`[WorkspaceCrdtBridge] Failed to sync body for ${path}:`, e);
        } finally {
          completed++;
          onProgress?.(completed, total);
        }
      })
    );
  }

  console.log(`[WorkspaceCrdtBridge] All ${filePaths.length} body subscriptions sent`);

  // Wait for sync_complete from server (arrives after 3-second quiet period)
  if (waitForComplete && multiplexedBodySync) {
    console.log(`[WorkspaceCrdtBridge] Waiting for body sync to complete (timeout: ${syncTimeout}ms)...`);
    const success = await multiplexedBodySync.waitForAllSyncs(syncTimeout);
    if (success) {
      console.log(`[WorkspaceCrdtBridge] Body sync complete for ${filePaths.length} files`);
    } else {
      console.warn(`[WorkspaceCrdtBridge] Body sync timed out after ${syncTimeout}ms`);
    }
  } else {
    console.log(`[WorkspaceCrdtBridge] Proactive body sync complete for ${filePaths.length} files (not waiting for server)`);
  }
}

/**
 * Close all body sync bridges.
 * Call during cleanup/disconnect.
 */
function closeAllBodyBridges(): void {
  // Clean up v2 unified transport
  // Note: Main disconnect happens in disconnectExistingSync(), but we
  // may need to clean up body subscriptions here for partial cleanup scenarios
  if (unifiedSyncTransport) {
    console.log('[UnifiedSyncTransport] Cleaning up body subscriptions');
    // The transport itself is cleaned up in disconnectExistingSync()
  }

  // Destroy the multiplexed sync transport
  if (multiplexedBodySync) {
    console.log(`[MultiplexedBodySync] Destroying multiplexed connection (${multiplexedBodySync.subscriptionCount} subscriptions)`);
    multiplexedBodySync.destroy();
    multiplexedBodySync = null;
  }

  // Also clean up any legacy per-file bridges
  if (bodyBridges.size > 0) {
    console.log(`[BodySyncBridge] Closing all ${bodyBridges.size} legacy bridges`);
    for (const bridge of bodyBridges.values()) {
      bridge.destroy();
    }
    bodyBridges.clear();
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

    // Build updated metadata (without modified_at initially)
    const newTitle = updates.title ?? existing?.title ?? null;
    const newPartOf = updates.part_of ?? existing?.part_of ?? null;
    const newContents = updates.contents ?? existing?.contents ?? null;
    const newAttachments = updates.attachments ?? existing?.attachments ?? [];
    const newDeleted = updates.deleted ?? existing?.deleted ?? false;
    const newAudience = updates.audience ?? existing?.audience ?? null;
    const newDescription = updates.description ?? existing?.description ?? null;
    const newExtra = updates.extra ?? existing?.extra ?? {};

    // Check if there are actual changes (excluding modified_at)
    const hasChanges = existing === null ||
      newTitle !== existing.title ||
      newPartOf !== existing.part_of ||
      newContents !== existing.contents ||
      JSON.stringify(newAttachments) !== JSON.stringify(existing.attachments) ||
      newDeleted !== existing.deleted ||
      newAudience !== existing.audience ||
      newDescription !== existing.description ||
      JSON.stringify(newExtra) !== JSON.stringify(existing.extra);

    if (!hasChanges) {
      console.log('[WorkspaceCrdtBridge] No changes detected, skipping update:', path);
      return;
    }

    const updated: FileMetadata = {
      filename: existing?.filename ?? path.split('/').pop() ?? '',
      title: newTitle,
      part_of: newPartOf,
      contents: newContents,
      attachments: newAttachments,
      deleted: newDeleted,
      audience: newAudience,
      description: newDescription,
      extra: newExtra,
      modified_at: BigInt(Date.now()),
    };

    console.log('[WorkspaceCrdtBridge] Updating file metadata:', path, updated);
    await rustApi.setFile(path, updated);
    console.log('[WorkspaceCrdtBridge] File metadata updated successfully:', path);
    // Sync happens automatically via SendSyncMessage events from Rust

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
    filename: '',
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
  // Sync happens automatically via SendSyncMessage events from Rust

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
      // Sync happens automatically via SendSyncMessage events from Rust

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
      // Sync happens automatically via SendSyncMessage events from Rust

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
    // Sync happens automatically via SendSyncMessage events from Rust

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
    // Sync happens automatically via SendSyncMessage events from Rust

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
function notifySyncStatus(status: 'idle' | 'connecting' | 'syncing' | 'synced' | 'error', error?: unknown): void {
  // Convert error to string if it's not already (defensive handling for Rust objects)
  let errorStr: string | undefined;
  if (error !== undefined && error !== null) {
    if (typeof error === 'string') {
      errorStr = error;
    } else if (error instanceof Error) {
      errorStr = error.message;
    } else if (typeof error === 'object') {
      const errObj = error as Record<string, unknown>;
      if (typeof errObj.message === 'string') {
        errorStr = errObj.message;
      } else if (typeof errObj.error === 'string') {
        errorStr = errObj.error;
      } else {
        try {
          errorStr = JSON.stringify(error);
        } catch {
          errorStr = 'Unknown error';
        }
      }
    } else {
      errorStr = String(error);
    }
  }

  // Only log sync status changes when there's an error or significant status change
  if (errorStr || status === 'synced' || status === 'error') {
    console.log('[WorkspaceCrdtBridge] Sync status:', status, errorStr ? `(${errorStr})` : '');
  }

  // Update collaborationStore for SyncStatusIndicator
  collaborationStore.setSyncStatus(status);
  if (errorStr) {
    collaborationStore.setSyncError(errorStr);
  } else if (status === 'synced' || status === 'idle') {
    collaborationStore.setSyncProgress(null); // Clear progress when done
  }

  for (const callback of syncStatusCallbacks) {
    try {
      callback(status, errorStr);
    } catch (err) {
      console.error('[WorkspaceCrdtBridge] Sync status callback error:', err);
    }
  }
}

/**
 * Notify all sync progress callbacks.
 */
function notifySyncProgress(completed: number, total: number): void {
  // Update collaborationStore for SyncStatusIndicator
  collaborationStore.setSyncProgress({ completed, total });

  for (const callback of syncProgressCallbacks) {
    try {
      callback(completed, total);
    } catch (err) {
      console.error('[WorkspaceCrdtBridge] Sync progress callback error:', err);
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

  switch (event.type) {
    case 'FileCreated':
      // Skip temporary files
      if (isTempFile(event.path)) return;
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
      // Close body sync bridge for deleted file (cleanup)
      closeBodySync(event.path);
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
      // Close body sync bridge for old path (cleanup)
      closeBodySync(event.old_path);
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
      // Close body sync bridge for old path (cleanup)
      if (event.old_parent !== undefined) {
        const filename = event.path.split('/').pop();
        if (filename) {
          closeBodySync(event.old_parent + '/' + filename);
        }
      }
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

    // Sync events - use helpers for dispatch
    case 'SyncStarted':
      console.log('[WorkspaceCrdtBridge] Sync started for:', event.doc_name);
      break;

    case 'SyncCompleted':
      console.log('[WorkspaceCrdtBridge] Sync completed for:', event.doc_name, 'files:', event.files_synced);
      markInitialSyncComplete();
      // Update file index after sync
      updateFileIndexFromCrdt();
      break;

    case 'SyncStatusChanged':
      console.log('[WorkspaceCrdtBridge] Sync status changed:', event.status, event.error);
      notifySyncStatus(event.status as 'idle' | 'connecting' | 'syncing' | 'synced' | 'error', event.error);
      break;

    case 'SyncProgress':
      console.log('[WorkspaceCrdtBridge] Sync progress:', event.completed, '/', event.total);
      notifySyncProgress(event.completed, event.total);
      break;

    case 'SendSyncMessage': {
      // Rust is requesting that we send a sync message over WebSocket.
      // This happens after CRDT updates (SaveEntry, CreateEntry, DeleteEntry, RenameEntry).
      //
      // For native sync (Tauri): Skip this event - the native sync client (TokioTransport)
      // handles WebSocket communication directly via the event bridge set up in start_websocket_sync.
      //
      // For browser sync (WASM/web): Forward to JavaScript WebSocket bridges.
      // We check hasNativeSync() (capability) not _nativeSyncActive (state) to prevent
      // fallback to TypeScript sync even if native sync fails to start
      if (_backend?.hasNativeSync?.()) {
        // Native sync handles this internally via the event bridge
        console.log('[WorkspaceCrdtBridge] SendSyncMessage skipped (native sync capable)');
        break;
      }

      const { doc_name, message, is_body } = event as any;
      const bytes = new Uint8Array(message);

      console.log(`[WorkspaceCrdtBridge] SendSyncMessage received: doc=${doc_name}, is_body=${is_body}, bytes=${bytes.length}`);

      if (is_body) {
        // Send via v2 unified transport or multiplexed body sync
        (async () => {
          try {
            if (unifiedSyncTransport) {
              // Only send if this body file has been subscribed (SyncStep1 sent).
              // This prevents phantom messages from importFromZip and CRDT clearing
              // from reaching the server before the Y-sync handshake.
              if (unifiedSyncTransport.isBodySubscribed(doc_name)) {
                unifiedSyncTransport.sendBodyMessage(doc_name, bytes);
                console.log('[WorkspaceCrdtBridge] v2 body sync message sent/queued for', doc_name, bytes.length, 'bytes');
              } else {
                console.log('[WorkspaceCrdtBridge] Body sync message dropped (not subscribed):', doc_name);
              }
            } else if (!_serverUrl) {
              // No server configured — local-only mode, skip
            } else {
              // v2 transport not ready yet (still connecting), drop the message
              console.warn('[WorkspaceCrdtBridge] Body sync message dropped (no transport ready):', doc_name);
            }
          } catch (err) {
            console.warn('[WorkspaceCrdtBridge] Failed to send body sync for', doc_name, err);
          }
        })();
      } else {
        // Send via v2 unified transport or workspace bridge
        if (unifiedSyncTransport) {
          unifiedSyncTransport.sendWorkspaceMessage(bytes);
          console.log('[WorkspaceCrdtBridge] v2 workspace sync message sent', bytes.length, 'bytes');
        } else if (syncBridge?.isConnected) {
          syncBridge.sendRawMessage(bytes);
          console.log('[WorkspaceCrdtBridge] Sent workspace sync', bytes.length, 'bytes');
        } else {
          // No workspace sync configured - that's OK for local-only mode
          console.log('[WorkspaceCrdtBridge] Workspace sync skipped (no server)');
        }
      }
      break;
    }
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
 * Debug function to check sync state.
 * Call this from browser console: window.debugSync()
 */
export function debugSync(): void {
  console.log('=== Sync Debug ===');
  console.log('serverUrl:', serverUrl);
  console.log('nativeSyncActive:', _nativeSyncActive);
  console.log('syncBridge:', syncBridge ? 'exists' : 'null');
  console.log('syncBridge.synced:', syncBridge?.isSynced);
  console.log('initialized:', initialized);
  console.log('rustApi:', rustApi ? 'exists' : 'null');
  console.log('hasNativeSync:', _backend?.hasNativeSync?.() ?? false);

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

// Expose debug function globally for browser console
if (typeof window !== 'undefined') {
  (window as any).debugSync = debugSync;
}

// Re-export types
export type { FileMetadata, BinaryRef };

// Re-export sync helpers for progressive integration
// These can replace TypeScript implementations with Rust-backed versions
export { syncHelpers };
