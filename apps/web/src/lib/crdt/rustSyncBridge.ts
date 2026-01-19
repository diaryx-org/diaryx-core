/**
 * Rust Sync Bridge
 *
 * Connects to our sync server and syncs the Rust CRDT using Y-sync protocol.
 * Unlike SimpleSyncBridge, this uses the Rust CRDT directly via RustCrdtApi,
 * eliminating the need for a JavaScript Y.Doc or protocol encoding in TypeScript.
 *
 * The Rust backend handles all Y-sync protocol encoding/decoding.
 */

import type { RustCrdtApi } from './rustCrdtApi';

export interface RustSyncBridgeOptions {
  /** WebSocket server URL (without query params) */
  serverUrl: string;
  /** Document name in the Rust CRDT */
  docName: string;
  /** Rust CRDT API instance */
  rustApi: RustCrdtApi;
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
  /** Callback when remote update is received (for UI refresh) */
  onRemoteUpdate?: () => void;
}

export class RustSyncBridge {
  private ws: WebSocket | null = null;
  private serverUrl: string;
  private docName: string;
  private rustApi: RustCrdtApi;
  private sessionCode?: string;
  private sendInitialState: boolean;
  private ownerId?: string;
  private authToken?: string;
  private onStatusChange?: (connected: boolean) => void;
  private onSynced?: () => void;
  private onRemoteUpdate?: () => void;
  private destroyed = false;
  private reconnectTimeout: ReturnType<typeof setTimeout> | null = null;
  private reconnectAttempts = 0;
  private synced = false;

  /** Last state vector we sent, used to detect local changes */
  private lastSentStateVector: Uint8Array | null = null;

  /** Last response we sent, used to detect and break ping-pong loops */
  private lastSentResponse: Uint8Array | null = null;

  constructor(options: RustSyncBridgeOptions) {
    this.serverUrl = options.serverUrl;
    this.docName = options.docName;
    this.rustApi = options.rustApi;
    this.sessionCode = options.sessionCode;
    this.sendInitialState = options.sendInitialState ?? false;
    this.ownerId = options.ownerId;
    this.authToken = options.authToken;
    this.onStatusChange = options.onStatusChange;
    this.onSynced = options.onSynced;
    this.onRemoteUpdate = options.onRemoteUpdate;
  }

  /**
   * Connect to the sync server.
   */
  async connect(): Promise<void> {
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

    console.log(`[RustSyncBridge] Connecting to: ${url.toString()}`);

    this.ws = new WebSocket(url.toString());
    this.ws.binaryType = 'arraybuffer';

    this.ws.onopen = async () => {
      console.log(`[RustSyncBridge] Connected to ${this.docName}`);
      this.reconnectAttempts = 0;
      this.onStatusChange?.(true);

      try {
        // Initiate Y-sync handshake with SyncStep1 (Rust handles encoding)
        await this.sendSyncStep1();

        // If configured to send initial state (for session hosts), also send full state
        if (this.sendInitialState && this.ws?.readyState === WebSocket.OPEN) {
          const fullState = await this.rustApi.getFullState(this.docName);
          if (fullState.length > 0) {
            const updateMsg = await this.rustApi.createUpdateMessage(fullState, this.docName);
            console.log(`[RustSyncBridge] Sending initial state as Update: ${fullState.length} bytes`);
            this.ws.send(updateMsg);
          }
        }
      } catch (err) {
        console.error('[RustSyncBridge] Error during connection setup:', err);
      }
    };

    this.ws.onmessage = async (event) => {
      try {
        const data = new Uint8Array(event.data as ArrayBuffer);
        await this.handleMessage(data);
      } catch (err) {
        console.error('[RustSyncBridge] Error handling message:', err);
      }
    };

    this.ws.onclose = () => {
      console.log(`[RustSyncBridge] Disconnected from ${this.docName}`);
      this.onStatusChange?.(false);
      this.synced = false;
      this.lastSentStateVector = null;
      this.lastSentResponse = null;

      if (!this.destroyed) {
        this.scheduleReconnect();
      }
    };

    this.ws.onerror = (err) => {
      console.error('[RustSyncBridge] WebSocket error:', err);
    };
  }

