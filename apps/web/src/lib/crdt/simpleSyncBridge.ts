/**
 * Simple Sync Bridge
 *
 * Connects to our sync server and syncs a Y.Doc using Y-sync protocol.
 * Protocol: Y-sync messages (SyncStep1, SyncStep2, Update) over WebSocket.
 *
 * Message format: [varUint(msgType)] [varUint(syncType)] [varUint(payloadLen)] [payload...]
 * - msgType: 0 = SYNC
 * - syncType: 0 = SyncStep1 (state vector), 1 = SyncStep2 (diff), 2 = Update (incremental)
 */

import * as Y from 'yjs';

// Y-sync message types
const MSG_SYNC = 0;
const SYNC_STEP1 = 0;
const SYNC_STEP2 = 1;
const SYNC_UPDATE = 2;

export interface SimpleSyncBridgeOptions {
  /** WebSocket server URL (without query params) */
  serverUrl: string;
  /** Document name */
  docName: string;
  /** Y.Doc to sync */
  doc: Y.Doc;
  /** Optional session code for session-scoped sync */
  sessionCode?: string;
  /** Send full state to server on connect (for session hosts) */
  sendInitialState?: boolean;
  /** Owner ID for read-only enforcement (required for hosts) */
  ownerId?: string;
  /** Auth token for authenticated sync */
  authToken?: string;
  /** Callback when connection status changes */
  onStatusChange?: (connected: boolean) => void;
  /** Callback when synced with server */
  onSynced?: () => void;
}

// ============================================================================
// VarUint encoding/decoding helpers
// ============================================================================

/**
 * Write a variable-length unsigned integer to an array.
 */
function writeVarUint(buf: number[], num: number): void {
  while (num > 0x7f) {
    buf.push((num & 0x7f) | 0x80);
    num >>>= 7;
  }
  buf.push(num);
}

/**
 * Read a variable-length unsigned integer from a Uint8Array.
 * Returns [value, bytesRead].
 */
function readVarUint(data: Uint8Array, offset: number): [number, number] {
  let num = 0;
  let shift = 0;
  let i = offset;
  while (i < data.length) {
    const byte = data[i++];
    num |= (byte & 0x7f) << shift;
    if (!(byte & 0x80)) break;
    shift += 7;
  }
  return [num, i - offset];
}

/**
 * Encode a Y-sync message.
 */
function encodeSyncMessage(syncType: 0 | 1 | 2, payload: Uint8Array): Uint8Array {
  const buf: number[] = [];
  writeVarUint(buf, MSG_SYNC);     // msgType = SYNC (0)
  writeVarUint(buf, syncType);      // syncType
  writeVarUint(buf, payload.length); // payload length
  return new Uint8Array([...buf, ...payload]);
}

/**
 * Decode a Y-sync message.
 * Returns null if not a valid sync message.
 */
function decodeSyncMessage(data: Uint8Array): { type: number; payload: Uint8Array } | null {
  let offset = 0;

  // Read message type
  const [msgType, msgBytes] = readVarUint(data, offset);
  if (msgType !== MSG_SYNC) {
    console.warn('[SimpleSyncBridge] Not a sync message, msgType:', msgType);
    return null;
  }
  offset += msgBytes;

  // Read sync type
  const [syncType, syncBytes] = readVarUint(data, offset);
  offset += syncBytes;

  // Read payload length
  const [payloadLen, lenBytes] = readVarUint(data, offset);
  offset += lenBytes;

  // Extract payload
  const payload = data.slice(offset, offset + payloadLen);

  return { type: syncType, payload };
}

export class SimpleSyncBridge {
  private ws: WebSocket | null = null;
  private serverUrl: string;
  private docName: string;
  private doc: Y.Doc;
  private sessionCode?: string;
  private sendInitialState: boolean;
  private ownerId?: string;
  private authToken?: string;
  private onStatusChange?: (connected: boolean) => void;
  private onSynced?: () => void;
  private updateHandler: ((update: Uint8Array, origin: unknown) => void) | null = null;
  private destroyed = false;
  private reconnectTimeout: ReturnType<typeof setTimeout> | null = null;
  private reconnectAttempts = 0;
  private synced = false;

