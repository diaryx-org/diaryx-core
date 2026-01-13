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
  /** Initial content to seed if CRDT is empty */
  initialContent?: string;
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
  private updateHandler: ((update: Uint8Array, origin: unknown) => void) | null = null;
  private destroyed = false;

  private constructor(options: YDocProxyOptions) {
    this.ydoc = new Y.Doc();
    this.docName = options.docName;
    this.rustApi = options.rustApi;
    this.onContentChange = options.onContentChange;
  }

  /**
   * Create and initialize a YDocProxy.
   * Loads initial state from Rust CRDT.
   */
  static async create(options: YDocProxyOptions): Promise<YDocProxy> {
    const proxy = new YDocProxy(options);
    await proxy.initialize(options.initialContent);
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
    const text = this.ydoc.getText('default');
    return text.toString();
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
      const text = this.ydoc.getText('default');
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
        await this.rustApi.applyBodyUpdate(this.docName, update);

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
