/**
 * Y.Doc Proxy for TipTap integration with Rust CRDT.
 *
 * TipTap's Collaboration extension requires a JavaScript Y.Doc.
 * This proxy creates a JS Y.Doc that stays in sync with the Rust CRDT backend.
 *
 * Architecture:
 * - JS Y.Doc serves as the render layer for TipTap
 * - Rust CRDT is the source of truth
 * - Changes flow bidirectionally via Y.js updates
 */

import * as Y from 'yjs';
import type { RustCrdtApi } from './rustCrdtApi';

export interface YDocProxyOptions {
  /** Document name/path for the CRDT */
  docName: string;
  /** Rust CRDT API instance */
  rustApi: RustCrdtApi;
  /** Optional callback when content changes */
  onContentChange?: (content: string) => void;
  /** Optional callback when local updates should be broadcast to peers */
  onLocalUpdate?: (update: Uint8Array) => void;
  /** Initial content to seed if CRDT is empty */
  initialContent?: string;
  /** Initialization timeout in milliseconds (default: 10000ms) */
  initTimeoutMs?: number;
}

/**
 * Proxy Y.Doc that syncs with Rust CRDT.
 *
 * Usage:
 * ```ts
 * const proxy = await YDocProxy.create({
 *   docName: 'workspace/notes.md',
 *   rustApi: crdtApi,
 * });
 *
 * // Use with TipTap
 * Collaboration.configure({ document: proxy.getYDoc() })
 *
 * // Cleanup when done
 * proxy.destroy();
 * ```
 */
export class YDocProxy {
  private ydoc: Y.Doc;
  private docName: string;
  private rustApi: RustCrdtApi;
  private onContentChange?: (content: string) => void;
  private onLocalUpdate?: (update: Uint8Array) => void;
  private updateHandler: ((update: Uint8Array, origin: unknown) => void) | null = null;
  private destroyed = false;

  private constructor(options: YDocProxyOptions) {
    this.ydoc = new Y.Doc();
    this.docName = options.docName;
    this.rustApi = options.rustApi;
    this.onContentChange = options.onContentChange;
    this.onLocalUpdate = options.onLocalUpdate;
  }

  /**
   * Create and initialize a YDocProxy.
   * Loads initial state from Rust CRDT.
   *
   * @throws Error if initialization times out
   */
  static async create(options: YDocProxyOptions): Promise<YDocProxy> {
    const proxy = new YDocProxy(options);
    const timeoutMs = options.initTimeoutMs ?? 10000;

    // Wrap initialization with timeout to prevent indefinite hangs
    const initPromise = proxy.initialize(options.initialContent);
    const timeoutPromise = new Promise<never>((_, reject) => {
      setTimeout(
        () => reject(new Error(`YDocProxy initialization timed out after ${timeoutMs}ms`)),
        timeoutMs
      );
    });

    try {
      await Promise.race([initPromise, timeoutPromise]);
    } catch (error) {
      // Cleanup on failure
      proxy.destroy();
      throw error;
    }

    return proxy;
  }

  /**
   * Get the underlying Y.Doc for TipTap.
   */
  getYDoc(): Y.Doc {
    return this.ydoc;
  }

  /**
   * Get the document name/path.
   */
  getDocName(): string {
    return this.docName;
  }

  /**
   * Check if the proxy has been destroyed.
   */
  isDestroyed(): boolean {
    return this.destroyed;
  }

  /**
   * Apply an update from a remote peer (e.g., Hocuspocus).
   * Routes through Rust CRDT first, then applies to JS Y.Doc.
   */
  async applyRemoteUpdate(update: Uint8Array): Promise<void> {
    if (this.destroyed) return;

    // Apply to Rust CRDT first (source of truth)
    await this.rustApi.applyBodyUpdate(this.docName, update);

    // Then apply to JS Y.Doc for rendering
    Y.applyUpdate(this.ydoc, update, 'rust');
  }

