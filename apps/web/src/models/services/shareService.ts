/**
 * Share Service - Manages live collaboration session lifecycle
 *
 * This service handles:
 * - Creating share sessions (host mode)
 * - Joining share sessions (guest mode)
 * - Guest storage management (OPFS folder isolation)
 * - Session cleanup
 */

import { shareSessionStore } from '../stores/shareSessionStore.svelte';
import { collaborationStore } from '../stores/collaborationStore.svelte';
import { startSessionSync, stopSessionSync } from '$lib/crdt/workspaceCrdtBridge';
import { setActiveSessionCode } from '$lib/crdt/collaborationBridge';

// ============================================================================
// Types
// ============================================================================

export interface SessionCreatedResponse {
  type: 'session_created';
  joinCode: string;
  workspaceId: string;
}

export interface SessionJoinedResponse {
  type: 'session_joined';
  joinCode: string;
  workspaceId: string;
}

export interface PeerJoinedMessage {
  type: 'peer_joined';
  guestId: string;
  peerCount: number;
}

export interface PeerLeftMessage {
  type: 'peer_left';
  guestId: string;
  peerCount: number;
}

export interface ErrorResponse {
  type: 'error';
  message: string;
}

type ServerMessage = SessionCreatedResponse | SessionJoinedResponse | PeerJoinedMessage | PeerLeftMessage | ErrorResponse;

// ============================================================================
// Constants
// ============================================================================

const GUEST_STORAGE_PREFIX = 'guest';
const CONNECTION_TIMEOUT = 10000; // 10 seconds

// ============================================================================
// State
// ============================================================================

let currentServerUrl: string | null = null;

// ============================================================================
// Helpers
// ============================================================================

function getServerUrl(): string {
  // Use collaboration server URL from store, or default
  const storeUrl = collaborationStore.collaborationServerUrl;
  if (storeUrl) {
    // Convert http(s) to ws(s)
    return storeUrl.replace(/^http/, 'ws');
  }
  // Default to localhost for development
  return 'ws://localhost:1234';
}

function validateJoinCode(code: string): boolean {
  return /^[A-Z0-9]{8}-[A-Z0-9]{8}$/i.test(code);
}

function normalizeJoinCode(code: string): string {
  return code.toUpperCase().trim();
}

// ============================================================================
// Session Creation (Host Mode)
// ============================================================================

/**
 * Create a new share session for the current workspace.
 * Returns the join code that others can use to join.
 */
export async function createShareSession(workspaceId: string): Promise<string> {
  const serverUrl = getServerUrl();
  currentServerUrl = serverUrl;

  shareSessionStore.setConnecting(true);

  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      ws.close();
      shareSessionStore.setError('Connection timeout');
      reject(new Error('Connection timeout'));
    }, CONNECTION_TIMEOUT);

    const wsUrl = `${serverUrl}?action=create&workspaceId=${encodeURIComponent(workspaceId)}`;
    const ws = new WebSocket(wsUrl);

    ws.onopen = () => {
      console.log('[ShareService] Connected to server for session creation');
    };

    ws.onmessage = (event) => {
      try {
        const msg: ServerMessage = JSON.parse(event.data);

        if (msg.type === 'session_created') {
          clearTimeout(timeout);
          shareSessionStore.startHosting(msg.joinCode, msg.workspaceId);
          shareSessionStore.setSessionWs(ws);
          console.log('[ShareService] Session created:', msg.joinCode);

          // Start document sync for this session (isHost=true to send initial state)
          const syncServerUrl = currentServerUrl!;
          startSessionSync(syncServerUrl, msg.joinCode, true);

          // Set active session code for per-document sync (real-time editing)
          setActiveSessionCode(msg.joinCode);

          resolve(msg.joinCode);
        } else if (msg.type === 'peer_joined') {
          shareSessionStore.addPeer(msg.guestId);
          console.log('[ShareService] Peer joined:', msg.guestId);
        } else if (msg.type === 'peer_left') {
          shareSessionStore.removePeer(msg.guestId);
          console.log('[ShareService] Peer left:', msg.guestId);
        } else if (msg.type === 'error') {
          clearTimeout(timeout);
          shareSessionStore.setError(msg.message);
          ws.close();
          reject(new Error(msg.message));
        }
      } catch (e) {
        console.error('[ShareService] Failed to parse message:', e);
      }
    };

    ws.onerror = (event) => {
      clearTimeout(timeout);
      console.error('[ShareService] WebSocket error:', event);
      shareSessionStore.setError('Connection failed');
      reject(new Error('Connection failed'));
    };

    ws.onclose = () => {
      clearTimeout(timeout);
      if (shareSessionStore.mode === 'hosting' && shareSessionStore.connected) {
        // Unexpected disconnect while hosting
        shareSessionStore.setConnected(false);
        shareSessionStore.setError('Disconnected from server');
      }
    };
  });
}

// ============================================================================
// Session Join (Guest Mode)
// ============================================================================

/**
 * Join an existing share session using a join code.
 * Returns the workspace ID of the session.
 */
