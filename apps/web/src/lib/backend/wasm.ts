// WASM backend implementation - uses WebAssembly module with InMemoryFileSystem + IndexedDB

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

// ============================================================================
// IndexedDB Storage
// ============================================================================

const DB_NAME = "diaryx";
const DB_VERSION = 1;
const STORE_FILES = "files";
const STORE_CONFIG = "config";

interface FileEntry {
  path: string;
  content: string;
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

        // Files store - keyed by path
        if (!db.objectStoreNames.contains(STORE_FILES)) {
          db.createObjectStore(STORE_FILES, { keyPath: "path" });
        }

        // Config store - single config object
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

  async saveFile(path: string, content: string): Promise<void> {
    if (!this.db) throw new Error("Database not initialized");

    return new Promise((resolve, reject) => {
      const transaction = this.db!.transaction(STORE_FILES, "readwrite");
      const store = transaction.objectStore(STORE_FILES);
      const entry: FileEntry = { path, content, updatedAt: Date.now() };
      const request = store.put(entry);

      request.onsuccess = () => resolve();
      request.onerror = () => reject(new Error("Failed to save file"));
    });
  }

  async deleteFile(path: string): Promise<void> {
    if (!this.db) throw new Error("Database not initialized");

    return new Promise((resolve, reject) => {
      const transaction = this.db!.transaction(STORE_FILES, "readwrite");
      const store = transaction.objectStore(STORE_FILES);
      const request = store.delete(path);

      request.onsuccess = () => resolve();
      request.onerror = () => reject(new Error("Failed to delete file"));
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

  async saveAllFiles(files: Map<string, string>): Promise<void> {
    if (!this.db) throw new Error("Database not initialized");

    return new Promise((resolve, reject) => {
      const transaction = this.db!.transaction(STORE_FILES, "readwrite");
      const store = transaction.objectStore(STORE_FILES);
      const now = Date.now();

      for (const [path, content] of files) {
        store.put({ path, content, updatedAt: now });
      }

      transaction.oncomplete = () => resolve();
      transaction.onerror = () => reject(new Error("Failed to save files"));
    });
  }
}

// ============================================================================
// In-Memory File System (mirrors the Rust InMemoryFileSystem)
// ============================================================================

class InMemoryFS {
  private files = new Map<string, string>();
  private directories = new Set<string>();
  private dirty = new Set<string>(); // Tracks modified files for persistence

  loadFromEntries(entries: FileEntry[]): void {
    this.files.clear();
    this.directories.clear();
    this.dirty.clear();

    for (const entry of entries) {
      this.files.set(entry.path, entry.content);
      // Extract and add parent directories
      this.addParentDirs(entry.path);
    }
  }

  private addParentDirs(path: string): void {
    const parts = path.split("/");
    let current = "";
    for (let i = 0; i < parts.length - 1; i++) {
      current = current ? `${current}/${parts[i]}` : parts[i];
      this.directories.add(current);
    }
  }

  read(path: string): string | undefined {
    return this.files.get(path);
  }

  write(path: string, content: string): void {
    this.files.set(path, content);
    this.addParentDirs(path);
    this.dirty.add(path);
  }

  delete(path: string): boolean {
    const existed = this.files.delete(path);
    if (existed) {
      this.dirty.add(path); // Mark as deleted
    }
    return existed;
  }

  exists(path: string): boolean {
    return this.files.has(path) || this.directories.has(path);
  }

  isDir(path: string): boolean {
    return this.directories.has(path);
  }

  isFile(path: string): boolean {
    return this.files.has(path);
  }

  list(dirPath: string): string[] {
    const results: string[] = [];
    const prefix = dirPath ? `${dirPath}/` : "";

    for (const path of this.files.keys()) {
      if (path.startsWith(prefix)) {
        const rest = path.slice(prefix.length);
        const firstPart = rest.split("/")[0];
        if (firstPart && !results.includes(firstPart)) {
          results.push(firstPart);
        }
      }
    }

    for (const dir of this.directories) {
      if (dir.startsWith(prefix)) {
        const rest = dir.slice(prefix.length);
        const firstPart = rest.split("/")[0];
        if (firstPart && !results.includes(firstPart)) {
          results.push(firstPart);
        }
      }
    }

    return results.sort();
  }

  createDir(path: string): void {
    this.directories.add(path);
    this.addParentDirs(path);
  }

  getAllFiles(): Map<string, string> {
    return new Map(this.files);
  }

  getDirtyFiles(): Map<string, string> {
    const dirty = new Map<string, string>();
    for (const path of this.dirty) {
      const content = this.files.get(path);
      if (content !== undefined) {
        dirty.set(path, content);
      }
    }
    return dirty;
  }

  clearDirty(): void {
    this.dirty.clear();
  }
}

// ============================================================================
// WASM Module Interface (to be generated by wasm-bindgen)
// ============================================================================

// This interface represents the expected WASM module exports
// The actual implementation will come from diaryx_wasm crate
interface DiaryxWasm {
  // Core operations - these will be implemented in Rust and exposed via wasm-bindgen
  parse_frontmatter(content: string): Record<string, unknown>;
  serialize_frontmatter(frontmatter: Record<string, unknown>): string;
  render_template(template: string, context: Record<string, string>): string;
  search_content(
    content: string,
    pattern: string,
    case_sensitive: boolean,
  ): SearchMatch[];
}

interface SearchMatch {
  line_number: number;
  line_content: string;
  match_start: number;
  match_end: number;
}

// Cached WASM module instance
let wasmModule: DiaryxWasm | null = null;

async function loadWasmModule(): Promise<DiaryxWasm> {
  if (wasmModule) return wasmModule;

  // Try to load the actual WASM module first
  try {
    console.log("[WasmBackend] Attempting to load WASM module...");
    // Dynamic import of the WASM module (built with wasm-pack --target web)
    // The module should be at ../wasm/diaryx_wasm.js after running:
    // wasm-pack build --target web --out-dir ../../apps/web/src/lib/wasm
    const wasm = await import("../wasm/diaryx_wasm.js");
    await wasm.default(); // Initialize WASM
    console.log("[WasmBackend] WASM module loaded successfully");

    wasmModule = {
      parse_frontmatter: (content: string) => wasm.parse_frontmatter(content),
      serialize_frontmatter: (frontmatter: Record<string, unknown>) =>
        wasm.serialize_frontmatter(frontmatter),
      render_template: (template: string, context: Record<string, string>) =>
        wasm.render_template(template, context),
      search_content: (
        content: string,
        pattern: string,
        case_sensitive: boolean,
      ) => wasm.search_content(content, pattern, case_sensitive),
    };
    return wasmModule;
  } catch (e) {
    console.log(
      "[WasmBackend] WASM module not available, using JS fallback:",
      e,
    );
  }

  // Fall back to JavaScript implementation
  console.log("[WasmBackend] Using JavaScript fallback implementation");
  wasmModule = createJsFallback();
  return wasmModule;
}

// JavaScript fallback for WASM functions (used during development)
function createJsFallback(): DiaryxWasm {
  return {
    parse_frontmatter(content: string): Record<string, unknown> {
      const match = content.match(/^---\n([\s\S]*?)\n---/);
      if (!match) return {};

      const yaml = match[1];
      const result: Record<string, unknown> = {};

      // Simple YAML parser for basic key: value pairs
      for (const line of yaml.split("\n")) {
        const colonIdx = line.indexOf(":");
        if (colonIdx > 0) {
          const key = line.slice(0, colonIdx).trim();
          let value = line.slice(colonIdx + 1).trim();

          // Remove quotes if present
          if (
            (value.startsWith('"') && value.endsWith('"')) ||
            (value.startsWith("'") && value.endsWith("'"))
          ) {
            value = value.slice(1, -1);
          }

          result[key] = value;
        }
      }

      return result;
    },

    serialize_frontmatter(frontmatter: Record<string, unknown>): string {
      const lines = ["---"];
      for (const [key, value] of Object.entries(frontmatter)) {
        if (typeof value === "string") {
          // Quote strings that contain special characters
          if (
            value.includes(":") ||
            value.includes("#") ||
            value.includes("\n")
          ) {
            lines.push(`${key}: "${value.replace(/"/g, '\\"')}"`);
          } else {
            lines.push(`${key}: ${value}`);
          }
        } else {
          lines.push(`${key}: ${JSON.stringify(value)}`);
        }
      }
      lines.push("---");
      return lines.join("\n");
    },

    render_template(template: string, context: Record<string, string>): string {
      let result = template;

      // Handle {{var}} and {{var:format}} patterns
      const regex = /\{\{(\w+)(?::([^}]+))?\}\}/g;

      result = result.replace(regex, (match, varName, format) => {
        const value = context[varName];
        if (value === undefined) return match;

        if (format && varName === "date") {
          // Basic date formatting
          const date = new Date(value);
          return formatDate(date, format);
        }

        return value;
      });

      return result;
    },

    search_content(
      content: string,
      pattern: string,
      caseSensitive: boolean,
    ): SearchMatch[] {
      const matches: SearchMatch[] = [];
      const flags = caseSensitive ? "g" : "gi";
      const regex = new RegExp(pattern, flags);
      const lines = content.split("\n");

      for (let i = 0; i < lines.length; i++) {
        const line = lines[i];
        let match;

        while ((match = regex.exec(line)) !== null) {
          matches.push({
            line_number: i + 1,
            line_content: line,
            match_start: match.index,
            match_end: match.index + match[0].length,
          });
        }
      }

      return matches;
    },
  };
}

function formatDate(date: Date, format: string): string {
  const months = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
  ];
  const shortMonths = months.map((m) => m.slice(0, 3));

  return format
    .replace("%Y", date.getFullYear().toString())
    .replace("%m", (date.getMonth() + 1).toString().padStart(2, "0"))
    .replace("%d", date.getDate().toString().padStart(2, "0"))
    .replace("%B", months[date.getMonth()])
    .replace("%b", shortMonths[date.getMonth()])
    .replace("%H", date.getHours().toString().padStart(2, "0"))
    .replace("%M", date.getMinutes().toString().padStart(2, "0"))
    .replace("%S", date.getSeconds().toString().padStart(2, "0"));
}

// ============================================================================
// WasmBackend Implementation
// ============================================================================

export class WasmBackend implements Backend {
  private storage = new IndexedDBStorage();
  private fs = new InMemoryFS();
  private config: Config | null = null;
  private wasm: DiaryxWasm | null = null;
  private ready = false;

