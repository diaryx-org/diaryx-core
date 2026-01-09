/**
 * Storage type management for the WASM backend.
 * 
 * Supports three storage backends:
 * - OPFS (default): High-performance origin-private file system
 * - IndexedDB: Fallback for browsers without OPFS support
 * - File System Access: User-visible directory (Chrome/Edge only)
 */



// ============================================================================
// Types
// ============================================================================

export type StorageType = 'opfs' | 'indexeddb' | 'filesystem-access';

const STORAGE_TYPE_KEY = 'diaryx-storage-type';
const FS_HANDLE_KEY = 'diaryx-fs-handle';

// ============================================================================
// Browser Support Detection
// ============================================================================

/**
 * Check if a storage type is supported in the current browser.
 */
export function isStorageTypeSupported(type: StorageType): boolean {
  switch (type) {
    case 'opfs':
      return (
        typeof navigator !== 'undefined' &&
        'storage' in navigator &&
        'getDirectory' in (navigator.storage || {})
      );

    case 'indexeddb':
      return typeof indexedDB !== 'undefined';

    case 'filesystem-access':
      return (
        typeof window !== 'undefined' &&
        'showDirectoryPicker' in window
      );

    default:
      return false;
  }
}

/**
 * Get all supported storage types for the current browser.
 */
export function getSupportedStorageTypes(): StorageType[] {
  const types: StorageType[] = [];
  if (isStorageTypeSupported('opfs')) types.push('opfs');
  if (isStorageTypeSupported('indexeddb')) types.push('indexeddb');
  if (isStorageTypeSupported('filesystem-access')) types.push('filesystem-access');
  return types;
}

// ============================================================================
// Storage Type Selection
// ============================================================================

/**
 * Get the currently selected storage type.
 * Defaults to OPFS if supported, otherwise IndexedDB.
 */
export function getStorageType(): StorageType {
  if (typeof localStorage === 'undefined') {
    return 'indexeddb';
  }

  const stored = localStorage.getItem(STORAGE_TYPE_KEY) as StorageType | null;
  
  if (stored && isStorageTypeSupported(stored)) {
    return stored;
  }

  // Default: prefer OPFS, fallback to IndexedDB
  if (isStorageTypeSupported('opfs')) {
    return 'opfs';
  }
  
  return 'indexeddb';
}

/**
 * Set the storage type preference.
 * Note: Changing storage type requires app restart to take effect.
 */
export function setStorageType(type: StorageType): void {
  if (!isStorageTypeSupported(type)) {
    throw new Error(`Storage type "${type}" is not supported in this browser`);
  }
  localStorage.setItem(STORAGE_TYPE_KEY, type);
}

// ============================================================================
// File System Access Handle Persistence
// ============================================================================

/**
 * Store the directory handle for File System Access API.
 * Uses IndexedDB to persist the handle across sessions.
 */
export async function storeFileSystemHandle(
  handle: FileSystemDirectoryHandle
): Promise<void> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open('diaryx-fs-handles', 1);

    request.onerror = () => reject(new Error('Failed to open handle database'));

    request.onupgradeneeded = (event) => {
      const db = (event.target as IDBOpenDBRequest).result;
      if (!db.objectStoreNames.contains('handles')) {
        db.createObjectStore('handles');
      }
    };

    request.onsuccess = () => {
      const db = request.result;
      const tx = db.transaction('handles', 'readwrite');
      tx.objectStore('handles').put(handle, FS_HANDLE_KEY);
      tx.oncomplete = () => {
        db.close();
        resolve();
      };
      tx.onerror = () => reject(new Error('Failed to store handle'));
    };
  });
}

/**
 * Retrieve the stored directory handle for File System Access API.
 * Returns null if no handle is stored or permission was revoked.
 */
export async function getStoredFileSystemHandle(): Promise<FileSystemDirectoryHandle | null> {
  return new Promise((resolve) => {
    const request = indexedDB.open('diaryx-fs-handles', 1);

    request.onerror = () => resolve(null);

    request.onupgradeneeded = (event) => {
      const db = (event.target as IDBOpenDBRequest).result;
      if (!db.objectStoreNames.contains('handles')) {
        db.createObjectStore('handles');
      }
    };

    request.onsuccess = () => {
      const db = request.result;
      const tx = db.transaction('handles', 'readonly');
      const getReq = tx.objectStore('handles').get(FS_HANDLE_KEY);

      getReq.onsuccess = () => {
        db.close();
        resolve(getReq.result ?? null);
      };
      getReq.onerror = () => {
        db.close();
        resolve(null);
      };
    };
  });
}

/**
 * Clear the stored file system handle.
 */
export async function clearFileSystemHandle(): Promise<void> {
  return new Promise((resolve) => {
    const request = indexedDB.open('diaryx-fs-handles', 1);

    request.onerror = () => resolve();

    request.onsuccess = () => {
      const db = request.result;
      const tx = db.transaction('handles', 'readwrite');
      tx.objectStore('handles').delete(FS_HANDLE_KEY);
      tx.oncomplete = () => {
        db.close();
        resolve();
      };
      tx.onerror = () => {
        db.close();
        resolve();
      };
    };
  });
}

// ============================================================================
// Storage Factory (Deprecated/Removed)
// ============================================================================

// createStorage() removed - storage is now handled by Rust backend


// ============================================================================
// Utility
// ============================================================================

/**
 * Get a human-readable name for a storage type.
 */
export function getStorageTypeName(type: StorageType): string {
  switch (type) {
    case 'opfs':
      return 'Private Storage (OPFS)';
    case 'indexeddb':
      return 'Browser Storage (IndexedDB)';
    case 'filesystem-access':
      return 'Local Folder';
    default:
      return type;
  }
}

/**
 * Get a description for a storage type.
 */
export function getStorageTypeDescription(type: StorageType): string {
  switch (type) {
    case 'opfs':
      return 'High-performance storage managed by the browser. Best for most users.';
    case 'indexeddb':
      return 'Traditional browser database. Compatible with all browsers.';
    case 'filesystem-access':
      return 'Store files in a folder on your computer. Requires Chrome or Edge.';
    default:
      return '';
  }
}
