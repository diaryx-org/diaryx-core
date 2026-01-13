<script lang="ts">
  import type { CrdtHistoryEntry } from '$lib/backend/generated';

  interface Props {
    entry: CrdtHistoryEntry;
    selected: boolean;
    formatRelativeTime: (timestamp: bigint) => string;
    onSelect: () => void;
    onRestore: () => void;
  }

  let { entry, selected, formatRelativeTime, onSelect, onRestore }: Props = $props();

  function getOriginLabel(origin: string): string {
    switch (origin) {
      case 'Local':
        return 'You';
      case 'Remote':
        return 'Remote';
      case 'Sync':
        return 'Sync';
      default:
        return origin;
    }
  }

  function getOriginClass(origin: string): string {
    switch (origin) {
      case 'Local':
        return 'origin-local';
      case 'Remote':
        return 'origin-remote';
      case 'Sync':
        return 'origin-sync';
      default:
        return '';
    }
  }
</script>

<div
  class="history-entry"
  class:selected
  role="button"
  tabindex="0"
  onclick={onSelect}
  onkeydown={(e) => e.key === 'Enter' && onSelect()}
>
  <div class="entry-content">
    <div class="entry-header">
      <span class="time">{formatRelativeTime(entry.timestamp)}</span>
      <span class="origin {getOriginClass(entry.origin)}">{getOriginLabel(entry.origin)}</span>
    </div>
    <div class="entry-id">#{entry.update_id.toString()}</div>
  </div>
  {#if selected}
    <button
      class="restore-btn"
      onclick|stopPropagation={onRestore}
    >
      Restore
    </button>
  {/if}
</div>

<style>
  .history-entry {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.5rem 0.75rem;
    border-radius: 6px;
    cursor: pointer;
    transition: background 0.1s;
  }

  .history-entry:hover {
    background: var(--muted);
  }

  .history-entry.selected {
    background: var(--accent);
  }

  .entry-content {
    flex: 1;
    min-width: 0;
  }

  .entry-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .time {
    font-size: 0.9rem;
    font-weight: 500;
    color: var(--foreground);
  }

  .origin {
    font-size: 0.7rem;
    padding: 0.1rem 0.4rem;
    border-radius: 4px;
    background: var(--muted);
    color: var(--muted-foreground);
  }

  .origin-local {
    background: var(--primary);
    color: var(--primary-foreground);
  }

  .origin-remote {
    background: var(--secondary);
    color: var(--secondary-foreground);
  }

  .origin-sync {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .entry-id {
    font-size: 0.75rem;
    color: var(--muted-foreground);
    margin-top: 0.1rem;
  }

  .restore-btn {
    padding: 0.25rem 0.5rem;
    font-size: 0.8rem;
    background: var(--primary);
    color: var(--primary-foreground);
    border: none;
    border-radius: 4px;
    cursor: pointer;
    flex-shrink: 0;
  }

  .restore-btn:hover {
    opacity: 0.9;
  }
</style>
