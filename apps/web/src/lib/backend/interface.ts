// Backend interface - abstracts over Tauri IPC and WASM implementations

// Import generated types from Rust
import type { Command, Response } from './generated';

// Re-export generated types for consumers
export type { Command, Response } from './generated';
export type {
  EntryData as GeneratedEntryData,
  TreeNode as GeneratedTreeNode,
  SearchResults as GeneratedSearchResults,
  ValidationResult as GeneratedValidationResult,
  FixResult as GeneratedFixResult,
  ExportPlan as GeneratedExportPlan,
} from './generated';

// ============================================================================
// Types (legacy - these will eventually be replaced by generated types)
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
  description: string | null;
  path: string;
  children: TreeNode[];
}

// Note: For full type compatibility with generated types, use import type { EntryData } from './generated'
export interface EntryData {
  path: string;
  title: string | null;
  frontmatter: { [key: string]: unknown };
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
  title: string | null;
  matches: SearchMatch[];
}

export interface SearchResults {
  files: FileSearchResult[];
  files_searched: number;
}

// Validation types
export interface ValidationError {
  type: "BrokenPartOf" | "BrokenContentsRef" | "BrokenAttachment";
  file?: string; // For BrokenPartOf, BrokenAttachment
  index?: string; // For BrokenContentsRef
  target?: string; // For BrokenPartOf, BrokenContentsRef
  attachment?: string; // For BrokenAttachment
}

export interface ValidationWarning {
  type:
    | "OrphanFile"
    | "CircularReference"
    | "UnlinkedEntry"
    | "UnlistedFile"
    | "NonPortablePath"
    | "MultipleIndexes"
    | "OrphanBinaryFile"
    | "MissingPartOf"
    | "InvalidContentsRef";
  file?: string; // For OrphanFile, UnlistedFile, NonPortablePath, OrphanBinaryFile, MissingPartOf
  files?: string[]; // For CircularReference
  path?: string; // For UnlinkedEntry
  is_dir?: boolean; // For UnlinkedEntry
  index?: string; // For UnlistedFile, InvalidContentsRef
  property?: string; // For NonPortablePath
  value?: string; // For NonPortablePath
  suggested?: string; // For NonPortablePath
  directory?: string; // For MultipleIndexes
  indexes?: string[]; // For MultipleIndexes
  suggested_index?: string | null; // For OrphanBinaryFile, MissingPartOf
  target?: string; // For InvalidContentsRef
}

export interface ValidationResult {
  errors: ValidationError[];
  warnings: ValidationWarning[];
  files_checked: number;
}

// Re-export the "with metadata" types from generated bindings
// These include computed display properties from core
export type {
  ValidationResultWithMeta,
  ValidationErrorWithMeta,
  ValidationWarningWithMeta,
} from './generated';

// Fix types
export interface FixResult {
  success: boolean;
  message: string;
}

