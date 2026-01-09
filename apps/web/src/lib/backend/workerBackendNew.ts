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
    await this.remote.init(Comlink.transfer(port2, [port2]), storageType);
    
    this._ready = true;
    console.log('[WorkerBackendNew] Ready');
  }

  isReady(): boolean {
    return this._ready;
  }

  // Event subscription
  on(event: BackendEventType, listener: BackendEventListener): void {
    this.eventEmitter.on(event, listener);
  }

  off(event: BackendEventType, listener: BackendEventListener): void {
    this.eventEmitter.off(event, listener);
  }

  // =========================================================================
  // Proxy methods - delegate to worker
  // =========================================================================

  getConfig = () => this.remote!.getConfig();
  saveConfig = (config: any) => this.remote!.saveConfig(config);

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
    return title
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '');
  }

  attachEntryToParent = (entry: string, parent: string): Promise<string> =>
    this.remote!.call('attachEntryToParent', [entry, parent]) as Promise<string>;
  
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
    this.remote!.call('getAttachments', [entryPath]) as Promise<string[]>;
  
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

  listTemplates = (): Promise<any> =>
    this.remote!.call('listTemplates', []) as Promise<any>;
  
  getTemplate = (name: string): Promise<string> =>
    this.remote!.call('getTemplate', [name]) as Promise<string>;
  
  saveTemplate = (name: string, content: string): Promise<void> =>
    this.remote!.call('saveTemplate', [name, content]) as Promise<void>;
  
  deleteTemplate = (name: string): Promise<void> =>
    this.remote!.call('deleteTemplate', [name]) as Promise<void>;

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
    _onProgress?: (bytesUploaded: number, totalBytes: number) => void,
  ): Promise<any> => {
    // Read file to Uint8Array and pass to worker
    const arrayBuffer = await file.arrayBuffer();
    const data = new Uint8Array(arrayBuffer);
    return this.remote!.call('importFromZip', [Array.from(data), workspacePath]) as Promise<any>;
  };
}

