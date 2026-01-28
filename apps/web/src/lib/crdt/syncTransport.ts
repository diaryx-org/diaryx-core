/**
 * Pure WebSocket transport layer for sync.
 *
 * All sync logic is delegated to Rust via RustSyncManager.
 * This TypeScript module only handles:
 * - WebSocket lifecycle (browser API)
 * - Reconnection with exponential backoff
 * - Message forwarding to Rust
 */

import type { Backend } from '../backend/interface';

/**
 * Configuration for a sync transport connection.
 */
export interface SyncTransportOptions {
  /** Server URL for WebSocket connection. */
  serverUrl: string;
  /** Type of document to sync. */
  docType: 'workspace' | 'body';
  /** Document name (e.g., "workspace" for workspace sync, or file path for body sync). */
  docName: string;
  /** Workspace ID (required for body sync to properly route messages). */
  workspaceId?: string;
  /** Backend for executing Rust commands. */
  backend: Backend;
  /** Whether to write changes to disk. */
  writeToDisk: boolean;
  /** Optional auth token for authentication. */
  authToken?: string;
  /** Optional session code for share sessions. */
  sessionCode?: string;
  /** Callback when connection status changes. */
  onStatusChange?: (connected: boolean) => void;
  /** Callback when initial sync completes. */
  onSynced?: () => void;
  /** Callback when body content changes (for body sync). */
  onContentChange?: (content: string) => void;
  /** Callback when workspace files change (for workspace sync). */
  onFilesChanged?: (changedFiles: string[]) => void;
  /** Callback for sync progress updates. */
  onProgress?: (completed: number, total: number) => void;
}

/**
 * WebSocket transport for Y-sync protocol.
 *
 * This class handles the WebSocket connection lifecycle and forwards
 * all sync messages to Rust for processing. It provides:
 * - Automatic reconnection with exponential backoff
 * - Initial sync handshake
 * - Message routing to appropriate Rust commands
 */
export class SyncTransport {
  private ws: WebSocket | null = null;
  private reconnectAttempts = 0;
  private maxReconnectAttempts = 10;
  private reconnectTimeout: ReturnType<typeof setTimeout> | null = null;
  private destroyed = false;
  private synced = false;

  private readonly options: SyncTransportOptions;

  constructor(options: SyncTransportOptions) {
    this.options = options;
  }

  /**
   * Connect to the sync server.
   */
  async connect(): Promise<void> {
    if (this.destroyed) {
      return;
    }

    try {
      const url = this.buildUrl();
      console.log(`[SyncTransport] Connecting to ${url}`);

      this.ws = new WebSocket(url);
      this.ws.binaryType = 'arraybuffer';

      this.ws.onopen = async () => {
        console.log(`[SyncTransport] Connected: ${this.options.docType}/${this.options.docName}`);
        this.reconnectAttempts = 0;
        this.options.onStatusChange?.(true);

        // Send initial sync step 1
        await this.sendSyncStep1();
      };

      this.ws.onmessage = async (event) => {
        if (this.destroyed) return;

        // Handle text messages (JSON control messages) separately from binary
        if (typeof event.data === 'string') {
          try {
            const msg = JSON.parse(event.data);
            if (msg.type === 'sync_progress') {
              this.options.onProgress?.(msg.completed, msg.total);
            }
            // Other control messages can be handled here
          } catch (e) {
            console.warn('[SyncTransport] Failed to parse control message:', e);
          }
          return;
        }

        const message = new Uint8Array(event.data as ArrayBuffer);
        await this.handleMessage(message);
      };

      this.ws.onclose = (event) => {
        console.log(`[SyncTransport] Closed: ${event.code} ${event.reason}`);
        this.ws = null;
        this.options.onStatusChange?.(false);

        if (!this.destroyed) {
          this.scheduleReconnect();
        }
      };

      this.ws.onerror = (error) => {
        console.error(`[SyncTransport] Error:`, error);
      };
    } catch (error) {
      console.error(`[SyncTransport] Connection error:`, error);
      this.scheduleReconnect();
    }
  }

