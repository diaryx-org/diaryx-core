/**
 * Collaboration Bridge - manages collaborative editing sessions using Rust CRDT.
 *
 * This replaces collaborationUtils.ts by routing through the Rust CRDT backend
 * instead of using JS Y.js directly.
 *
 * Key features:
 * - YDocProxy for TipTap integration (JS Y.Doc synced with Rust CRDT)
 * - HocuspocusBridge for real-time sync
 * - Support for both workspace and per-file CRDTs
 */

import type { RustCrdtApi } from './rustCrdtApi';
import { YDocProxy, createYDocProxy } from './yDocProxy';
import { HocuspocusBridge, createHocuspocusBridge } from './hocuspocusBridge';

// ============================================================================
// Types
// ============================================================================

export interface CollaborationSession {
  docName: string;
  yDocProxy: YDocProxy;
  bridge: HocuspocusBridge | null;
  saveTimeout: ReturnType<typeof setTimeout> | null;
  onMarkdownSave?: (markdown: string) => void;
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

const SAVE_DEBOUNCE_MS = 5000;
let connectionStatusCallback: ((connected: boolean) => void) | null = null;

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
 * Configure the collaboration server.
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
        session.bridge = createBridge(session.docName, session.yDocProxy);
        session.bridge?.connect();
      } else {
        session.bridge = null;
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
  bridge: HocuspocusBridge | null;
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
    };
  }

  // Create room name
  const docName = config.workspaceId
    ? `${config.workspaceId}:doc:${documentPath}`
    : `doc:${documentPath}`;

  // Create YDocProxy
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
  });

  // Create bridge if server configured
  const bridge = createBridge(docName, yDocProxy);

  // Store session
  const session: CollaborationSession = {
    docName,
    yDocProxy,
    bridge,
    saveTimeout: null,
    onMarkdownSave: options?.onMarkdownSave,
  };
  sessions.set(documentPath, session);

  // Connect bridge
  if (bridge) {
    await bridge.connect();
  }

  return { yDocProxy, bridge };
}

/**
 * Release a collaborative document session.
 */
export async function releaseDocument(documentPath: string): Promise<void> {
  const session = sessions.get(documentPath);
  if (!session) return;

  // Save before releasing
  try {
    await session.yDocProxy.save();
  } catch (error) {
    console.error('[CollaborationBridge] Failed to save on release:', error);
  }

  // Clear save timeout
  if (session.saveTimeout) {
    clearTimeout(session.saveTimeout);
  }

  // Disconnect and cleanup
  session.bridge?.destroy();
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
 * Check if any session is connected.
 */
export function isConnected(): boolean {
  for (const session of sessions.values()) {
    if (session.bridge?.isSynced()) {
      return true;
    }
  }
  return false;
}

// ============================================================================
// Helpers
// ============================================================================

function createBridge(docName: string, yDocProxy: YDocProxy): HocuspocusBridge | null {
  if (!config.serverUrl || !rustApi) {
    return null;
  }

  return createHocuspocusBridge({
    url: config.serverUrl,
    docName,
    rustApi,
    yDocProxy,
    token: config.authToken,
    onStatusChange: (connected) => {
      connectionStatusCallback?.(connected);
    },
    onSynced: () => {
      console.log(`[CollaborationBridge] Synced: ${docName}`);
    },
  });
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
    session.yDocProxy.destroy();
  }
  sessions.clear();
  rustApi = null;
}

// Register cleanup on page unload
if (typeof window !== 'undefined') {
  window.addEventListener('beforeunload', () => {
    cleanup();
  });
}
