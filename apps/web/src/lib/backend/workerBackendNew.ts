/**
 * WorkerBackendNew - Main thread proxy for DiaryxBackend in Web Worker.
 * 
 * Uses the new native storage backends (OPFS/IndexedDB) directly,
 * eliminating the need for InMemoryFileSystem sync.
 */

import * as Comlink from 'comlink';
import type {
  Backend,
  BackendEventType,
  BackendEventListener,
  Command,
  Response,
  Config,
} from './interface';
import { BackendEventEmitter } from './eventEmitter';
import type { WorkerApi } from './wasmWorkerNew';
import { getStorageType } from './storageType';

export class WorkerBackendNew implements Backend {
  private worker: Worker | null = null;
  private remote: Comlink.Remote<WorkerApi> | null = null;
  private eventEmitter = new BackendEventEmitter();
  private _ready = false;

  async init(): Promise<void> {
    // Create the worker
    this.worker = new Worker(
      new URL('./wasmWorkerNew.ts', import.meta.url),
      { type: 'module' }
    );
    
    // Wrap with Comlink
    this.remote = Comlink.wrap<WorkerApi>(this.worker);
    
    // Create message channel for events (future use)
    const { port1, port2 } = new MessageChannel();
    
    // Listen for events from worker
    port1.onmessage = (event) => {
      this.eventEmitter.emit(event.data);
    };
    port1.start();
    
    // Initialize the backend with storage type
    const storageType = getStorageType();
    console.log(`[WorkerBackendNew] Initializing with storage: ${storageType}`);
    
    if (storageType === 'filesystem-access') {
      // For FSA, we need to get or request the directory handle
      const { getStoredFileSystemHandle, storeFileSystemHandle } = await import('./storageType');
      let handle = await getStoredFileSystemHandle();
      
      if (!handle) {
        // No stored handle - prompt user to select a folder
        // Note: showDirectoryPicker requires user gesture, so this will fail if not triggered by user action
        try {
          handle = await (window as any).showDirectoryPicker({ mode: 'readwrite' });
          await storeFileSystemHandle(handle!);
        } catch (e) {
          console.error('[WorkerBackendNew] Failed to get directory handle:', e);
          throw new Error('Failed to open local folder. Please try again from Settings.');
        }
      } else {
        // Verify we still have permission
        // Note: queryPermission/requestPermission are not in TypeScript's lib types yet
        const permission = await (handle as any).queryPermission({ mode: 'readwrite' });
        if (permission !== 'granted') {
          try {
            const newPermission = await (handle as any).requestPermission({ mode: 'readwrite' });
            if (newPermission !== 'granted') {
              throw new Error('Permission denied');
            }
          } catch (e) {
            console.error('[WorkerBackendNew] Permission denied for directory:', e);
            throw new Error('Permission denied for local folder. Please reselect in Settings.');
          }
        }
      }
      
      await this.remote.initWithDirectoryHandle(Comlink.transfer(port2, [port2]), handle!);
    } else {
      await this.remote.init(Comlink.transfer(port2, [port2]), storageType);
    }
    
    this._ready = true;
    console.log('[WorkerBackendNew] Ready');
  }

  isReady(): boolean {
    return this._ready;
  }

  /**
   * Get the workspace path.
   * For WASM, this is always "workspace" (virtual path in OPFS/IndexedDB).
   */
  getWorkspacePath(): string {
    return "workspace/index.md";
  }

  /**
   * Get config (not applicable for WASM - config is Tauri-specific).
   */
  getConfig(): Config | null {
    return {
      default_workspace: "workspace",
    };
  }

  /**
   * Get app paths (Tauri-specific, returns null for WASM).
   */
  getAppPaths(): Record<string, string | boolean | null> | null {
    return null;
  }

  // Event subscription
  on(event: BackendEventType, listener: BackendEventListener): void {
    this.eventEmitter.on(event, listener);
  }

  off(event: BackendEventType, listener: BackendEventListener): void {
    this.eventEmitter.off(event, listener);
  }

  // =========================================================================
  // Unified Command API
  // =========================================================================

  /**
   * Execute a command via the unified command pattern.
   * This is the new primary API - all operations can be performed via execute().
   */
  async execute(command: Command): Promise<Response> {
    // Custom replacer to handle BigInt serialization
    const commandJson = JSON.stringify(command, (_key, value) =>
      typeof value === 'bigint' ? Number(value) : value
    );
    const responseJson = await this.remote!.execute(commandJson);
    // Custom reviver to handle BigInt deserialization for known fields
    return JSON.parse(responseJson, (key, value) => {
      // Convert numeric timestamps back to BigInt for specific fields
      if ((key === 'modified_at' || key === 'uploaded_at' || key === 'size' || 
           key === 'timestamp' || key === 'update_id') && typeof value === 'number') {
        return BigInt(value);
      }
      return value;
    }) as Response;
  }