  async init(): Promise<void> {
    console.log("[WasmBackend] Starting init...");

    // Open IndexedDB
    console.log("[WasmBackend] Opening IndexedDB...");
    await this.storage.open();
    console.log("[WasmBackend] IndexedDB opened");

    // Load all files into memory
    console.log("[WasmBackend] Loading files from IndexedDB...");
    const files = await this.storage.loadAllFiles();
    console.log(`[WasmBackend] Loaded ${files.length} files`);
    this.fs.loadFromEntries(files);

    // Load config
    console.log("[WasmBackend] Loading config...");
    this.config = await this.storage.loadConfig();
    console.log("[WasmBackend] Config loaded:", this.config);

    // Initialize with default config if none exists
    if (!this.config) {
      console.log("[WasmBackend] No config found, creating default...");
      this.config = {
        default_workspace: "/workspace",
      };
      await this.storage.saveConfig(this.config);

      // Create default workspace structure
      console.log("[WasmBackend] Creating default workspace...");
      this.fs.createDir("/workspace");
      this.fs.write(
        "/workspace/index.md",
        `---
title: My Journal
created: ${new Date().toISOString()}
---

# My Journal

Welcome to Diaryx! Start writing your first entry.
`,
      );
      console.log("[WasmBackend] Default workspace created");
    }

    // Load WASM module (or JS fallback)
    console.log("[WasmBackend] Loading WASM module...");
    this.wasm = await loadWasmModule();
    console.log("[WasmBackend] WASM module loaded");

    this.ready = true;
    console.log("[WasmBackend] Init complete, ready =", this.ready);
  }

