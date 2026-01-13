<script lang="ts">
  import { onMount } from 'svelte';
  import type { RustCrdtApi } from '$lib/crdt/rustCrdtApi';
  import type { CrdtHistoryEntry, FileDiff } from '$lib/backend/generated';
  import HistoryEntry from './HistoryEntry.svelte';
  import VersionDiff from './VersionDiff.svelte';

  // Props
  interface Props {
    rustApi: RustCrdtApi;
    docName?: string;
    onRestore?: (updateId: bigint) => void;
  }

  let { rustApi, docName = 'workspace', onRestore }: Props = $props();

  // State
  let history: CrdtHistoryEntry[] = $state([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let selectedEntry: CrdtHistoryEntry | null = $state(null);
  let diffs: FileDiff[] = $state([]);
  let loadingDiff = $state(false);

  async function loadHistory() {
    loading = true;
    error = null;
    try {
      history = await rustApi.getHistory(docName, 100);
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to load history';
      console.error('[HistoryPanel] Error loading history:', e);
    } finally {
      loading = false;
    }
  }

  async function selectEntry(entry: CrdtHistoryEntry) {
    if (selectedEntry?.update_id === entry.update_id) {
      // Deselect
      selectedEntry = null;
      diffs = [];
      return;
    }

    selectedEntry = entry;
    loadingDiff = true;
    diffs = [];

    try {
      // Find the previous entry to diff against
      const idx = history.findIndex((h) => h.update_id === entry.update_id);
      if (idx < history.length - 1) {
        const previousEntry = history[idx + 1];
        diffs = await rustApi.getVersionDiff(previousEntry.update_id, entry.update_id, docName);
      }
    } catch (e) {
      console.error('[HistoryPanel] Error loading diff:', e);
    } finally {
      loadingDiff = false;
    }
  }

  async function handleRestore(entry: CrdtHistoryEntry) {
    if (!confirm(`Restore to version from ${formatTimestamp(entry.timestamp)}?`)) {
      return;
    }

    try {
      await rustApi.restoreVersion(entry.update_id, docName);
      onRestore?.(entry.update_id);
      await loadHistory(); // Refresh history
    } catch (e) {
      console.error('[HistoryPanel] Error restoring version:', e);
      alert('Failed to restore version');
    }
  }

  function formatTimestamp(timestamp: bigint): string {
    const date = new Date(Number(timestamp));
    return date.toLocaleString();
  }

  function formatRelativeTime(timestamp: bigint): string {
    const now = Date.now();
    const diff = now - Number(timestamp);
    const minutes = Math.floor(diff / 60000);
    const hours = Math.floor(diff / 3600000);
    const days = Math.floor(diff / 86400000);

    if (minutes < 1) return 'Just now';
    if (minutes < 60) return `${minutes}m ago`;
    if (hours < 24) return `${hours}h ago`;
    return `${days}d ago`;
  }

  onMount(() => {
    loadHistory();
  });
</script>

<div class="history-panel">
  <div class="header">
    <h2>Version History</h2>
    <button class="refresh-btn" onclick={loadHistory} disabled={loading}>
      {loading ? 'Loading...' : 'Refresh'}
    </button>
  </div>

  {#if error}
    <div class="error">{error}</div>
  {/if}

  {#if loading && history.length === 0}
    <div class="loading">Loading history...</div>
  {:else if history.length === 0}
    <div class="empty">No version history available</div>
  {:else}
    <div class="history-list">
      {#each history as entry (entry.update_id)}
        <HistoryEntry
          {entry}
          selected={selectedEntry?.update_id === entry.update_id}
          formatRelativeTime={formatRelativeTime}
          onSelect={() => selectEntry(entry)}
          onRestore={() => handleRestore(entry)}
        />
      {/each}
    </div>
  {/if}

  {#if selectedEntry && (diffs.length > 0 || loadingDiff)}
    <div class="diff-section">
      <h3>Changes in this version</h3>
      {#if loadingDiff}
        <div class="loading">Loading changes...</div>
      {:else}
        <VersionDiff {diffs} />
      {/if}
    </div>
  {/if}
</div>

<style>
  .history-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    padding: 1rem;
    overflow: hidden;
  }

  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 1rem;
    padding-bottom: 0.5rem;
    border-bottom: 1px solid var(--border);
  }

  .header h2 {
    margin: 0;
    font-size: 1.1rem;
    font-weight: 600;
    color: var(--foreground);
  }

  .refresh-btn {
    padding: 0.25rem 0.5rem;
    font-size: 0.8rem;
    background: var(--muted);
    border: 1px solid var(--border);
    border-radius: 4px;
    cursor: pointer;
  }

  .refresh-btn:hover:not(:disabled) {
    background: var(--accent);
  }

  .refresh-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .error {
    padding: 0.5rem;
    color: var(--destructive);
    background: var(--destructive-foreground);
    border-radius: 4px;
    margin-bottom: 1rem;
  }

  .loading,
  .empty {
    color: var(--muted-foreground);
    text-align: center;
    padding: 2rem;
  }

  .history-list {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .diff-section {
    margin-top: 1rem;
    padding-top: 1rem;
    border-top: 1px solid var(--border);
  }

  .diff-section h3 {
    font-size: 0.9rem;
    font-weight: 600;
    margin: 0 0 0.5rem 0;
    color: var(--foreground);
  }
</style>
