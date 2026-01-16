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
import { SimpleSyncBridge, createSimpleSyncBridge } from './simpleSyncBridge';
import type { FileMetadata, BinaryRef } from '../backend/generated';
import {
  isP2PEnabled,
  createP2PProvider,
  destroyP2PProvider,
} from './p2pSyncBridge';
import type { WebrtcProvider } from 'y-webrtc';
import type { Api } from '../backend/api';
import { shareSessionStore } from '@/models/stores/shareSessionStore.svelte';

// State
let rustApi: RustCrdtApi | null = null;
let syncBridge: SimpleSyncBridge | null = null;
let backendApi: Api | null = null;

// Workspace server sync - re-enabled after fixing incremental update issue.
// The previous issue was caused by re-encoding full state instead of applying incremental updates.
const WORKSPACE_SERVER_SYNC_ENABLED = true;
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

// Session sync callbacks - called when session data is received and synced
type SessionSyncCallback = () => void;
const sessionSyncCallbacks = new Set<SessionSyncCallback>();

// Body change callbacks - called when a file's body content changes remotely
type BodyChangeCallback = (path: string, body: string) => void;
const bodyChangeCallbacks = new Set<BodyChangeCallback>();

// ===========================================================================
// Configuration
// ===========================================================================

/**
 * Set the server URL for workspace sync.
 * Creates and connects a SimpleSyncBridge if the URL is set.
 *
 * NOTE: Workspace server sync is currently disabled (WORKSPACE_SERVER_SYNC_ENABLED = false).
 * Per-document sync works fine, but workspace sync causes content duplication issues.
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
    // Ensure workspaceYDoc exists
    await ensureWorkspaceYDoc();

    if (!workspaceYDoc) {
      console.error('[WorkspaceCrdtBridge] Failed to create workspace Y.Doc');
      return;
    }

    const workspaceDocName = _workspaceId ? `${_workspaceId}:workspace` : 'workspace';
    console.log('[WorkspaceCrdtBridge] Creating sync bridge for workspace:', url, 'docName:', workspaceDocName);

    syncBridge = createSimpleSyncBridge({
      serverUrl: url,
      docName: workspaceDocName,
      doc: workspaceYDoc,
      onStatusChange: (connected) => {
        console.log('[WorkspaceCrdtBridge] Connection status:', connected);
      },
      onSynced: async () => {
        console.log('[WorkspaceCrdtBridge] Initial sync complete');
        // Full state sync is appropriate here - this is the initial sync
        await syncFullStateToRust();
      },
    });

    // Set up listener for remote updates - apply incremental updates only
    workspaceYDoc.on('update', async (update: Uint8Array, origin: unknown) => {
      if (origin === 'server') {
        console.log('[WorkspaceCrdtBridge] Remote update received:', update.length, 'bytes');
        // Apply the incremental update directly to Rust CRDT (not full state)
        await applyIncrementalUpdateToRust(update);
      }
    });

    syncBridge.connect();
  }
}

/**
 * Ensure workspaceYDoc exists and is loaded with Rust CRDT state.
 */
async function ensureWorkspaceYDoc(): Promise<void> {
  if (workspaceYDoc) return;
  if (!rustApi) return;

  workspaceYDoc = new Y.Doc();

  // Load initial state from Rust CRDT
  try {
    const fullState = await rustApi.getFullState('workspace');
    console.log('[WorkspaceCrdtBridge] Loading initial state from Rust, bytes:', fullState.length);
    if (fullState.length > 0) {
      Y.applyUpdate(workspaceYDoc, fullState, 'rust');
      const filesMap = workspaceYDoc.getMap('files');
      console.log('[WorkspaceCrdtBridge] Loaded Y.Doc has', filesMap.size, 'files');
    }
    lastStateVector = await rustApi.getSyncState('workspace');
    console.log('[WorkspaceCrdtBridge] Initial state vector stored, bytes:', lastStateVector.length);
  } catch (error) {
    console.warn('[WorkspaceCrdtBridge] Failed to load workspace state:', error);
  }
}

/**
 * Apply an incremental update to Rust CRDT and notify file changes.
 * Reads the updated files from the Y.Doc and syncs them to Rust.
 * Also writes file content to disk if _body is present (for guests receiving synced files).
 */