  isReady(): boolean {
    return this.ready;
  }

  // --------------------------------------------------------------------------
  // Configuration
  // --------------------------------------------------------------------------

  async getConfig(): Promise<Config> {
    if (!this.config) {
      throw new BackendError("Backend not initialized", "NotInitialized");
    }
    return { ...this.config };
  }

  async saveConfig(config: Config): Promise<void> {
    this.config = { ...config };
    await this.storage.saveConfig(config);
  }

  // --------------------------------------------------------------------------
  // Workspace
  // --------------------------------------------------------------------------

  async getWorkspaceTree(
    workspacePath?: string,
    depth?: number,
  ): Promise<TreeNode> {
    const rootPath =
      workspacePath || this.config?.default_workspace || "/workspace";

    return this.buildTreeNode(rootPath, depth ?? 10, 0);
  }

  private buildTreeNode(
    path: string,
    maxDepth: number,
    currentDepth: number,
  ): TreeNode {
    const name = path.split("/").pop() || path;
    const indexPath = `${path}/index.md`;
    const indexContent = this.fs.read(indexPath);

    let description: string | undefined;
    if (indexContent) {
      const frontmatter = this.wasm?.parse_frontmatter(indexContent) || {};
      description = frontmatter.description as string | undefined;
    }

    const children: TreeNode[] = [];

    if (currentDepth < maxDepth) {
      const entries = this.fs.list(path);

      for (const entry of entries) {
        const entryPath = `${path}/${entry}`;

        if (this.fs.isDir(entryPath)) {
          // Recurse into subdirectories
          children.push(
            this.buildTreeNode(entryPath, maxDepth, currentDepth + 1),
          );
        } else if (entry.endsWith(".md") && entry !== "index.md") {
          // Add markdown files as leaf nodes
          const content = this.fs.read(entryPath);
          const frontmatter = content
            ? this.wasm?.parse_frontmatter(content) || {}
            : {};

          children.push({
            name: (frontmatter.title as string) || entry.replace(".md", ""),
            description: frontmatter.description as string | undefined,
            path: entryPath,
            children: [],
          });
        }
      }
    }

    return {
      name,
      description,
      path: indexPath,
      children,
    };
  }

