/**
 * Body Sync Bridge
 *
 * Handles WebSocket-based sync for individual file body documents.
 * Each file that needs body sync gets its own BodySyncBridge instance.
 *
 * This separates body content sync from workspace metadata sync,
 * preventing large files from bloating the workspace CRDT document.
 */

import type { RustCrdtApi } from './rustCrdtApi';

export interface BodySyncBridgeOptions {
  /** WebSocket server URL (without query params) */
  serverUrl: string;
  /** Workspace ID */
  workspaceId: string;
  /** File path for this body doc */
  filePath: string;
  /** Rust CRDT API instance */
  rustApi: RustCrdtApi;
  /** Optional session code for session-scoped sync */
  sessionCode?: string;
  /** Guest ID (for session guests) */
  guestId?: string;
  /** Auth token for authenticated sync */
  authToken?: string;
  /** Callback when connection status changes */
  onStatusChange?: (connected: boolean) => void;
  /** Callback when synced with server */
  onSynced?: () => void;
  /** Callback when body content changes from remote */
  onBodyChange?: (content: string) => void;
}

export class BodySyncBridge {
  private ws: WebSocket | null = null;
  private serverUrl: string;
  private workspaceId: string;
  private filePath: string;
  private rustApi: RustCrdtApi;
  private sessionCode?: string;
  private guestId?: string;
  private authToken?: string;
  private onStatusChange?: (connected: boolean) => void;
  private onSynced?: () => void;
  private onBodyChange?: (content: string) => void;
  private destroyed = false;
  private reconnectTimeout: ReturnType<typeof setTimeout> | null = null;
  private reconnectAttempts = 0;
  private synced = false;

  /**
   * Suppress body change notifications during initial sync.
   * This prevents the server's state from overwriting local content during connection.
   * Set to false after the first explicit sendBodyUpdate call.
   */
  private suppressBodyChange = true;

  constructor(options: BodySyncBridgeOptions) {
    this.serverUrl = options.serverUrl;
    this.workspaceId = options.workspaceId;
    this.filePath = options.filePath;
    this.rustApi = options.rustApi;
    this.sessionCode = options.sessionCode;
    this.guestId = options.guestId;
    this.authToken = options.authToken;
    this.onStatusChange = options.onStatusChange;
    this.onSynced = options.onSynced;
    this.onBodyChange = options.onBodyChange;
  }

  /**
   * Connect to the sync server for this body doc.
   */
  async connect(): Promise<void> {
    if (this.destroyed) return;
    if (this.ws?.readyState === WebSocket.OPEN) return;

    // Build URL with workspace ID, file path, and optional session/auth
    const url = new URL(this.serverUrl);
    url.searchParams.set('doc', this.workspaceId);
    url.searchParams.set('file', this.filePath);

    if (this.sessionCode) {
      url.searchParams.set('session', this.sessionCode);
    }
    if (this.guestId) {
      url.searchParams.set('guest_id', this.guestId);
    }
    if (this.authToken) {
      url.searchParams.set('token', this.authToken);
    }

    console.log(`[BodySyncBridge] Connecting for file: ${this.filePath}`);

    this.ws = new WebSocket(url.toString());
    this.ws.binaryType = 'arraybuffer';

    this.ws.onopen = async () => {
      console.log(`[BodySyncBridge] Connected for file: ${this.filePath}`);
      this.reconnectAttempts = 0;
      this.onStatusChange?.(true);

      try {
        // Send SyncStep1 to initiate sync
        await this.sendSyncStep1();
      } catch (err) {
        console.error('[BodySyncBridge] Error during connection setup:', err);
      }
    };

    this.ws.onmessage = async (event) => {
      try {
        const data = new Uint8Array(event.data as ArrayBuffer);
        await this.handleMessage(data);
      } catch (err) {
        console.error('[BodySyncBridge] Error handling message:', err);
      }
    };

    this.ws.onclose = () => {
      console.log(`[BodySyncBridge] Disconnected from file: ${this.filePath}`);
      this.onStatusChange?.(false);
      this.synced = false;
      // Re-enable suppression on disconnect so reconnect doesn't immediately write to disk
      this.suppressBodyChange = true;

      if (!this.destroyed) {
        this.scheduleReconnect();
      }
    };

    this.ws.onerror = (err) => {
      console.error('[BodySyncBridge] WebSocket error:', err);
    };
  }

