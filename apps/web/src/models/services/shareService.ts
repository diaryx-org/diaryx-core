/**
 * Share Service - Manages live collaboration session lifecycle
 *
 * This service handles:
 * - Creating share sessions (host mode)
 * - Joining share sessions (guest mode)
 * - Guest backend management (in-memory filesystem for web guests)
 * - Session cleanup
 */

import { shareSessionStore } from '../stores/shareSessionStore.svelte';
import { collaborationStore } from '../stores/collaborationStore.svelte';
import { workspaceStore } from '../stores/workspaceStore.svelte';
import { startSessionSync, stopSessionSync, setBackendApi, setBackend, type SessionSyncCallbacks } from '$lib/crdt';
import { createGuestBackend, type WorkerBackendNew } from '$lib/backend/workerBackendNew';
import { createApi, type Api } from '$lib/backend/api';
import { isTauri, type Backend } from '$lib/backend/interface';
import type { TauriBackend } from '$lib/backend/tauri';
import { getToken } from '$lib/auth/authStore.svelte';

// ============================================================================
// Types
// ============================================================================

export interface SessionCreatedResponse {
  type: 'session_created';
  joinCode: string;
  workspaceId: string;
  readOnly: boolean;
}

export interface SessionJoinedResponse {
  type: 'session_joined';
  joinCode: string;
  workspaceId: string;
  readOnly: boolean;
}

export interface ReadOnlyChangedMessage {
  type: 'read_only_changed';
  readOnly: boolean;
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

export interface SessionEndedMessage {
  type: 'session_ended';
}

// ============================================================================
// Constants
// ============================================================================

const GUEST_STORAGE_PREFIX = 'guest';

// ============================================================================
// State
// ============================================================================

let currentServerUrl: string | null = null;

// Optional override for share session server URL (for users who haven't set up sync
// but want to use a custom/self-hosted server for live editing sessions).
let shareServerUrlOverride: string | null = null;

// Guest backend and API (for web guests using in-memory storage)
let guestBackend: WorkerBackendNew | null = null;
let guestApi: Api | null = null;

// Original backend (saved before switching to guest backend)
let originalBackend: Backend | null = null;

// ============================================================================
// Helpers
// ============================================================================

/**
 * Set a custom server URL for share sessions.
 * This overrides the collaboration store URL and the default.
 * Pass null to clear the override.
 */
export function setShareServerUrl(url: string | null): void {
  shareServerUrlOverride = url;
}

/**
 * Get the current share server URL (for display in UI).
 */
export function getShareServerUrl(): string {
  return shareServerUrlOverride || '';
}

/**
 * Get the base URL for the Rust sync server (for REST API calls).
 * Priority: share override > collaboration store > default.
 * Strips /sync2 suffix if present and ensures HTTP(S) protocol.
 */
function getBaseServerUrl(): string {
  const url = shareServerUrlOverride || collaborationStore.collaborationServerUrl;
  if (url) {
    // Strip /sync or /sync2 suffix and convert ws(s) to http(s)
    return url
      .replace(/\/sync2?$/, '')
      .replace(/^wss:/, 'https:')
      .replace(/^ws:/, 'http:');
  }
  return 'https://sync.diaryx.org';
}

/**
 * Get the WebSocket URL for the sync server (with /sync2 path).
 */
function getWsServerUrl(): string {
  const baseUrl = getBaseServerUrl();
  // Convert http(s) to ws(s) and add /sync2 path
  return baseUrl.replace(/^https:/, 'wss:').replace(/^http:/, 'ws:') + '/sync2';
}

/**
 * Get the auth token from authStore if available.
 */
function getAuthToken(): string | null {
  const token = getToken();
  console.log('[ShareService] getAuthToken:', token ? 'found' : 'not found');
  return token;
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

// Owner ID for this client (used for read-only enforcement)
const clientOwnerId = `owner-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;

/**
 * Create a new share session for the current workspace.
 * Returns the join code that others can use to join.
 *
 * This now uses the Rust sync server's REST API to create sessions,
 * then connects via WebSocket for real-time sync.
 */
export async function createShareSession(workspaceId: string, readOnly: boolean = false, audience: string | null = null): Promise<string> {
  const selectedAudience = audience;
  const baseUrl = getBaseServerUrl();
  const wsUrl = getWsServerUrl();
  currentServerUrl = wsUrl;

  shareSessionStore.setConnecting(true);

  try {
    // Step 1: Create session via REST API
    const token = getAuthToken();
    if (!token) {
      shareSessionStore.setError('Authentication required to create a session');
      throw new Error('Authentication required to create a session');
    }

    const response = await fetch(`${baseUrl}/api/sessions`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${token}`,
      },
      body: JSON.stringify({
        workspace_id: workspaceId,
        read_only: readOnly,
      }),
    });

    if (!response.ok) {
      const error = await response.json().catch(() => ({ error: 'Failed to create session' }));
      shareSessionStore.setError(error.error || 'Failed to create session');
      throw new Error(error.error || 'Failed to create session');
    }

    const sessionData = await response.json();
    const joinCode = sessionData.code;

    console.log('[ShareService] Session created via REST API:', joinCode);

    // Step 2: Update store with session info
    shareSessionStore.startHosting(joinCode, workspaceId, readOnly, selectedAudience);

    // Step 3: Start V2 sync with peer notification callbacks
    const callbacks: SessionSyncCallbacks = {
      onPeerJoined: (guestId, peerCount) => {
        shareSessionStore.addPeer(guestId);
        console.log('[ShareService] Peer joined:', guestId, 'count:', peerCount);
      },
      onPeerLeft: (guestId, peerCount) => {
        shareSessionStore.removePeer(guestId);
        console.log('[ShareService] Peer left:', guestId, 'count:', peerCount);
      },
    };

    await startSessionSync(wsUrl, joinCode, true, workspaceId, callbacks);

    console.log('[ShareService] Host session sync started');
    return joinCode;
  } catch (e) {
    shareSessionStore.setConnecting(false);
    throw e;
  }
}