async function applyIncrementalUpdateToRust(_update: Uint8Array): Promise<void> {
  if (!rustApi || !workspaceYDoc) return;

  try {
    // Read files from the Y.Doc (which has already had the update applied)
    // and sync to Rust CRDT
    const filesMap = workspaceYDoc.getMap('files');
    console.log('[WorkspaceCrdtBridge] Applying incremental update, files in Y.Doc:', filesMap.size);

    for (const [path, value] of filesMap.entries()) {
      const metadataObj = value as Record<string, unknown>;
      const extraObj = metadataObj.extra as Record<string, unknown> | undefined;

      // Extract _body from extra (if present) before creating metadata
      const body = extraObj?._body as string | undefined;

      // Create clean extra without _body (we don't store body in metadata)
      const cleanExtra: Record<string, unknown> = {};
      if (extraObj) {
        for (const [key, val] of Object.entries(extraObj)) {
          if (key !== '_body') {
            cleanExtra[key] = val;
          }
        }
      }

      const metadata: FileMetadata = {
        title: (metadataObj.title as string | null) ?? null,
        part_of: (metadataObj.part_of as string | null) ?? null,
        contents: Array.isArray(metadataObj.contents) ? metadataObj.contents as string[] : null,
        attachments: Array.isArray(metadataObj.attachments) ? metadataObj.attachments as BinaryRef[] : [],
        deleted: (metadataObj.deleted as boolean) ?? false,
        audience: Array.isArray(metadataObj.audience) ? metadataObj.audience as string[] : null,
        description: (metadataObj.description as string | null) ?? null,
        extra: cleanExtra as FileMetadata['extra'],
        modified_at: metadataObj.modified_at ? BigInt(metadataObj.modified_at as number) : BigInt(Date.now()),
      };

      await rustApi.setFile(path, metadata);

      // If we have body content and API is available, write file to disk
      // This is needed for guests who receive synced files but don't have them on disk
      if (body !== undefined && backendApi) {
        // For guests, use isolated storage path to avoid overwriting their workspace
        const storagePath = getGuestStoragePath(path);
        try {
          const exists = await backendApi.fileExists(storagePath);
          if (!exists) {
            // File doesn't exist - create it with the synced content
            console.log('[WorkspaceCrdtBridge] Creating file on disk (incremental):', storagePath);
            await writeFileWithFrontmatter(storagePath, metadata, body);
          } else {
            // File exists - update just the body content
            console.log('[WorkspaceCrdtBridge] Updating file content on disk (incremental):', storagePath);
            await backendApi.saveEntry(storagePath, body);
          }
        } catch (e) {
          console.warn('[WorkspaceCrdtBridge] Failed to write file to disk:', storagePath, e);
        }

        // Notify about body change so the editor can reload if this is the current file
        // Pass storage path so the listener can match directly against currentEntry.path
        notifyBodyChange(storagePath, body);
      }

      notifyFileChange(path, metadata);
    }

    // Also notify session sync for incremental updates
    notifySessionSync();
  } catch (error) {
    console.error('[WorkspaceCrdtBridge] Failed to apply incremental update to Rust:', error);
  }
}

/**
 * Sync full Y.Doc state to Rust CRDT (only for initial sync).
 * Use applyIncrementalUpdateToRust() for subsequent updates.
 * Also writes file content to disk if _body is present (for guests receiving synced files).
 */
