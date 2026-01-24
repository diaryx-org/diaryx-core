/**
 * CRDT module for Diaryx web app.
 *
 * This module provides integration between the Rust CRDT backend
 * and the frontend, including:
 * - Type-safe API wrapper for CRDT operations
 * - Sync transport for real-time sync (WebSocket transport layer)
 * - Sync helpers for Rust sync manager commands
 * - Workspace CRDT bridge for UI integration
 */

export { RustCrdtApi, createCrdtApi } from './rustCrdtApi';
export {
  SyncTransport,
  createWorkspaceSyncTransport,
  createBodySyncTransport,
  type SyncTransportOptions,
} from './syncTransport';
// Export sync helpers (low-level Rust command wrappers) with namespace to avoid conflicts
export * as syncHelpers from './syncHelpers';

// Export workspace CRDT bridge (high-level UI-facing API)
export * from './workspaceCrdtBridge';

// ============================================================================
// Backwards Compatibility Aliases
// ============================================================================

// These aliases maintain backwards compatibility with code that imported from
// the now-deleted collaborationBridge.ts. They re-export workspace CRDT functions
// under their old names.

import {
  setWorkspaceServer,
  getWorkspaceServer,
  setWorkspaceId as setWorkspaceIdInternal,
  stopSessionSync,
  getSessionCode,
} from './workspaceCrdtBridge';

/**
 * @deprecated Use setWorkspaceServer instead
 */
export function setCollaborationServer(url: string | null): void {
  // Note: setWorkspaceServer is async but the original was sync.
  // The async part is only needed when workspace is already initialized.
  void setWorkspaceServer(url);
}

/**
 * @deprecated Use getWorkspaceServer instead
 */
export function getCollaborationServer(): string | null {
  return getWorkspaceServer();
}

/**
 * @deprecated Use setWorkspaceId instead
 */
export function setCollaborationWorkspaceId(workspaceId: string | null): void {
  setWorkspaceIdInternal(workspaceId);
}

/**
 * @deprecated Auth token is now read directly from authStore via getToken()
 * This function is a no-op for backwards compatibility.
 */
export function setAuthToken(_token: string | undefined): void {
  // No-op: workspaceCrdtBridge reads auth token directly from authStore.getToken()
  // This function exists only for backwards compatibility with callers that
  // used to call setAuthToken() from collaborationBridge.
}

/**
 * @deprecated Use startSessionSync instead
 */
export function setActiveSessionCode(code: string | null): void {
  if (code === null) {
    // Fire and forget - callers should use stopSessionSync() directly and await it
    void stopSessionSync();
  } else {
    // Note: startSessionSync requires a server URL and isHost flag.
    // This shim cannot fully replicate the old behavior.
    // Callers should migrate to using startSessionSync directly.
    console.warn('[CRDT] setActiveSessionCode is deprecated. Use startSessionSync/stopSessionSync instead.');
  }
}

/**
 * @deprecated Use getSessionCode instead
 */
export function getActiveSessionCode(): string | null {
  return getSessionCode();
}
