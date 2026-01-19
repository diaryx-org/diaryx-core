/**
 * Simple Sync Bridge
 *
 * Connects to our simple sync server and syncs a Y.Doc.
 * Protocol: raw Y.js updates over WebSocket.
 */

import * as Y from 'yjs';

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

      // Send initial state if configured (for session hosts)
      // This ensures the server has all the host's data when a session starts.
      if (this.sendInitialState && this.ws?.readyState === WebSocket.OPEN) {
        const state = Y.encodeStateAsUpdate(this.doc);
        if (state.length > 0) {
          console.log(`[SimpleSyncBridge] Sending initial state: ${state.length} bytes`);
          this.ws.send(state);
        }
      }
    };

    this.ws.onmessage = (event) => {
      try {
        const update = new Uint8Array(event.data as ArrayBuffer);
        console.log(`[SimpleSyncBridge] Received update: ${update.length} bytes`);

        // Apply update from server (with 'server' origin to avoid echo)
        Y.applyUpdate(this.doc, update, 'server');

        // Log current state
        const bodyText = this.doc.getText('body');
        console.log(`[SimpleSyncBridge] Doc body length after update: ${bodyText.length}`);

        if (!this.synced) {
          this.synced = true;
          this.onSynced?.();
        }
      } catch (err) {
        console.error('[SimpleSyncBridge] Error applying update:', err);
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
        console.log(`[SimpleSyncBridge] Sending local update: ${update.length} bytes`);
        this.ws.send(update);
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