  // =========================================================================
  // Proxy methods - delegate to worker (legacy, will be deprecated)
  // =========================================================================

  saveConfig = (config: any) => this.remote!.saveConfig(config);

  // Root index discovery
  findRootIndex = (dirPath?: string) => this.remote!.findRootIndex(dirPath);
  getDefaultWorkspacePath = () => this.remote!.getDefaultWorkspacePath();

  getWorkspaceTree = (path?: string, depth?: number) => 
    this.remote!.getWorkspaceTree(path, depth);
  createWorkspace = (path?: string, name?: string) => 
    this.remote!.createWorkspace(path, name);
  getFilesystemTree = (path?: string, showHidden?: boolean) => 
    this.remote!.getFilesystemTree(path, showHidden);

  getEntry = (path: string) => this.remote!.getEntry(path);
  saveEntry = (path: string, content: string) => this.remote!.saveEntry(path, content);
  createEntry = (path: string, options?: any) => this.remote!.createEntry(path, options);
  deleteEntry = (path: string) => this.remote!.deleteEntry(path);
  moveEntry = (from: string, to: string) => this.remote!.moveEntry(from, to);
  renameEntry = (path: string, newName: string) => this.remote!.renameEntry(path, newName);
  duplicateEntry = (path: string) => this.remote!.duplicateEntry(path);

  getFrontmatter = (path: string) => this.remote!.getFrontmatter(path);
  setFrontmatterProperty = (path: string, key: string, value: any) =>
    this.remote!.setFrontmatterProperty(path, key, value);
  
  searchWorkspace = (pattern: string, options?: any) =>
    this.remote!.searchWorkspace(pattern, options);
  
  validateWorkspace = (path?: string) => this.remote!.validateWorkspace(path);

  // File operations
  fileExists = (path: string) => this.remote!.fileExists(path);
  readFile = (path: string) => this.remote!.readFile(path);
  writeFile = (path: string, content: string) => this.remote!.writeFile(path, content);
  deleteFile = (path: string) => this.remote!.deleteFile(path);
  readBinary = (path: string) => this.remote!.readBinary(path);
  writeBinary = (path: string, data: Uint8Array) => this.remote!.writeBinary(path, data);

  // =========================================================================
  // Stubs for methods not yet in new backend (delegate via call)
  // =========================================================================
  
  async persist(): Promise<void> {
    // No-op: native storage persists automatically
  }

