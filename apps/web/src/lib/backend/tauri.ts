// Tauri backend implementation - uses Tauri IPC to communicate with Rust backend
// NOTE: We use dynamic imports for @tauri-apps/api to avoid load-time failures
// when this module is bundled but running in a non-Tauri environment.

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
// Internal Types (matching Rust backend structures)
// ============================================================================

interface SerializableError {
  kind: string;
  message: string;
  path?: string;
}

interface SaveEntryRequest {
  path: string;
  content: string;
}

// Type for the invoke function from Tauri
type InvokeFn = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

// ============================================================================
// Helper Functions
// ============================================================================

function handleError(error: unknown): never {
  if (typeof error === "object" && error !== null && "message" in error) {
    const serError = error as SerializableError;
    throw new BackendError(serError.message, serError.kind, serError.path);
  }
  throw new BackendError(String(error), "UnknownError");
}

// ============================================================================
// TauriBackend Implementation
// ============================================================================

export class TauriBackend implements Backend {
  private ready = false;
  private invoke: InvokeFn | null = null;

  async init(): Promise<void> {
    // Dynamically import Tauri API to avoid load-time failures
    try {
      const tauriCore = await import("@tauri-apps/api/core");
      this.invoke = tauriCore.invoke;
      this.ready = true;
    } catch (e) {
      throw new BackendError(
        `Failed to load Tauri API: ${e}`,
        "TauriLoadError",
      );
    }
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
      return await this.getInvoke()<TreeNode>("get_workspace_tree", {
        workspacePath,
        depth,
      });
    } catch (e) {
      handleError(e);
    }
  }

  async createWorkspace(path: string, name: string): Promise<void> {
    try {
      await this.getInvoke()("create_workspace", { path, name });
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
      return await this.getInvoke()<string>("create_entry", {
        path,
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
  // Persistence
  // --------------------------------------------------------------------------

  async persist(): Promise<void> {
    // No-op for Tauri - changes are written directly to disk
  }
}
