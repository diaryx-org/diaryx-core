/**
 * Collaboration Bridge - manages collaborative editing sessions using Rust CRDT.
 *
 * This replaces collaborationUtils.ts by routing through the Rust CRDT backend
 * instead of using JS Y.js directly.
 *
 * Key features:
 * - YDocProxy for TipTap integration (JS Y.Doc synced with Rust CRDT)
 * - HocuspocusBridge for real-time sync (server-based)
 * - P2P sync via y-webrtc (peer-to-peer, no server required)
 * - Support for both workspace and per-file CRDTs
 */

import type { RustCrdtApi } from './rustCrdtApi';
import { YDocProxy, createYDocProxy } from './yDocProxy';
import { SimpleSyncBridge, createSimpleSyncBridge } from './simpleSyncBridge';
import {
  isP2PEnabled,
  createP2PProvider,
  destroyP2PProvider,
} from './p2pSyncBridge';
import type { WebrtcProvider } from 'y-webrtc';

// ============================================================================
// Types
// ============================================================================

export interface CollaborationSession {
  docName: string;
  yDocProxy: YDocProxy;
  bridge: SimpleSyncBridge | null;
  p2pProvider: WebrtcProvider | null;
  saveTimeout: ReturnType<typeof setTimeout> | null;
  onMarkdownSave?: (markdown: string) => void;
  onRemoteContentChange?: (content: string) => void;
}

export interface CollaborationConfig {
  serverUrl: string | null;
  workspaceId: string | null;
  authToken?: string;
}

// ============================================================================
// State
// ============================================================================

const sessions = new Map<string, CollaborationSession>();
let rustApi: RustCrdtApi | null = null;
let config: CollaborationConfig = {
  serverUrl: null,
  workspaceId: null,
};

// Active session code for share sessions - when set, per-document sync uses session scope
let activeSessionCode: string | null = null;

const SAVE_DEBOUNCE_MS = 5000;
let connectionStatusCallback: ((connected: boolean) => void) | null = null;
let p2pStatusUnsubscribe: (() => void) | null = null;

// ============================================================================
// Configuration
// ============================================================================

/**
 * Initialize the collaboration system with the Rust CRDT API.
 */
export function initCollaboration(api: RustCrdtApi): void {
  rustApi = api;
}

/**
 * Configure the collaboration server (for simple sync).
 */
export function setCollaborationServer(url: string | null): void {
  const previousUrl = config.serverUrl;
  config.serverUrl = url;

  // Reconnect all sessions when server changes
  if (previousUrl !== url) {
    console.log('[CollaborationBridge] Server URL changed, reconnecting sessions...');
    for (const session of sessions.values()) {
      if (session.bridge) {
        session.bridge.disconnect();
      }
      if (url) {
        session.bridge = createBridge(session.docName, session.yDocProxy, session.onRemoteContentChange);
        session.bridge?.connect();
      } else {
        session.bridge = null;
      }
    }
  }
}

/**
 * Refresh P2P providers for all sessions.
 * Call this after enabling/disabling P2P sync.
 */
export function refreshP2PProviders(): void {
  console.log('[CollaborationBridge] Refreshing P2P providers...');

  for (const [path, session] of sessions.entries()) {
    // Destroy existing P2P provider
    if (session.p2pProvider) {
      destroyP2PProvider(session.docName);
      session.p2pProvider = null;
    }

    // Create new P2P provider if enabled
    if (isP2PEnabled()) {
      session.p2pProvider = createP2PProvider(session.yDocProxy.getYDoc(), session.docName);
      if (session.p2pProvider) {
        console.log(`[CollaborationBridge] P2P provider created for: ${path}`);
      }
    }
  }
}

/**
 * Get the current server URL.
 */
export function getCollaborationServer(): string | null {
  return config.serverUrl;
}

/**
 * Set the workspace ID for room naming in collaboration.
 */
export function setCollaborationWorkspaceId(workspaceId: string | null): void {
  config.workspaceId = workspaceId;
}

/**
 * Get the current workspace ID for collaboration.
 */
export function getCollaborationWorkspaceId(): string | null {
  return config.workspaceId;
}

/**
 * Set auth token for server authentication.
 */
export function setAuthToken(token: string | undefined): void {
  config.authToken = token;
}

/**
 * Set the active session code for share sessions.
 * When set, per-document sync will be scoped to this session.
 * Pass null to disable session-scoped sync.
 */
