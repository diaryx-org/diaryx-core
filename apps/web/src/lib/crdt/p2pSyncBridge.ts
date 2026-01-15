/**
 * P2P Sync Bridge - Peer-to-peer device sync using y-webrtc.
 *
 * Enables sync between devices without requiring a dedicated server.
 * Uses WebRTC for direct peer-to-peer connections with a signaling
 * server only for initial handshake.
 */

import { WebrtcProvider } from 'y-webrtc';
import type { Doc as YDoc } from 'yjs';
import {
  P2PFileTransfer,
  getFileTransfer,
  type P2PFileMessage,
  type SyncProgress,
  type SyncResult,
  type ConflictInfo,
} from './p2pFileTransfer';
import { getApi } from '../backend';
import { getDeviceId, getDeviceName } from '../device/deviceId';

// ============================================================================
// Types
// ============================================================================

export interface P2PConfig {
  /** Signaling servers for WebRTC connection setup */
  signalingServers: string[];
  /** Maximum peer connections */
  maxConns: number;
}

export interface P2PSession {
  roomName: string;
  provider: WebrtcProvider;
}

export type P2PStatus = 'disabled' | 'connecting' | 'connected' | 'error';

export interface P2PState {
  enabled: boolean;
  syncCode: string | null;
  status: P2PStatus;
  connectedPeers: number;
}

// ============================================================================
// Constants
// ============================================================================

// Sync code stored in sessionStorage for security (not persisted across browser sessions)
// This prevents the encryption key from being exposed if localStorage is compromised
const STORAGE_KEY_SYNC_CODE = 'diaryx-p2p-sync-code';
// P2P enabled preference stored in localStorage (survives browser close)
const STORAGE_KEY_P2P_ENABLED = 'diaryx-p2p-enabled';

const DEFAULT_CONFIG: P2PConfig = {
  signalingServers: [
    'wss://vps.adammharris.me',
  ],
  maxConns: 10,
};

// ============================================================================
// State
// ============================================================================

let config: P2PConfig = { ...DEFAULT_CONFIG };
let syncCode: string | null = null;
let enabled = false;
let status: P2PStatus = 'disabled';
let connectedPeers = 0;

const sessions = new Map<string, P2PSession>();
const statusCallbacks = new Set<(state: P2PState) => void>();
const messageCallbacks = new Set<(msg: P2PFileMessage, peerId: string) => void>();
const peerConnectedCallbacks = new Set<(peerId: string) => void>();
const syncProgressCallbacks = new Set<(progress: SyncProgress) => void>();

// Custom message channel name
const FILE_TRANSFER_CHANNEL = 'diaryx-file-transfer';

// File transfer instance
let fileTransfer: P2PFileTransfer | null = null;
let fileTransferInitialized = false;

// Track cleanup functions for file transfer callbacks
let fileTransferUnsubscribers: (() => void)[] = [];

// ============================================================================
// Initialization
// ============================================================================

/**
 * Initialize P2P sync from stored settings.
 */
export function initP2PSync(): void {
  if (typeof window === 'undefined') return;

  // Load stored state
  // Sync code is in sessionStorage for security (requires re-entry each browser session)
  const storedCode = sessionStorage.getItem(STORAGE_KEY_SYNC_CODE);
  // P2P enabled preference is in localStorage (persists across sessions)
  const storedEnabled = localStorage.getItem(STORAGE_KEY_P2P_ENABLED);

  if (storedCode) {
    syncCode = storedCode;
  }

  // Only auto-enable if we have both the preference AND the sync code
  // (sync code may not be present if this is a new browser session)
  if (storedEnabled === 'true' && syncCode) {
    enabled = true;
    status = 'connecting';
  }

  notifyStatusChange();
}

/**
 * Configure P2P settings.
 */
export function configureP2P(newConfig: Partial<P2PConfig>): void {
  config = { ...config, ...newConfig };
}

// ============================================================================
// Sync Code Management
// ============================================================================

