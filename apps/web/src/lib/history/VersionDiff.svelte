<script lang="ts">
  import type { FileDiff, ChangeType } from '$lib/backend/generated';

  interface Props {
    diffs: FileDiff[];
  }

  let { diffs }: Props = $props();

  function getChangeIcon(changeType: ChangeType): string {
    switch (changeType) {
      case 'Added':
        return '+';
      case 'Modified':
        return '~';
      case 'Deleted':
        return '-';
      case 'Restored':
        return 'R';
      default:
        return '?';
    }
  }

  function getChangeClass(changeType: ChangeType): string {
    switch (changeType) {
      case 'Added':
        return 'change-added';
      case 'Modified':
        return 'change-modified';
      case 'Deleted':
        return 'change-deleted';
      case 'Restored':
        return 'change-restored';
      default:
        return '';
    }
  }

  function getChangeLabel(changeType: ChangeType): string {
    switch (changeType) {
      case 'Added':
        return 'Added';
      case 'Modified':
        return 'Modified';
      case 'Deleted':
        return 'Deleted';
      case 'Restored':
        return 'Restored';
      default:
        return String(changeType);
    }
  }

  function getFileName(path: string): string {
    return path.split('/').pop() || path;
  }
</script>

<div class="version-diff">
  {#if diffs.length === 0}
    <div class="empty">No changes in this version</div>
  {:else}
    <div class="diff-list">
      {#each diffs as diff}
        <div class="diff-item {getChangeClass(diff.change_type)}">
          <span class="change-icon">{getChangeIcon(diff.change_type)}</span>
          <span class="file-path" title={diff.path}>
            {getFileName(diff.path)}
          </span>
          <span class="change-type">{getChangeLabel(diff.change_type)}</span>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .version-diff {
    font-size: 0.85rem;
  }

  .empty {
    color: var(--muted-foreground);
    text-align: center;
    padding: 1rem;
  }

  .diff-list {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .diff-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.35rem 0.5rem;
    border-radius: 4px;
    background: var(--muted);
  }

  .change-icon {
    width: 1.2rem;
    height: 1.2rem;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    font-weight: 600;
    font-size: 0.8rem;
    flex-shrink: 0;
  }

  .change-added .change-icon {
    background: hsl(142, 76%, 36%);
    color: white;
  }

  .change-modified .change-icon {
    background: hsl(48, 96%, 53%);
    color: black;
  }

  .change-deleted .change-icon {
    background: hsl(0, 72%, 51%);
    color: white;
  }

  .change-restored .change-icon {
    background: hsl(199, 89%, 48%);
    color: white;
  }

  .file-path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--foreground);
  }

  .change-type {
    font-size: 0.7rem;
    color: var(--muted-foreground);
    flex-shrink: 0;
  }
</style>
