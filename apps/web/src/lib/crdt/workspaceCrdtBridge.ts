/**
 * Workspace CRDT Bridge - replaces workspaceCrdt.ts with Rust CRDT backend.
 *
 * This module provides the same API surface as the original workspaceCrdt.ts
 * but delegates all operations to the Rust CRDT via RustCrdtApi.
 */

import type { RustCrdtApi } from './rustCrdtApi';
import type { HocuspocusBridge } from './hocuspocusBridge';
import type { FileMetadata, BinaryRef } from '../backend/generated';

// State
let rustApi: RustCrdtApi | null = null;
let syncBridge: HocuspocusBridge | null = null;
let serverUrl: string | null = null;
let _workspaceId: string | null = null;
let initialized = false;
let _initializing = false;

// Callbacks
type FileChangeCallback = (path: string, metadata: FileMetadata | null) => void;
const fileChangeCallbacks = new Set<FileChangeCallback>();

// ===========================================================================
// Configuration
// ===========================================================================

/**
 * Set the Hocuspocus server URL for workspace sync.
 */
export function setWorkspaceServer(url: string | null): void {
  serverUrl = url;
}

/**
 * Get the current workspace server URL.
 */
export function getWorkspaceServer(): string | null {
  return serverUrl;
}

/**
 * Set the initializing state (for UI feedback).
 */
export function setInitializing(value: boolean): void {
  _initializing = value;
}

/**
 * Set the workspace ID for room naming.
 */
export function setWorkspaceId(id: string | null): void {
  _workspaceId = id;
}

/**
 * Get the current workspace ID.
 */
export function getWorkspaceId(): string | null {
  return _workspaceId;
}

/**
 * Check if workspace is currently initializing.
 */
export function isInitializing(): boolean {
  return _initializing;
}

// ===========================================================================
// Initialization
// ===========================================================================

export interface WorkspaceInitOptions {
  /** Rust CRDT API instance */
  rustApi: RustCrdtApi;
  /** Hocuspocus bridge (optional, for sync) */
  syncBridge?: HocuspocusBridge;
  /** Server URL (optional) */
  serverUrl?: string;
  /** Workspace ID for room naming */
  workspaceId?: string;
  /** Called when initialization completes */
  onReady?: () => void;
  /** Called when file metadata changes */
  onFileChange?: FileChangeCallback;
}

/**
 * Initialize the workspace CRDT.
 */
export async function initWorkspace(options: WorkspaceInitOptions): Promise<void> {
  if (initialized) {
    console.warn('[WorkspaceCrdtBridge] Already initialized');
    return;
  }

  _initializing = true;

  try {
    rustApi = options.rustApi;
    syncBridge = options.syncBridge ?? null;
    serverUrl = options.serverUrl ?? null;
    _workspaceId = options.workspaceId ?? null;

    if (options.onFileChange) {
      fileChangeCallbacks.add(options.onFileChange);
    }

    // Connect sync bridge if provided
    if (syncBridge) {
      await syncBridge.connect();
    }

    initialized = true;
    options.onReady?.();
  } finally {
    _initializing = false;
  }
}

/**
 * Disconnect the workspace sync.
 */
export function disconnectWorkspace(): void {
  syncBridge?.disconnect();
}

/**
 * Reconnect the workspace sync.
 */
export function reconnectWorkspace(): void {
  syncBridge?.connect();
}

/**
 * Destroy the workspace and cleanup.
 */
export async function destroyWorkspace(): Promise<void> {
  syncBridge?.destroy();
  syncBridge = null;
  rustApi = null;
  initialized = false;
  fileChangeCallbacks.clear();
}

/**
 * Check if workspace is initialized.
 */
export function isWorkspaceInitialized(): boolean {
  return initialized;
}

/**
 * Check if workspace is connected to sync server.
 */
export function isWorkspaceConnected(): boolean {
  return syncBridge?.isSynced() ?? false;
}

// ===========================================================================
// File Operations
// ===========================================================================

/**
 * Get file metadata from the CRDT.
 */