  async createWorkspace(path: string, name: string): Promise<void> {
    this.fs.createDir(path);
    this.fs.write(
      `${path}/index.md`,
      `---
title: ${name}
created: ${new Date().toISOString()}
---

# ${name}
`,
    );
    await this.persist();
  }

  // --------------------------------------------------------------------------
  // Entries
  // --------------------------------------------------------------------------

  async getEntry(path: string): Promise<EntryData> {
    const content = this.fs.read(path);

    if (content === undefined) {
      throw new BackendError(`Entry not found: ${path}`, "NotFound", path);
    }

    const frontmatter = this.wasm?.parse_frontmatter(content) || {};
    const title = frontmatter.title as string | undefined;

    // Extract body content (after frontmatter)
    const bodyMatch = content.match(/^---\n[\s\S]*?\n---\n([\s\S]*)$/);
    const body = bodyMatch ? bodyMatch[1] : content;

    return {
      path,
      title,
      frontmatter,
      content: body,
    };
  }

  async saveEntry(path: string, content: string): Promise<void> {
    const existingContent = this.fs.read(path);

    if (existingContent === undefined) {
      throw new BackendError(`Entry not found: ${path}`, "NotFound", path);
    }

    // Preserve existing frontmatter, update content
    const frontmatter = this.wasm?.parse_frontmatter(existingContent) || {};
    frontmatter.updated = new Date().toISOString();

    const frontmatterStr =
      this.wasm?.serialize_frontmatter(frontmatter) || "---\n---";
    const fullContent = `${frontmatterStr}\n${content}`;

    this.fs.write(path, fullContent);
  }