/**
 * Generate a new sync code for this workspace.
 *
 * Format: {workspacePrefix}-{randomSecret}
 * The code is used as the room name for WebRTC connections.
 */
export function generateSyncCode(workspaceId?: string): string {
  const prefix = workspaceId?.slice(0, 8) ?? generateRandomString(8);
  const secret = generateRandomString(8);
  const code = `${prefix}-${secret}`.toUpperCase();

  syncCode = code;
  // Store in sessionStorage for security (not persisted across browser sessions)
  sessionStorage.setItem(STORAGE_KEY_SYNC_CODE, code);
  notifyStatusChange();

  return code;
}

/**
 * Get the current sync code.
 */
export function getSyncCode(): string | null {
  return syncCode;
}

/**
 * Join an existing sync room using a code from another device.
 */
export function joinWithSyncCode(code: string): void {
  const normalizedCode = code.trim().toUpperCase();

  if (!validateSyncCode(normalizedCode)) {
    throw new Error('Invalid sync code format. Expected format: XXXXXXXX-XXXXXXXX');
  }

  syncCode = normalizedCode;
  // Store in sessionStorage for security (not persisted across browser sessions)
  sessionStorage.setItem(STORAGE_KEY_SYNC_CODE, normalizedCode);
  notifyStatusChange();
}

/**
 * Validate sync code format.
 */
export function validateSyncCode(code: string): boolean {
  // Format: 8 chars, hyphen, 8 chars (alphanumeric)
  return /^[A-Z0-9]{8}-[A-Z0-9]{8}$/.test(code);
}

/**
 * Clear the sync code and disable P2P sync.
 */
export function clearSyncCode(): void {
  disableP2PSync();
  syncCode = null;
  sessionStorage.removeItem(STORAGE_KEY_SYNC_CODE);
  notifyStatusChange();
}

// ============================================================================
// P2P Enable/Disable
// ============================================================================

/**
 * Enable P2P sync. Generates a sync code if one doesn't exist.
 */
export function enableP2PSync(workspaceId?: string): string {
  if (!syncCode) {
    generateSyncCode(workspaceId);
  }

  enabled = true;
  status = 'connecting';
  localStorage.setItem(STORAGE_KEY_P2P_ENABLED, 'true');
  notifyStatusChange();

  // Initialize file transfer in background
  initializeFileTransfer().catch((e) => {
    console.error('[P2PSyncBridge] Failed to initialize file transfer:', e);
  });

  return syncCode!;
}

/**
 * Disable P2P sync and disconnect all sessions.
 */
export function disableP2PSync(): void {
  enabled = false;
  status = 'disabled';
  connectedPeers = 0;
  localStorage.setItem(STORAGE_KEY_P2P_ENABLED, 'false');

  // Disconnect all sessions
  for (const session of sessions.values()) {
    session.provider.destroy();
  }
  sessions.clear();

  // Cleanup file transfer
  for (const unsub of fileTransferUnsubscribers) {
    unsub();
  }
  fileTransferUnsubscribers = [];

  if (fileTransfer) {
    fileTransfer.destroy();
    fileTransfer = null;
  }
  fileTransferInitialized = false;

  notifyStatusChange();
}

/**
 * Check if P2P sync is enabled.
 */
export function isP2PEnabled(): boolean {
  return enabled;
}

// ============================================================================
// Provider Management
// ============================================================================

/**
 * Create a WebRTC provider for a Y.Doc.
 *
 * @param ydoc - The Y.js document to sync
 * @param docName - Unique name for the document (used for room naming)
 * @returns The WebRTC provider, or null if P2P is disabled
 */