  /**
   * Disconnect from the sync server.
   */
  disconnect(): void {
    if (this.reconnectTimeout) {
      clearTimeout(this.reconnectTimeout);
      this.reconnectTimeout = null;
    }

    if (this.ws) {
      this.ws.close(1000, 'Client disconnecting');
      this.ws = null;
    }

    this.options.onStatusChange?.(false);
  }

  /**
   * Destroy the transport. Cannot be reconnected after this.
   */
  destroy(): void {
    this.destroyed = true;
    this.disconnect();
  }

  /**
   * Check if connected.
   */
  get isConnected(): boolean {
    return this.ws !== null && this.ws.readyState === WebSocket.OPEN;
  }

  /**
   * Check if currently connecting.
   */
  get isConnecting(): boolean {
    return this.ws !== null && this.ws.readyState === WebSocket.CONNECTING;
  }

  /**
   * Check if initial sync is complete.
   */
  get isSynced(): boolean {
    return this.synced;
  }

  /**
   * Send raw bytes directly over the WebSocket.
   * Used by SendSyncMessage events from Rust to push updates.
   */
  sendRawMessage(bytes: Uint8Array): void {
    if (!this.isConnected) {
      console.warn('[SyncTransport] Not connected, cannot send raw message');
      return;
    }
    this.ws?.send(bytes);
  }

  /**
   * Send local changes to the server.
   */
  async sendLocalChanges(content?: string): Promise<void> {
    if (!this.isConnected) {
      console.warn('[SyncTransport] Not connected, cannot send changes');
      return;
    }

    try {
      let response: any;

      if (this.options.docType === 'workspace') {
        response = await this.options.backend.execute({
          type: 'CreateWorkspaceUpdate' as any,
          params: {
            since_state_vector: null,
          },
        } as any);
      } else {
        if (content === undefined) {
          console.warn('[SyncTransport] Body sync requires content');
          return;
        }
        console.log(`[SyncTransport] sendLocalChanges: doc_name='${this.options.docName}', content_preview='${content.substring(0, 50)}'`);
        response = await this.options.backend.execute({
          type: 'CreateBodyUpdate' as any,
          params: {
            doc_name: this.options.docName,
            content,
          },
        } as any);
      }

      if (response.type === 'Binary' && response.data && response.data.length > 0) {
        this.ws?.send(new Uint8Array(response.data));
      }
    } catch (error) {
      console.error('[SyncTransport] Error sending local changes:', error);
    }
  }

  // =========================================================================
  // Private Methods
  // =========================================================================

  private buildUrl(): string {
    let url = this.options.serverUrl;

    // For body sync, use workspace ID as doc and file path as file parameter
    // This ensures the sync server routes to the correct body doc handler
    if (this.options.docType === 'body' && this.options.workspaceId) {
      // Body sync URL format: /sync?doc=workspace_id&file=file_path
      if (!url.includes('?')) {
        url += `?doc=${encodeURIComponent(this.options.workspaceId)}`;
      } else {
        url += `&doc=${encodeURIComponent(this.options.workspaceId)}`;
      }
      url += `&file=${encodeURIComponent(this.options.docName)}`;
    } else {
      // Workspace sync or body sync without workspace ID (fallback)
      if (!url.includes('?')) {
        url += `?doc=${encodeURIComponent(this.options.docName)}`;
      } else {
        url += `&doc=${encodeURIComponent(this.options.docName)}`;
      }
    }

    // Add auth token if provided
    if (this.options.authToken) {
      url += `&token=${encodeURIComponent(this.options.authToken)}`;
    }

    // Add session code if provided
    if (this.options.sessionCode) {
      url += `&session=${encodeURIComponent(this.options.sessionCode)}`;
    }

    return url;
  }

