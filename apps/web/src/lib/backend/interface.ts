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

// Validation types
export interface ValidationError {
  type: 'BrokenPartOf' | 'BrokenContentsRef';
  file?: string;  // For BrokenPartOf
  index?: string; // For BrokenContentsRef
  target: string;
}

export interface ValidationWarning {
  type: 'OrphanFile' | 'CircularReference';
  file?: string;   // For OrphanFile
  files?: string[]; // For CircularReference
}

export interface ValidationResult {
  errors: ValidationError[];
  warnings: ValidationWarning[];
  files_checked: number;
}

// Export types
export interface ExportPlan {
  included: { path: string; relative_path: string }[];
  excluded: { path: string; reason: string }[];
  audience: string;
}

export interface ExportedFile {
  path: string;
  content: string;
}

export interface BinaryExportFile {
  path: string;
  data: number[];
}

export interface StorageInfo {
  used: number;
  limit: number;
  attachment_limit: number;
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
   * @param path Path where the workspace should be created. Uses platform default if not provided.
   * @param name Name of the workspace. Defaults to "My Workspace".
   * @returns The path to the created workspace.
   */
  createWorkspace(path?: string, name?: string): Promise<string>;

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

  /**
   * Move/rename an entry.
   * Implementations should keep parent index `contents` and the entry's `part_of` metadata consistent.
   * @param fromPath Existing path to the entry file.
   * @param toPath New path for the entry file.
   * @returns The destination path.
   */
  moveEntry(fromPath: string, toPath: string): Promise<string>;

  /**
   * Attach an existing entry to a parent index.
   * This is equivalent to "workspace add": it adds the entry to the parent's `contents`
   * and sets the entry's `part_of` to point back to the parent index (both as relative paths).
   *
   * @param entryPath Path to the entry to attach.
   * @param parentIndexPath Path to the parent index file (typically ".../index.md").
   */
  attachEntryToParent(
    entryPath: string,
    parentIndexPath: string,
  ): Promise<void>;

  /**
   * Convert a leaf file to an index file with a directory.
   * Example: `journal/my-note.md` → `journal/my-note/index.md`
   *
   * @param path Path to the leaf file to convert.
   * @returns The new path to the index file.
   */
  convertToIndex(path: string): Promise<string>;

  /**
   * Convert an empty index file back to a leaf file.
   * Example: `journal/my-note/index.md` → `journal/my-note.md`
   * Fails if the index has non-empty contents.
   *
   * @param path Path to the index file to convert.
   * @returns The new path to the leaf file.
   */
  convertToLeaf(path: string): Promise<string>;

  /**
   * Create a new child entry under a parent.
   * If the parent is a leaf file, it will be automatically converted to an index first.
   * Generates a unique filename like "new-entry.md", "new-entry-1.md", etc.
   *
   * @param parentPath Path to the parent entry (can be leaf or index).
   * @returns The path to the newly created child entry.
   */
  createChildEntry(parentPath: string): Promise<string>;

  /**
   * Convert a title to a kebab-case filename.
   * Example: "My Cool Entry" → "my-cool-entry.md"
   *
   * @param title The title to convert.
   * @returns The slugified filename.
   */
  slugifyTitle(title: string): string;

  /**
   * Rename an entry file by giving it a new filename.
   * For leaf files: renames the file and updates parent `contents`.
   * For index files: renames the containing directory and updates grandparent `contents`.
   *
   * @param path Path to the entry to rename.
   * @param newFilename The new filename (e.g., "new-name.md").
   * @returns The new path to the renamed file.
   */
  renameEntry(path: string, newFilename: string): Promise<string>;

  /**
   * Ensure today's daily entry exists, creating it if necessary.
   * @returns The path to today's daily entry.
   */
  ensureDailyEntry(): Promise<string>;

  // --------------------------------------------------------------------------
  // Export
  // --------------------------------------------------------------------------

  /**
   * Get all available audience tags from files in the workspace.
   * @param rootPath Path to start scanning from.
   * @returns Array of unique audience tag strings.
   */
  getAvailableAudiences(rootPath: string): Promise<string[]>;

  /**
   * Plan an export operation (preview what files would be included/excluded).
   * @param rootPath Path to the entry to export from.
   * @param audience Target audience to filter by.
   * @returns Export plan with included and excluded files.
   */
  planExport(rootPath: string, audience: string): Promise<ExportPlan>;

  /**
   * Export files to memory for download.
   * @param rootPath Path to the entry to export from.
   * @param audience Target audience to filter by.
   * @returns Array of files with path and content.
   */
  exportToMemory(rootPath: string, audience: string): Promise<ExportedFile[]>;

  /**
   * Export files as HTML (converts markdown to HTML).
   * @param rootPath Path to the entry to export from.
   * @param audience Target audience to filter by.
   * @returns Array of HTML files with path and content.
   */
  exportToHtml(rootPath: string, audience: string): Promise<ExportedFile[]>;

  /**
   * Export binary attachment files.
   * @param rootPath Path to the entry to export from.
   * @param audience Target audience to filter by.
   * @returns Array of binary files with path and data.
   */
  exportBinaryAttachments(rootPath: string, audience: string): Promise<BinaryExportFile[]>;

  // --------------------------------------------------------------------------
  // Attachments
  // --------------------------------------------------------------------------

  /**
   * Get the list of attachments for an entry.
   * @param entryPath Path to the entry file.
   * @returns Array of attachment relative paths.
   */
  getAttachments(entryPath: string): Promise<string[]>;

  /**
   * Upload an attachment file.
   * @param entryPath Path to the entry file.
   * @param filename Name of the attachment file.
   * @param dataBase64 Base64 encoded file data.
   * @returns The relative path where the attachment was stored.
   */
  uploadAttachment(entryPath: string, filename: string, dataBase64: string): Promise<string>;

  /**
   * Delete an attachment file.
   * @param entryPath Path to the entry file.
   * @param attachmentPath Relative path to the attachment.
   */
  deleteAttachment(entryPath: string, attachmentPath: string): Promise<void>;

  /**
   * Get storage usage information.
   * @returns Storage usage stats including limits.
   */
  getStorageUsage(): Promise<StorageInfo>;

  /**
   * Get binary data for an attachment.
   * @param entryPath Path to the entry file.
   * @param attachmentPath Relative path to the attachment.
   * @returns Uint8Array of the attachment data.
   */
  getAttachmentData(entryPath: string, attachmentPath: string): Promise<Uint8Array>;

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
    value: unknown,
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
    options?: SearchOptions,
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
  // Validation
  // --------------------------------------------------------------------------

  /**
   * Validate workspace links (contents and part_of references).
   * @param workspacePath Optional path to workspace. Uses default if not provided.
   */
  validateWorkspace(workspacePath?: string): Promise<ValidationResult>;

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
    public readonly path?: string,
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
