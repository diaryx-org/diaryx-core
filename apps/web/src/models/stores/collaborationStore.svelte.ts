/**
 * Collaboration Store - Manages Y.js collaboration state
 *
 * This store holds state related to real-time collaboration,
 * including Y.Doc, provider, connection status, and server configuration.
 */

import type { Doc as YDoc } from 'yjs';
import type { HocuspocusProvider } from '@hocuspocus/provider';

// ============================================================================
// Types
// ============================================================================

export type SyncStatus = 'not_configured' | 'idle' | 'connecting' | 'syncing' | 'synced' | 'error';
export type BodySyncStatus = 'idle' | 'syncing' | 'synced';

// ============================================================================
// Helpers
// ============================================================================

function getInitialServerUrl(): string | null {
  if (typeof window !== 'undefined') {
    const saved = localStorage.getItem('diaryx-sync-server');
    if (saved) return saved;
  }
  if (typeof import.meta !== 'undefined' && (import.meta as any).env?.VITE_COLLAB_SERVER) {
    return (import.meta as any).env.VITE_COLLAB_SERVER;
  }
  return null;
}

// ============================================================================
// Store
// ============================================================================

class CollaborationStore {
  // Y.js document and provider
  currentYDoc = $state<YDoc | null>(null);
  currentProvider = $state<HocuspocusProvider | null>(null);
  currentCollaborationPath = $state<string | null>(null);

  // Connection status
  collaborationEnabled = $state(false);
  collaborationConnected = $state(false);

  // Sync status for multi-device sync
  syncStatus = $state<SyncStatus>('not_configured');
  syncProgress = $state<{ total: number; completed: number } | null>(null);
  syncError = $state<string | null>(null);

  // Body sync status (tracked separately from metadata sync)
  bodySyncStatus = $state<BodySyncStatus>('idle');
  bodySyncProgress = $state<{ total: number; completed: number } | null>(null);

  // Server configuration
  collaborationServerUrl = $state<string | null>(getInitialServerUrl());

  /**
   * Effective sync status that only shows 'synced' when BOTH metadata AND body are synced.
   * Use this in UI components to accurately represent sync completion.
   */
  get effectiveSyncStatus(): SyncStatus {
    // If there's an error, show error
    if (this.syncStatus === 'error') return 'error';
    // If not configured, show not configured
    if (this.syncStatus === 'not_configured') return 'not_configured';
    // If either is connecting, show connecting
    if (this.syncStatus === 'connecting') return 'connecting';
    // If either is syncing, show syncing
    if (this.syncStatus === 'syncing' || this.bodySyncStatus === 'syncing') return 'syncing';
    // Only show synced if both metadata and body are synced
    if (this.syncStatus === 'synced' && this.bodySyncStatus === 'synced') return 'synced';
    // Otherwise show idle (metadata synced but body not yet synced)
    if (this.syncStatus === 'synced' && this.bodySyncStatus === 'idle') return 'syncing';
    return this.syncStatus;
  }

  // Y.Doc management
  setYDoc(ydoc: YDoc | null) {
    this.currentYDoc = ydoc;
  }

  setProvider(provider: HocuspocusProvider | null) {
    this.currentProvider = provider;
  }

  setCollaborationPath(path: string | null) {
    this.currentCollaborationPath = path;
  }

  // Set all collaboration state at once
  setCollaborationSession(
    ydoc: YDoc | null,
    provider: HocuspocusProvider | null,
    path: string | null
  ) {
    this.currentYDoc = ydoc;
    this.currentProvider = provider;
    this.currentCollaborationPath = path;
  }

  // Clear collaboration session
  clearCollaborationSession() {
    this.currentYDoc = null;
    this.currentProvider = null;
    this.currentCollaborationPath = null;
  }

  // Connection status
  setEnabled(enabled: boolean) {
    this.collaborationEnabled = enabled;
  }

  setConnected(connected: boolean) {
    this.collaborationConnected = connected;
  }

  // Sync status for multi-device sync
  setSyncStatus(status: SyncStatus) {
    this.syncStatus = status;
    // Clear error when status changes to non-error state
    if (status !== 'error') {
      this.syncError = null;
    }
  }

  setSyncProgress(progress: { total: number; completed: number } | null) {
    this.syncProgress = progress;
  }

  setSyncError(error: string | null | unknown) {
    // Defensive string conversion - error might be an object from Rust
    if (error === null || error === undefined) {
      this.syncError = null;
    } else if (typeof error === 'string') {
      this.syncError = error;
    } else if (error instanceof Error) {
      this.syncError = error.message;
    } else if (typeof error === 'object') {
      // Try to extract message from object, fallback to JSON stringify
      const errObj = error as Record<string, unknown>;
      if (typeof errObj.message === 'string') {
        this.syncError = errObj.message;
      } else if (typeof errObj.error === 'string') {
        this.syncError = errObj.error;
      } else {
        try {
          this.syncError = JSON.stringify(error);
        } catch {
          this.syncError = 'Unknown error';
        }
      }
    } else {
      this.syncError = String(error);
    }
    if (this.syncError) {
      this.syncStatus = 'error';
    }
  }

  // Body sync status methods
  setBodySyncStatus(status: BodySyncStatus) {
    this.bodySyncStatus = status;
  }

  setBodySyncProgress(progress: { total: number; completed: number } | null) {
    this.bodySyncProgress = progress;
    // Auto-update body sync status based on progress
    if (progress) {
      if (progress.completed < progress.total) {
        this.bodySyncStatus = 'syncing';
      } else {
        this.bodySyncStatus = 'synced';
      }
    }
  }

  /**
   * Reset body sync status to idle.
   * Call when starting a new sync session or when body sync is not needed.
   */
  resetBodySyncStatus() {
    this.bodySyncStatus = 'idle';
    this.bodySyncProgress = null;
  }

  // Server URL
  setServerUrl(url: string | null) {
    this.collaborationServerUrl = url;
    if (typeof window !== 'undefined') {
      if (url) {
        localStorage.setItem('diaryx-sync-server', url);
      } else {
        localStorage.removeItem('diaryx-sync-server');
      }
    }
  }
}

// ============================================================================
// Convenience export
// ============================================================================

export const collaborationStore = new CollaborationStore();

/**
 * Get the collaboration store singleton.
 */
export function getCollaborationStore() {
  return collaborationStore;
}