  /**
   * Send SyncStep1 - our state vector to initiate sync.
   * Rust handles the protocol encoding.
   */
  private async sendSyncStep1(): Promise<void> {
    if (this.ws?.readyState !== WebSocket.OPEN) return;

    // Get encoded SyncStep1 message from Rust
    const syncStep1 = await this.rustApi.createSyncStep1(this.docName);
    console.log(`[RustSyncBridge] Sending SyncStep1: ${syncStep1.length} bytes`);
    this.ws.send(syncStep1);

    // Save current state vector for tracking local changes
    this.lastSentStateVector = await this.rustApi.getSyncState(this.docName);
  }

  /**
   * Handle incoming Y-sync message(s).
   * Rust handles all protocol decoding and update application.
   */
  private async handleMessage(data: Uint8Array): Promise<void> {
    console.log(`[RustSyncBridge] Received message: ${data.length} bytes`);

    // Let Rust handle the message (decode and apply)
    // Rust returns a response if one is needed (e.g., SyncStep2)
    const response = await this.rustApi.handleSyncMessage(data, this.docName);

    if (response && response.length > 0 && this.ws?.readyState === WebSocket.OPEN) {
      // Detect ping-pong loops: if we're about to send the same response as last time,
      // and we're already synced, skip it to break the loop
      if (this.synced && this.lastSentResponse && this.arraysEqual(response, this.lastSentResponse)) {
        console.log(`[RustSyncBridge] Skipping duplicate response to break sync loop`);
      } else {
        console.log(`[RustSyncBridge] Sending response: ${response.length} bytes`);
        this.ws.send(response);
        this.lastSentResponse = response;
      }
    }

    // Mark as synced after first successful message exchange
    if (!this.synced) {
      this.synced = true;
      this.onSynced?.();
    }

    // Notify caller of remote update
    this.onRemoteUpdate?.();

    // Update our state vector tracking
    this.lastSentStateVector = await this.rustApi.getSyncState(this.docName);
  }

  /**
   * Compare two Uint8Arrays for equality.
   */
  private arraysEqual(a: Uint8Array, b: Uint8Array): boolean {
    if (a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) {
      if (a[i] !== b[i]) return false;
    }
    return true;
  }

  /**
   * Send local changes to the server.
   * Call this after making changes to the Rust CRDT.
   */
  async sendLocalChanges(): Promise<void> {
    if (this.ws?.readyState !== WebSocket.OPEN) return;
    if (!this.lastSentStateVector) return;

    try {
      // Get diff since last sent state
      const diff = await this.rustApi.getMissingUpdates(
        this.lastSentStateVector,
        this.docName
      );

      if (diff.length > 0) {
        // Wrap in protocol format and send
        const updateMsg = await this.rustApi.createUpdateMessage(diff, this.docName);
        console.log(`[RustSyncBridge] Sending local Update: ${diff.length} bytes`);
        this.ws.send(updateMsg);

        // Clear lastSentResponse since we're sending new data -
        // the next response from server will be different
        this.lastSentResponse = null;

        // Update tracking
        this.lastSentStateVector = await this.rustApi.getSyncState(this.docName);
      }
    } catch (err) {
      console.error('[RustSyncBridge] Error sending local changes:', err);
    }
  }

  /**
   * Disconnect from the sync server.
   */
  disconnect(): void {
    this.cancelReconnect();
    if (this.ws) {
      this.ws.onclose = null;
      this.ws.close();
      this.ws = null;
    }
    this.lastSentStateVector = null;
    this.lastSentResponse = null;
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

  /**
   * Check if connected.
   */
  isConnected(): boolean {
    return this.ws?.readyState === WebSocket.OPEN;
  }

  private scheduleReconnect(): void {
    if (this.destroyed) return;
    if (this.reconnectAttempts >= 10) {
      console.error('[RustSyncBridge] Max reconnect attempts reached');
      return;
    }

    this.cancelReconnect();

    const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempts), 30000);
    console.log(`[RustSyncBridge] Reconnecting in ${delay}ms`);

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
 * Create a Rust sync bridge.
 */
export function createRustSyncBridge(options: RustSyncBridgeOptions): RustSyncBridge {
  return new RustSyncBridge(options);
}