  /**
   * Send SyncStep1 - our state vector to initiate sync.
   */
  private async sendSyncStep1(): Promise<void> {
    if (this.ws?.readyState !== WebSocket.OPEN) return;

    // Get encoded SyncStep1 message for body doc
    let stateVector = await this.rustApi.getBodySyncState(this.filePath);

    // Ensure we have a valid state vector
    // An empty byte array is NOT a valid state vector - minimum is [0] for empty map
    if (stateVector.length === 0) {
      // Use a minimal valid state vector: varUint(0) = empty map
      stateVector = new Uint8Array([0]);
      console.log(`[BodySyncBridge] Using minimal state vector for ${this.filePath} (body doc empty)`);
    }

    // Encode as SyncStep1 message (msgType=0, syncType=0, varByteArray(stateVector))
    const syncStep1 = this.encodeSyncStep1(stateVector);

    console.log(`[BodySyncBridge] Sending SyncStep1 for ${this.filePath}: ${syncStep1.length} bytes, sv: ${stateVector.length} bytes`);
    this.ws.send(syncStep1);
  }

  /**
   * Encode a state vector as a SyncStep1 message.
   */
  private encodeSyncStep1(stateVector: Uint8Array): Uint8Array {
    // Format: varUint(0) + varUint(0) + varByteArray(stateVector)
    const result: number[] = [];
    result.push(0); // msgType = SYNC
    result.push(0); // syncType = STEP1
    this.writeVarUint(result, stateVector.length);
    result.push(...stateVector);
    return new Uint8Array(result);
  }

  /**
   * Encode an update as a Y-sync Update message.
   */
  private encodeUpdateMessage(update: Uint8Array): Uint8Array {
    // Format: varUint(0) + varUint(2) + varByteArray(update)
    const result: number[] = [];
    result.push(0); // msgType = SYNC
    result.push(2); // syncType = UPDATE
    this.writeVarUint(result, update.length);
    result.push(...update);
    return new Uint8Array(result);
  }

  /**
   * Write a variable-length unsigned integer.
   */
  private writeVarUint(buf: number[], num: number): void {
    while (num > 0x7f) {
      buf.push((num & 0x7f) | 0x80);
      num >>>= 7;
    }
    buf.push(num & 0x7f);
  }

  /**
   * Handle incoming Y-sync message.
   */
  private async handleMessage(data: Uint8Array): Promise<void> {
    console.log(`[BodySyncBridge] Received message for ${this.filePath}: ${data.length} bytes`);

    // Decode and process the message
    // The server will send SyncStep1 or SyncStep2 messages
    const messages = this.decodeMessages(data);

    for (const msg of messages) {
      if (msg.type === 'SyncStep1') {
        // Server is asking for our diff
        const diff = await this.rustApi.getBodyMissingUpdates(this.filePath, msg.payload);
        if (diff.length > 0) {
          // Send SyncStep2 with our diff
          const syncStep2 = this.encodeSyncStep2(diff);
          if (this.ws?.readyState === WebSocket.OPEN) {
            console.log(`[BodySyncBridge] Sending SyncStep2: ${syncStep2.length} bytes`);
            this.ws.send(syncStep2);
          }
        }
      } else if (msg.type === 'SyncStep2' || msg.type === 'Update') {
        // Apply the update to our body doc
        if (msg.payload.length > 0) {
          // Get content BEFORE applying update
          const contentBefore = await this.rustApi.getBodyContent(this.filePath);

          // Apply the remote update to the CRDT
          await this.rustApi.applyBodyUpdate(this.filePath, msg.payload);

          // Get content AFTER applying update
          const contentAfter = await this.rustApi.getBodyContent(this.filePath);

          // Check if content actually changed
          if (contentAfter !== contentBefore) {
            // IMPORTANT: If the update would result in LOSING content (non-empty -> empty),
            // restore the original content and DON'T notify.
            // This protects against server's empty state overwriting local content.
            if (contentAfter.length === 0 && contentBefore.length > 0) {
              console.warn(`[BodySyncBridge] Remote update would erase ${contentBefore.length} chars for ${this.filePath} - restoring local content`);
              // Restore our content by re-setting it
              await this.rustApi.setBodyContent(this.filePath, contentBefore);
              // Re-send our content to the server so it gets our data
              const fullState = await this.rustApi.getBodyFullState(this.filePath);
              if (fullState.length > 0 && this.ws?.readyState === WebSocket.OPEN) {
                const updateMsg = this.encodeUpdateMessage(fullState);
                console.log(`[BodySyncBridge] Re-sending local content to server: ${fullState.length} bytes`);
                this.ws.send(updateMsg);
              }
            } else if (this.suppressBodyChange) {
              // During initial sync, don't notify about body changes
              // This prevents race conditions where server state overwrites local content
              console.log(`[BodySyncBridge] Suppressing body change during initial sync for ${this.filePath}`);
            } else {
              // Content changed and new content is not empty (or old was also empty)
              console.log(`[BodySyncBridge] Content changed for ${this.filePath}: ${contentBefore.length} -> ${contentAfter.length} chars`);
              this.onBodyChange?.(contentAfter);
            }
          }
        }
      }
    }

    // Mark as synced after first successful message exchange
    if (!this.synced) {
      this.synced = true;
      this.onSynced?.();
    }
  }

