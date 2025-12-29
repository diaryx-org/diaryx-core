// WASM backend implementation - thin wrapper around the WASM module with IndexedDB persistence

import type {
  Backend,
  Config,
  TreeNode,
  EntryData,
  SearchResults,
  SearchOptions,
  CreateEntryOptions,
  TemplateInfo,
} from "./interface";
import { BackendError } from "./interface";

function normalizeEntryPathToWorkspaceRoot(
  inputPath: string,
  workspaceRoot: string,
): string {
  const raw = inputPath.trim();

  // If the user only typed a filename (no folder), treat it as relative to the
  // configured workspace root folder so it appears in the workspace tree.
  if (!raw.includes("/")) {
    const root = (workspaceRoot || "workspace").replace(/\/+$/, "");
    return `${root}/${raw}`;
  }

  return raw;
}

function normalizeIndexPathToWorkspaceRoot(
  inputPath: string,
  workspaceRoot: string,
): string {
  // For consistency with createEntry: if the caller passes "index.md" we interpret
  // it as the workspace root index file.
  return normalizeEntryPathToWorkspaceRoot(inputPath, workspaceRoot);
}

// ============================================================================
// IndexedDB Storage (for persisting the in-memory filesystem)
// ============================================================================

const DB_NAME = "diaryx";
const DB_VERSION = 2; // Upgraded for binary file support
const STORE_FILES = "files";
const STORE_BINARY_FILES = "binary_files";
const STORE_CONFIG = "config";

interface FileEntry {
  path: string;
  content: string;
  updatedAt: number;
}

interface BinaryFileEntry {
  path: string;
  data: Uint8Array;
  updatedAt: number;
}

class IndexedDBStorage {
  private db: IDBDatabase | null = null;

  async open(): Promise<void> {
    return new Promise((resolve, reject) => {
      const request = indexedDB.open(DB_NAME, DB_VERSION);

      request.onerror = () => reject(new Error("Failed to open IndexedDB"));

      request.onsuccess = () => {
        this.db = request.result;
        resolve();
      };

      request.onupgradeneeded = (event) => {
        const db = (event.target as IDBOpenDBRequest).result;

        if (!db.objectStoreNames.contains(STORE_FILES)) {
          db.createObjectStore(STORE_FILES, { keyPath: "path" });
        }

        if (!db.objectStoreNames.contains(STORE_BINARY_FILES)) {
          db.createObjectStore(STORE_BINARY_FILES, { keyPath: "path" });
        }

        if (!db.objectStoreNames.contains(STORE_CONFIG)) {
          db.createObjectStore(STORE_CONFIG, { keyPath: "id" });
        }
      };
    });
  }

  async loadAllFiles(): Promise<FileEntry[]> {
    if (!this.db) throw new Error("Database not initialized");

    return new Promise((resolve, reject) => {
      const transaction = this.db!.transaction(STORE_FILES, "readonly");
      const store = transaction.objectStore(STORE_FILES);
      const request = store.getAll();

      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(new Error("Failed to load files"));
    });
  }

  async saveAllFiles(entries: [string, string][]): Promise<void> {
    if (!this.db) throw new Error("Database not initialized");

    return new Promise((resolve, reject) => {
      const transaction = this.db!.transaction(STORE_FILES, "readwrite");
      const store = transaction.objectStore(STORE_FILES);

      // Clear existing files and save new ones
      store.clear();
      const now = Date.now();
      for (const [path, content] of entries) {
        store.put({ path, content, updatedAt: now });
      }

      transaction.oncomplete = () => resolve();
      transaction.onerror = () => reject(new Error("Failed to save files"));
    });
  }

  async loadConfig(): Promise<Config | null> {
    if (!this.db) throw new Error("Database not initialized");

    return new Promise((resolve, reject) => {
      const transaction = this.db!.transaction(STORE_CONFIG, "readonly");
      const store = transaction.objectStore(STORE_CONFIG);
      const request = store.get("config");

      request.onsuccess = () => {
        const result = request.result;
        resolve(result ? result.data : null);
      };
      request.onerror = () => reject(new Error("Failed to load config"));
    });
  }

  async saveConfig(config: Config): Promise<void> {
    if (!this.db) throw new Error("Database not initialized");

    return new Promise((resolve, reject) => {
      const transaction = this.db!.transaction(STORE_CONFIG, "readwrite");
      const store = transaction.objectStore(STORE_CONFIG);
      const request = store.put({ id: "config", data: config });

      request.onsuccess = () => resolve();
      request.onerror = () => reject(new Error("Failed to save config"));
    });
  }

