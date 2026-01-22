/**
 * Web Worker entry point for DiaryxBackend.
 *
 * This worker uses the new native storage backends (OPFS/IndexedDB) directly,
 * eliminating the need for InMemoryFileSystem and JS↔WASM sync.
 *
 * All operations use the unified `execute()` command API, except:
 * - `getConfig` / `saveConfig`: WASM-specific config stored in root frontmatter
 * - `readBinary` / `writeBinary`: Efficient Uint8Array handling (no base64 overhead)
 */

import * as Comlink from 'comlink';
import type { BackendEventType, BackendEventListener } from './interface';
import type { StorageType } from './storageType';

// We'll dynamically import the WASM module
let backend: any | null = null;

// Discovered workspace root path (set after init)
let rootPath: string | null = null;

// Event port for forwarding filesystem events to main thread
let eventPort: MessagePort | null = null;

// Subscription ID for filesystem events (to clean up on shutdown)
let fsEventSubscriptionId: number | null = null;

// Clear cached root path (call after rename operations)
function clearRootPathCache() {
  rootPath = null;
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
 * Execute a command and parse the response.
 * Helper to avoid repetitive JSON.stringify/parse in each method.
 */
async function executeCommand<T = any>(type: string, params?: Record<string, any>): Promise<T> {
  const command = params ? { type, params } : { type };
  const json = await getBackend().execute(JSON.stringify(command));
  return JSON.parse(json);
}

/**
 * Execute a command and extract the data from the response.
 * Throws if the response type doesn't match the expected type.
 */
async function executeAndExtract<T>(
  type: string,
  params: Record<string, any> | undefined,
  expectedResponseType: string
): Promise<T> {
  const response = await executeCommand(type, params);
  if (response.type === expectedResponseType) {
    return response.data as T;
  }
  if (response.type === 'Ok') {
    return undefined as T; // For void responses
  }
  throw new Error(`Expected ${expectedResponseType}, got ${response.type}: ${JSON.stringify(response)}`);
}

/**
 * Initialize the backend and set up event forwarding.
 */
async function init(port: MessagePort, storageType: StorageType, directoryHandle?: FileSystemDirectoryHandle): Promise<void> {
  // Store event port for forwarding filesystem events
  eventPort = port;

  // Initialize CRDT storage bridge BEFORE importing WASM
  // This sets up the global bridge that Rust will use for persistent CRDT storage
  // Skip for memory storage (guest mode) since CRDT doesn't need persistence there
  if (storageType !== 'memory') {
    try {
      const { setupCrdtStorageBridge } = await import('../storage/index.js');
      await setupCrdtStorageBridge();
      console.log('[WasmWorker] CRDT storage bridge initialized');
    } catch (e) {
      console.warn('[WasmWorker] Failed to initialize CRDT storage bridge, using memory storage:', e);
    }
  }

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
  } else if (storageType === 'memory') {
    // In-memory storage for guest mode - files live only in memory
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    backend = (wasm.DiaryxBackend as any).createInMemory();
  } else {
    backend = await wasm.DiaryxBackend.createIndexedDb();
  }

  // Subscribe to filesystem events and forward them to the main thread
  if (backend.onFileSystemEvent && eventPort) {
    fsEventSubscriptionId = backend.onFileSystemEvent((eventJson: string) => {
      // Forward the event JSON to the main thread via MessagePort
      eventPort!.postMessage({ type: 'FileSystemEvent', data: eventJson });
    });
    console.log('[WasmWorker] Subscribed to filesystem events, id:', fsEventSubscriptionId);
  } else {
    console.log('[WasmWorker] Filesystem events not available on this backend');
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
 * Worker API exposed via Comlink.
 *
 * All methods use the unified command API through `execute()` except where noted.
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
  // Config (kept as native calls - WASM-specific frontmatter storage)
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
  // Root Index Discovery (uses commands)
  // =========================================================================

  async findRootIndex(dirPath?: string): Promise<string | null> {
    const directory = dirPath ?? '.';
    const response = await executeCommand('FindRootIndex', { directory });
    if (response.type === 'String') {
      return response.data;
    }
    return null;
  },

  async getDefaultWorkspacePath(): Promise<string> {
    // Return cached path if available
    if (rootPath) return rootPath;

    // Try to discover root index in current directory first
    let root = await this.findRootIndex('.');

    // Fallback: try "workspace" directory (OPFS default)
    if (!root) {
      root = await this.findRootIndex('workspace');
    }

    // Fallback: scan all top-level directories for a root index
    if (!root) {
      try {
        // Use GetFilesystemTree to list directories
        const response = await executeCommand('GetFilesystemTree', {
          path: '.',
          show_hidden: false,
          depth: 1
        });
        if (response.type === 'Tree' && response.data?.children) {
          for (const child of response.data.children) {
            if (child.children && child.children.length >= 0) {
              // It's a directory
              const found = await this.findRootIndex(child.path);
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
  // Workspace (uses commands)
  // =========================================================================

  async getWorkspaceTree(workspacePath?: string, depth?: number): Promise<any> {
    const path = workspacePath ?? await this.getDefaultWorkspacePath();
    return executeAndExtract('GetWorkspaceTree', { path, depth: depth ?? null }, 'Tree');
  },

  async createWorkspace(path?: string, name?: string): Promise<string> {
    const workspacePath = path ?? '.';
    const workspaceName = name ?? 'My Workspace';
    await executeCommand('CreateWorkspace', { path: workspacePath, name: workspaceName });
    rootPath = workspacePath; // Cache the new workspace path
    return workspacePath;
  },

  async getFilesystemTree(workspacePath?: string, showHidden?: boolean): Promise<any> {
    const path = workspacePath ?? await this.getDefaultWorkspacePath();
    return executeAndExtract('GetFilesystemTree', {
      path,
      show_hidden: showHidden ?? false,
      depth: null
    }, 'Tree');
  },

  // =========================================================================
  // Entries (uses commands)
  // =========================================================================

  async getEntry(path: string): Promise<any> {
    return executeAndExtract('GetEntry', { path }, 'Entry');
  },

  async saveEntry(path: string, content: string): Promise<void> {
    await executeCommand('SaveEntry', { path, content });
  },

  async createEntry(path: string, options?: { title?: string }): Promise<string> {
    return executeAndExtract('CreateEntry', {
      path,
      options: {
        title: options?.title ?? null,
        part_of: null,
        template: null
      }
    }, 'String');
  },

  async deleteEntry(path: string): Promise<void> {
    await executeCommand('DeleteEntry', { path });
  },

  async moveEntry(fromPath: string, toPath: string): Promise<string> {
    await executeCommand('MoveEntry', { from: fromPath, to: toPath });
    return toPath;
  },

  async renameEntry(path: string, newFilename: string): Promise<string> {
    const result = await executeAndExtract<string>('RenameEntry', { path, new_filename: newFilename }, 'String');
    // Clear cached root path in case we renamed the root index
    clearRootPathCache();
    return result;
  },

  async duplicateEntry(path: string): Promise<string> {
    return executeAndExtract('DuplicateEntry', { path }, 'String');
  },

  // =========================================================================
  // Frontmatter (uses commands)
  // =========================================================================

  async getFrontmatter(path: string): Promise<any> {
    return executeAndExtract('GetFrontmatter', { path }, 'Frontmatter');
  },

  async setFrontmatterProperty(path: string, key: string, value: any): Promise<void> {
    await executeCommand('SetFrontmatterProperty', { path, key, value });
  },

  // =========================================================================
  // Search (uses commands)
  // =========================================================================

  async searchWorkspace(pattern: string, options?: any): Promise<any> {
    const workspacePath = options?.workspacePath ?? await this.getDefaultWorkspacePath();
    return executeAndExtract('SearchWorkspace', {
      pattern,
      options: {
        workspace_path: workspacePath,
        search_frontmatter: options?.searchFrontmatter ?? false,
        property: options?.property ?? null,
        case_sensitive: options?.caseSensitive ?? false
      }
    }, 'SearchResults');
  },

  // =========================================================================
  // Validation (uses commands)
  // =========================================================================

  async validateWorkspace(workspacePath?: string): Promise<any> {
    const path = workspacePath ?? await this.getDefaultWorkspacePath();
    return executeAndExtract('ValidateWorkspace', { path }, 'ValidationResult');
  },

  // =========================================================================
  // File Operations (uses commands except binary)
  // =========================================================================

  async fileExists(path: string): Promise<boolean> {
    return executeAndExtract('FileExists', { path }, 'Bool');
  },

  async readFile(path: string): Promise<string> {
    return executeAndExtract('ReadFile', { path }, 'String');
  },

  async writeFile(path: string, content: string): Promise<void> {
    await executeCommand('WriteFile', { path, content });
  },

  async deleteFile(path: string): Promise<void> {
    await executeCommand('DeleteFile', { path });
  },

  // Binary operations kept as native calls for efficiency (no base64 overhead)
  async readBinary(path: string): Promise<Uint8Array> {
    return getBackend().readBinary(path);
  },

  async writeBinary(path: string, data: Uint8Array): Promise<void> {
    return getBackend().writeBinary(path, data);
  },

  // =========================================================================
  // Export Operations (uses commands)
  // =========================================================================

  async getAvailableAudiences(rootPath: string): Promise<string[]> {
    return executeAndExtract('GetAvailableAudiences', { root_path: rootPath }, 'Strings');
  },

  async planExport(rootPath: string, audience: string): Promise<any> {
    return executeAndExtract('PlanExport', { root_path: rootPath, audience }, 'ExportPlan');
  },

  async exportToMemory(rootPath: string, audience: string): Promise<any[]> {
    return executeAndExtract('ExportToMemory', { root_path: rootPath, audience }, 'ExportedFiles');
  },

  async exportToHtml(rootPath: string, audience: string): Promise<any[]> {
    return executeAndExtract('ExportToHtml', { root_path: rootPath, audience }, 'ExportedFiles');
  },

  async exportBinaryAttachments(rootPath: string, audience: string): Promise<{ source_path: string; relative_path: string }[]> {
    return executeAndExtract('ExportBinaryAttachments', { root_path: rootPath, audience }, 'BinaryFilePaths');
  },

  // Generic method call for any other operations (fallback to native)
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