  async createEntry(
    path: string,
    options?: CreateEntryOptions,
  ): Promise<string> {
    if (this.fs.exists(path)) {
      throw new BackendError(
        `Entry already exists: ${path}`,
        "AlreadyExists",
        path,
      );
    }

    const now = new Date().toISOString();
    const title =
      options?.title || path.split("/").pop()?.replace(".md", "") || "Untitled";

    const frontmatter: Record<string, unknown> = {
      title,
      created: now,
      updated: now,
    };

    if (options?.partOf) {
      frontmatter.part_of = options.partOf;
    }

    let body = `\n# ${title}\n`;

    // Apply template if specified
    if (options?.template) {
      const templateContent = await this.getTemplate(options.template);
      const context: Record<string, string> = {
        title,
        date: now,
        timestamp: now,
      };
      body = this.wasm?.render_template(templateContent, context) || body;
    }

    const frontmatterStr =
      this.wasm?.serialize_frontmatter(frontmatter) || "---\n---";
    const fullContent = `${frontmatterStr}\n${body}`;

    // Ensure parent directory exists
    const parentDir = path.split("/").slice(0, -1).join("/");
    if (parentDir && !this.fs.exists(parentDir)) {
      this.fs.createDir(parentDir);
    }

    this.fs.write(path, fullContent);

    return path;
  }

  async deleteEntry(path: string): Promise<void> {
    if (!this.fs.exists(path)) {
      throw new BackendError(`Entry not found: ${path}`, "NotFound", path);
    }

    this.fs.delete(path);
    await this.storage.deleteFile(path);
  }

  // --------------------------------------------------------------------------
  // Frontmatter
  // --------------------------------------------------------------------------

  async getFrontmatter(path: string): Promise<Record<string, unknown>> {
    const content = this.fs.read(path);

    if (content === undefined) {
      throw new BackendError(`Entry not found: ${path}`, "NotFound", path);
    }

    return this.wasm?.parse_frontmatter(content) || {};
  }

  async setFrontmatterProperty(
    path: string,
    key: string,
    value: unknown,
  ): Promise<void> {
    const content = this.fs.read(path);

    if (content === undefined) {
      throw new BackendError(`Entry not found: ${path}`, "NotFound", path);
    }

    const frontmatter = this.wasm?.parse_frontmatter(content) || {};
    frontmatter[key] = value;
    frontmatter.updated = new Date().toISOString();

    // Extract body content
    const bodyMatch = content.match(/^---\n[\s\S]*?\n---\n([\s\S]*)$/);
    const body = bodyMatch ? bodyMatch[1] : "";

    const frontmatterStr =
      this.wasm?.serialize_frontmatter(frontmatter) || "---\n---";
    const fullContent = `${frontmatterStr}\n${body}`;

    this.fs.write(path, fullContent);
  }

  async removeFrontmatterProperty(path: string, key: string): Promise<void> {
    const content = this.fs.read(path);

    if (content === undefined) {
      throw new BackendError(`Entry not found: ${path}`, "NotFound", path);
    }

    const frontmatter = this.wasm?.parse_frontmatter(content) || {};
    delete frontmatter[key];
    frontmatter.updated = new Date().toISOString();

    // Extract body content
    const bodyMatch = content.match(/^---\n[\s\S]*?\n---\n([\s\S]*)$/);
    const body = bodyMatch ? bodyMatch[1] : "";

    const frontmatterStr =
      this.wasm?.serialize_frontmatter(frontmatter) || "---\n---";
    const fullContent = `${frontmatterStr}\n${body}`;

    this.fs.write(path, fullContent);
  }

  // --------------------------------------------------------------------------
  // Search
  // --------------------------------------------------------------------------