export async function getFileMetadata(path: string): Promise<FileMetadata | null> {
  if (!rustApi) return null;
  return rustApi.getFile(path);
}

/**
 * Get all files (excluding deleted).
 */
export async function getAllFiles(): Promise<Map<string, FileMetadata>> {
  if (!rustApi) return new Map();
  const files = await rustApi.listFiles(false);
  return new Map(files);
}

/**
 * Get all files including deleted.
 */
export async function getAllFilesIncludingDeleted(): Promise<Map<string, FileMetadata>> {
  if (!rustApi) return new Map();
  const files = await rustApi.listFiles(true);
  return new Map(files);
}

/**
 * Set file metadata in the CRDT.
 */
export async function setFileMetadata(path: string, metadata: FileMetadata): Promise<void> {
  if (!rustApi) {
    throw new Error('Workspace not initialized');
  }
  await rustApi.setFile(path, metadata);
  notifyFileChange(path, metadata);
}

/**
 * Update specific fields of file metadata.
 */
export async function updateFileMetadata(
  path: string,
  updates: Partial<FileMetadata>
): Promise<void> {
  if (!rustApi) {
    throw new Error('Workspace not initialized');
  }

  const existing = await rustApi.getFile(path);
  const updated: FileMetadata = {
    title: updates.title ?? existing?.title ?? null,
    part_of: updates.part_of ?? existing?.part_of ?? null,
    contents: updates.contents ?? existing?.contents ?? null,
    attachments: updates.attachments ?? existing?.attachments ?? [],
    deleted: updates.deleted ?? existing?.deleted ?? false,
    audience: updates.audience ?? existing?.audience ?? null,
    description: updates.description ?? existing?.description ?? null,
    extra: updates.extra ?? existing?.extra ?? {},
    modified_at: BigInt(Date.now()),
  };

  await rustApi.setFile(path, updated);
  notifyFileChange(path, updated);
}

/**
 * Delete a file (soft delete via tombstone).
 */
export async function deleteFile(path: string): Promise<void> {
  await updateFileMetadata(path, { deleted: true });
}

/**
 * Restore a deleted file.
 */
export async function restoreFile(path: string): Promise<void> {
  await updateFileMetadata(path, { deleted: false });
}

/**
 * Permanently remove a file from the CRDT.
 * Note: This sets all fields to null/empty, as CRDTs don't support true deletion.
 */
export async function purgeFile(path: string): Promise<void> {
  if (!rustApi) {
    throw new Error('Workspace not initialized');
  }

  const metadata: FileMetadata = {
    title: null,
    part_of: null,
    contents: null,
    attachments: [],
    deleted: true,
    audience: null,
    description: null,
    extra: {},
    modified_at: BigInt(Date.now()),
  };

  await rustApi.setFile(path, metadata);
  notifyFileChange(path, null);
}

// ===========================================================================
// Hierarchy Operations
// ===========================================================================

/**
 * Add a child to a parent's contents array.
 */
export async function addToContents(parentPath: string, childPath: string): Promise<void> {
  const parent = await getFileMetadata(parentPath);
  if (!parent) return;

  const contents = parent.contents ?? [];
  if (!contents.includes(childPath)) {
    contents.push(childPath);
    await updateFileMetadata(parentPath, { contents });
  }
}

/**
 * Remove a child from a parent's contents array.
 */
export async function removeFromContents(parentPath: string, childPath: string): Promise<void> {
  const parent = await getFileMetadata(parentPath);
  if (!parent) return;

  const contents = parent.contents ?? [];
  const index = contents.indexOf(childPath);
  if (index !== -1) {
    contents.splice(index, 1);
    await updateFileMetadata(parentPath, { contents: contents.length > 0 ? contents : null });
  }
}

/**
 * Set the part_of (parent) for a file.
 */
export async function setPartOf(childPath: string, parentPath: string | null): Promise<void> {
  await updateFileMetadata(childPath, { part_of: parentPath });
}

/**
 * Move a file to a new parent.
 */