export function setActiveSessionCode(code: string | null): void {
  const previousCode = activeSessionCode;
  activeSessionCode = code;

  console.log('[CollaborationBridge] Active session code changed:', previousCode, '->', code);

  // Reconnect all sessions with new session code
  if (previousCode !== code && config.serverUrl) {
    console.log('[CollaborationBridge] Reconnecting sessions with new session code...');
    for (const [path, session] of sessions.entries()) {
      // Destroy existing bridge
      if (session.bridge) {
        session.bridge.destroy();
      }

      // Create new bridge with updated session code
      session.bridge = createBridge(session.docName, session.yDocProxy, session.onRemoteContentChange);
      session.bridge?.connect();
      console.log('[CollaborationBridge] Reconnected session:', path, 'with session code:', code);
    }
  }
}

/**
 * Get the current active session code.
 */
export function getActiveSessionCode(): string | null {
  return activeSessionCode;
}

/**
 * Set callback for connection status changes.
 */
export function setConnectionStatusCallback(
  callback: ((connected: boolean) => void) | null
): void {
  connectionStatusCallback = callback;
}

// ============================================================================
// Session Management
// ============================================================================

export interface GetDocumentOptions {
  onMarkdownSave?: (markdown: string) => void;
  initialContent?: string;
  /** Callback when remote content changes (for re-rendering editor) */
  onRemoteContentChange?: (content: string) => void;
}

/**
 * Get or create a collaborative document session.
 *
 * Returns a YDocProxy that can be used with TipTap's Collaboration extension.
 */
export async function getCollaborativeDocument(
  documentPath: string,
  options?: GetDocumentOptions
): Promise<{
  yDocProxy: YDocProxy;
  bridge: SimpleSyncBridge | null;
  p2pProvider: WebrtcProvider | null;
}> {
  if (!rustApi) {
    throw new Error('Collaboration not initialized. Call initCollaboration first.');
  }

  // Check for existing session
  const existing = sessions.get(documentPath);
  if (existing) {
    return {
      yDocProxy: existing.yDocProxy,
      bridge: existing.bridge,
      p2pProvider: existing.p2pProvider,
    };
  }

  // Create room name
  const docName = config.workspaceId
    ? `${config.workspaceId}:doc:${documentPath}`
    : `doc:${documentPath}`;

  // Create YDocProxy first
  const yDocProxy = await createYDocProxy({
    docName,
    rustApi,
    initialContent: options?.initialContent,
    onContentChange: (content) => {
      // Debounced save
      const session = sessions.get(documentPath);
      if (session?.onMarkdownSave) {
        if (session.saveTimeout) {
          clearTimeout(session.saveTimeout);
        }
        session.saveTimeout = setTimeout(() => {
          session.onMarkdownSave?.(content);
        }, SAVE_DEBOUNCE_MS);
      }
    },
    // Note: onLocalUpdate not needed - SimpleSyncBridge handles Y.Doc updates directly
  });

  // Create simple sync bridge (syncs the Y.Doc directly)
  const bridge = createBridge(docName, yDocProxy, options?.onRemoteContentChange);

  // Create P2P provider if P2P sync is enabled
  const p2pProvider = isP2PEnabled()
    ? createP2PProvider(yDocProxy.getYDoc(), docName)
    : null;

  if (p2pProvider) {
    console.log(`[CollaborationBridge] P2P provider created for: ${documentPath}`);
  }

  // Store session
  const session: CollaborationSession = {
    docName,
    yDocProxy,
    bridge,
    p2pProvider,
    saveTimeout: null,
    onMarkdownSave: options?.onMarkdownSave,
    onRemoteContentChange: options?.onRemoteContentChange,
  };
  sessions.set(documentPath, session);

  // Connect bridge
  if (bridge) {
    await bridge.connect();
  }

  return { yDocProxy, bridge, p2pProvider };
}

/**
 * Release a collaborative document session.
 * Ensures any pending debounced save is executed before cleanup.
 */
