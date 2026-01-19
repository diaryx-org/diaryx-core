/**
 * Collaboration Store - Manages Y.js collaboration state
 *
 * This store holds state related to real-time collaboration,
 * including Y.Doc, provider, connection status, and server configuration.
 */

import type { Doc as YDoc } from 'yjs';
import type { HocuspocusProvider } from '@hocuspocus/provider';

// ============================================================================
// State
// ============================================================================

// Y.js document and provider
let currentYDoc = $state<YDoc | null>(null);
let currentProvider = $state<HocuspocusProvider | null>(null);
let currentCollaborationPath = $state<string | null>(null);

// Connection status
let collaborationEnabled = $state(false);
let collaborationConnected = $state(false);

// Sync status for multi-device sync
export type SyncStatus = 'not_configured' | 'idle' | 'connecting' | 'syncing' | 'synced' | 'error';
let syncStatus = $state<SyncStatus>('not_configured');
let syncProgress = $state<{ total: number; completed: number } | null>(null);
let syncError = $state<string | null>(null);

// Server configuration
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

let collaborationServerUrl = $state<string | null>(getInitialServerUrl());

// ============================================================================
// Store Factory
// ============================================================================

/**
 * Get the collaboration store singleton.
 */
export function getCollaborationStore() {
  return {
    // Getters
    get currentYDoc() { return currentYDoc; },
    get currentProvider() { return currentProvider; },
    get currentCollaborationPath() { return currentCollaborationPath; },
    get collaborationEnabled() { return collaborationEnabled; },
    get collaborationConnected() { return collaborationConnected; },
    get collaborationServerUrl() { return collaborationServerUrl; },
    get syncStatus() { return syncStatus; },
    get syncProgress() { return syncProgress; },
    get syncError() { return syncError; },

    // Y.Doc management
    setYDoc(ydoc: YDoc | null) {
      currentYDoc = ydoc;
    },

    setProvider(provider: HocuspocusProvider | null) {
      currentProvider = provider;
    },

    setCollaborationPath(path: string | null) {
      currentCollaborationPath = path;
    },

    // Set all collaboration state at once
    setCollaborationSession(
      ydoc: YDoc | null,
      provider: HocuspocusProvider | null,
      path: string | null
    ) {
      currentYDoc = ydoc;
      currentProvider = provider;
      currentCollaborationPath = path;
    },

    // Clear collaboration session
    clearCollaborationSession() {
      currentYDoc = null;
      currentProvider = null;
      currentCollaborationPath = null;
    },

    // Connection status
    setEnabled(enabled: boolean) {
      collaborationEnabled = enabled;
    },

    setConnected(connected: boolean) {
      collaborationConnected = connected;
    },

    // Sync status for multi-device sync
    setSyncStatus(status: SyncStatus) {
      syncStatus = status;
      // Clear error when status changes to non-error state
      if (status !== 'error') {
        syncError = null;
      }
    },

    setSyncProgress(progress: { total: number; completed: number } | null) {
      syncProgress = progress;
    },

    setSyncError(error: string | null) {
      syncError = error;
      if (error) {
        syncStatus = 'error';
      }
    },

    // Server URL
    setServerUrl(url: string | null) {
      collaborationServerUrl = url;
      if (typeof window !== 'undefined') {
        if (url) {
          localStorage.setItem('diaryx-sync-server', url);
        } else {
          localStorage.removeItem('diaryx-sync-server');
        }
      }
    },
  };
}

// ============================================================================
// Convenience export
// ============================================================================

export const collaborationStore = getCollaborationStore();
