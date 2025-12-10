// Backend interface - abstracts over Tauri IPC and WASM implementations

// ============================================================================
// Types
// ============================================================================

export interface Config {
  default_workspace: string;
  daily_entry_folder?: string;
  editor?: string;
  default_template?: string;
  daily_template?: string;
}

export interface TreeNode {
  name: string;
  description?: string;
  path: string;
  children: TreeNode[];
}

export interface EntryData {
  path: string;
  title?: string;
  frontmatter: Record<string, unknown>;
  content: string;
}

export interface SearchMatch {
  line_number: number;
  line_content: string;
  match_start: number;
  match_end: number;
}

export interface FileSearchResult {
  path: string;
  title?: string;
  matches: SearchMatch[];
}

export interface SearchResults {
  files: FileSearchResult[];
  files_searched: number;
}

export interface SearchOptions {
  workspacePath?: string;
  searchFrontmatter?: boolean;
  property?: string;
  caseSensitive?: boolean;
}

export interface CreateEntryOptions {
  title?: string;
  partOf?: string;
  template?: string;
}

export interface TemplateInfo {
  name: string;
  path: string;
  source: "workspace" | "user" | "builtin";
}

// ============================================================================
// Backend Interface
// ============================================================================

/**
 * Backend interface that abstracts over different runtime environments.
 *
 * - TauriBackend: Uses Tauri IPC to communicate with the Rust backend
 * - WasmBackend: Uses WebAssembly module with InMemoryFileSystem + IndexedDB
 */
export interface Backend {
  /**
   * Initialize the backend. Must be called before any other methods.
   * For WASM, this loads data from IndexedDB into the InMemoryFileSystem.
   * For Tauri, this is a no-op.
   */
  init(): Promise<void>;

  /**
   * Check if the backend is ready to use.
   */
  isReady(): boolean;

  // --------------------------------------------------------------------------
  // Configuration
  // --------------------------------------------------------------------------

  /**
   * Get the current configuration.
   */
  getConfig(): Promise<Config>;

  /**
   * Update the configuration.
   */
  saveConfig(config: Config): Promise<void>;

  // --------------------------------------------------------------------------
  // Workspace
  // --------------------------------------------------------------------------

  /**
   * Get the workspace tree structure.
   * @param workspacePath Optional path to a specific workspace. Uses default if not provided.
   * @param depth Optional maximum depth to traverse.
   */
  getWorkspaceTree(workspacePath?: string, depth?: number): Promise<TreeNode>;

  /**
   * Create a new workspace at the given path.
   * @param path Path where the workspace should be created.
   * @param name Name of the workspace.
   */
  createWorkspace(path: string, name: string): Promise<void>;

  // --------------------------------------------------------------------------
  // Entries
  // --------------------------------------------------------------------------

  /**
   * Get an entry's content and metadata.
   * @param path Path to the entry file.
   */
  getEntry(path: string): Promise<EntryData>;

  /**
   * Save an entry's content.
   * @param path Path to the entry file.
   * @param content New markdown content.
   */
  saveEntry(path: string, content: string): Promise<void>;

  /**
   * Create a new entry.
   * @param path Path where the entry should be created.
   * @param options Optional creation options (title, partOf, template).
   * @returns The path to the created entry.
   */
  createEntry(path: string, options?: CreateEntryOptions): Promise<string>;

  /**
   * Delete an entry.
   * @param path Path to the entry to delete.
   */
  deleteEntry(path: string): Promise<void>;

  // --------------------------------------------------------------------------
  // Frontmatter
  // --------------------------------------------------------------------------

  /**
   * Get all frontmatter properties for an entry.
   * @param path Path to the entry file.
   */
  getFrontmatter(path: string): Promise<Record<string, unknown>>;

  /**
   * Set a frontmatter property.
   * @param path Path to the entry file.
   * @param key Property key.
   * @param value Property value.
   */
  setFrontmatterProperty(
    path: string,
    key: string,
    value: unknown
  ): Promise<void>;

  /**
   * Remove a frontmatter property.
   * @param path Path to the entry file.
   * @param key Property key to remove.
   */
  removeFrontmatterProperty(path: string, key: string): Promise<void>;

  // --------------------------------------------------------------------------
  // Search
  // --------------------------------------------------------------------------

  /**
   * Search the workspace for entries matching a pattern.
   * @param pattern Search pattern (regex supported).
   * @param options Search options.
   */
  searchWorkspace(
    pattern: string,
    options?: SearchOptions
  ): Promise<SearchResults>;

  // --------------------------------------------------------------------------
  // Templates
  // --------------------------------------------------------------------------

  /**
   * List available templates.
   */
  listTemplates(): Promise<TemplateInfo[]>;

  /**
   * Get a template's content.
   * @param name Template name.
   */
  getTemplate(name: string): Promise<string>;

  /**
   * Create or update a user template.
   * @param name Template name.
   * @param content Template content.
   */
  saveTemplate(name: string, content: string): Promise<void>;

  /**
   * Delete a user template.
   * @param name Template name.
   */
  deleteTemplate(name: string): Promise<void>;

  // --------------------------------------------------------------------------
  // Persistence (WASM-specific, no-op for Tauri)
  // --------------------------------------------------------------------------

  /**
   * Persist any pending changes to storage.
   * For WASM: writes InMemoryFileSystem contents to IndexedDB.
   * For Tauri: no-op (changes are written directly to disk).
   */
  persist(): Promise<void>;
}

// ============================================================================
// Error Types
// ============================================================================

export class BackendError extends Error {
  constructor(
    message: string,
    public readonly kind: string,
    public readonly path?: string
  ) {
    super(message);
    this.name = "BackendError";
  }
}

// ============================================================================
// Runtime Detection
// ============================================================================

/**
 * Check if running in a Tauri environment.
 */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI__" in window;
}

/**
 * Check if running in a browser (non-Tauri) environment.
 */
export function isBrowser(): boolean {
  return typeof window !== "undefined" && !("__TAURI__" in window);
}