  async loadBinaryFiles(): Promise<BinaryFileEntry[]> {
    if (!this.db) throw new Error("Database not initialized");

    return new Promise((resolve, reject) => {
      const transaction = this.db!.transaction(STORE_BINARY_FILES, "readonly");
      const store = transaction.objectStore(STORE_BINARY_FILES);
      const request = store.getAll();

      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(new Error("Failed to load binary files"));
    });
  }

  async saveBinaryFiles(entries: { path: string; data: number[] }[]): Promise<void> {
    if (!this.db) throw new Error("Database not initialized");

    return new Promise((resolve, reject) => {
      const transaction = this.db!.transaction(STORE_BINARY_FILES, "readwrite");
      const store = transaction.objectStore(STORE_BINARY_FILES);

      // Clear existing and save new ones 
      store.clear();
      const now = Date.now();
      for (const { path, data } of entries) {
        store.put({ path, data: new Uint8Array(data), updatedAt: now });
      }

      transaction.oncomplete = () => resolve();
      transaction.onerror = () => reject(new Error("Failed to save binary files"));
    });
  }
}

// ============================================================================
// WASM Module Loading
// ============================================================================

type WasmModule = typeof import("../wasm/diaryx_wasm.js");

let wasm: WasmModule | null = null;

async function loadWasm(): Promise<WasmModule> {
  if (wasm) return wasm;

  console.log("[WasmBackend] Loading WASM module...");
  const module = await import("../wasm/diaryx_wasm.js");
  await module.default();
  wasm = module;
  console.log("[WasmBackend] WASM module loaded");

  return wasm;
}

// ============================================================================
// WASM Backend Implementation
// ============================================================================

export class WasmBackend implements Backend {
  private storage = new IndexedDBStorage();
  private config: Config | null = null;
  private wasm: WasmModule | null = null;
  private ready = false;

  async init(): Promise<void> {
    if (this.ready) return;

    console.log("[WasmBackend] Initializing...");

    // Load WASM module
    this.wasm = await loadWasm();

    // Open IndexedDB
    await this.storage.open();

    // Load text files from IndexedDB into WASM's in-memory filesystem
    const files = await this.storage.loadAllFiles();
    const entries: [string, string][] = files.map((f) => [f.path, f.content]);
    this.wasm.load_files(entries);
    console.log(`[WasmBackend] Loaded ${files.length} text files from IndexedDB`);

    // Load binary files (attachments) from IndexedDB
    try {
      const binaryFiles = await this.storage.loadBinaryFiles();
      if (binaryFiles.length > 0) {
        const binaryEntries = binaryFiles.map((f) => ({
          path: f.path,
          data: Array.from(f.data),
        }));
        this.wasm.load_binary_files(binaryEntries);
        console.log(`[WasmBackend] Loaded ${binaryFiles.length} binary files from IndexedDB`);
      }
    } catch (e) {
      // Binary store might not exist in older databases
      console.warn("[WasmBackend] Could not load binary files:", e);
    }

    // Load config
    this.config = await this.storage.loadConfig();

    // Create default config and workspace if none exists
    if (!this.config) {
      this.config = { default_workspace: "workspace" };
      await this.storage.saveConfig(this.config);

      // Create default workspace
      this.wasm.create_workspace("workspace", "My Workspace");
      await this.persist();
    }

    this.ready = true;
    console.log("[WasmBackend] Initialization complete");
  }

  isReady(): boolean {
    return this.ready;
  }

  private requireWasm(): WasmModule {
    if (!this.wasm) {
      throw new BackendError(
        "WASM module not loaded. Call init() first.",
        "NotInitialized",
      );
    }
    return this.wasm;
  }

  // --------------------------------------------------------------------------
  // Configuration
  // --------------------------------------------------------------------------

  async getConfig(): Promise<Config> {
    if (!this.config) {
      throw new BackendError("Backend not initialized", "NotInitialized");
    }
    return this.config;
  }

  async saveConfig(config: Config): Promise<void> {
    this.config = config;
    await this.storage.saveConfig(config);
  }

  // --------------------------------------------------------------------------
  // Workspace
  // --------------------------------------------------------------------------

