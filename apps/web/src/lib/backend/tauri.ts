// Tauri backend implementation - uses Tauri IPC to communicate with Rust backend
// NOTE: We use dynamic imports for @tauri-apps/api to avoid load-time failures
// when this module is bundled but running in a non-Tauri environment.
// Tauri backend implementation - wrapper around Tauri invoke commands

import type {
  Backend,
  Config,
  TreeNode,
  EntryData,
  SearchResults,
  SearchOptions,
  CreateEntryOptions,
  TemplateInfo,
  ValidationResult,
} from "./interface";

interface MoveEntryRequest {
  fromPath: string;
  toPath: string;
}

interface AttachEntryToParentRequest {
  entryPath: string;
  parentIndexPath: string;
}

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

// ============================================================================
// Internal Types (matching Rust backend structures)
// ============================================================================

interface SaveEntryRequest {
  path: string;
  content: string;
}

/** App paths returned by the Rust backend */
interface AppPaths {
  data_dir: string;
  document_dir: string;
  default_workspace: string;
  config_path: string;
  is_mobile: boolean;
}

// Type for the invoke function from Tauri
type InvokeFn = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

// ============================================================================
// Helper Functions
// ============================================================================

function handleError(error: unknown): never {
  console.error("[TauriBackend] Command error (raw):", error);

  if (typeof error === "string") {
    throw new BackendError(error, "CommandError");
  }

  if (typeof error === "object" && error !== null) {
    const err = error as Record<string, unknown>;

    // Handle direct SerializableError format
    if (typeof err.message === "string") {
      throw new BackendError(
        err.message,
        typeof err.kind === "string" ? err.kind : "UnknownError",
        typeof err.path === "string" ? err.path : undefined,
      );
    }

    // Handle nested error format (Tauri sometimes wraps errors)
    if (typeof err.error === "object" && err.error !== null) {
      const nested = err.error as Record<string, unknown>;
      if (typeof nested.message === "string") {
        throw new BackendError(
          nested.message,
          typeof nested.kind === "string" ? nested.kind : "UnknownError",
          typeof nested.path === "string" ? nested.path : undefined,
        );
      }
    }

    // Fallback: stringify the object
    throw new BackendError(JSON.stringify(error), "UnknownError");
  }

  throw new BackendError(String(error), "UnknownError");
}

// ============================================================================
// TauriBackend Implementation
// ============================================================================

export class TauriBackend implements Backend {
  private ready = false;
  private invoke: InvokeFn | null = null;
  private appPaths: AppPaths | null = null;

  async init(): Promise<void> {
    // Step 1: Dynamically import Tauri API
    console.log("[TauriBackend] Step 1: Importing Tauri API...");
    let tauriCore;
    try {
      tauriCore = await import("@tauri-apps/api/core");
      this.invoke = tauriCore.invoke;
      console.log("[TauriBackend] Step 1 complete: Tauri API imported");
    } catch (e) {
      console.error(
        "[TauriBackend] Step 1 failed: Could not import Tauri API:",
        e,
      );
      throw new BackendError(
        `Failed to import Tauri API: ${e instanceof Error ? e.message : String(e)}`,
        "TauriImportError",
      );
    }

    // Step 2: Call initialize_app command
    console.log("[TauriBackend] Step 2: Calling initialize_app...");
    try {
      const result = await this.invoke<AppPaths>("initialize_app");
      console.log("[TauriBackend] Step 2 complete: initialize_app returned");
      console.log("[TauriBackend] Raw result type:", typeof result);
      console.log(
        "[TauriBackend] Raw result:",
        JSON.stringify(result, null, 2),
      );
      this.appPaths = result;
    } catch (e) {
      console.error("[TauriBackend] Step 2 failed: initialize_app error:", e);
      console.error("[TauriBackend] Error type:", typeof e);
      console.error(
        "[TauriBackend] Error stringified:",
        JSON.stringify(e, null, 2),
      );
      throw new BackendError(
        `Failed to initialize app: ${this.formatError(e)}`,
        "InitializeAppError",
      );
    }

    // Step 3: Validate the result
    console.log("[TauriBackend] Step 3: Validating result...");
    if (!this.appPaths) {
      console.error("[TauriBackend] Step 3 failed: appPaths is null/undefined");
      throw new BackendError(
        "initialize_app returned null/undefined",
        "InvalidResult",
      );
    }
    if (typeof this.appPaths.default_workspace !== "string") {
      console.error(
        "[TauriBackend] Step 3 failed: default_workspace is not a string:",
        this.appPaths.default_workspace,
      );
      throw new BackendError(
        `Invalid default_workspace: ${typeof this.appPaths.default_workspace}`,
        "InvalidResult",
      );
    }
    console.log("[TauriBackend] Step 3 complete: Result validated");
    console.log("[TauriBackend] App paths:", this.appPaths);

    this.ready = true;
    console.log("[TauriBackend] Initialization complete!");
  }

