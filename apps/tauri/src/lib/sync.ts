// Live Sync API for Diaryx Frontend
// Example usage of the Tauri live sync commands

import { invoke } from '@tauri-apps/api/core';

export interface SyncStatus {
  connected: boolean;
  document_count: number;
}

export interface SyncStats {
  messages_received: number;
  messages_sent: number;
  documents_updated: number;
}

/**
 * Start live sync for a workspace
 * @param serverUrl - WebSocket server URL (e.g., "ws://localhost:8080")
 * @param workspacePath - Absolute path to workspace directory
 * @param workspaceId - Unique workspace identifier for grouping peers
 */
export async function startLiveSync(
  serverUrl: string,
  workspacePath: string,
  workspaceId: string
): Promise<string> {
  return await invoke<string>('start_live_sync', {
    serverUrl,
    workspacePath,
    workspaceId,
  });
}

/**
 * Stop live sync
 */
export async function stopLiveSync(): Promise<string> {
  return await invoke<string>('stop_live_sync');
}

/**
 * Get current sync status
 */
export async function getSyncStatus(): Promise<SyncStatus> {
  return await invoke<SyncStatus>('get_sync_status');
}

/**
 * Perform a single sync round
 * Call this periodically (e.g., every 100ms) while syncing is active
 */
export async function syncRound(): Promise<SyncStats> {
  return await invoke<SyncStats>('sync_round');
}

/**
 * Update a synced document with new content
 * Call this whenever the user edits a file
 * @param filePath - Absolute path to the file
 * @param content - New content of the file
 */
export async function updateSyncedDocument(
  filePath: string,
  content: string
): Promise<void> {
  return await invoke<void>('update_synced_document', {
    filePath,
    content,
  });
}

// Example usage:
export async function exampleUsage() {
  try {
    // Start syncing
    const result = await startLiveSync(
      'ws://localhost:8080',
      '/Users/yourname/Documents/journal',
      'my-workspace-id'
    );
    console.log(result);

    // Set up periodic sync rounds (every 100ms)
    const syncInterval = setInterval(async () => {
      try {
        const stats = await syncRound();
        if (stats.messages_received > 0 || stats.messages_sent > 0) {
          console.log('Sync stats:', stats);
        }
      } catch (err) {
        console.error('Sync round error:', err);
      }
    }, 100);

    // Get status
    const status = await getSyncStatus();
    console.log('Sync status:', status);

    // When user edits a file
    await updateSyncedDocument(
      '/Users/yourname/Documents/journal/notes.md',
      '# Updated content'
    );

    // Later, stop syncing
    // clearInterval(syncInterval);
    // await stopLiveSync();
  } catch (error) {
    console.error('Sync error:', error);
  }
}
