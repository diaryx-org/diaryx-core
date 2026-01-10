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

// Discovered workspace root path (set after init)
let rootPath: string | null = null;

// Clear cached root path (call after rename operations)
function clearRootPathCache() {
  rootPath = null;
}

// Event port for streaming events back to main thread - currently unused in new implementation
// as events are forwarded differently or just log warnings for now?
// Actually, I should probably remove the variable.

/**
 * Initialize the backend and set up event forwarding.
 */
async function init(_port: MessagePort, storageType: StorageType, directoryHandle?: FileSystemDirectoryHandle): Promise<void> {
  // _port unused for now, as events are not yet fully implemented in this worker wrapper
  // or are handled via side channels? 
  // The original code had: _eventPort = port;
  
  // Import WASM module
  const wasm = await import('../wasm/diaryx_wasm.js');
  await wasm.default();
  
  // Create backend with specified storage type
  if (storageType === 'opfs') {
    backend = await wasm.DiaryxBackend.createOpfs();
  } else if (storageType === 'filesystem-access') {
    if (!directoryHandle) {
      throw new Error('Directory handle required for filesystem-access storage type');
    }
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    backend = (wasm.DiaryxBackend as any).createFromDirectoryHandle(directoryHandle);
  } else {
    backend = await wasm.DiaryxBackend.createIndexedDb();
  }
  
  console.log('[WasmWorker] DiaryxBackend initialized with storage:', storageType);
}

/**
 * Initialize the backend with a File System Access API directory handle.
 * This is called when the user selects "Local Folder" storage.
 */
async function initWithDirectoryHandle(_port: MessagePort, directoryHandle: FileSystemDirectoryHandle): Promise<void> {
  return init(_port, 'filesystem-access', directoryHandle);
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
  initWithDirectoryHandle,
  
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
  // Unified Command API
  // =========================================================================

  /**
   * Execute a command using the unified command pattern.
   * Takes JSON string command, returns JSON string response.
   */
  async execute(commandJson: string): Promise<string> {
    return getBackend().execute(commandJson);
  },

  // =========================================================================
  // Root Index Discovery
  // =========================================================================

  async findRootIndex(dirPath?: string): Promise<string | null> {
    const path = dirPath ?? '.';
    const result = await getBackend().findRootIndex(path);
    return result ?? null;
  },

  async getDefaultWorkspacePath(): Promise<string> {
    // Return cached path if available
    if (rootPath) return rootPath;
    
    // Try to discover root index in current directory first
    let root = await getBackend().findRootIndex('.');
    
    // Fallback: try "workspace" directory (OPFS default)
    if (!root) {
      root = await getBackend().findRootIndex('workspace');
    }
    
    // Fallback: scan all top-level directories for a root index
    if (!root) {
      try {
        const entries = await getBackend().listDirectories('.');
        if (entries && Array.isArray(entries)) {
          for (const entry of entries) {
            if (typeof entry === 'string' && entry !== '.' && entry !== '..') {
              const found = await getBackend().findRootIndex(entry);
              if (found) {
                root = found;
                break;
              }
            }
          }
        }
      } catch (e) {
        console.warn('[WasmWorker] Failed to scan directories:', e);
      }
    }
    
    if (root) {
      // Get parent directory of root index
      const lastSlash = root.lastIndexOf('/');
      const discoveredPath = lastSlash > 0 ? root.substring(0, lastSlash) : '.';
      rootPath = discoveredPath;
      return discoveredPath;
    }
    
    // Fallback to current directory
    return '.';
  },
  
  // Clear cached root path (for after rename operations)
  clearRootPathCache(): void {
    clearRootPathCache();
  },

  // =========================================================================
  // Workspace
  // =========================================================================
  
  async getWorkspaceTree(workspacePath?: string, depth?: number): Promise<any> {
    const path = workspacePath ?? await this.getDefaultWorkspacePath();
    return getBackend().getTree(path, depth ?? null);
  },
  
  async createWorkspace(path?: string, name?: string): Promise<string> {
    const workspacePath = path ?? '.';
    const workspaceName = name ?? 'My Workspace';
    await getBackend().createWorkspace(workspacePath, workspaceName);
    rootPath = workspacePath; // Cache the new workspace path
    return workspacePath;
  },
  
  async getFilesystemTree(workspacePath?: string, showHidden?: boolean): Promise<any> {
    const path = workspacePath ?? await this.getDefaultWorkspacePath();
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
    const result = await getBackend().renameEntry(path, newFilename);
    // Clear cached root path in case we renamed the root index
    clearRootPathCache();
    return result;
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
    const workspacePath = options?.workspacePath ?? await this.getDefaultWorkspacePath();
    return getBackend().search(workspacePath, pattern);
  },

  // =========================================================================
  // Validation
  // =========================================================================
  
  async validateWorkspace(workspacePath?: string): Promise<any> {
    const path = workspacePath ?? await this.getDefaultWorkspacePath();
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