export interface FixSummary {
  error_fixes: FixResult[];
  warning_fixes: FixResult[];
  total_fixed: number;
  total_failed: number;
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

// Backup types
export interface BackupStatus {
  target_name: string;
  success: boolean;
  files_processed: number;
  error?: string;
}

export interface BackupData {
  text_files: [string, string][];
  binary_files: { path: string; data: number[] }[];
  text_count: number;
  binary_count: number;
}

export interface SearchOptions {
  workspacePath?: string;
  searchFrontmatter?: boolean;
  property?: string;
  caseSensitive?: boolean;
}

export interface CreateEntryOptions {
  title?: string | null;
  partOf?: string | null;
  template?: string | null;
}

export interface TemplateInfo {
  name: string;
  path: string;
  source: "workspace" | "user" | "builtin";
}

// Import types
export interface ImportResult {
  success: boolean;
  files_imported: number;
  error?: string;
}

// ============================================================================
// Backend Events
// ============================================================================

/**
 * Events emitted by Backend operations.
 * Subscribe to these to automatically update CRDT state.
 */
export type BackendEventType =
  | 'file:created'
  | 'file:deleted'
  | 'file:renamed'
  | 'file:moved'
  | 'metadata:changed'
  | 'contents:changed';

export interface FileCreatedEvent {
  type: 'file:created';
  path: string;
  frontmatter: Record<string, unknown>;
  parentPath?: string;
}

export interface FileDeletedEvent {
  type: 'file:deleted';
  path: string;
  parentPath?: string;
}

export interface FileRenamedEvent {
  type: 'file:renamed';
  oldPath: string;
  newPath: string;
}

export interface FileMovedEvent {
  type: 'file:moved';
  path: string;
  oldParent?: string;
  newParent?: string;
}

export interface MetadataChangedEvent {
  type: 'metadata:changed';
  path: string;
  frontmatter: Record<string, unknown>;
}

export interface ContentsChangedEvent {
  type: 'contents:changed';
  path: string;
  contents: string[];
}

export type BackendEvent =
  | FileCreatedEvent
  | FileDeletedEvent
  | FileRenamedEvent
  | FileMovedEvent
  | MetadataChangedEvent
  | ContentsChangedEvent;

export type BackendEventListener = (event: BackendEvent) => void;


// ============================================================================
// Backend Interface
// ============================================================================

/**
 * Backend interface that abstracts over different runtime environments.
 *
 * - TauriBackend: Uses Tauri IPC to communicate with the Rust backend
 * - WasmBackend: Uses WebAssembly module with InMemoryFileSystem + IndexedDB
 *
 * ## API: execute()
 *
 * All operations go through the `execute()` method with typed Command objects.
 * Use the `api.ts` wrapper for ergonomic typed access.
 *
 * @example
 * ```ts
 * // Preferred: Use api wrapper
 * import { createApi } from './api';
 * const api = createApi(backend);
 * const entry = await api.getEntry('workspace/notes.md');
 *
 * // Or use execute() directly
 * const response = await backend.execute({
 *   type: 'GetEntry',
 *   params: { path: 'workspace/notes.md' }
 * });
 * ```
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

  /**
   * Get the default workspace path for this backend.
   * For Tauri: Returns the path from config/platform (e.g., ~/diaryx)
   * For WASM: Returns "workspace" (virtual path in IndexedDB/OPFS)
   */
  getWorkspacePath(): string;

  /**
   * Get the config for this backend (if available).
   * For Tauri: Returns the config loaded from disk.
   * For WASM: Returns null (config not applicable).
   */
  getConfig(): Config | null;

  /**
   * Get app paths (Tauri-specific, returns null for WASM).
   * Includes data_dir, document_dir, default_workspace, config_path, is_mobile.
   */
  getAppPaths(): Record<string, string | boolean> | null;

  // --------------------------------------------------------------------------
  // Unified Command API
  // --------------------------------------------------------------------------

  /**
   * Execute a command and return the response.
   *
   * **This is the primary API.** All operations should use this method.
   * The typed wrapper in `api.ts` provides ergonomic access to this method.
   *
   * @example
   * ```ts
   * const response = await backend.execute({
   *   type: 'GetEntry',
   *   params: { path: 'workspace/notes.md' }
   * });
   * if (response.type === 'Entry') {
   *   console.log(response.data.title);
   * }
   * ```
   */
  execute(command: Command): Promise<Response>;

  // --------------------------------------------------------------------------
  // Events
  // --------------------------------------------------------------------------

  /**
   * Subscribe to backend events.
   * Use this to automatically update CRDT state when files change.
   */
  on(event: BackendEventType, listener: BackendEventListener): void;

  /**
   * Unsubscribe from backend events.
   */
  off(event: BackendEventType, listener: BackendEventListener): void;

  // --------------------------------------------------------------------------
  // Platform-specific methods
  // --------------------------------------------------------------------------

  /**
   * Persist any pending changes to storage.
   * For WASM: writes InMemoryFileSystem contents to IndexedDB.
   * For Tauri: no-op (changes are written directly to disk).
   */
  persist(): Promise<void>;

  /**
   * Import workspace from a zip file.
   * Handles large files by streaming in chunks.
   * @param file The File object from a file input.
   * @param workspacePath Optional workspace path to import into.
   * @param onProgress Optional callback for progress updates.
   * @returns Import result with success status and file count.
   *
   * Note: This requires the browser File API.
   */
  importFromZip(
    file: File,
    workspacePath?: string,
    onProgress?: (bytesUploaded: number, totalBytes: number) => void,
  ): Promise<ImportResult>;
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