// ============================================================================
// Session Join (Guest Mode)
// ============================================================================

/**
 * Restore state after a failed join attempt.
 * Undoes the backend swap and tree state changes made during joinShareSession.
 */
function restoreAfterFailedJoin(): void {
  if (!isTauri() && originalBackend) {
    workspaceStore.setBackend(originalBackend);
    setBackendApi(createApi(originalBackend));
    setBackend(originalBackend);
    guestBackend = null;
    guestApi = null;
    originalBackend = null;
  }
  workspaceStore.restoreTreeState();
}

/**
 * Join an existing share session using a join code.
 * Returns the workspace ID of the session.
 *
 * For web guests: Creates an in-memory backend (files stored in memory only).
 * For Tauri guests: Uses Rust-side in-memory filesystem.
 *
 * This now uses the Rust sync server's WebSocket endpoint with session param.
 */
export async function joinShareSession(joinCode: string): Promise<string> {
  const normalizedCode = normalizeJoinCode(joinCode);

  if (!validateJoinCode(normalizedCode)) {
    throw new Error('Invalid join code format');
  }

  const wsUrl = getWsServerUrl();
  currentServerUrl = wsUrl;

  shareSessionStore.setConnecting(true);

  // For guests, create an in-memory backend (Tauri or web)
  if (isTauri()) {
    // Tauri guest: use Rust-side in-memory filesystem
    console.log('[ShareService] Starting Tauri guest mode...');
    try {
      const backend = workspaceStore.backend as TauriBackend;
      await backend.startGuestMode(normalizedCode);

      // Save tree state so we can restore when leaving
      workspaceStore.saveTreeState();

      // Clear the tree - it will be populated from CRDT sync
      workspaceStore.setTree(null);

      console.log('[ShareService] Tauri guest mode ready');
    } catch (e) {
      console.error('[ShareService] Failed to start Tauri guest mode:', e);
      shareSessionStore.setError('Failed to initialize guest mode');
      throw e;
    }
  } else {
    // Web guest: use JavaScript-side in-memory backend
    console.log('[ShareService] Creating in-memory backend for web guest...');
    try {
      // Save the original backend and tree state so we can restore when leaving
      originalBackend = workspaceStore.backend;
      workspaceStore.saveTreeState();

      // Create the in-memory guest backend
      guestBackend = await createGuestBackend();
      guestApi = createApi(guestBackend);

      // Set the guest backend in workspaceStore so App.svelte uses it
      workspaceStore.setBackend(guestBackend);

      // Clear the tree - it will be populated from CRDT sync
      workspaceStore.setTree(null);

      // Set the guest API and backend for the CRDT bridge to use for file operations
      setBackendApi(guestApi);
      setBackend(guestBackend);

      console.log('[ShareService] In-memory guest backend ready');
    } catch (e) {
      console.error('[ShareService] Failed to create guest backend:', e);
      shareSessionStore.setError('Failed to initialize guest storage');
      throw e;
    }
  }

  // Look up the session's real workspace ID via REST before connecting.
  // The V2 transport constructs doc IDs like "workspace:<id>" and the server
  // validates workspace ownership, so we need the correct ID up front.
  const baseUrl = getBaseServerUrl();
  let sessionInfo: { workspace_id: string; read_only: boolean };
  try {
    const resp = await fetch(`${baseUrl}/api/sessions/${encodeURIComponent(normalizedCode)}`);
    if (!resp.ok) {
      const err = await resp.json().catch(() => ({ error: 'Session not found' }));
      throw new Error(err.error || 'Session not found');
    }
    sessionInfo = await resp.json();
    console.log('[ShareService] Session lookup:', sessionInfo);
  } catch (e) {
    shareSessionStore.setError('Session not found or expired');
    restoreAfterFailedJoin();
    throw e;
  }

  const backendType = 'memory';
  shareSessionStore.startGuest(normalizedCode, sessionInfo.workspace_id, undefined, backendType, sessionInfo.read_only);

  // Start V2 sync with the real workspace ID so doc IDs are constructed correctly.
  const callbacks: SessionSyncCallbacks = {
    onSessionJoined: (data) => {
      // Update store if server sends updated info (e.g. read_only changed since lookup)
      if (data.readOnly !== sessionInfo.read_only) {
        shareSessionStore.setReadOnly(data.readOnly);
      }
      console.log('[ShareService] session_joined confirmed:', data.joinCode);
    },
    onPeerJoined: (guestId, peerCount) => {
      shareSessionStore.addPeer(guestId);
      console.log('[ShareService] Peer joined:', guestId, 'count:', peerCount);
    },
    onPeerLeft: (guestId, peerCount) => {
      shareSessionStore.removePeer(guestId);
      console.log('[ShareService] Peer left:', guestId, 'count:', peerCount);
    },
    onSessionEnded: () => {
      console.log('[ShareService] Session ended by host');
      shareSessionStore.setError('Session ended by host');
    },
  };

  try {
    await startSessionSync(wsUrl, normalizedCode, false, sessionInfo.workspace_id, callbacks);
    console.log('[ShareService] Guest session sync started');
  } catch (err) {
    console.error('[ShareService] Session sync failed:', err);
    shareSessionStore.setError('Failed to connect to session');
    restoreAfterFailedJoin();
    throw err;
  }

  return sessionInfo.workspace_id;
}