  constructor(options: SimpleSyncBridgeOptions) {
    this.serverUrl = options.serverUrl;
    this.docName = options.docName;
    this.doc = options.doc;
    this.sessionCode = options.sessionCode;
    this.sendInitialState = options.sendInitialState ?? false;
    this.ownerId = options.ownerId;
    this.authToken = options.authToken;
    this.onStatusChange = options.onStatusChange;
    this.onSynced = options.onSynced;
  }

  /**
   * Connect to the sync server.
   */
  connect(): void {
    if (this.destroyed) return;
    if (this.ws?.readyState === WebSocket.OPEN) return;

    // Build URL with doc name, optional session code, ownerId, and auth token
    const url = new URL(this.serverUrl);
    url.searchParams.set('doc', this.docName);
    if (this.sessionCode) {
      url.searchParams.set('session', this.sessionCode);
    }
    if (this.ownerId) {
      url.searchParams.set('ownerId', this.ownerId);
    }
    if (this.authToken) {
      url.searchParams.set('token', this.authToken);
    }

    console.log(`[SimpleSyncBridge] Connecting to: ${url.toString()}`);

    this.ws = new WebSocket(url.toString());
    this.ws.binaryType = 'arraybuffer';

    this.ws.onopen = () => {
      console.log(`[SimpleSyncBridge] Connected to ${this.docName}`);
      this.reconnectAttempts = 0;
      this.onStatusChange?.(true);

      // Set up local update handler to send changes to server
      this.setupUpdateHandler();

      // Initiate Y-sync handshake with SyncStep1
      this.sendSyncStep1();

      // If configured to send initial state (for session hosts), also send full state
      if (this.sendInitialState && this.ws?.readyState === WebSocket.OPEN) {
        const state = Y.encodeStateAsUpdate(this.doc);
        if (state.length > 0) {
          console.log(`[SimpleSyncBridge] Sending initial state as Update: ${state.length} bytes`);
          const msg = encodeSyncMessage(SYNC_UPDATE, state);
          this.ws.send(msg);
        }
      }
    };

    this.ws.onmessage = (event) => {
      try {
        const data = new Uint8Array(event.data as ArrayBuffer);
        this.handleMessage(data);
      } catch (err) {
        console.error('[SimpleSyncBridge] Error handling message:', err);
      }
    };

    this.ws.onclose = () => {
      console.log(`[SimpleSyncBridge] Disconnected from ${this.docName}`);
      this.onStatusChange?.(false);
      this.synced = false;
      this.removeUpdateHandler();

      if (!this.destroyed) {
        this.scheduleReconnect();
      }
    };

    this.ws.onerror = (err) => {
      console.error('[SimpleSyncBridge] WebSocket error:', err);
    };
  }

  /**
   * Send SyncStep1 - our state vector to initiate sync.
   */
  private sendSyncStep1(): void {
    if (this.ws?.readyState !== WebSocket.OPEN) return;

    const stateVector = Y.encodeStateVector(this.doc);
    const msg = encodeSyncMessage(SYNC_STEP1, stateVector);
    console.log(`[SimpleSyncBridge] Sending SyncStep1: ${stateVector.length} bytes state vector`);
    this.ws.send(msg);
  }

