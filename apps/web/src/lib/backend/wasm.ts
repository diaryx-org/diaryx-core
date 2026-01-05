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
// Zip import helpers (browser-only)
// ============================================================================

function isProbablyTextFile(path: string): boolean {
  const lower = path.toLowerCase();
  if (
    lower.endsWith(".md") ||
    lower.endsWith(".markdown") ||
    lower.endsWith(".txt") ||
    lower.endsWith(".toml") ||
    lower.endsWith(".json") ||
    lower.endsWith(".yaml") ||
    lower.endsWith(".yml") ||
    lower.endsWith(".csv") ||
    lower.endsWith(".ts") ||
    lower.endsWith(".js") ||
    lower.endsWith(".css") ||
    lower.endsWith(".html")
  )
    return true;

  if (
    lower.includes("/_attachments/") ||
    lower.includes("\\_attachments\\") ||
    lower.includes("/attachments/") ||
    lower.includes("\\attachments\\")
  )
    return false;

  return false;
}

function normalizeZipPath(path: string): string {
  // Zip entries always use forward slashes, but normalize just in case.
  return path.replace(/^\.\/+/, "").replace(/\\/g, "/");
}

async function importZipEntriesViaJszip(
  file: File,
  onProgress?: (bytesUploaded: number, totalBytes: number) => void,
): Promise<{
  textEntries: [string, string][];
  binaryEntries: { path: string; data: number[] }[];
}> {
  // Dynamic import so this only loads in the browser when needed.
  const { default: JSZip } = await import("jszip");

  const totalBytes = file.size;
  onProgress?.(0, totalBytes);

  const zip = await JSZip.loadAsync(file);

  const textEntries: [string, string][] = [];
  const binaryEntries: { path: string; data: number[] }[] = [];

  // Iterate deterministically for stable behavior
  const names = Object.keys(zip.files).sort((a, b) => a.localeCompare(b));

  let processedBytesEstimate = 0;

  for (const name of names) {
    const entry = zip.files[name];
    if (!entry || entry.dir) continue;

    const normalized = normalizeZipPath(name);
    if (!normalized) continue;

    // Best-effort progress: JSZip doesn't expose compressed sizes uniformly.
    // We increment by 1 per file and also by uncompressed byte length once read.
    // This keeps the UI moving without pretending to be exact.
    processedBytesEstimate += 1;
    if (onProgress && processedBytesEstimate % 50 === 0) {
      onProgress(Math.min(processedBytesEstimate, totalBytes), totalBytes);
    }

    if (isProbablyTextFile(normalized)) {
      const content = await entry.async("string");
      textEntries.push([normalized, content]);
      processedBytesEstimate += content.length;
    } else {
      const data = await entry.async("uint8array");
      binaryEntries.push({ path: normalized, data: Array.from(data) });
      processedBytesEstimate += data.byteLength;
    }

    if (onProgress && processedBytesEstimate % (2 * 1024 * 1024) < 64 * 1024) {
      onProgress(Math.min(processedBytesEstimate, totalBytes), totalBytes);
    }
  }

  onProgress?.(totalBytes, totalBytes);

  return { textEntries, binaryEntries };
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

  async saveBinaryFiles(
    entries: { path: string; data: number[] }[],
  ): Promise<void> {
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
      transaction.onerror = () =>
        reject(new Error("Failed to save binary files"));
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

  // Typed WASM class instances
  private _workspace: InstanceType<WasmModule["DiaryxWorkspace"]> | null = null;
  private _entry: InstanceType<WasmModule["DiaryxEntry"]> | null = null;
  private _frontmatter: InstanceType<WasmModule["DiaryxFrontmatter"]> | null =
    null;
  private _search: InstanceType<WasmModule["DiaryxSearch"]> | null = null;
  private _template: InstanceType<WasmModule["DiaryxTemplate"]> | null = null;
  private _validation: InstanceType<WasmModule["DiaryxValidation"]> | null =
    null;
  private _export: InstanceType<WasmModule["DiaryxExport"]> | null = null;
  private _attachment: InstanceType<WasmModule["DiaryxAttachment"]> | null =
    null;
  private _filesystem: InstanceType<WasmModule["DiaryxFilesystem"]> | null =
    null;

  async init(): Promise<void> {
    if (this.ready) return;

    console.log("[WasmBackend] Initializing...");

    // Load WASM module
    this.wasm = await loadWasm();

    // Initialize typed class instances
    this._workspace = new this.wasm.DiaryxWorkspace();
    this._entry = new this.wasm.DiaryxEntry();
    this._frontmatter = new this.wasm.DiaryxFrontmatter();
    this._search = new this.wasm.DiaryxSearch();
    this._template = new this.wasm.DiaryxTemplate();
    this._validation = new this.wasm.DiaryxValidation();
    this._export = new this.wasm.DiaryxExport();
    this._attachment = new this.wasm.DiaryxAttachment();
    this._filesystem = new this.wasm.DiaryxFilesystem();

    // Open IndexedDB
    await this.storage.open();

    // Load text files from IndexedDB into WASM's in-memory filesystem
    const files = await this.storage.loadAllFiles();
    const entries: [string, string][] = files.map((f) => [f.path, f.content]);
    this._filesystem.load_files(entries);
    console.log(
      `[WasmBackend] Loaded ${files.length} text files from IndexedDB`,
    );

    // Load binary files (attachments) from IndexedDB
    try {
      const binaryFiles = await this.storage.loadBinaryFiles();
      if (binaryFiles.length > 0) {
        const binaryEntries = binaryFiles.map((f) => ({
          path: f.path,
          data: Array.from(f.data),
        }));
        this.wasm.load_binary_files(binaryEntries);
        console.log(
          `[WasmBackend] Loaded ${binaryFiles.length} binary files from IndexedDB`,
        );
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
      this._workspace.create("workspace", "My Workspace");
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

  private get workspace() {
    if (!this._workspace)
      throw new BackendError("Not initialized", "NotInitialized");
    return this._workspace;
  }

  private get entry() {
    if (!this._entry)
      throw new BackendError("Not initialized", "NotInitialized");
    return this._entry;
  }

  private get frontmatter() {
    if (!this._frontmatter)
      throw new BackendError("Not initialized", "NotInitialized");
    return this._frontmatter;
  }

  private get search() {
    if (!this._search)
      throw new BackendError("Not initialized", "NotInitialized");
    return this._search;
  }

  private get template() {
    if (!this._template)
      throw new BackendError("Not initialized", "NotInitialized");
    return this._template;
  }

  private get validation() {
    if (!this._validation)
      throw new BackendError("Not initialized", "NotInitialized");
    return this._validation;
  }

  private get exportApi() {
    if (!this._export)
      throw new BackendError("Not initialized", "NotInitialized");
    return this._export;
  }

  private get attachment() {
    if (!this._attachment)
      throw new BackendError("Not initialized", "NotInitialized");
    return this._attachment;
  }

  private get filesystem() {
    if (!this._filesystem)
      throw new BackendError("Not initialized", "NotInitialized");
    return this._filesystem;
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
    const path = workspacePath ?? this.config?.default_workspace ?? "workspace";
    return this.workspace.get_tree(path, depth ?? null);
  }

  async createWorkspace(path?: string, name?: string): Promise<string> {
    const workspacePath = path ?? "workspace";
    const workspaceName = name ?? "My Workspace";
    this.workspace.create(workspacePath, workspaceName);
    return workspacePath;
  }

  async getFilesystemTree(
    workspacePath?: string,
    showHidden?: boolean,
  ): Promise<TreeNode> {
    const path = workspacePath ?? this.config?.default_workspace ?? "workspace";
    return this.workspace.get_filesystem_tree(path, showHidden ?? false);
  }

  // --------------------------------------------------------------------------
  // Entries
  // --------------------------------------------------------------------------

  async getEntry(path: string): Promise<EntryData> {
    return this.entry.get(path);
  }

  async saveEntry(path: string, content: string): Promise<void> {
    this.entry.save(path, content);
  }

  async createEntry(
    path: string,
    options?: CreateEntryOptions,
  ): Promise<string> {
    const workspaceRoot = this.config?.default_workspace ?? "workspace";
    const normalizedPath = normalizeEntryPathToWorkspaceRoot(
      path,
      workspaceRoot,
    );
    return this.entry.create(normalizedPath, options ?? null);
  }

  async deleteEntry(path: string): Promise<void> {
    this.entry.delete(path);
  }

  async moveEntry(fromPath: string, toPath: string): Promise<string> {
    return this.entry.move_entry(fromPath, toPath);
  }

  async attachEntryToParent(
    entryPath: string,
    parentIndexPath: string,
  ): Promise<string> {
    const workspaceRoot = this.config?.default_workspace ?? "workspace";
    const normalizedEntryPath = normalizeEntryPathToWorkspaceRoot(
      entryPath,
      workspaceRoot,
    );
    const normalizedParentIndexPath = normalizeIndexPathToWorkspaceRoot(
      parentIndexPath,
      workspaceRoot,
    );
    return this.entry.attach_to_parent(
      normalizedEntryPath,
      normalizedParentIndexPath,
    );
  }

  async convertToIndex(path: string): Promise<string> {
    return this.entry.convert_to_index(path);
  }

  async convertToLeaf(path: string): Promise<string> {
    return this.entry.convert_to_leaf(path);
  }

  async createChildEntry(parentPath: string): Promise<string> {
    return this.entry.create_child(parentPath);
  }

  slugifyTitle(title: string): string {
    const wasm = this.requireWasm();
    return wasm.slugify_title(title);
  }

  async renameEntry(path: string, newFilename: string): Promise<string> {
    return this.entry.rename(path, newFilename);
  }

  async ensureDailyEntry(): Promise<string> {
    return this.entry.ensure_daily();
  }

  // --------------------------------------------------------------------------
  // Export
  // --------------------------------------------------------------------------

  async getAvailableAudiences(rootPath: string): Promise<string[]> {
    return this.exportApi.get_audiences(rootPath);
  }

  async planExport(
    rootPath: string,
    audience: string,
  ): Promise<import("./interface").ExportPlan> {
    return this.exportApi.plan(rootPath, audience);
  }

  async exportToMemory(
    rootPath: string,
    audience: string,
  ): Promise<import("./interface").ExportedFile[]> {
    return this.exportApi.to_memory(rootPath, audience);
  }

  async exportToHtml(
    rootPath: string,
    audience: string,
  ): Promise<import("./interface").ExportedFile[]> {
    return this.exportApi.to_html(rootPath, audience);
  }

  async exportBinaryAttachments(
    rootPath: string,
    audience: string,
  ): Promise<import("./interface").BinaryExportFile[]> {
    return this.exportApi.binary_attachments(rootPath, audience);
  }

  // --------------------------------------------------------------------------
  // Attachments
  // --------------------------------------------------------------------------

  async getAttachments(entryPath: string): Promise<string[]> {
    return this.attachment.list(entryPath);
  }

  async uploadAttachment(
    entryPath: string,
    filename: string,
    dataBase64: string,
  ): Promise<string> {
    return this.attachment.upload(entryPath, filename, dataBase64);
  }

  async deleteAttachment(
    entryPath: string,
    attachmentPath: string,
  ): Promise<void> {
    return this.attachment.delete(entryPath, attachmentPath);
  }

  async getStorageUsage(): Promise<import("./interface").StorageInfo> {
    return this.attachment.get_storage_usage();
  }

  async getAttachmentData(
    entryPath: string,
    attachmentPath: string,
  ): Promise<Uint8Array> {
    return this.attachment.read_data(entryPath, attachmentPath);
  }

  // --------------------------------------------------------------------------
  // Frontmatter
  // --------------------------------------------------------------------------

  async getFrontmatter(path: string): Promise<Record<string, unknown>> {
    return this.frontmatter.get_all(path);
  }

  async setFrontmatterProperty(
    path: string,
    key: string,
    value: unknown,
  ): Promise<void> {
    this.frontmatter.set_property(path, key, value);
  }

  async removeFrontmatterProperty(path: string, key: string): Promise<void> {
    this.frontmatter.remove_property(path, key);
  }

  // --------------------------------------------------------------------------
  // Search
  // --------------------------------------------------------------------------

  async searchWorkspace(
    pattern: string,
    options?: SearchOptions,
  ): Promise<SearchResults> {
    const wasmOptions = options
      ? {
          workspace_path:
            options.workspacePath ?? this.config?.default_workspace,
          search_frontmatter: options.searchFrontmatter,
          property: options.property,
          case_sensitive: options.caseSensitive,
        }
      : { workspace_path: this.config?.default_workspace };

    return this.search.search(pattern, wasmOptions);
  }

  // --------------------------------------------------------------------------
  // Templates
  // --------------------------------------------------------------------------

  async listTemplates(): Promise<TemplateInfo[]> {
    return this.template.list(this.config?.default_workspace ?? null);
  }

  async getTemplate(name: string): Promise<string> {
    return this.template.get(name, this.config?.default_workspace ?? null);
  }

  async saveTemplate(name: string, content: string): Promise<void> {
    this.template.save(
      name,
      content,
      this.config?.default_workspace ?? "workspace",
    );
  }

  async deleteTemplate(name: string): Promise<void> {
    this.template.delete(name, this.config?.default_workspace ?? "workspace");
  }

  // --------------------------------------------------------------------------
  // Validation
  // --------------------------------------------------------------------------

  async validateWorkspace(
    workspacePath?: string,
  ): Promise<import("./interface").ValidationResult> {
    const path = workspacePath ?? this.config?.default_workspace ?? "workspace";
    return this.validation.validate(path);
  }

  async validateFile(
    filePath: string,
  ): Promise<import("./interface").ValidationResult> {
    return this.validation.validate_file(filePath);
  }

  async fixBrokenPartOf(
    filePath: string,
  ): Promise<import("./interface").FixResult> {
    return this.validation.fix_broken_part_of(filePath);
  }

  async fixBrokenContentsRef(
    indexPath: string,
    target: string,
  ): Promise<import("./interface").FixResult> {
    return this.validation.fix_broken_contents_ref(indexPath, target);
  }

  async fixBrokenAttachment(
    filePath: string,
    attachment: string,
  ): Promise<import("./interface").FixResult> {
    return this.validation.fix_broken_attachment(filePath, attachment);
  }

  async fixNonPortablePath(
    filePath: string,
    property: string,
    oldValue: string,
    newValue: string,
  ): Promise<import("./interface").FixResult> {
    return this.validation.fix_non_portable_path(
      filePath,
      property,
      oldValue,
      newValue,
    );
  }

  async fixUnlistedFile(
    indexPath: string,
    filePath: string,
  ): Promise<import("./interface").FixResult> {
    return this.validation.fix_unlisted_file(indexPath, filePath);
  }

  async fixOrphanBinaryFile(
    indexPath: string,
    filePath: string,
  ): Promise<import("./interface").FixResult> {
    return this.validation.fix_orphan_binary_file(indexPath, filePath);
  }

  async fixMissingPartOf(
    filePath: string,
    indexPath: string,
  ): Promise<import("./interface").FixResult> {
    return this.validation.fix_missing_part_of(filePath, indexPath);
  }

  async fixAll(
    validationResult: import("./interface").ValidationResult,
  ): Promise<import("./interface").FixSummary> {
    return this.validation.fix_all(validationResult);
  }

  // --------------------------------------------------------------------------
  // Import
  // --------------------------------------------------------------------------

  async importFromZip(
    file: File,
    workspacePath?: string,
    onProgress?: (bytesUploaded: number, totalBytes: number) => void,
  ): Promise<import("./interface").ImportResult> {
    const totalBytes = file.size;
    const targetRoot = (
      workspacePath ||
      this.config?.default_workspace ||
      "workspace"
    ).replace(/\/+$/, "");

    try {
      console.log(
        `[WasmBackend] Importing zip ${(totalBytes / 1024 / 1024).toFixed(2)} MB into ${targetRoot}`,
      );

      const { textEntries, binaryEntries } = await importZipEntriesViaJszip(
        file,
        onProgress,
      );

      // Prefix all extracted paths into the target workspace root.
      // Note: this assumes the zip contains a workspace folder structure (README.md etc).
      // If the zip already includes a top-level "workspace/" folder, this will nest it;
      // that’s acceptable for now and can be improved later with a smarter root-stripper.
      const prefixedText: [string, string][] = textEntries.map(([p, c]) => [
        `${targetRoot}/${p}`,
        c,
      ]);
      const prefixedBinary: { path: string; data: number[] }[] =
        binaryEntries.map((e) => ({
          path: `${targetRoot}/${e.path}`,
          data: e.data,
        }));

      // Load into the WASM in-memory filesystem
      if (prefixedText.length > 0) {
        this.filesystem.load_files(prefixedText);
      }
      if (prefixedBinary.length > 0) {
        this.wasm!.load_binary_files(prefixedBinary);
      }

      // Persist to IndexedDB so it survives refresh
      await this.persist();

      console.log(
        `[WasmBackend] Import complete: ${prefixedText.length} text files, ${prefixedBinary.length} binaries`,
      );

      return {
        success: true,
        files_imported: prefixedText.length + prefixedBinary.length,
        error: undefined,
      };
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      console.error("[WasmBackend] Import failed:", message);
      return {
        success: false,
        files_imported: 0,
        error: message,
      };
    }
  }

  // --------------------------------------------------------------------------
  // Persistence
  // --------------------------------------------------------------------------

  async persist(): Promise<void> {
    // Export all text files from WASM's in-memory filesystem
    const entries: [string, string][] = this.filesystem.export_files();

    if (entries.length > 0) {
      console.log(`[WasmBackend] Persisting ${entries.length} text files...`);
      await this.storage.saveAllFiles(entries);
    }

    // Export all binary files (attachments)
    try {
      const binaryEntries: { path: string; data: number[] }[] =
        this.filesystem.export_binary_files();
      if (binaryEntries.length > 0) {
        console.log(
          `[WasmBackend] Persisting ${binaryEntries.length} binary files...`,
        );
        await this.storage.saveBinaryFiles(binaryEntries);
      }
    } catch (e) {
      console.warn("[WasmBackend] Could not persist binary files:", e);
    }
  }
}