export function createP2PProvider(
  ydoc: YDoc,
  docName: string
): WebrtcProvider | null {
  if (!enabled || !syncCode) {
    return null;
  }

  // Check for existing session
  const existing = sessions.get(docName);
  if (existing) {
    return existing.provider;
  }

  // Create room name: syncCode:docName
  const roomName = `${syncCode}:${docName}`;

  console.log(`[P2PSyncBridge] Creating provider for room: ${roomName}`);

  console.log(`[P2PSyncBridge] Connecting to signaling servers:`, config.signalingServers);

  const provider = new WebrtcProvider(roomName, ydoc, {
    signaling: config.signalingServers,
    password: syncCode, // Encrypts connection with the sync code
    maxConns: config.maxConns,
    // Use default ICE servers - no custom config for now
  });

  // Set device awareness for multi-device attribution
  provider.awareness.setLocalStateField('device', {
    id: getDeviceId(),
    name: getDeviceName(),
  });

  // Track peer connections
  provider.on('peers', (event: { added: string[]; removed: string[]; webrtcPeers: string[]; bcPeers: string[] }) => {
    try {
      console.log('[P2PSyncBridge] Peers event:', {
        added: event.added,
        removed: event.removed,
        webrtcPeers: event.webrtcPeers,
        bcPeers: event.bcPeers,
      });
      const peerCount = event.webrtcPeers.length;
      updatePeerCount(peerCount);
    } catch (e) {
      console.error('[P2PSyncBridge] Error in peers handler:', e);
    }
  });

  // Track connection status
  provider.on('status', (event: { connected: boolean }) => {
    console.log('[P2PSyncBridge] Status event:', event);
    if (event.connected) {
      status = 'connected';
    } else if (enabled) {
      status = 'connecting';
    }
    notifyStatusChange();
  });

  // Track synced state
  provider.on('synced', (event: { synced: boolean }) => {
    console.log('[P2PSyncBridge] Synced event:', event);
  });

  // Track signaling errors
  // @ts-expect-error - signalingError event exists but isn't in type definitions
  provider.on('signalingError', (event: { error: Error }) => {
    console.error('[P2PSyncBridge] Signaling error:', event.error);
    // Don't set error status immediately - other signaling servers may work
  });

  // Log awareness changes (other devices connecting)
  provider.awareness.on('change', () => {
    const states = provider.awareness.getStates();
    console.log('[P2PSyncBridge] Awareness states:', states.size, 'devices');
  });

  // Setup message listeners for file transfer
  setupMessageListeners(provider);

  // Store session
  sessions.set(docName, { roomName, provider });

  return provider;
}

/**
 * Destroy a P2P provider for a document.
 */
export function destroyP2PProvider(docName: string): void {
  const session = sessions.get(docName);
  if (session) {
    session.provider.destroy();
    sessions.delete(docName);
  }
}

/**
 * Destroy all P2P providers.
 */
export function destroyAllP2PProviders(): void {
  for (const session of sessions.values()) {
    session.provider.destroy();
  }
  sessions.clear();
  connectedPeers = 0;
  notifyStatusChange();
}

/**
 * Get the number of active P2P sessions.
 */
export function getP2PSessionCount(): number {
  return sessions.size;
}

// ============================================================================
// Status
// ============================================================================

/**
 * Get the current P2P state.
 */
export function getP2PState(): P2PState {
  return {
    enabled,
    syncCode,
    status,
    connectedPeers,
  };
}

/**
 * Get the number of connected peers.
 */
export function getConnectedPeers(): number {
  return connectedPeers;
}

/**
 * Subscribe to P2P status changes.
 */
export function onP2PStatusChange(callback: (state: P2PState) => void): () => void {
  statusCallbacks.add(callback);
  // Immediately call with current state
  callback(getP2PState());
  return () => statusCallbacks.delete(callback);
}

// ============================================================================
// Helpers
// ============================================================================

function generateRandomString(length: number): string {
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789';
  let result = '';
  const randomValues = new Uint8Array(length);
  crypto.getRandomValues(randomValues);
  for (let i = 0; i < length; i++) {
    result += chars[randomValues[i] % chars.length];
  }
  return result;
}

function updatePeerCount(newCount: number): void {
  // Aggregate peer count across all sessions
  let totalPeers = 0;
  for (const session of sessions.values()) {
    // Access internal WebRTC connections - provider.room is available at runtime
    const provider = session.provider as any;
    const peers = provider?.room?.webrtcConns?.size ?? 0;
    totalPeers = Math.max(totalPeers, peers);
  }

  // Use the provided count if we can't aggregate
  connectedPeers = totalPeers || newCount;

  if (connectedPeers > 0) {
    status = 'connected';
  } else if (enabled) {
    status = 'connecting';
  }

  notifyStatusChange();
}