  /**
   * Encode a diff as a SyncStep2 message.
   */
  private encodeSyncStep2(diff: Uint8Array): Uint8Array {
    // Format: varUint(0) + varUint(1) + varByteArray(diff)
    const result: number[] = [];
    result.push(0); // msgType = SYNC
    result.push(1); // syncType = STEP2
    this.writeVarUint(result, diff.length);
    result.push(...diff);
    return new Uint8Array(result);
  }

  /**
   * Decode Y-sync messages from raw data.
   */
  private decodeMessages(data: Uint8Array): Array<{ type: string; payload: Uint8Array }> {
    const messages: Array<{ type: string; payload: Uint8Array }> = [];
    let offset = 0;

    while (offset < data.length) {
      // Read message type
      const [msgType, msgTypeLen] = this.readVarUint(data, offset);
      if (msgType === undefined || msgType !== 0) {
        // Not a sync message, skip
        offset++;
        continue;
      }
      offset += msgTypeLen;

      if (offset >= data.length) break;

      // Read sync type
      const [syncType, syncTypeLen] = this.readVarUint(data, offset);
      if (syncType === undefined) break;
      offset += syncTypeLen;

      // Read payload
      const [payloadLen, payloadLenLen] = this.readVarUint(data, offset);
      if (payloadLen === undefined) break;
      offset += payloadLenLen;

      if (offset + payloadLen > data.length) break;

      const payload = data.slice(offset, offset + payloadLen);
      offset += payloadLen;

      const type = syncType === 0 ? 'SyncStep1' : syncType === 1 ? 'SyncStep2' : 'Update';
      messages.push({ type, payload });
    }

    return messages;
  }

  /**
   * Read a variable-length unsigned integer.
   */
  private readVarUint(data: Uint8Array, offset: number): [number | undefined, number] {
    let num = 0;
    let shift = 0;
    let bytesRead = 0;

    while (offset + bytesRead < data.length) {
      const byte = data[offset + bytesRead];
      num |= (byte & 0x7f) << shift;
      bytesRead++;

      if ((byte & 0x80) === 0) {
        return [num, bytesRead];
      }
      shift += 7;
      if (shift > 28) {
        return [undefined, 0]; // Overflow
      }
    }

    return [undefined, 0]; // Incomplete
  }

  /**
   * Send local body changes to the server.
   * Call this after making changes to the body doc.
   */
  async sendBodyUpdate(content: string): Promise<void> {
    if (this.ws?.readyState !== WebSocket.OPEN) {
      console.warn('[BodySyncBridge] Cannot send update - not connected');
      return;
    }

    try {
      // Update the local body doc first
      await this.rustApi.setBodyContent(this.filePath, content);

      // Get the full state to send
      // Using full state is more reliable than diff, especially for new docs
      const fullState = await this.rustApi.getBodyFullState(this.filePath);

      if (fullState.length > 0) {
        // Wrap in protocol format and send as Update message
        const updateMsg = this.encodeUpdateMessage(fullState);
        console.log(`[BodySyncBridge] Sending body Update for ${this.filePath}: ${fullState.length} bytes`);
        this.ws.send(updateMsg);
      }

      // After first explicit send, allow body change notifications
      // This means we've established our content and can now listen for remote changes
      if (this.suppressBodyChange) {
        console.log(`[BodySyncBridge] Enabling body change notifications for ${this.filePath}`);
        this.suppressBodyChange = false;
      }
    } catch (err) {
      console.error('[BodySyncBridge] Error sending body update:', err);
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

  /**
   * Get the file path this bridge is syncing.
   */
  getFilePath(): string {
    return this.filePath;
  }

  private scheduleReconnect(): void {
    if (this.destroyed) return;
    if (this.reconnectAttempts >= 10) {
      console.error('[BodySyncBridge] Max reconnect attempts reached');
      return;
    }

    this.cancelReconnect();

    const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempts), 30000);
    console.log(`[BodySyncBridge] Reconnecting in ${delay}ms`);

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
 * Create a body sync bridge.
 */
export function createBodySyncBridge(options: BodySyncBridgeOptions): BodySyncBridge {
  return new BodySyncBridge(options);
}