async function syncFullStateToRust(): Promise<void> {
  if (!workspaceYDoc || !rustApi) return;

  try {
    // Read files directly from the Y.Doc's files map and sync to Rust
    // This is more reliable than applying Y.js updates which may not be
    // interpreted correctly by the Rust CRDT backend
    const filesMap = workspaceYDoc.getMap('files');
    console.log('[WorkspaceCrdtBridge] Syncing Y.Doc files to Rust, count:', filesMap.size);

    for (const [path, value] of filesMap.entries()) {
      const metadataObj = value as Record<string, unknown>;
      const extraObj = metadataObj.extra as Record<string, unknown> | undefined;

      // Extract _body from extra (if present) before creating metadata
      const body = extraObj?._body as string | undefined;

      // Create clean extra without _body (we don't store body in metadata)
      const cleanExtra: Record<string, unknown> = {};
      if (extraObj) {
        for (const [key, val] of Object.entries(extraObj)) {
          if (key !== '_body') {
            cleanExtra[key] = val;
          }
        }
      }

      const metadata: FileMetadata = {
        title: (metadataObj.title as string | null) ?? null,
        part_of: (metadataObj.part_of as string | null) ?? null,
        contents: Array.isArray(metadataObj.contents) ? metadataObj.contents as string[] : null,
        attachments: Array.isArray(metadataObj.attachments) ? metadataObj.attachments as BinaryRef[] : [],
        deleted: (metadataObj.deleted as boolean) ?? false,
        audience: Array.isArray(metadataObj.audience) ? metadataObj.audience as string[] : null,
        description: (metadataObj.description as string | null) ?? null,
        extra: cleanExtra as FileMetadata['extra'],
        modified_at: metadataObj.modified_at ? BigInt(metadataObj.modified_at as number) : BigInt(Date.now()),
      };

      console.log('[WorkspaceCrdtBridge] Syncing file to Rust:', path);
      await rustApi.setFile(path, metadata);

      // If we have body content and API is available, write file to disk
      // This is needed for guests who receive synced files but don't have them on disk
      if (body !== undefined && backendApi) {
        // For guests, use isolated storage path to avoid overwriting their workspace
        const storagePath = getGuestStoragePath(path);
        try {
          const exists = await backendApi.fileExists(storagePath);
          if (!exists) {
            // File doesn't exist - create it with the synced content
            console.log('[WorkspaceCrdtBridge] Creating file on disk:', storagePath);
            await writeFileWithFrontmatter(storagePath, metadata, body);
          } else {
            // File exists - update just the body content
            console.log('[WorkspaceCrdtBridge] Updating file content on disk:', storagePath);
            await backendApi.saveEntry(storagePath, body);
          }
        } catch (e) {
          console.warn('[WorkspaceCrdtBridge] Failed to write file to disk:', storagePath, e);
        }

        // Notify about body change so the editor can reload if this is the current file
        // Pass storage path so the listener can match directly against currentEntry.path
        notifyBodyChange(storagePath, body);
      }

      notifyFileChange(path, metadata);
    }

    console.log('[WorkspaceCrdtBridge] Finished syncing', filesMap.size, 'files to Rust');

    // Notify listeners that session data has been synced
    notifySessionSync();
  } catch (error) {
    console.error('[WorkspaceCrdtBridge] Failed to sync full state to Rust:', error);
  }
}

/**
 * Write a file to disk with frontmatter and body content.
 * Used when syncing files to guests who don't have the file on disk.
 */
async function writeFileWithFrontmatter(path: string, metadata: FileMetadata, body: string): Promise<void> {
  if (!backendApi) return;

  // Build frontmatter YAML
  const frontmatterLines: string[] = [];

  if (metadata.title) {
    frontmatterLines.push(`title: ${yamlString(metadata.title)}`);
  }
  if (metadata.part_of) {
    frontmatterLines.push(`part_of: ${yamlString(metadata.part_of)}`);
  }
  if (metadata.contents && metadata.contents.length > 0) {
    frontmatterLines.push(`contents:`);
    for (const item of metadata.contents) {
      frontmatterLines.push(`  - ${yamlString(item)}`);
    }
  }
  if (metadata.audience && metadata.audience.length > 0) {
    frontmatterLines.push(`audience:`);
    for (const item of metadata.audience) {
      frontmatterLines.push(`  - ${yamlString(item)}`);
    }
  }
  if (metadata.description) {
    frontmatterLines.push(`description: ${yamlString(metadata.description)}`);
  }

  // Add extra properties (excluding internal ones)
  if (metadata.extra) {
    for (const [key, value] of Object.entries(metadata.extra)) {
      if (!key.startsWith('_')) {
        frontmatterLines.push(`${key}: ${yamlValue(value)}`);
      }
    }
  }

  // Combine frontmatter and body
  const content = frontmatterLines.length > 0
    ? `---\n${frontmatterLines.join('\n')}\n---\n\n${body}`
    : body;

  await backendApi.writeFile(path, content);
}

/**
 * Format a string for YAML (quote if necessary).
 */