  slugifyTitle(title: string): string {
    const slug = title
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '');
    return slug ? `${slug}.md` : 'untitled.md';
  }

  attachEntryToParent = (entry: string, parent: string): Promise<string> =>
    this.remote!.call('attachToParent', [entry, parent]) as Promise<string>;
  
  convertToIndex = (path: string): Promise<string> =>
    this.remote!.call('convertToIndex', [path]) as Promise<string>;
  
  convertToLeaf = (path: string): Promise<string> =>
    this.remote!.call('convertToLeaf', [path]) as Promise<string>;
  
  createChildEntry = (parentPath: string): Promise<string> =>
    this.remote!.call('createChildEntry', [parentPath]) as Promise<string>;
  
  ensureDailyEntry = (): Promise<string> =>
    this.remote!.call('ensureDailyEntry', []) as Promise<string>;

  getAvailableAudiences = (rootPath: string): Promise<string[]> =>
    this.remote!.call('getAvailableAudiences', [rootPath]) as Promise<string[]>;
  
  planExport = (rootPath: string, audience: string): Promise<any> =>
    this.remote!.call('planExport', [rootPath, audience]) as Promise<any>;
  
  exportToMemory = (rootPath: string, audience: string): Promise<any> =>
    this.remote!.call('exportToMemory', [rootPath, audience]) as Promise<any>;
  
  exportToHtml = (rootPath: string, audience: string): Promise<any> =>
    this.remote!.call('exportToHtml', [rootPath, audience]) as Promise<any>;
  
  exportBinaryAttachments = (rootPath: string, audience: string): Promise<any> =>
    this.remote!.call('exportBinaryAttachments', [rootPath, audience]) as Promise<any>;

  getAttachments = (entryPath: string): Promise<string[]> =>
    this.remote!.call('listAttachments', [entryPath]) as Promise<string[]>;
  
  uploadAttachment = (entryPath: string, filename: string, dataBase64: string): Promise<string> =>
    this.remote!.call('uploadAttachment', [entryPath, filename, dataBase64]) as Promise<string>;
  
  deleteAttachment = (entryPath: string, attachmentPath: string): Promise<void> =>
    this.remote!.call('deleteAttachment', [entryPath, attachmentPath]) as Promise<void>;
  
  getStorageUsage = (): Promise<any> =>
    this.remote!.call('getStorageUsage', []) as Promise<any>;
  
  getAttachmentData = (entryPath: string, attachmentPath: string): Promise<Uint8Array> =>
    this.remote!.call('getAttachmentData', [entryPath, attachmentPath]) as Promise<Uint8Array>;

  removeFrontmatterProperty = (path: string, key: string): Promise<void> =>
    this.remote!.call('removeFrontmatterProperty', [path, key]) as Promise<void>;

  listTemplates = async (): Promise<any> => {
    const wsPath = await this.getDefaultWorkspacePath();
    return this.remote!.call('listTemplates', [wsPath]) as Promise<any>;
  };
  
  getTemplate = async (name: string): Promise<string> => {
    const wsPath = await this.getDefaultWorkspacePath();
    return this.remote!.call('getTemplate', [name, wsPath]) as Promise<string>;
  };
  
  saveTemplate = async (name: string, content: string): Promise<void> => {
    const wsPath = await this.getDefaultWorkspacePath();
    return this.remote!.call('saveTemplate', [name, content, wsPath]) as Promise<void>;
  };
  
  deleteTemplate = async (name: string): Promise<void> => {
    const wsPath = await this.getDefaultWorkspacePath();
    return this.remote!.call('deleteTemplate', [name, wsPath]) as Promise<void>;
  };

  validateFile = (filePath: string): Promise<any> =>
    this.remote!.call('validateFile', [filePath]) as Promise<any>;
  
  fixBrokenPartOf = (filePath: string): Promise<any> =>
    this.remote!.call('fixBrokenPartOf', [filePath]) as Promise<any>;
  
  fixBrokenContentsRef = (indexPath: string, target: string): Promise<any> =>
    this.remote!.call('fixBrokenContentsRef', [indexPath, target]) as Promise<any>;
  
  fixBrokenAttachment = (filePath: string, attachment: string): Promise<any> =>
    this.remote!.call('fixBrokenAttachment', [filePath, attachment]) as Promise<any>;
  
  fixNonPortablePath = (filePath: string, property: string, oldValue: string, newValue: string): Promise<any> =>
    this.remote!.call('fixNonPortablePath', [filePath, property, oldValue, newValue]) as Promise<any>;
  
  fixUnlistedFile = (indexPath: string, filePath: string): Promise<any> =>
    this.remote!.call('fixUnlistedFile', [indexPath, filePath]) as Promise<any>;
  
  fixOrphanBinaryFile = (indexPath: string, filePath: string): Promise<any> =>
    this.remote!.call('fixOrphanBinaryFile', [indexPath, filePath]) as Promise<any>;
  
  fixMissingPartOf = (filePath: string, indexPath: string): Promise<any> =>
    this.remote!.call('fixMissingPartOf', [filePath, indexPath]) as Promise<any>;
  
  fixAll = (validationResult: any): Promise<any> =>
    this.remote!.call('fixAll', [validationResult]) as Promise<any>;

  importFromZip = async (
    file: File,
    workspacePath?: string,
    onProgress?: (bytesUploaded: number, totalBytes: number) => void,
  ): Promise<any> => {
    // Use JSZip to extract the zip on the main thread
    const JSZip = (await import('jszip')).default;
    const arrayBuffer = await file.arrayBuffer();
    const zip = await JSZip.loadAsync(arrayBuffer);

    // Get workspace path
    const workspace = workspacePath || await this.remote!.getDefaultWorkspacePath();

    const fileNames = Object.keys(zip.files);
    const totalFiles = fileNames.length;
    let filesImported = 0;
    let filesSkipped = 0;

    for (let i = 0; i < fileNames.length; i++) {
      const fileName = fileNames[i];
      const zipEntry = zip.files[fileName];

      // Skip directories
      if (zipEntry.dir) {
        continue;
      }

      // Skip hidden files and system files
      const shouldSkip = fileName
        .split('/')
        .some(part => part.startsWith('.') || part === 'Thumbs.db' || part === 'desktop.ini');

      if (shouldSkip) {
        filesSkipped++;
        continue;
      }

      // Determine if it's a text file (markdown) or binary
      const isMarkdown = fileName.endsWith('.md');
      const isBinary = !isMarkdown;

      // Only import markdown and common binary attachments
      const isCommonAttachment = /\.(png|jpg|jpeg|gif|svg|pdf|webp|heic|heif|mp3|mp4|wav|mov|docx?|xlsx?|pptx?)$/i.test(fileName);

      if (!isMarkdown && !isCommonAttachment) {
        filesSkipped++;
        continue;
      }

      const filePath = `${workspace}/${fileName}`;

      try {
        if (isBinary) {
          const data = await zipEntry.async('uint8array');
          await this.remote!.writeBinary(filePath, data);
        } else {
          const content = await zipEntry.async('string');
          await this.remote!.writeFile(filePath, content);
        }
        filesImported++;
      } catch (e) {
        console.warn(`[Import] Failed to write ${filePath}:`, e);
        filesSkipped++;
      }

      // Report progress
      if (onProgress) {
        onProgress(i + 1, totalFiles);
      }
    }

    return {
      success: true,
      files_imported: filesImported,
      files_skipped: filesSkipped,
    };
  };
}