  private formatError(e: unknown): string {
    if (typeof e === "string") {
      return e;
    } else if (e instanceof Error) {
      return e.message;
    } else if (typeof e === "object" && e !== null) {
      const err = e as { message?: string; kind?: string; path?: string };
      if (err.message) {
        return err.path
          ? `${err.message} (${err.kind}: ${err.path})`
          : `${err.message} (${err.kind || "Unknown"})`;
      }
      return JSON.stringify(e);
    }
    return String(e);
  }

  isReady(): boolean {
    return this.ready;
  }

  private getInvoke(): InvokeFn {
    if (!this.invoke) {
      throw new BackendError("Tauri not initialized", "NotInitialized");
    }
    return this.invoke;
  }

  /**
   * Get the app paths (useful for debugging or displaying to user)
   */
  getAppPaths(): AppPaths | null {
    return this.appPaths;
  }

  /**
   * Check if running on a mobile platform (iOS/Android)
   */
  isMobile(): boolean {
    return this.appPaths?.is_mobile ?? false;
  }

  /**
   * Get the default workspace path for the current platform
   */
  getDefaultWorkspacePath(): string | null {
    return this.appPaths?.default_workspace ?? null;
  }

  // --------------------------------------------------------------------------
  // Configuration
  // --------------------------------------------------------------------------

  async getConfig(): Promise<Config> {
    try {
      return await this.getInvoke()<Config>("get_config");
    } catch (e) {
      handleError(e);
    }
  }

  async saveConfig(config: Config): Promise<void> {
    try {
      await this.getInvoke()("save_config", { config });
    } catch (e) {
      handleError(e);
    }
  }

  // --------------------------------------------------------------------------
  // Workspace
  // --------------------------------------------------------------------------

  async getWorkspaceTree(
    workspacePath?: string,
    depth?: number,
  ): Promise<TreeNode> {
    try {
      console.log("[TauriBackend] getWorkspaceTree called with:", {
        workspacePath,
        depth,
        appPaths: this.appPaths,
      });
      const result = await this.getInvoke()<TreeNode>("get_workspace_tree", {
        workspacePath,
        depth,
      });
      console.log("[TauriBackend] getWorkspaceTree result:", result);
      return result;
    } catch (e) {
      console.error("[TauriBackend] getWorkspaceTree error:", e);
      handleError(e);
    }
  }

  async createWorkspace(path?: string, name?: string): Promise<string> {
    try {
      // Use platform-appropriate default path if not provided
      const result = await this.getInvoke()<string>("create_workspace", {
        path,
        name,
      });
      return result;
    } catch (e) {
      handleError(e);
    }
  }

  // --------------------------------------------------------------------------
  // Entries
  // --------------------------------------------------------------------------

  async getEntry(path: string): Promise<EntryData> {
    try {
      return await this.getInvoke()<EntryData>("get_entry", { path });
    } catch (e) {
      handleError(e);
    }
  }

  async saveEntry(path: string, content: string): Promise<void> {
    try {
      const request: SaveEntryRequest = { path, content };
      await this.getInvoke()("save_entry", { request });
    } catch (e) {
      handleError(e);
    }
  }

  async createEntry(
    path: string,
    options?: CreateEntryOptions,
  ): Promise<string> {
    try {
      const config = await this.getConfig();
      const workspaceRoot = config?.default_workspace ?? "workspace";
      const normalizedPath = normalizeEntryPathToWorkspaceRoot(
        path,
        workspaceRoot,
      );

      return await this.getInvoke()<string>("create_entry", {
        path: normalizedPath,
        title: options?.title,
        partOf: options?.partOf,
        template: options?.template,
      });
    } catch (e) {
      handleError(e);
    }
  }

  async deleteEntry(path: string): Promise<void> {
    try {
      await this.getInvoke()("delete_entry", { path });
    } catch (e) {
      handleError(e);
    }
  }