function yamlString(value: string): string {
  // Check if the string needs quoting
  if (/[:#\[\]{}|>&*!?'"%@`]/.test(value) ||
      value.includes('\n') ||
      value.startsWith(' ') ||
      value.endsWith(' ') ||
      value === '' ||
      /^[\d.]+$/.test(value) ||  // Looks like a number
      /^(true|false|null|yes|no|on|off)$/i.test(value)) {  // YAML keywords
    // Use double quotes and escape internal quotes
    return `"${value.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
  }
  return value;
}

/**
 * Format a value for YAML.
 */
function yamlValue(value: unknown): string {
  if (value === null || value === undefined) {
    return 'null';
  }
  if (typeof value === 'string') {
    return yamlString(value);
  }
  if (typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }
  if (Array.isArray(value)) {
    return `[${value.map(v => yamlValue(v)).join(', ')}]`;
  }
  if (typeof value === 'object') {
    return JSON.stringify(value);
  }
  return String(value);
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
 * Set the backend API for file operations.
 * This is used to write synced file content to disk for guests.
 */
export function setBackendApi(api: Api): void {
  backendApi = api;
}

/**
 * Get the storage path for a file in guest mode.
 * Guests have isolated storage at guest/{joinCode}/... to avoid overwriting their workspace.
 * Returns the original path for hosts.
 */
function getGuestStoragePath(originalPath: string): string {
  const isGuest = shareSessionStore.isGuest;
  const joinCode = shareSessionStore.joinCode;
  console.log('[WorkspaceCrdtBridge] getGuestStoragePath:', { originalPath, isGuest, joinCode, mode: shareSessionStore.mode });

  if (!isGuest || !joinCode) {
    return originalPath;
  }
  // Guest storage is isolated under guest/{joinCode}/
  const guestPath = `guest/${joinCode}/${originalPath}`;
  console.log('[WorkspaceCrdtBridge] Using guest path:', guestPath);
  return guestPath;
}

/**
 * Convert a guest storage path back to the canonical path.
 * This strips the guest/{joinCode}/ prefix if present.
 * Used when syncing to Y.Doc to ensure consistent keys across host and guest.
 */
function getCanonicalPath(storagePath: string): string {
  const isGuest = shareSessionStore.isGuest;
  const joinCode = shareSessionStore.joinCode;

  if (!isGuest || !joinCode) {
    return storagePath;
  }

  // Strip guest/{joinCode}/ prefix if present
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
 * Start syncing with a share session.
 * This will connect to the server with the session code.
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

  // Ensure workspaceYDoc exists
  await ensureWorkspaceYDoc();

  if (!workspaceYDoc) {
    console.error('[WorkspaceCrdtBridge] Failed to create workspace Y.Doc for session');
    return;
  }

  // If we're the host, populate the Y.Doc with all files from Rust CRDT
  // This is necessary because the Rust CRDT may store files in a format
  // that isn't automatically reflected in getFullState() Y.js encoding
  if (isHost && rustApi) {
    const files = await rustApi.listFiles(false);
    console.log('[WorkspaceCrdtBridge] Host populating Y.Doc with', files.length, 'files from Rust');

    const filesMap = workspaceYDoc.getMap('files');
    for (const [path, metadata] of files) {
      // Convert FileMetadata to a plain object for Y.js storage
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
    }
    console.log('[WorkspaceCrdtBridge] Y.Doc now has', filesMap.size, 'files');
  }

  const workspaceDocName = 'workspace';
  console.log('[WorkspaceCrdtBridge] Creating session sync bridge:', sessionServerUrl, 'doc:', workspaceDocName, 'session:', sessionCode);

  syncBridge = createSimpleSyncBridge({
    serverUrl: sessionServerUrl,
    docName: workspaceDocName,
    doc: workspaceYDoc,
    sessionCode: sessionCode,
    sendInitialState: isHost, // Host sends their state to server
    onStatusChange: (connected) => {
      console.log('[WorkspaceCrdtBridge] Session sync status:', connected);
    },
    onSynced: async () => {
      console.log('[WorkspaceCrdtBridge] Session sync complete');
      await syncFullStateToRust();
    },
  });

  // Set up listener for remote updates
  workspaceYDoc.on('update', async (update: Uint8Array, origin: unknown) => {
    if (origin === 'server') {
      console.log('[WorkspaceCrdtBridge] Session remote update received:', update.length, 'bytes');
      await applyIncrementalUpdateToRust(update);
    }
  });

  syncBridge.connect();
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
  syncBridge?: SimpleSyncBridge;
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

    // Connect sync bridge if provided (and workspace server sync is enabled)
    if (WORKSPACE_SERVER_SYNC_ENABLED) {
      if (syncBridge) {
        syncBridge.connect();
      } else if (serverUrl && rustApi) {
        // Ensure workspaceYDoc exists and create sync bridge
        await ensureWorkspaceYDoc();

        if (workspaceYDoc) {
          const workspaceDocName = _workspaceId ? `${_workspaceId}:workspace` : 'workspace';
          console.log('[WorkspaceCrdtBridge] Creating sync bridge during init:', serverUrl, 'docName:', workspaceDocName);
          syncBridge = createSimpleSyncBridge({
            serverUrl,
            docName: workspaceDocName,
            doc: workspaceYDoc,
            onStatusChange: (connected) => {
              console.log('[WorkspaceCrdtBridge] Connection status:', connected);
            },
            onSynced: async () => {
              console.log('[WorkspaceCrdtBridge] Initial sync complete');
              // Full state sync is appropriate here - this is the initial sync
              await syncFullStateToRust();
            },
          });

          // Set up listener for remote updates - apply incremental updates only
          workspaceYDoc.on('update', async (update: Uint8Array, origin: unknown) => {
            if (origin === 'server') {
              console.log('[WorkspaceCrdtBridge] Remote update received:', update.length, 'bytes');
              // Apply the incremental update directly to Rust CRDT (not full state)
              await applyIncrementalUpdateToRust(update);
            }
          });

          syncBridge.connect();
        }
      }
    } else {
      console.log('[WorkspaceCrdtBridge] Workspace server sync disabled during init');
    }

    // Initialize P2P if enabled (reuses workspaceYDoc if already created)
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

  // Reuse workspaceYDoc if already created (by ensureWorkspaceYDoc), otherwise create new
  if (!workspaceYDoc) {
    await ensureWorkspaceYDoc();
  }

  if (!workspaceYDoc) {
    console.error('[WorkspaceCrdtBridge] Failed to create workspace Y.Doc for P2P');
    return;
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
 */
export async function populateCrdtFromFiles(files: Array<{ path: string; metadata: Partial<FileMetadata> }>): Promise<void> {
  if (!rustApi) {
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

    await rustApi.setFile(path, fullMetadata);
    console.log('[WorkspaceCrdtBridge] Added file to CRDT:', path);
  }

  console.log('[WorkspaceCrdtBridge] CRDT population complete');
}

/**
 * Build a tree from CRDT file metadata.
 * This is used for guests who don't have files on disk but have synced metadata.
 */
export async function getTreeFromCrdt(): Promise<TreeNode | null> {
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
  await rustApi.setFile(path, metadata);
  console.log('[WorkspaceCrdtBridge] Rust setFile complete, now syncing...');
  // Sync to Y.Doc so session/P2P peers receive the update
  syncFileToYDoc(path, metadata);
  // Also sync to P2P Y.Doc if enabled
  await syncRustChangesToP2P();
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
 * Update file body content in the session Y.Doc.
 * This allows live sync of file content changes during share sessions.
 * Called when the editor saves content.
 */
export function updateFileBodyInYDoc(path: string, body: string): void {
  if (!workspaceYDoc || !_sessionCode) {
    return; // No session active
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
    // Sync to Y.Doc so session/P2P peers receive the update
    syncFileToYDoc(path, updated);
    // Also sync to P2P Y.Doc if enabled
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
 * Notify all body change callbacks.
 */
function notifyBodyChange(path: string, body: string): void {
  console.log('[WorkspaceCrdtBridge] Notifying body change callbacks, path:', path, 'callbacks:', bodyChangeCallbacks.size);
  for (const callback of bodyChangeCallbacks) {
    try {
      callback(path, body);
    } catch (error) {
      console.error('[WorkspaceCrdtBridge] Body change callback error:', error);
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
  console.log('=== Sync Bridge Debug ===');
  console.log('serverUrl:', serverUrl);
  console.log('syncBridge:', syncBridge ? 'exists' : 'null');
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