  /**
   * Handle incoming Y-sync message.
   */
  private handleMessage(data: Uint8Array): void {
    const decoded = decodeSyncMessage(data);

    if (!decoded) {
      console.warn('[SimpleSyncBridge] Received invalid sync message, trying as raw update');
      // Fallback: try applying as raw Y.js update for backwards compatibility
      try {
        Y.applyUpdate(this.doc, data, 'server');
        console.log(`[SimpleSyncBridge] Applied raw update: ${data.length} bytes`);
      } catch (e) {
        console.error('[SimpleSyncBridge] Failed to apply as raw update:', e);
      }
      return;
    }

    switch (decoded.type) {
      case SYNC_STEP1: {
        // Server sent SyncStep1: respond with SyncStep2 (diff based on their state vector)
        console.log(`[SimpleSyncBridge] Received SyncStep1: ${decoded.payload.length} bytes state vector`);
        const diff = Y.encodeStateAsUpdate(this.doc, decoded.payload);
        if (diff.length > 0) {
          const msg = encodeSyncMessage(SYNC_STEP2, diff);
          console.log(`[SimpleSyncBridge] Sending SyncStep2: ${diff.length} bytes diff`);
          this.ws?.send(msg);
        }
        break;
      }

      case SYNC_STEP2: {
        // Server sent SyncStep2: apply the diff
        console.log(`[SimpleSyncBridge] Received SyncStep2: ${decoded.payload.length} bytes diff`);
        Y.applyUpdate(this.doc, decoded.payload, 'server');

        // Mark as synced after receiving SyncStep2
        if (!this.synced) {
          this.synced = true;
          this.onSynced?.();
        }
        break;
      }

      case SYNC_UPDATE: {
        // Server sent incremental update: apply it
        console.log(`[SimpleSyncBridge] Received Update: ${decoded.payload.length} bytes`);
        Y.applyUpdate(this.doc, decoded.payload, 'server');

        if (!this.synced) {
          this.synced = true;
          this.onSynced?.();
        }
        break;
      }

      default:
        console.warn(`[SimpleSyncBridge] Unknown sync type: ${decoded.type}`);
    }

    // Log current state
    const bodyText = this.doc.getText('body');
    console.log(`[SimpleSyncBridge] Doc body length after message: ${bodyText.length}`);
  }

  /**
   * Disconnect from the sync server.
   */
  disconnect(): void {
    this.cancelReconnect();
    this.removeUpdateHandler();
    if (this.ws) {
      this.ws.onclose = null;
      this.ws.close();
      this.ws = null;
    }
    this.onStatusChange?.(false);
  }

  /**
   * Destroy the bridge.
   */
  destroy(): void {
    this.destroyed = true;
    this.disconnect();
  }

  /**
   * Check if connected and synced.
   */
  isSynced(): boolean {
    return this.synced;
  }

  private setupUpdateHandler(): void {
    this.removeUpdateHandler();

    this.updateHandler = (update: Uint8Array, origin: unknown) => {
      // Don't send updates that came from the server
      if (origin === 'server') return;

      if (this.ws?.readyState === WebSocket.OPEN) {
        // Wrap update in Y-sync protocol message
        const msg = encodeSyncMessage(SYNC_UPDATE, update);
        console.log(`[SimpleSyncBridge] Sending local Update: ${update.length} bytes`);
        this.ws.send(msg);
      }
    };

    this.doc.on('update', this.updateHandler);
  }

  private removeUpdateHandler(): void {
    if (this.updateHandler) {
      this.doc.off('update', this.updateHandler);
      this.updateHandler = null;
    }
  }

  private scheduleReconnect(): void {
    if (this.destroyed) return;
    if (this.reconnectAttempts >= 10) {
      console.error('[SimpleSyncBridge] Max reconnect attempts reached');
      return;
    }

    this.cancelReconnect();

    const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempts), 30000);
    console.log(`[SimpleSyncBridge] Reconnecting in ${delay}ms`);

    this.reconnectTimeout = setTimeout(() => {
      this.reconnectAttempts++;
      this.connect();
    }, delay);
  }

  private cancelReconnect(): void {
    if (this.reconnectTimeout) {
      clearTimeout(this.reconnectTimeout);
      this.reconnectTimeout = null;
    }
  }
}

/**
 * Create a simple sync bridge.
 */
export function createSimpleSyncBridge(options: SimpleSyncBridgeOptions): SimpleSyncBridge {
  return new SimpleSyncBridge(options);
}
