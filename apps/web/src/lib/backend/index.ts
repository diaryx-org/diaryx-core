// Backend factory - auto-detects runtime environment and provides appropriate backend

import type { Backend } from "./interface";
import { isTauri, isBrowser } from "./interface";
import { createApi, type Api } from "./api";

// Re-export types and utilities
export type {
  Backend,
  Config,
  TreeNode,
  EntryData,
  SearchResults,
  SearchOptions,
  CreateEntryOptions,
  TemplateInfo,
  SearchMatch,
  FileSearchResult,
  ValidationResult,
  ValidationResultWithMeta,
  ValidationError,
  ValidationErrorWithMeta,
  ValidationWarning,
  ValidationWarningWithMeta,
  ExportPlan,
  ExportedFile,
  BinaryExportFile,
  StorageInfo,
} from "./interface";

export { BackendError, isTauri, isBrowser } from "./interface";

// Re-export API types from generated
export type { CreateChildResult } from "./generated";

// Re-export API wrapper
export { createApi, type Api } from "./api";

// ============================================================================
// Singleton Backend Instance
// ============================================================================

let backendInstance: Backend | null = null;
let initPromise: Promise<Backend> | null = null;

/**
 * Get the backend instance, creating it if necessary.
 * This is the main entry point for the backend abstraction.
 *
 * Usage:
 * ```ts
 * const backend = await getBackend();
 * const config = await backend.getConfig();
 * ```
 */
export async function getBackend(): Promise<Backend> {
  if (backendInstance?.isReady()) {
    console.log("[Backend] Returning existing ready backend");
    return backendInstance;
  }

  // Prevent multiple simultaneous initializations
  if (initPromise) {
    console.log("[Backend] Waiting for existing initialization...");
    return initPromise;
  }

  console.log("[Backend] Starting initialization...");
  initPromise = initializeBackend();
  return initPromise;
}

/**
 * Initialize the appropriate backend based on runtime environment.
 */
async function initializeBackend(): Promise<Backend> {
  console.log("[Backend] Detecting runtime environment...");
  console.log("[Backend] isTauri():", isTauri());
  console.log("[Backend] isBrowser():", isBrowser());
  console.log(
    "[Backend] window.__TAURI__:",
    typeof window !== "undefined" ? (window as any).__TAURI__ : "N/A",
  );

  try {
    if (isTauri()) {
      console.log("[Backend] Using Tauri backend");
      const { TauriBackend } = await import("./tauri");
      backendInstance = new TauriBackend();
    } else if (isBrowser()) {
      // Use WorkerBackend which runs WasmBackend in a Web Worker
      // This enables OPFS with createSyncAccessHandle() for Safari
      console.log("[Backend] Using WorkerBackend (WASM in Web Worker)");
      const { WorkerBackendNew } = await import("./workerBackendNew");
      backendInstance = new WorkerBackendNew();
    } else {
      throw new Error("Unsupported runtime environment");
    }

    console.log("[Backend] Calling backend.init()...");
    await backendInstance.init();
    console.log("[Backend] Backend initialized successfully");
    return backendInstance;
  } catch (error) {
    console.error("[Backend] Initialization failed:", error);
    // Reset state so we can retry
    backendInstance = null;
    initPromise = null;
    throw error;
  }
}

/**
 * Reset the backend instance (useful for testing).
 */
export function resetBackend(): void {
  console.log("[Backend] Resetting backend instance");
  backendInstance = null;
  initPromise = null;
}

// ============================================================================
// Convenience Functions
// ============================================================================

/**
 * Check if the backend is ready to use.
 */
export function isBackendReady(): boolean {
  return backendInstance?.isReady() ?? false;
}

/**
 * Get the backend instance synchronously.
 * Throws if the backend hasn't been initialized yet.
 */
export function getBackendSync(): Backend {
  if (!backendInstance?.isReady()) {
    throw new Error(
      "Backend not initialized. Call getBackend() first and await it.",
    );
  }
  return backendInstance;
}

// ============================================================================
// API Wrapper Access
// ============================================================================

let apiInstance: Api | null = null;

/**
 * Get the typed API wrapper, initializing if necessary.
 * This is the recommended way to interact with the backend.
 *
 * Usage:
 * ```ts
 * const api = await getApi();
 * const entry = await api.getEntry('workspace/notes.md');
 * ```
 */
export async function getApi(): Promise<Api> {
  if (apiInstance) {
    return apiInstance;
  }
  const backend = await getBackend();
  apiInstance = createApi(backend);
  return apiInstance;
}

/**
 * Get the API wrapper synchronously.
 * Throws if the backend hasn't been initialized yet.
 */
export function getApiSync(): Api {
  if (apiInstance) {
    return apiInstance;
  }
  const backend = getBackendSync();
  apiInstance = createApi(backend);
  return apiInstance;
}

// ============================================================================
// Auto-Persist Hook (Deprecated/Removed)
// ============================================================================

// startAutoPersist/persistNow removed - persistence is handled automatically by the backend