export async function moveFile(
  path: string,
  newParentPath: string,
  newPath: string
): Promise<void> {
  const file = await getFileMetadata(path);
  if (!file) return;

  // Remove from old parent
  if (file.part_of) {
    await removeFromContents(file.part_of, path);
  }

  // Add to new parent
  await addToContents(newParentPath, newPath);

  // Update the file's part_of
  if (path !== newPath) {
    // If path changed, create new entry and delete old
    await setFileMetadata(newPath, { ...file, part_of: newParentPath });
    await purgeFile(path);
  } else {
    await updateFileMetadata(path, { part_of: newParentPath });
  }
}

/**
 * Rename a file (change its path).
 */
export async function renameFile(oldPath: string, newPath: string): Promise<void> {
  const file = await getFileMetadata(oldPath);
  if (!file) return;

  // Create new entry with new path
  await setFileMetadata(newPath, { ...file, modified_at: BigInt(Date.now()) });

  // Update parent's contents
  if (file.part_of) {
    const parent = await getFileMetadata(file.part_of);
    if (parent?.contents) {
      const contents = parent.contents.map((c) => (c === oldPath ? newPath : c));
      await updateFileMetadata(file.part_of, { contents });
    }
  }

  // Delete old entry
  await purgeFile(oldPath);
}

// ===========================================================================
// Attachment Operations
// ===========================================================================

/**
 * Add an attachment to a file.
 */
export async function addAttachment(filePath: string, attachment: BinaryRef): Promise<void> {
  const file = await getFileMetadata(filePath);
  if (!file) return;

  const attachments = [...file.attachments, attachment];
  await updateFileMetadata(filePath, { attachments });
}

/**
 * Remove an attachment from a file.
 */
export async function removeAttachment(filePath: string, attachmentPath: string): Promise<void> {
  const file = await getFileMetadata(filePath);
  if (!file) return;

  const attachments = file.attachments.filter((a) => a.path !== attachmentPath);
  await updateFileMetadata(filePath, { attachments });
}

/**
 * Get attachments for a file.
 */
export async function getAttachments(filePath: string): Promise<BinaryRef[]> {
  const file = await getFileMetadata(filePath);
  return file?.attachments ?? [];
}

// ===========================================================================
// Sync Operations
// ===========================================================================

/**
 * Save CRDT state to storage.
 */
export async function saveCrdtState(): Promise<void> {
  if (!rustApi) return;
  await rustApi.saveCrdtState('workspace');
}

/**
 * Wait for sync to complete (with timeout).
 */
export function waitForSync(timeoutMs = 5000): Promise<boolean> {
  return new Promise((resolve) => {
    if (isWorkspaceConnected()) {
      resolve(true);
      return;
    }

    const timeout = setTimeout(() => {
      resolve(false);
    }, timeoutMs);

    const checkInterval = setInterval(() => {
      if (isWorkspaceConnected()) {
        clearInterval(checkInterval);
        clearTimeout(timeout);
        resolve(true);
      }
    }, 100);
  });
}

// ===========================================================================
// Statistics
// ===========================================================================

/**
 * Get workspace statistics.
 */
export async function getWorkspaceStats(): Promise<{
  totalFiles: number;
  activeFiles: number;
  deletedFiles: number;
}> {
  const allFiles = await getAllFilesIncludingDeleted();
  const activeFiles = await getAllFiles();

  return {
    totalFiles: allFiles.size,
    activeFiles: activeFiles.size,
    deletedFiles: allFiles.size - activeFiles.size,
  };
}

// ===========================================================================
// Callbacks
// ===========================================================================

/**
 * Subscribe to file changes.
 */
export function onFileChange(callback: FileChangeCallback): () => void {
  fileChangeCallbacks.add(callback);
  return () => fileChangeCallbacks.delete(callback);
}

// Private helpers

function notifyFileChange(path: string, metadata: FileMetadata | null): void {
  for (const callback of fileChangeCallbacks) {
    try {
      callback(path, metadata);
    } catch (error) {
      console.error('[WorkspaceCrdtBridge] File change callback error:', error);
    }
  }
}

// Re-export types
export type { FileMetadata, BinaryRef };