// ============================================================================
// Session End
// ============================================================================

/**
 * End the current share session (works for both host and guest).
 */
export async function endShareSession(): Promise<void> {
  console.log('[ShareService] Ending share session');

  // Capture state before endSession() clears it
  const joinCode = shareSessionStore.joinCode;
  const wasGuest = shareSessionStore.isGuest;
  const wasHost = shareSessionStore.isHosting;
  const usedInMemory = shareSessionStore.usesInMemoryStorage;

  // If we're the host, delete the session via REST API
  if (wasHost && joinCode) {
    const token = getAuthToken();
    if (token) {
      const baseUrl = getBaseServerUrl();
      try {
        await fetch(`${baseUrl}/api/sessions/${encodeURIComponent(joinCode)}`, {
          method: 'DELETE',
          headers: {
            'Authorization': `Bearer ${token}`,
          },
        });
        console.log('[ShareService] Session deleted via REST API');
      } catch (e) {
        console.error('[ShareService] Failed to delete session via REST API:', e);
        // Continue with cleanup anyway
      }
    }
  }

  // Stop document sync
  await stopSessionSync();

  // End the session (clears state)
  shareSessionStore.endSession();

  // Clean up guest resources
  if (wasGuest) {
    if (isTauri()) {
      // For Tauri guests: end guest mode in Rust backend
      console.log('[ShareService] Ending Tauri guest mode...');
      try {
        const backend = workspaceStore.backend as TauriBackend;
        await backend.endGuestMode();

        // Restore the original tree state
        workspaceStore.restoreTreeState();

        console.log('[ShareService] Tauri guest mode ended');
      } catch (e) {
        console.error('[ShareService] Failed to end Tauri guest mode:', e);
      }
    } else if (usedInMemory) {
      // For web in-memory guests: restore the original backend and clear guest references
      console.log('[ShareService] Cleaning up in-memory guest backend...');

      // Restore the original backend in workspaceStore
      if (originalBackend) {
        workspaceStore.setBackend(originalBackend);
        // Also restore the original API and backend for the CRDT bridge
        setBackendApi(createApi(originalBackend));
        setBackend(originalBackend);
        console.log('[ShareService] Restored original backend');
      }

      // Restore the original tree state
      workspaceStore.restoreTreeState();

      // Clear guest references (memory freed when garbage collected)
      guestBackend = null;
      guestApi = null;
      originalBackend = null;
    } else if (joinCode) {
      // For OPFS guests: clean up the OPFS storage
      console.log('[ShareService] Cleaning up OPFS guest storage...');
      await cleanupGuestStorage(joinCode);
    }
  }
}

