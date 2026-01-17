// Tauri backend implementation - uses Tauri IPC to communicate with Rust backend
// NOTE: We use dynamic imports for @tauri-apps/api to avoid load-time failures
// when this module is bundled but running in a non-Tauri environment.

import type {
  Backend,
  BackendEventType,
  BackendEventListener,
  Command,
  Response,
  Config,
} from "./interface";

import { BackendError } from "./interface";
import { BackendEventEmitter } from "./eventEmitter";

// ============================================================================
// Internal Types
// ============================================================================

/** App paths returned by the Rust backend */
interface AppPaths {
  data_dir: string;
  document_dir: string;
  default_workspace: string;
  config_path: string;
  is_mobile: boolean;
  /** Whether CRDT storage was successfully initialized */
  crdt_initialized: boolean;
  /** Error message if CRDT initialization failed */
  crdt_error: string | null;
  /** Index signature for Record compatibility */
  [key: string]: string | boolean | null;
}

interface ImportResult {
  success: boolean;
  files_imported: number;
  error?: string;
}

// Type for the invoke function from Tauri
type InvokeFn = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

// ============================================================================
// Helper Functions
// ============================================================================

// Error kinds that are expected during normal operation (e.g., validation checking broken refs)
const EXPECTED_ERROR_KINDS = new Set(["FileRead", "NotFound", "FileNotFound", "IoError"]);