  /**
   * Get the current sync state vector.
   * Can be used for incremental sync.
   */
  async getSyncState(): Promise<Uint8Array> {
    return this.rustApi.getBodySyncState(this.docName);
  }

  /**
   * Get updates that a peer is missing.
   */
  async getMissingUpdates(remoteStateVector: Uint8Array): Promise<Uint8Array> {
    return this.rustApi.getBodyMissingUpdates(this.docName, remoteStateVector);
  }

  /**
   * Get the full CRDT state as an update.
   */
  async getFullState(): Promise<Uint8Array> {
    return this.rustApi.getBodyFullState(this.docName);
  }

  /**
   * Get the current text content.
   */
  getContent(): string {
    const text = this.ydoc.getText('body');
    return text.toString();
  }

  /**
   * Set the text content (replaces existing content).
   * Used to sync local edits from TipTap to the CRDT.
   */
  setContent(newContent: string): void {
    if (this.destroyed) return;

    const text = this.ydoc.getText('body');
    const currentContent = text.toString();

    // Skip if content is the same (avoid unnecessary updates)
    if (currentContent === newContent) return;

    // Use a transaction to batch the delete + insert as a single update
    this.ydoc.transact(() => {
      // Clear existing content
      if (text.length > 0) {
        text.delete(0, text.length);
      }
      // Insert new content
      if (newContent.length > 0) {
        text.insert(0, newContent);
      }
    }, 'local'); // Origin 'local' to distinguish from 'rust' updates
  }

  /**
   * Save the current state to Rust storage.
   */
  async save(): Promise<void> {
    await this.rustApi.saveBodyDoc(this.docName);
  }

  /**
   * Destroy the proxy and cleanup resources.
   */
  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;

    // Remove update handler
    if (this.updateHandler) {
      this.ydoc.off('update', this.updateHandler);
      this.updateHandler = null;
    }

    // Destroy Y.Doc
    this.ydoc.destroy();
  }

  // Private methods

  private async initialize(initialContent?: string): Promise<void> {
    // Load initial state from Rust CRDT
    const fullState = await this.rustApi.getBodyFullState(this.docName);

    if (fullState.length > 0) {
      // Apply existing state from Rust
      Y.applyUpdate(this.ydoc, fullState, 'rust');
    } else if (initialContent) {
      // Seed with initial content if CRDT is empty
      const text = this.ydoc.getText('body');
      text.insert(0, initialContent);

      // Sync initial content to Rust
      const update = Y.encodeStateAsUpdate(this.ydoc);
      await this.rustApi.applyBodyUpdate(this.docName, update);
    }

    // Set up JS -> Rust sync
    this.setupJsToRustSync();
  }

  private setupJsToRustSync(): void {
    // Listen for local updates and forward to Rust
    this.updateHandler = async (update: Uint8Array, origin: unknown) => {
      if (this.destroyed) return;

      // Skip updates that came from Rust (origin === 'rust')
      if (origin === 'rust') return;

      try {
        // Forward update to Rust CRDT
        console.log('[YDocProxy] Syncing update to Rust:', this.docName, 'size:', update.length);
        const updateId = await this.rustApi.applyBodyUpdate(this.docName, update);
        console.log('[YDocProxy] Update synced, ID:', updateId);

        // Broadcast update to peers (for real-time sync)
        if (this.onLocalUpdate) {
          console.log('[YDocProxy] Broadcasting update to peers:', update.length, 'bytes');
          this.onLocalUpdate(update);
        }

        // Notify content change
        if (this.onContentChange) {
          const content = this.getContent();
          this.onContentChange(content);
        }
      } catch (error) {
        console.error('[YDocProxy] Failed to sync update to Rust:', error);
      }
    };

    this.ydoc.on('update', this.updateHandler);
  }
}

/**
 * Create a YDocProxy for a document.
 */
export async function createYDocProxy(options: YDocProxyOptions): Promise<YDocProxy> {
  return YDocProxy.create(options);
}