  async moveEntry(fromPath: string, toPath: string): Promise<string> {
    try {
      const request: MoveEntryRequest = { fromPath, toPath };
      return await this.getInvoke()<string>("move_entry", { request });
    } catch (e) {
      handleError(e);
    }
  }

  async attachEntryToParent(
    entryPath: string,
    parentIndexPath: string,
  ): Promise<void> {
    try {
      const config = await this.getConfig();
      const workspaceRoot = config?.default_workspace ?? "workspace";

      const normalizedEntryPath = normalizeEntryPathToWorkspaceRoot(
        entryPath,
        workspaceRoot,
      );
      const normalizedParentIndexPath = normalizeEntryPathToWorkspaceRoot(
        parentIndexPath,
        workspaceRoot,
      );

      const request: AttachEntryToParentRequest = {
        entryPath: normalizedEntryPath,
        parentIndexPath: normalizedParentIndexPath,
      };

      await this.getInvoke()("attach_entry_to_parent", { request });
    } catch (e) {
      handleError(e);
    }
  }

  async convertToIndex(path: string): Promise<string> {
    // TODO: Implement Tauri command for convert_to_index
    throw new BackendError(
      "convertToIndex not yet implemented for Tauri backend",
      "NotImplemented",
      path,
    );
  }

  async convertToLeaf(path: string): Promise<string> {
    // TODO: Implement Tauri command for convert_to_leaf
    throw new BackendError(
      "convertToLeaf not yet implemented for Tauri backend",
      "NotImplemented",
      path,
    );
  }

  async createChildEntry(parentPath: string): Promise<string> {
    // TODO: Implement Tauri command for create_child_entry
    throw new BackendError(
      "createChildEntry not yet implemented for Tauri backend",
      "NotImplemented",
      parentPath,
    );
  }

  // --------------------------------------------------------------------------
  // Frontmatter
  // --------------------------------------------------------------------------

  async getFrontmatter(path: string): Promise<Record<string, unknown>> {
    try {
      return await this.getInvoke()<Record<string, unknown>>(
        "get_frontmatter",
        { path },
      );
    } catch (e) {
      handleError(e);
    }
  }

  async setFrontmatterProperty(
    path: string,
    key: string,
    value: unknown,
  ): Promise<void> {
    try {
      await this.getInvoke()("set_frontmatter_property", { path, key, value });
    } catch (e) {
      handleError(e);
    }
  }

  async removeFrontmatterProperty(path: string, key: string): Promise<void> {
    try {
      await this.getInvoke()("remove_frontmatter_property", { path, key });
    } catch (e) {
      handleError(e);
    }
  }

  // --------------------------------------------------------------------------
  // Search
  // --------------------------------------------------------------------------

  async searchWorkspace(
    pattern: string,
    options?: SearchOptions,
  ): Promise<SearchResults> {
    try {
      return await this.getInvoke()<SearchResults>("search_workspace", {
        pattern,
        workspacePath: options?.workspacePath,
        searchFrontmatter: options?.searchFrontmatter,
        property: options?.property,
        caseSensitive: options?.caseSensitive,
      });
    } catch (e) {
      handleError(e);
    }
  }

  // --------------------------------------------------------------------------
  // Templates
  // --------------------------------------------------------------------------

  async listTemplates(): Promise<TemplateInfo[]> {
    try {
      return await this.getInvoke()<TemplateInfo[]>("list_templates");
    } catch (e) {
      handleError(e);
    }
  }

  async getTemplate(name: string): Promise<string> {
    try {
      return await this.getInvoke()<string>("get_template", { name });
    } catch (e) {
      handleError(e);
    }
  }

  async saveTemplate(name: string, content: string): Promise<void> {
    try {
      await this.getInvoke()("save_template", { name, content });
    } catch (e) {
      handleError(e);
    }
  }

  async deleteTemplate(name: string): Promise<void> {
    try {
      await this.getInvoke()("delete_template", { name });
    } catch (e) {
      handleError(e);
    }
  }

  // --------------------------------------------------------------------------
  // Validation
  // --------------------------------------------------------------------------

  async validateWorkspace(workspacePath?: string): Promise<ValidationResult> {
    try {
      return await this.getInvoke()<ValidationResult>("validate_workspace", {
        workspacePath,
      });
    } catch (e) {
      handleError(e);
    }
  }

  // --------------------------------------------------------------------------
  // Persistence
  // --------------------------------------------------------------------------

  async persist(): Promise<void> {
    // No-op for Tauri - changes are written directly to disk
  }
}