function notifyStatusChange(): void {
  const state = getP2PState();
  for (const callback of statusCallbacks) {
    try {
      callback(state);
    } catch (error) {
      console.error('[P2PSyncBridge] Status callback error:', error);
    }
  }
}

// ============================================================================
// Message Passing (for File Transfer)
// ============================================================================

/**
 * Broadcast a message to all connected peers.
 */
export function broadcastMessage(msg: P2PFileMessage): void {
  if (!enabled || sessions.size === 0) {
    console.warn('[P2PSyncBridge] Cannot broadcast: no active sessions');
    return;
  }

  const msgStr = JSON.stringify(msg);

  for (const session of sessions.values()) {
    try {
      const provider = session.provider as any;
      const room = provider?.room;

      if (room?.webrtcConns) {
        // Send to all WebRTC connections
        for (const [peerId, conn] of room.webrtcConns) {
          try {
            // Use the data channel if available
            if (conn.peer?.connected && conn.peer._channel?.readyState === 'open') {
              conn.peer.send(JSON.stringify({
                type: FILE_TRANSFER_CHANNEL,
                data: msgStr,
              }));
            }
          } catch (e) {
            console.warn(`[P2PSyncBridge] Failed to send to peer ${peerId}:`, e);
          }
        }
      }
    } catch (e) {
      console.warn('[P2PSyncBridge] Broadcast error:', e);
    }
  }
}

/**
 * Register a callback for incoming messages.
 */
export function onP2PMessage(callback: (msg: P2PFileMessage, peerId: string) => void): () => void {
  messageCallbacks.add(callback);
  return () => messageCallbacks.delete(callback);
}

/**
 * Register a callback for peer connection events.
 */
export function onPeerConnected(callback: (peerId: string) => void): () => void {
  peerConnectedCallbacks.add(callback);
  return () => peerConnectedCallbacks.delete(callback);
}

/**
 * Handle incoming message from a peer.
 */
function handleIncomingMessage(data: string, peerId: string): void {
  try {
    const parsed = JSON.parse(data);

    // Check if this is a file transfer message
    if (parsed.type === FILE_TRANSFER_CHANNEL && parsed.data) {
      const msg = JSON.parse(parsed.data) as P2PFileMessage;

      for (const callback of messageCallbacks) {
        try {
          callback(msg, peerId);
        } catch (e) {
          console.error('[P2PSyncBridge] Message callback error:', e);
        }
      }
    }
  } catch (e) {
    // Not a valid JSON message, ignore
  }
}

/**
 * Setup message listeners on a provider.
 */
function setupMessageListeners(provider: WebrtcProvider): void {
  const providerAny = provider as any;

  // Listen for new peer connections
  provider.on('peers', (event: { added: string[]; removed: string[]; webrtcPeers: string[]; bcPeers: string[] }) => {
    // Notify about new peers
    for (const peerId of event.added) {
      for (const callback of peerConnectedCallbacks) {
        try {
          callback(peerId);
        } catch (e) {
          console.error('[P2PSyncBridge] Peer connected callback error:', e);
        }
      }

      // Try to set up message listener for this peer
      setTimeout(() => {
        const room = providerAny?.room;
        const conn = room?.webrtcConns?.get(peerId);
        if (conn?.peer) {
          conn.peer.on('data', (data: string) => {
            handleIncomingMessage(data, peerId);
          });
        }
      }, 100); // Small delay to ensure connection is ready
    }
  });
}

// ============================================================================
// File Transfer Integration
// ============================================================================

/**
 * Initialize the file transfer system.
 * Should be called when P2P is enabled and API is available.
 */