export async function releaseDocument(documentPath: string): Promise<void> {
  const session = sessions.get(documentPath);
  if (!session) return;

  // Execute pending debounced save immediately before cleanup
  // This prevents data loss when releasing while a save is pending
  if (session.saveTimeout) {
    clearTimeout(session.saveTimeout);
    session.saveTimeout = null;

    // Execute the pending save callback immediately
    if (session.onMarkdownSave && !session.yDocProxy.isDestroyed()) {
      try {
        const content = session.yDocProxy.getContent();
        session.onMarkdownSave(content);
        console.log('[CollaborationBridge] Executed pending save on release:', documentPath);
      } catch (error) {
        console.error('[CollaborationBridge] Failed to execute pending save:', error);
      }
    }
  }

  // Save CRDT state before releasing
  try {
    await session.yDocProxy.save();
  } catch (error) {
    console.error('[CollaborationBridge] Failed to save CRDT on release:', error);
  }

  // Disconnect and cleanup
  session.bridge?.destroy();
  if (session.p2pProvider) {
    destroyP2PProvider(session.docName);
  }
  session.yDocProxy.destroy();
  sessions.delete(documentPath);
}

/**
 * Release all document sessions.
 */
export async function releaseAllDocuments(): Promise<void> {
  const paths = Array.from(sessions.keys());
  await Promise.all(paths.map((path) => releaseDocument(path)));
}

/**
 * Check if a document session exists.
 */
export function hasSession(documentPath: string): boolean {
  return sessions.has(documentPath);
}

/**
 * Get the number of active sessions.
 */
export function getSessionCount(): number {
  return sessions.size;
}

// ============================================================================
// Connection Management
// ============================================================================

/**
 * Disconnect all sessions from the sync server.
 */
export function disconnectAll(): void {
  for (const session of sessions.values()) {
    session.bridge?.disconnect();
  }
}

/**
 * Reconnect all sessions to the sync server.
 */
export function reconnectAll(): void {
  for (const session of sessions.values()) {
    session.bridge?.connect();
  }
}

/**
 * Check if any session is connected (via Hocuspocus or P2P).
 */
export function isConnected(): boolean {
  for (const session of sessions.values()) {
    // Check Hocuspocus connection
    if (session.bridge?.isSynced()) {
      return true;
    }
    // Check P2P connection
    if (session.p2pProvider?.connected) {
      return true;
    }
  }
  return false;
}

// ============================================================================
// Helpers
// ============================================================================

function createBridge(
  docName: string,
  yDocProxy: YDocProxy,
  onRemoteContentChange?: (content: string) => void
): SimpleSyncBridge | null {
  if (!config.serverUrl) {
    return null;
  }

  // Use session-scoped sync if active session code is set
  const sessionCode = activeSessionCode ?? undefined;
  if (sessionCode) {
    console.log(`[CollaborationBridge] Creating session-scoped bridge for ${docName} with session: ${sessionCode}`);
  }

  const bridge = createSimpleSyncBridge({
    serverUrl: config.serverUrl,
    docName,
    doc: yDocProxy.getYDoc(),
    sessionCode, // Pass session code for session-scoped sync
    authToken: config.authToken, // Pass auth token for authenticated sync
    onStatusChange: (connected) => {
      connectionStatusCallback?.(connected);
    },
    onSynced: () => {
      console.log(`[CollaborationBridge] Synced: ${docName}${sessionCode ? ` (session: ${sessionCode})` : ''}`);
      // Notify about remote content change after sync
      if (onRemoteContentChange) {
        const content = yDocProxy.getContent();
        console.log(`[CollaborationBridge] Remote content after sync, length: ${content.length}`);
        onRemoteContentChange(content);
      }
    },
  });

  // Also set up a listener for remote updates to notify content changes
  const doc = yDocProxy.getYDoc();
  doc.on('update', (_update: Uint8Array, origin: unknown) => {
    if (origin === 'server' && onRemoteContentChange) {
      const content = yDocProxy.getContent();
      console.log(`[CollaborationBridge] Remote update received, content length: ${content.length}`);
      onRemoteContentChange(content);
    }
  });

  return bridge;
}

// ============================================================================
// Cleanup
// ============================================================================

/**
 * Cleanup on page unload.
 */
export function cleanup(): void {
  for (const session of sessions.values()) {
    session.bridge?.destroy();
    if (session.p2pProvider) {
      destroyP2PProvider(session.docName);
    }
    session.yDocProxy.destroy();
  }
  sessions.clear();
  rustApi = null;

  // Cleanup P2P status subscription
  if (p2pStatusUnsubscribe) {
    p2pStatusUnsubscribe();
    p2pStatusUnsubscribe = null;
  }
}

// Register cleanup on page unload
if (typeof window !== 'undefined') {
  window.addEventListener('beforeunload', () => {
    cleanup();
  });
}