  private async sendSyncStep1(): Promise<void> {
    try {
      let response: any;

      if (this.options.docType === 'workspace') {
        response = await this.options.backend.execute({
          type: 'CreateWorkspaceSyncStep1',
        } as any);
      } else {
        // Initialize body sync first
        await this.options.backend.execute({
          type: 'InitBodySync' as any,
          params: {
            doc_name: this.options.docName,
          },
        } as any);

        response = await this.options.backend.execute({
          type: 'CreateBodySyncStep1' as any,
          params: {
            doc_name: this.options.docName,
          },
        } as any);
      }

      if (response.type === 'Binary' && response.data) {
        this.ws?.send(new Uint8Array(response.data));
      }
    } catch (error) {
      console.error('[SyncTransport] Error sending sync step 1:', error);
    }
  }

  private async handleMessage(message: Uint8Array): Promise<void> {
    try {
      if (this.options.docType === 'workspace') {
        await this.handleWorkspaceMessage(message);
      } else {
        await this.handleBodyMessage(message);
      }
    } catch (error) {
      console.error('[SyncTransport] Error handling message:', error);
    }
  }

  private async handleWorkspaceMessage(message: Uint8Array): Promise<void> {
    const response = await this.options.backend.execute({
      type: 'HandleWorkspaceSyncMessage' as any,
      params: {
        message: Array.from(message),
        write_to_disk: this.options.writeToDisk,
      },
    } as any);

    if ((response.type as string) === 'WorkspaceSyncResult') {
      const result = response as any;

      // Send response if Rust returns one
      if (result.data?.response && result.data.response.length > 0) {
        this.ws?.send(new Uint8Array(result.data.response));
      }

      // Handle sync complete notification
      if (result.data?.sync_complete && !this.synced) {
        this.synced = true;
        this.options.onSynced?.();
      }

      // Handle changed files notification
      if (result.data?.changed_files && result.data.changed_files.length > 0) {
        this.options.onFilesChanged?.(result.data.changed_files);
      }
    }
  }

  private async handleBodyMessage(message: Uint8Array): Promise<void> {
    console.log(`[SyncTransport] handleBodyMessage: doc_name='${this.options.docName}', ${message.length} bytes`);
    const response = await this.options.backend.execute({
      type: 'HandleBodySyncMessage' as any,
      params: {
        doc_name: this.options.docName,
        message: Array.from(message),
        write_to_disk: this.options.writeToDisk,
      },
    } as any);

    if ((response.type as string) === 'BodySyncResult') {
      const result = response as any;

      // Send response if Rust returns one
      if (result.data?.response && result.data.response.length > 0) {
        this.ws?.send(new Uint8Array(result.data.response));
      }

      // Handle content change notification (if not an echo)
      if (result.data?.content && !result.data?.is_echo) {
        this.options.onContentChange?.(result.data.content);
      }

      // Mark as synced after first message exchange
      if (!this.synced) {
        this.synced = true;
        this.options.onSynced?.();
      }
    }
  }

  private scheduleReconnect(): void {
    if (this.destroyed || this.reconnectAttempts >= this.maxReconnectAttempts) {
      console.log('[SyncTransport] Max reconnect attempts reached');
      return;
    }

    // Exponential backoff: 1s, 2s, 4s, 8s, 16s, 32s (max)
    const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempts), 32000);
    this.reconnectAttempts++;

    console.log(`[SyncTransport] Reconnecting in ${delay}ms (attempt ${this.reconnectAttempts})`);

    this.reconnectTimeout = setTimeout(() => {
      this.reconnectTimeout = null;
      this.connect();
    }, delay);
  }
}

/**
 * Create a workspace sync transport.
 */
export function createWorkspaceSyncTransport(options: Omit<SyncTransportOptions, 'docType'>): SyncTransport {
  return new SyncTransport({
    ...options,
    docType: 'workspace',
  });
}

/**
 * Create a body sync transport.
 */
export function createBodySyncTransport(options: Omit<SyncTransportOptions, 'docType'>): SyncTransport {
  return new SyncTransport({
    ...options,
    docType: 'body',
  });
}