async function initializeFileTransfer(): Promise<void> {
  if (fileTransferInitialized) return;

  try {
    const api = await getApi();
    fileTransfer = getFileTransfer(api);

    // Clear any existing unsubscribers
    for (const unsub of fileTransferUnsubscribers) {
      unsub();
    }
    fileTransferUnsubscribers = [];

    // Connect broadcast function
    fileTransfer.setBroadcast(broadcastMessage);

    // Register message handler to forward to file transfer
    const unsubMessage = onP2PMessage(async (msg, _peerId) => {
      await fileTransfer?.handleMessage(msg);
    });
    fileTransferUnsubscribers.push(unsubMessage);

    // Register progress forwarding
    const unsubProgress = fileTransfer.onProgress((progress) => {
      for (const callback of syncProgressCallbacks) {
        try {
          callback(progress);
        } catch (e) {
          console.error('[P2PSyncBridge] Sync progress callback error:', e);
        }
      }
    });
    fileTransferUnsubscribers.push(unsubProgress);

    // Auto-initiate sync when a peer connects
    const unsubPeer = onPeerConnected(async (_peerId) => {
      // Small delay to ensure connection is stable
      setTimeout(async () => {
        if (fileTransfer && !fileTransfer.isSyncInProgress()) {
          console.log('[P2PSyncBridge] Peer connected, initiating file sync...');
          try {
            const result = await fileTransfer.initiateSync();
            console.log('[P2PSyncBridge] File sync complete:', result.stats);
          } catch (e) {
            console.error('[P2PSyncBridge] File sync failed:', e);
          }
        }
      }, 1000);
    });
    fileTransferUnsubscribers.push(unsubPeer);

    fileTransferInitialized = true;
    console.log('[P2PSyncBridge] File transfer initialized');
  } catch (e) {
    console.error('[P2PSyncBridge] Failed to initialize file transfer:', e);
  }
}

/**
 * Manually trigger a full workspace sync with connected peers.
 */
export async function initiateFileSync(): Promise<SyncResult | null> {
  if (!fileTransfer) {
    await initializeFileTransfer();
  }

  if (!fileTransfer) {
    console.error('[P2PSyncBridge] File transfer not available');
    return null;
  }

  return fileTransfer.initiateSync();
}

/**
 * Subscribe to sync progress updates.
 */
export function onSyncProgress(callback: (progress: SyncProgress) => void): () => void {
  syncProgressCallbacks.add(callback);

  // Send current state if available
  if (fileTransfer) {
    callback(fileTransfer.getProgress());
  }

  return () => syncProgressCallbacks.delete(callback);
}

/**
 * Get current sync progress.
 */
export function getSyncProgress(): SyncProgress | null {
  return fileTransfer?.getProgress() ?? null;
}

/**
 * Check if a sync is currently in progress.
 */
export function isSyncInProgress(): boolean {
  return fileTransfer?.isSyncInProgress() ?? false;
}

/**
 * Set the conflict resolution handler.
 */
export function setConflictHandler(
  handler: (conflicts: ConflictInfo[]) => Promise<Map<string, 'local' | 'remote' | 'both'>>
): void {
  if (fileTransfer) {
    fileTransfer.setConflictHandler(handler);
  }
}

// Re-export types for convenience
export type { SyncProgress, SyncResult, ConflictInfo };

// ============================================================================
// Cleanup
// ============================================================================

/**
 * Cleanup on page unload.
 */
export function cleanupP2P(): void {
  destroyAllP2PProviders();
  statusCallbacks.clear();
  messageCallbacks.clear();
  peerConnectedCallbacks.clear();
  syncProgressCallbacks.clear();

  // Cleanup file transfer unsubscribers
  for (const unsub of fileTransferUnsubscribers) {
    unsub();
  }
  fileTransferUnsubscribers = [];

  // Destroy file transfer instance
  if (fileTransfer) {
    fileTransfer.destroy();
    fileTransfer = null;
  }
  fileTransferInitialized = false;
}

// Register cleanup on page unload
if (typeof window !== 'undefined') {
  window.addEventListener('beforeunload', () => {
    cleanupP2P();
  });
}
