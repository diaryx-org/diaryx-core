/**
 * Sync Helpers - Uses new Rust sync handler commands.
 *
 * This module provides helper functions that leverage the new Rust SyncHandler
 * infrastructure. These helpers can be progressively integrated into
 * workspaceCrdtBridge.ts to simplify its implementation.
 *
 * Key improvements over the current TypeScript implementation:
 * - Path handling (guest/host) is done in Rust
 * - Metadata merging (CRDT vs disk) is done in Rust
 * - Disk writes after remote updates are done in Rust
 *
 * NOTE: These commands require regenerating TypeScript bindings after the
 * Rust changes are complete. Run: `cargo test -p diaryx_core export_bindings`
 */

import type { Backend, FileSystemEvent } from '../backend/interface';

/**
 * Configure the sync handler for guest mode.
 *
 * Call this when entering a share session as a guest.
 * The Rust sync handler will prefix storage paths appropriately.
 *
 * @param backend - The backend instance
 * @param joinCode - The session join code, or null to disable guest mode
 * @param usesOpfs - Whether the guest uses OPFS (requires path prefixing)
 */
export async function configureSyncHandler(
  backend: Backend,
  joinCode: string | null,
  usesOpfs: boolean
): Promise<void> {
  // Use type assertion since bindings may not be regenerated yet
  await backend.execute({
    type: 'ConfigureSyncHandler' as any,
    params: {
      guest_join_code: joinCode,
      uses_opfs: usesOpfs,
    },
  } as any);
}

/**
 * Get the storage path for a canonical path.
 *
 * For guests using OPFS: prefixes with `guest/{join_code}/`
 * For guests using in-memory storage or hosts: returns the path unchanged
 *
 * @param backend - The backend instance
 * @param canonicalPath - The canonical path (e.g., "notes/hello.md")
 * @returns The storage path (possibly with guest prefix)
 */
export async function getStoragePath(backend: Backend, canonicalPath: string): Promise<string> {
  const response = await backend.execute({
    type: 'GetStoragePath' as any,
    params: {
      canonical_path: canonicalPath,
    },
  } as any);
  if (response.type === 'String') {
    return (response as any).data;
  }
  throw new Error('Unexpected response type from GetStoragePath');
}

/**
 * Get the canonical path from a storage path.
 *
 * Strips the `guest/{join_code}/` prefix if present for OPFS guests.
 *
 * @param backend - The backend instance
 * @param storagePath - The storage path (possibly with guest prefix)
 * @returns The canonical path
 */
export async function getCanonicalPath(backend: Backend, storagePath: string): Promise<string> {
  const response = await backend.execute({
    type: 'GetCanonicalPath' as any,
    params: {
      storage_path: storagePath,
    },
  } as any);
  if (response.type === 'String') {
    return (response as any).data;
  }
  throw new Error('Unexpected response type from GetCanonicalPath');
}

/**
 * Apply a remote workspace update with disk write side effects.
 *
 * This is a higher-level command that:
 * 1. Applies the update to the Rust CRDT
 * 2. Optionally writes changed files to disk
 * 3. Merges metadata (CRDT wins, disk as fallback for nulls)
 * 4. Emits FileSystemEvents
 *
 * Use this instead of manually calling applyUpdate + processRemoteMetadataUpdates.
 *
 * @param backend - The backend instance
 * @param update - The binary update data
 * @param writeToDisk - If true, write changed files to disk
 * @returns The update ID if available
 */
export async function applyRemoteWorkspaceUpdate(
  backend: Backend,
  update: Uint8Array,
  writeToDisk: boolean
): Promise<bigint | null> {
  const response = await backend.execute({
    type: 'ApplyRemoteWorkspaceUpdateWithEffects' as any,
    params: {
      update: Array.from(update),
      write_to_disk: writeToDisk,
    },
  } as any);
  if (response.type === 'UpdateId') {
    return (response as any).data;
  }
  if (response.type === 'Ok') {
    return null;
  }
  throw new Error(`Unexpected response type from ApplyRemoteWorkspaceUpdateWithEffects: ${response.type}`);
}

/**
 * Apply a remote body update with disk write side effects.
 *
 * This is a higher-level command that:
 * 1. Applies the update to the body CRDT
 * 2. Optionally writes the body to disk with proper frontmatter
 * 3. Emits ContentsChanged FileSystemEvent
 *
 * Use this instead of manually calling applyBodyUpdate + writeFileWithFrontmatter.
 *
 * @param backend - The backend instance
 * @param docName - The document name (file path)
 * @param update - The binary update data
 * @param writeToDisk - If true, write body to disk
 * @returns The update ID if available
 */
export async function applyRemoteBodyUpdate(
  backend: Backend,
  docName: string,
  update: Uint8Array,
  writeToDisk: boolean
): Promise<bigint | null> {
  const response = await backend.execute({
    type: 'ApplyRemoteBodyUpdateWithEffects' as any,
    params: {
      doc_name: docName,
      update: Array.from(update),
      write_to_disk: writeToDisk,
    },
  } as any);
  if (response.type === 'UpdateId') {
    return (response as any).data;
  }
  if (response.type === 'Ok') {
    return null;
  }
  throw new Error(`Unexpected response type from ApplyRemoteBodyUpdateWithEffects: ${response.type}`);
}

/**
 * Subscribe to FileSystemEvents for sync status updates.
 *
 * The Rust SyncHandler emits these events:
 * - SyncStarted: When a sync session starts
 * - SyncCompleted: When initial sync is done
 * - SyncStatusChanged: When sync status changes (idle/connecting/syncing/synced/error)
 * - SyncProgress: When sync progress updates
 *
 * This can replace the multiple callback systems in workspaceCrdtBridge.
 */
export type SyncEventHandler = {
  onSyncStarted?: (docName: string) => void;
  onSyncCompleted?: (docName: string, filesSynced: number) => void;
  onSyncStatusChanged?: (status: string, error?: string) => void;
  onSyncProgress?: (completed: number, total: number) => void;
  onFileCreated?: (path: string, frontmatter?: unknown) => void;
  onFileDeleted?: (path: string) => void;
  onContentsChanged?: (path: string, body: string) => void;
  onMetadataChanged?: (path: string, frontmatter: unknown) => void;
};

/**
 * Process a FileSystemEvent and dispatch to the appropriate handler.
 *
 * @param event - The FileSystemEvent from Rust
 * @param handlers - The event handlers to dispatch to
 */
export function handleFileSystemEvent(
  event: FileSystemEvent,
  handlers: SyncEventHandler
): void {
  switch (event.type) {
    case 'SyncStarted':
      handlers.onSyncStarted?.(event.doc_name);
      break;
    case 'SyncCompleted':
      handlers.onSyncCompleted?.(event.doc_name, event.files_synced);
      break;
    case 'SyncStatusChanged':
      handlers.onSyncStatusChanged?.(event.status, event.error ?? undefined);
      break;
    case 'SyncProgress':
      handlers.onSyncProgress?.(event.completed, event.total);
      break;
    case 'FileCreated':
      handlers.onFileCreated?.(event.path, event.frontmatter ?? undefined);
      break;
    case 'FileDeleted':
      handlers.onFileDeleted?.(event.path);
      break;
    case 'ContentsChanged':
      handlers.onContentsChanged?.(event.path, event.body);
      break;
    case 'MetadataChanged':
      handlers.onMetadataChanged?.(event.path, event.frontmatter);
      break;
    default:
      // Other event types (FileRenamed, FileMoved) - handle as needed
      break;
  }
}