// ============================================================================
// Session Control
// ============================================================================

/**
 * Toggle read-only mode for the current session (host only).
 * Uses REST API to update the session, which broadcasts to all connected clients.
 * @param readOnly - Whether the session should be read-only
 */
export async function setSessionReadOnly(readOnly: boolean): Promise<void> {
  if (!shareSessionStore.isHosting) {
    console.warn('[ShareService] setSessionReadOnly called but not hosting');
    return;
  }

  const joinCode = shareSessionStore.joinCode;
  if (!joinCode) {
    console.warn('[ShareService] setSessionReadOnly called but no join code');
    return;
  }

  const token = getAuthToken();
  if (!token) {
    console.warn('[ShareService] setSessionReadOnly called but no auth token');
    return;
  }

  const baseUrl = getBaseServerUrl();
  try {
    const response = await fetch(`${baseUrl}/api/sessions/${encodeURIComponent(joinCode)}`, {
      method: 'PATCH',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${token}`,
      },
      body: JSON.stringify({ read_only: readOnly }),
    });

    if (!response.ok) {
      const error = await response.json().catch(() => ({ error: 'Failed to update session' }));
      console.error('[ShareService] Failed to update read-only:', error.error);
      return;
    }

    // Update local state (server will broadcast to other clients)
    shareSessionStore.setReadOnly(readOnly);
    console.log('[ShareService] Read-only updated:', readOnly);
  } catch (e) {
    console.error('[ShareService] Failed to update read-only:', e);
  }
}

/**
 * Get the owner ID for this client.
 * Used for read-only enforcement in document sync.
 */
export function getClientOwnerId(): string {
  return clientOwnerId;
}

// ============================================================================
// Guest Backend Access
// ============================================================================

/**
 * Get the guest backend (for web guests using in-memory storage).
 * Returns null if not in guest mode or if using Tauri.
 */
export function getGuestBackend(): WorkerBackendNew | null {
  return guestBackend;
}

/**
 * Get the guest API wrapper (for web guests using in-memory storage).
 * Returns null if not in guest mode or if using Tauri.
 */
export function getGuestApi(): Api | null {
  return guestApi;
}

// ============================================================================
// Guest Storage Management (OPFS - for Tauri guests, future)
// ============================================================================

/**
 * Get the OPFS storage path for a guest session.
 * Only used for Tauri guests (future) - web guests use in-memory storage.
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