export async function joinShareSession(joinCode: string): Promise<string> {
  const normalizedCode = normalizeJoinCode(joinCode);

  if (!validateJoinCode(normalizedCode)) {
    throw new Error('Invalid join code format');
  }

  const serverUrl = getServerUrl();
  currentServerUrl = serverUrl;

  shareSessionStore.setConnecting(true);

  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      ws.close();
      shareSessionStore.setError('Connection timeout');
      reject(new Error('Connection timeout'));
    }, CONNECTION_TIMEOUT);

    const guestId = `guest-${Date.now()}`;
    const wsUrl = `${serverUrl}?action=join&code=${encodeURIComponent(normalizedCode)}&guestId=${encodeURIComponent(guestId)}`;
    const ws = new WebSocket(wsUrl);

    ws.onopen = () => {
      console.log('[ShareService] Connected to server for session join');
    };

    ws.onmessage = (event) => {
      try {
        const msg: ServerMessage = JSON.parse(event.data);

        if (msg.type === 'session_joined') {
          clearTimeout(timeout);
          shareSessionStore.startGuest(msg.joinCode, msg.workspaceId);
          shareSessionStore.setSessionWs(ws);
          console.log('[ShareService] Joined session:', msg.joinCode);

          // Start document sync for this session (isHost=false, receive state from server)
          const syncServerUrl = currentServerUrl!;
          startSessionSync(syncServerUrl, msg.joinCode, false);

          // Set active session code for per-document sync (real-time editing)
          setActiveSessionCode(msg.joinCode);

          resolve(msg.workspaceId);
        } else if (msg.type === 'error') {
          clearTimeout(timeout);
          shareSessionStore.setError(msg.message);
          ws.close();
          reject(new Error(msg.message));
        }
      } catch (e) {
        console.error('[ShareService] Failed to parse message:', e);
      }
    };

    ws.onerror = (event) => {
      clearTimeout(timeout);
      console.error('[ShareService] WebSocket error:', event);
      shareSessionStore.setError('Connection failed');
      reject(new Error('Connection failed'));
    };

    ws.onclose = () => {
      clearTimeout(timeout);
      if (shareSessionStore.mode === 'guest' && shareSessionStore.connected) {
        // Unexpected disconnect while guest
        shareSessionStore.setConnected(false);
        shareSessionStore.setError('Disconnected from session');
      }
    };
  });
}

// ============================================================================
// Session End
// ============================================================================

/**
 * End the current share session (works for both host and guest).
 */
export async function endShareSession(): Promise<void> {
  console.log('[ShareService] Ending share session');

  // Clean up guest storage if we were a guest
  // Must capture joinCode before endSession() clears it
  const joinCode = shareSessionStore.joinCode;
  const wasGuest = shareSessionStore.isGuest;

  // Stop document sync
  stopSessionSync();

  // Clear active session code for per-document sync
  setActiveSessionCode(null);

  // End the session (clears state)
  shareSessionStore.endSession();

  // Clean up guest storage after session ends
  if (wasGuest && joinCode) {
    console.log('[ShareService] Cleaning up guest storage...');
    await cleanupGuestStorage(joinCode);
  }
}

// ============================================================================
// Guest Storage Management
// ============================================================================

/**
 * Get the OPFS storage path for a guest session.
 */
export function getGuestStoragePath(joinCode: string): string {
  return `/${GUEST_STORAGE_PREFIX}/${joinCode}`;
}

/**
 * Check if we're currently in guest mode.
 */
export function isGuestMode(): boolean {
  return shareSessionStore.isGuest;
}

/**
 * Get the current session's join code.
 */
export function getCurrentJoinCode(): string | null {
  return shareSessionStore.joinCode;
}

/**
 * Get the server URL for document sync within the current session.
 */
export function getSessionSyncUrl(docName: string): string | null {
  if (!currentServerUrl || !shareSessionStore.joinCode) {
    return null;
  }
  return `${currentServerUrl}?doc=${encodeURIComponent(docName)}&session=${encodeURIComponent(shareSessionStore.joinCode)}`;
}

// ============================================================================
// Cleanup
// ============================================================================

/**
 * Clean up guest storage for a specific session.
 * This should be called when leaving a guest session.
 */
export async function cleanupGuestStorage(joinCode: string): Promise<void> {
  try {
    // Get OPFS root
    const root = await navigator.storage.getDirectory();

    // Try to get the guest folder
    try {
      const guestFolder = await root.getDirectoryHandle(GUEST_STORAGE_PREFIX);

      // Try to remove the session folder
      try {
        await guestFolder.removeEntry(joinCode, { recursive: true });
        console.log(`[ShareService] Cleaned up guest storage for session: ${joinCode}`);
      } catch {
        // Session folder doesn't exist, that's fine
      }

      // Check if guest folder is now empty
      let hasEntries = false;
      // Type assertion needed as TypeScript types may not include keys()
      for await (const _ of (guestFolder as any).keys()) {
        hasEntries = true;
        break;
      }

      // If empty, remove the guest folder too
      if (!hasEntries) {
        await root.removeEntry(GUEST_STORAGE_PREFIX);
        console.log('[ShareService] Removed empty guest folder');
      }
    } catch {
      // Guest folder doesn't exist, that's fine
    }
  } catch (e) {
    console.error('[ShareService] Failed to cleanup guest storage:', e);
  }
}

/**
 * Clean up all guest storage (used during app reset/cleanup).
 */
export async function cleanupAllGuestStorage(): Promise<void> {
  try {
    const root = await navigator.storage.getDirectory();
    try {
      await root.removeEntry(GUEST_STORAGE_PREFIX, { recursive: true });
      console.log('[ShareService] Cleaned up all guest storage');
    } catch {
      // Guest folder doesn't exist, that's fine
    }
  } catch (e) {
    console.error('[ShareService] Failed to cleanup all guest storage:', e);
  }
}