  async searchWorkspace(
    pattern: string,
    options?: SearchOptions,
  ): Promise<SearchResults> {
    const rootPath =
      options?.workspacePath || this.config?.default_workspace || "/workspace";
    const caseSensitive = options?.caseSensitive ?? false;

    const results: SearchResults = {
      files: [],
      files_searched: 0,
    };

    const searchFiles = (dirPath: string) => {
      const entries = this.fs.list(dirPath);

      for (const entry of entries) {
        const entryPath = `${dirPath}/${entry}`;

        if (this.fs.isDir(entryPath)) {
          searchFiles(entryPath);
        } else if (entry.endsWith(".md")) {
          results.files_searched++;

          const content = this.fs.read(entryPath);
          if (!content) continue;

          const frontmatter = this.wasm?.parse_frontmatter(content) || {};

          // Determine what to search
          let searchContent = content;
          if (options?.searchFrontmatter) {
            searchContent = JSON.stringify(frontmatter);
          } else if (options?.property) {
            const propValue = frontmatter[options.property];
            searchContent = propValue ? String(propValue) : "";
          }

          const matches =
            this.wasm?.search_content(searchContent, pattern, caseSensitive) ||
            [];

          if (matches.length > 0) {
            results.files.push({
              path: entryPath,
              title: frontmatter.title as string | undefined,
              matches,
            });
          }
        }
      }
    };

    searchFiles(rootPath);

    return results;
  }

  // --------------------------------------------------------------------------
  // Templates
  // --------------------------------------------------------------------------

  private readonly TEMPLATES_PATH = "/.diaryx/templates";

  private readonly BUILTIN_TEMPLATES: Record<string, string> = {
    note: `# {{title}}

`,
    daily: `# {{date:%B %d, %Y}}

## Morning

## Evening

## Notes

`,
  };

  async listTemplates(): Promise<TemplateInfo[]> {
    const templates: TemplateInfo[] = [];

    // Built-in templates
    for (const name of Object.keys(this.BUILTIN_TEMPLATES)) {
      templates.push({
        name,
        path: `builtin:${name}`,
        source: "builtin",
      });
    }

    // User templates from IndexedDB
    if (this.fs.exists(this.TEMPLATES_PATH)) {
      const entries = this.fs.list(this.TEMPLATES_PATH);
      for (const entry of entries) {
        if (entry.endsWith(".md")) {
          const name = entry.replace(".md", "");
          templates.push({
            name,
            path: `${this.TEMPLATES_PATH}/${entry}`,
            source: "user",
          });
        }
      }
    }

    return templates;
  }

  async getTemplate(name: string): Promise<string> {
    // Check built-in templates first
    if (name in this.BUILTIN_TEMPLATES) {
      return this.BUILTIN_TEMPLATES[name];
    }

    // Check user templates
    const userPath = `${this.TEMPLATES_PATH}/${name}.md`;
    const content = this.fs.read(userPath);

    if (content !== undefined) {
      return content;
    }

    throw new BackendError(`Template not found: ${name}`, "NotFound", name);
  }

  async saveTemplate(name: string, content: string): Promise<void> {
    if (!this.fs.exists(this.TEMPLATES_PATH)) {
      this.fs.createDir(this.TEMPLATES_PATH);
    }

    const templatePath = `${this.TEMPLATES_PATH}/${name}.md`;
    this.fs.write(templatePath, content);
    await this.storage.saveFile(templatePath, content);
  }

  async deleteTemplate(name: string): Promise<void> {
    const templatePath = `${this.TEMPLATES_PATH}/${name}.md`;

    if (!this.fs.exists(templatePath)) {
      throw new BackendError(`Template not found: ${name}`, "NotFound", name);
    }

    this.fs.delete(templatePath);
    await this.storage.deleteFile(templatePath);
  }

  // --------------------------------------------------------------------------
  // Persistence
  // --------------------------------------------------------------------------

  async persist(): Promise<void> {
    const dirtyFiles = this.fs.getDirtyFiles();

    if (dirtyFiles.size > 0) {
      await this.storage.saveAllFiles(dirtyFiles);
      this.fs.clearDirty();
    }
  }
}