  async getWorkspaceTree(
    workspacePath?: string,
    depth?: number,
  ): Promise<TreeNode> {
    const wasm = this.requireWasm();
    const path = workspacePath ?? this.config?.default_workspace ?? "workspace";
    return wasm.get_workspace_tree(path, depth ?? null);
  }

  async createWorkspace(path?: string, name?: string): Promise<string> {
    const wasm = this.requireWasm();
    const workspacePath = path ?? "workspace";
    const workspaceName = name ?? "My Workspace";
    wasm.create_workspace(workspacePath, workspaceName);
    return workspacePath;
  }

  async getFilesystemTree(
    workspacePath?: string,
    showHidden?: boolean,
  ): Promise<TreeNode> {
    const wasm = this.requireWasm();
    const path = workspacePath ?? this.config?.default_workspace ?? "workspace";
    return wasm.get_filesystem_tree(path, showHidden ?? false);
  }

  // --------------------------------------------------------------------------
  // Entries
  // --------------------------------------------------------------------------

  async getEntry(path: string): Promise<EntryData> {
    const wasm = this.requireWasm();
    return wasm.get_entry(path);
  }

  async saveEntry(path: string, content: string): Promise<void> {
    const wasm = this.requireWasm();
    wasm.save_entry(path, content);
  }

  async createEntry(
    path: string,
    options?: CreateEntryOptions,
  ): Promise<string> {
    const wasm = this.requireWasm();

    const workspaceRoot = this.config?.default_workspace ?? "workspace";
    const normalizedPath = normalizeEntryPathToWorkspaceRoot(
      path,
      workspaceRoot,
    );

    return wasm.create_entry(normalizedPath, options ?? null);
  }

  async deleteEntry(path: string): Promise<void> {
    const wasm = this.requireWasm();
    wasm.delete_entry(path);
  }

  async moveEntry(fromPath: string, toPath: string): Promise<string> {
    const wasm = this.requireWasm();
    return wasm.move_entry(fromPath, toPath);
  }

  async attachEntryToParent(
    entryPath: string,
    parentIndexPath: string,
  ): Promise<string> {
    const wasm = this.requireWasm();
    const workspaceRoot = this.config?.default_workspace ?? "workspace";

    const normalizedEntryPath = normalizeEntryPathToWorkspaceRoot(
      entryPath,
      workspaceRoot,
    );
    const normalizedParentIndexPath = normalizeIndexPathToWorkspaceRoot(
      parentIndexPath,
      workspaceRoot,
    );

    return wasm.attach_entry_to_parent(normalizedEntryPath, normalizedParentIndexPath) as unknown as string;
  }

  async convertToIndex(path: string): Promise<string> {
    const wasm = this.requireWasm();
    return wasm.convert_to_index(path);
  }

  async convertToLeaf(path: string): Promise<string> {
    const wasm = this.requireWasm();
    return wasm.convert_to_leaf(path);
  }

  async createChildEntry(parentPath: string): Promise<string> {
    const wasm = this.requireWasm();
    return wasm.create_child_entry(parentPath);
  }

  slugifyTitle(title: string): string {
    const wasm = this.requireWasm();
    return wasm.slugify_title(title);
  }

  async renameEntry(path: string, newFilename: string): Promise<string> {
    const wasm = this.requireWasm();
    return wasm.rename_entry(path, newFilename);
  }

  async ensureDailyEntry(): Promise<string> {
    const wasm = this.requireWasm();
    return wasm.ensure_daily_entry();
  }

  // --------------------------------------------------------------------------
  // Export
  // --------------------------------------------------------------------------

  async getAvailableAudiences(rootPath: string): Promise<string[]> {
    const wasm = this.requireWasm();
    return wasm.get_available_audiences(rootPath);
  }

  async planExport(rootPath: string, audience: string): Promise<import("./interface").ExportPlan> {
    const wasm = this.requireWasm();
    return wasm.plan_export(rootPath, audience);
  }

  async exportToMemory(rootPath: string, audience: string): Promise<import("./interface").ExportedFile[]> {
    const wasm = this.requireWasm();
    return wasm.export_to_memory(rootPath, audience);
  }

  async exportToHtml(rootPath: string, audience: string): Promise<import("./interface").ExportedFile[]> {
    const wasm = this.requireWasm();
    return wasm.export_to_html(rootPath, audience);
  }

  async exportBinaryAttachments(rootPath: string, audience: string): Promise<import("./interface").BinaryExportFile[]> {
    const wasm = this.requireWasm();
    return wasm.export_binary_attachments(rootPath, audience);
  }

