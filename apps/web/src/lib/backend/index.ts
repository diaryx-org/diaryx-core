// Backend factory - auto-detects runtime environment and provides appropriate backend

import type { Backend } from "./interface";
import { isTauri, isBrowser } from "./interface";

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
  ValidationError,
  ValidationWarning,
  ExportPlan,
  ExportedFile,
  BinaryExportFile,
  StorageInfo,
} from "./interface";

export { BackendError, isTauri, isBrowser } from "./interface";

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
      console.log("[Backend] Using WASM backend");
      const { WasmBackend } = await import("./wasm");
      backendInstance = new WasmBackend();
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
// Auto-Persist Hook (for WASM backend)
// ============================================================================

let persistInterval: ReturnType<typeof setInterval> | null = null;

/**
 * Start auto-persisting changes to IndexedDB.
 * Only has an effect when using the WASM backend.
 *
 * @param intervalMs How often to persist (default: 5000ms)
 */
export function startAutoPersist(intervalMs = 5000): void {
  if (persistInterval) return;

  persistInterval = setInterval(async () => {
    if (backendInstance?.isReady()) {
      try {
        await backendInstance.persist();
      } catch (e) {
        console.error("[Backend] Auto-persist failed:", e);
      }
    }
  }, intervalMs);
}

/**
 * Stop auto-persisting.
 */
export function stopAutoPersist(): void {
  if (persistInterval) {
    clearInterval(persistInterval);
    persistInterval = null;
  }
}

/**
 * Manually trigger a persist operation.
 */
export async function persistNow(): Promise<void> {
  if (backendInstance?.isReady()) {
    await backendInstance.persist();
  }
}