function handleError(error: unknown): never {
  // Extract error kind to determine log level
  let errorKind: string | undefined;
  if (typeof error === "object" && error !== null) {
    const err = error as Record<string, unknown>;
    errorKind = typeof err.kind === "string" ? err.kind : undefined;
  }

  // Only log unexpected errors to avoid spam from expected validation errors
  if (!errorKind || !EXPECTED_ERROR_KINDS.has(errorKind)) {
    console.error("[TauriBackend] Command error (raw):", error);
  }

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
  private config: Config | null = null;
  private eventEmitter = new BackendEventEmitter();

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
      this.appPaths = result;
    } catch (e) {
      console.error("[TauriBackend] Step 2 failed: initialize_app error:", e);
      throw new BackendError(
        `Failed to initialize app: ${this.formatError(e)}`,
        "InitializeAppError",
      );
    }

    // Step 3: Validate the result
    console.log("[TauriBackend] Step 3: Validating result...");
    if (!this.appPaths) {
      throw new BackendError(
        "initialize_app returned null/undefined",
        "InvalidResult",
      );
    }
    if (typeof this.appPaths.default_workspace !== "string") {
      throw new BackendError(
        `Invalid default_workspace: ${typeof this.appPaths.default_workspace}`,
        "InvalidResult",
      );
    }
    console.log("[TauriBackend] Step 3 complete: Result validated");
    console.log("[TauriBackend] App paths:", this.appPaths);

    // Step 4: Check CRDT initialization status
    if (!this.appPaths.crdt_initialized) {
      console.warn(
        "[TauriBackend] CRDT storage failed to initialize:",
        this.appPaths.crdt_error || "Unknown error",
      );
      console.warn(
        "[TauriBackend] Sync and history features may not work correctly.",
      );
    } else {
      console.log("[TauriBackend] CRDT storage initialized successfully");
    }

    // Create config object from appPaths (no separate command needed)
    // The workspace path is already resolved by initialize_app
    this.config = {
      default_workspace: this.appPaths.default_workspace,
    };

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
   * Get the workspace path from config, falling back to platform default.
   * This is the path to the workspace index file (e.g., ~/diaryx/index.md).
   */
  getWorkspacePath(): string {
    // Use config's default_workspace if available
    if (this.config?.default_workspace) {
      // Append /index.md if it's a directory path
      const ws = this.config.default_workspace;
      if (!ws.endsWith('.md')) {
        return `${ws}/index.md`;
      }
      return ws;
    }
    // Fall back to app paths
    if (this.appPaths?.default_workspace) {
      return `${this.appPaths.default_workspace}/index.md`;
    }
    // Last resort fallback
    return "workspace/index.md";
  }

  /**
   * Get the config loaded from disk.
   */
  getConfig(): import("./interface").Config | null {
    return this.config;
  }

  /**
   * Get the app paths (useful for debugging or displaying to user)
   */
  getAppPaths(): Record<string, string | boolean | null> | null {
    return this.appPaths;
  }

  /**
   * Check if running on a mobile platform (iOS/Android)
   */
  isMobile(): boolean {
    return this.appPaths?.is_mobile ?? false;
  }

  /**
   * Check if CRDT storage was successfully initialized.
   * If false, sync and history features may not work.
   */
  isCrdtInitialized(): boolean {
    return this.appPaths?.crdt_initialized ?? false;
  }

  /**
   * Get the CRDT initialization error message, if any.
   */
  getCrdtError(): string | null {
    return this.appPaths?.crdt_error ?? null;
  }

  // --------------------------------------------------------------------------
  // Events
  // --------------------------------------------------------------------------

  on(event: BackendEventType, listener: BackendEventListener): void {
    this.eventEmitter.on(event, listener);
  }

  off(event: BackendEventType, listener: BackendEventListener): void {
    this.eventEmitter.off(event, listener);
  }

  // --------------------------------------------------------------------------
  // Unified Command API
  // --------------------------------------------------------------------------

  async execute(command: Command): Promise<Response> {
    try {
      // Custom replacer to handle BigInt serialization
      const commandJson = JSON.stringify(command, (_key, value) =>
        typeof value === 'bigint' ? Number(value) : value
      );
      const responseJson = await this.getInvoke()<string>("execute", { commandJson });
      // Custom reviver to handle BigInt deserialization for known fields
      return JSON.parse(responseJson, (key, value) => {
        // Convert numeric timestamps back to BigInt for specific fields
        if ((key === 'modified_at' || key === 'uploaded_at' || key === 'size' || 
             key === 'timestamp' || key === 'update_id') && typeof value === 'number') {
          return BigInt(value);
        }
        return value;
      }) as Response;
    } catch (e) {
      handleError(e);
    }
  }

  // --------------------------------------------------------------------------
  // Persistence (no-op for Tauri - changes are written directly to disk)
  // --------------------------------------------------------------------------

  async persist(): Promise<void> {
    // No-op for Tauri - changes are written directly to disk
  }

  // --------------------------------------------------------------------------
  // Import (platform-specific - requires File API and chunked upload)
  // --------------------------------------------------------------------------

  async importFromZip(
    file: File,
    workspacePath?: string,
    onProgress?: (bytesUploaded: number, totalBytes: number) => void,
  ): Promise<ImportResult> {
    const invoke = this.getInvoke();
    const totalBytes = file.size;

    console.log(
      `[TauriBackend] Starting import of ${(totalBytes / 1024 / 1024).toFixed(2)} MB`,
    );

    // Start upload session on Rust side
    const sessionId = await invoke<string>("start_import_upload");

    // Read and send file in chunks (1MB each)
    const CHUNK_SIZE = 1024 * 1024; // 1MB
    const reader = file.stream().getReader();
    let bytesUploaded = 0;

    try {
      // Read chunks and send to Rust
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        // Process in 1MB sub-chunks if needed
        for (let offset = 0; offset < value.length; offset += CHUNK_SIZE) {
          const subChunk = value.slice(offset, offset + CHUNK_SIZE);
          const base64 = btoa(
            Array.from(subChunk)
              .map((b) => String.fromCharCode(b))
              .join(""),
          );

          await invoke<number>("append_import_chunk", {
            sessionId,
            chunk: base64,
          });

          bytesUploaded += subChunk.length;

          if (onProgress) {
            onProgress(bytesUploaded, totalBytes);
          }
        }
      }

      console.log(`[TauriBackend] Upload complete, extracting zip...`);

      // Finish upload and import
      const result = await invoke<{
        success: boolean;
        files_imported: number;
        error?: string;
      }>("finish_import_upload", {
        sessionId,
        workspacePath,
      });

      return {
        success: result.success,
        files_imported: result.files_imported,
        error: result.error,
      };
    } catch (e) {
      // Format error properly
      const errorMessage = this.formatError(e);
      console.error("[TauriBackend] Import failed:", errorMessage);
      return {
        success: false,
        files_imported: 0,
        error: errorMessage,
      };
    }
  }

  /**
   * Read a binary file's content.
   * Uses Tauri's read_binary_file command.
   */
  async readBinary(path: string): Promise<Uint8Array> {
    const invoke = this.getInvoke();
    const data = await invoke<number[]>("read_binary_file", { path });
    return new Uint8Array(data);
  }

  /**
   * Write binary content to a file.
   * Uses Tauri's write_binary_file command.
   */
  async writeBinary(path: string, data: Uint8Array): Promise<void> {
    const invoke = this.getInvoke();
    await invoke("write_binary_file", { path, data: Array.from(data) });
  }
}