  // --------------------------------------------------------------------------
  // Attachments
  // --------------------------------------------------------------------------

  async getAttachments(entryPath: string): Promise<string[]> {
    const wasm = this.requireWasm();
    return wasm.get_attachments(entryPath);
  }

  async uploadAttachment(entryPath: string, filename: string, dataBase64: string): Promise<string> {
    const wasm = this.requireWasm();
    return wasm.upload_attachment(entryPath, filename, dataBase64);
  }

  async deleteAttachment(entryPath: string, attachmentPath: string): Promise<void> {
    const wasm = this.requireWasm();
    return wasm.delete_attachment(entryPath, attachmentPath);
  }

  async getStorageUsage(): Promise<import("./interface").StorageInfo> {
    const wasm = this.requireWasm();
    return wasm.get_storage_usage();
  }

  async getAttachmentData(entryPath: string, attachmentPath: string): Promise<Uint8Array> {
    const wasm = this.requireWasm();
    return wasm.read_attachment_data(entryPath, attachmentPath);
  }

  // --------------------------------------------------------------------------
  // Frontmatter
  // --------------------------------------------------------------------------

  async getFrontmatter(path: string): Promise<Record<string, unknown>> {
    const wasm = this.requireWasm();
    return wasm.get_frontmatter(path);
  }

  async setFrontmatterProperty(
    path: string,
    key: string,
    value: unknown,
  ): Promise<void> {
    const wasm = this.requireWasm();
    wasm.set_frontmatter_property(path, key, value);
  }

  async removeFrontmatterProperty(path: string, key: string): Promise<void> {
    const wasm = this.requireWasm();
    wasm.remove_frontmatter_property(path, key);
  }

  // --------------------------------------------------------------------------
  // Search
  // --------------------------------------------------------------------------

  async searchWorkspace(
    pattern: string,
    options?: SearchOptions,
  ): Promise<SearchResults> {
    const wasm = this.requireWasm();

    const wasmOptions = options
      ? {
          workspace_path:
            options.workspacePath ?? this.config?.default_workspace,
          search_frontmatter: options.searchFrontmatter,
          property: options.property,
          case_sensitive: options.caseSensitive,
        }
      : { workspace_path: this.config?.default_workspace };

    return wasm.search_workspace(pattern, wasmOptions);
  }

  // --------------------------------------------------------------------------
  // Templates
  // --------------------------------------------------------------------------

  async listTemplates(): Promise<TemplateInfo[]> {
    const wasm = this.requireWasm();
    return wasm.list_templates(this.config?.default_workspace ?? null);
  }

  async getTemplate(name: string): Promise<string> {
    const wasm = this.requireWasm();
    return wasm.get_template(name, this.config?.default_workspace ?? null);
  }

  async saveTemplate(name: string, content: string): Promise<void> {
    const wasm = this.requireWasm();
    wasm.save_template(
      name,
      content,
      this.config?.default_workspace ?? "workspace",
    );
  }

  async deleteTemplate(name: string): Promise<void> {
    const wasm = this.requireWasm();
    wasm.delete_template(name, this.config?.default_workspace ?? "workspace");
  }

  // --------------------------------------------------------------------------
  // Validation
  // --------------------------------------------------------------------------

  async validateWorkspace(workspacePath?: string): Promise<import("./interface").ValidationResult> {
    const wasm = this.requireWasm();
    const path = workspacePath ?? this.config?.default_workspace ?? "workspace";
    return wasm.validate_workspace(path);
  }

  // --------------------------------------------------------------------------
  // Persistence
  // --------------------------------------------------------------------------

  async persist(): Promise<void> {
    const wasm = this.requireWasm();

    // Export all text files from WASM's in-memory filesystem
    const entries: [string, string][] = wasm.export_files();

    if (entries.length > 0) {
      console.log(`[WasmBackend] Persisting ${entries.length} text files...`);
      await this.storage.saveAllFiles(entries);
    }

    // Export all binary files (attachments)
    try {
      const binaryEntries: { path: string; data: number[] }[] = wasm.export_binary_files();
      if (binaryEntries.length > 0) {
        console.log(`[WasmBackend] Persisting ${binaryEntries.length} binary files...`);
        await this.storage.saveBinaryFiles(binaryEntries);
      }
    } catch (e) {
      console.warn("[WasmBackend] Could not persist binary files:", e);
    }
  }
}
