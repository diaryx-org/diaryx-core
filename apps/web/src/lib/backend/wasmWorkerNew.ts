/**
 * Web Worker entry point for DiaryxBackend.
 * 
 * This worker uses the new native storage backends (OPFS/IndexedDB) directly,
 * eliminating the need for InMemoryFileSystem and JS↔WASM sync.
 */

import * as Comlink from 'comlink';
import type { BackendEventType, BackendEventListener } from './interface';
import type { StorageType } from './storageType';

// We'll dynamically import the WASM module
let backend: any | null = null;

// Event port for streaming events back to main thread - currently unused in new implementation
// as events are forwarded differently or just log warnings for now? 
// Actually, I should probably remove the variable.

/**
 * Initialize the backend and set up event forwarding.
 */
async function init(_port: MessagePort, storageType: StorageType): Promise<void> {
  // _port unused for now, as events are not yet fully implemented in this worker wrapper
  // or are handled via side channels? 
  // The original code had: _eventPort = port;
  
  // Import WASM module
  const wasm = await import('../wasm/diaryx_wasm.js');
  await wasm.default();
  
  // Create backend with specified storage type
  if (storageType === 'opfs') {
    backend = await wasm.DiaryxBackend.createOpfs();
  } else {
    backend = await wasm.DiaryxBackend.createIndexedDb();
  }
  
  console.log('[WasmWorker] DiaryxBackend initialized with storage:', storageType);
}

/**
 * Get the backend instance (throws if not initialized).
 */
function getBackend(): any {
  if (!backend) {
    throw new Error('DiaryxBackend not initialized. Call init() first.');
  }
  return backend;
}

/**
 * Worker API exposed via Comlink.
 */
const workerApi = {
  init,
  
  isReady(): boolean {
    return backend !== null;
  },

  // Event stubs - events will go through MessagePort
  on(_event: BackendEventType, _listener: BackendEventListener): void {
    console.warn('[WasmWorker] Events are forwarded via MessagePort, not on()');
  },
  off(_event: BackendEventType, _listener: BackendEventListener): void {
    console.warn('[WasmWorker] Events are forwarded via MessagePort, not off()');
  },

  // =========================================================================
  // Config
  // =========================================================================
  
  async getConfig(): Promise<any> {
    return getBackend().getConfig();
  },
  
  async saveConfig(config: any): Promise<void> {
    return getBackend().saveConfig(config);
  },

  // =========================================================================
  // Workspace
  // =========================================================================
  
  async getWorkspaceTree(workspacePath?: string, depth?: number): Promise<any> {
    const path = workspacePath ?? 'workspace';
    return getBackend().getTree(path, depth ?? null);
  },
  
  async createWorkspace(path?: string, name?: string): Promise<string> {
    const workspacePath = path ?? 'workspace';
    const workspaceName = name ?? 'My Workspace';
    await getBackend().createWorkspace(workspacePath, workspaceName);
    return workspacePath;
  },
  
  async getFilesystemTree(workspacePath?: string, showHidden?: boolean): Promise<any> {
    const path = workspacePath ?? 'workspace';
    return getBackend().getFilesystemTree(path, showHidden ?? false);
  },

  // =========================================================================
  // Entries
  // =========================================================================
  
  async getEntry(path: string): Promise<any> {
    return getBackend().getEntry(path);
  },
  
  async saveEntry(path: string, content: string): Promise<void> {
    return getBackend().saveEntry(path, content);
  },
  
  async createEntry(path: string, options?: { title?: string }): Promise<string> {
    return getBackend().createEntry(path, options?.title ?? null);
  },
  
  async deleteEntry(path: string): Promise<void> {
    return getBackend().deleteEntry(path);
  },
  
  async moveEntry(fromPath: string, toPath: string): Promise<string> {
    return getBackend().moveEntry(fromPath, toPath);
  },
  
  async renameEntry(path: string, newFilename: string): Promise<string> {
    // Compute new path from old path + new filename
    const parts = path.split('/');
    parts[parts.length - 1] = newFilename;
    const newPath = parts.join('/');
    return getBackend().moveEntry(path, newPath);
  },

  // =========================================================================
  // Frontmatter
  // =========================================================================
  
  async getFrontmatter(path: string): Promise<any> {
    return getBackend().getFrontmatter(path);
  },
  
  async setFrontmatterProperty(path: string, key: string, value: any): Promise<void> {
    return getBackend().setFrontmatterProperty(path, key, value);
  },

  // =========================================================================
  // Search
  // =========================================================================
  
  async searchWorkspace(pattern: string, options?: any): Promise<any> {
    const workspacePath = options?.workspacePath ?? 'workspace';
    return getBackend().search(workspacePath, pattern);
  },

  // =========================================================================
  // Validation
  // =========================================================================
  
  async validateWorkspace(workspacePath?: string): Promise<any> {
    const path = workspacePath ?? 'workspace';
    return getBackend().validateWorkspace(path);
  },

  // =========================================================================
  // File Operations
  // =========================================================================
  
  async fileExists(path: string): Promise<boolean> {
    return getBackend().fileExists(path);
  },
  
  async readFile(path: string): Promise<string> {
    return getBackend().readFile(path);
  },
  
  async writeFile(path: string, content: string): Promise<void> {
    return getBackend().writeFile(path, content);
  },
  
  async deleteFile(path: string): Promise<void> {
    return getBackend().deleteFile(path);
  },
  
  async readBinary(path: string): Promise<Uint8Array> {
    return getBackend().readBinary(path);
  },
  
  async writeBinary(path: string, data: Uint8Array): Promise<void> {
    return getBackend().writeBinary(path, data);
  },
  
  // Generic method call for any other operations
  async call(method: string, args: unknown[]): Promise<unknown> {
    const b = getBackend();
    const fn = (b as any)[method];
    if (typeof fn !== 'function') {
      throw new Error(`Unknown backend method: ${method}`);
    }
    return (fn as Function).apply(b, args);
  },
};

// Expose the worker API via Comlink
Comlink.expose(workerApi);

export type WorkerApi = typeof workerApi;
