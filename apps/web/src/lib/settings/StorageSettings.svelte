<script lang="ts">
  /**
   * StorageSettings - Storage backend selection component
   * 
   * Allows users to choose between:
   * - OPFS (default): High-performance, browser-managed
   * - IndexedDB: Traditional browser database
   * - File System Access: User-visible local folder
   */
  import { Button } from "$lib/components/ui/button";
  import { HardDrive, Database, FolderOpen, AlertCircle, Check } from "@lucide/svelte";
  import {
    getStorageType,
    setStorageType,
    isStorageTypeSupported,
    getStorageTypeName,
    getStorageTypeDescription,
    type StorageType,
  } from "$lib/backend/storageType";

  // Current and selected storage type
  let currentType = $state(getStorageType());
  let selectedType = $state(getStorageType());
  let needsRestart = $derived(selectedType !== currentType);
  let isChanging = $state(false);

  // Storage options with icons
  const storageOptions: { type: StorageType; icon: typeof HardDrive }[] = [
    { type: 'opfs', icon: HardDrive },
    { type: 'indexeddb', icon: Database },
    { type: 'filesystem-access', icon: FolderOpen },
  ];

  function handleSelect(type: StorageType) {
    if (isStorageTypeSupported(type)) {
      selectedType = type;
    }
  }

  function applyChange() {
    if (selectedType !== currentType) {
      isChanging = true;
      setStorageType(selectedType);
      
      // Reload the page to use new storage
      setTimeout(() => {
        window.location.reload();
      }, 100);
    }
  }

  function cancelChange() {
    selectedType = currentType;
  }
</script>

<div class="space-y-3">
  <h3 class="font-medium flex items-center gap-2">
    <HardDrive class="size-4" />
    Storage
  </h3>

  <p class="text-xs text-muted-foreground px-1">
    Choose where to store your workspace data.
  </p>

  <!-- Storage Options -->
  <div class="space-y-2 px-1">
    {#each storageOptions as option}
      {@const isSupported = isStorageTypeSupported(option.type)}
      {@const isSelected = selectedType === option.type}
      {@const isCurrent = currentType === option.type}
      
      <button
        type="button"
        class="w-full flex items-start gap-3 p-3 rounded-lg border text-left transition-colors
          {isSelected ? 'border-primary bg-primary/5' : 'border-border hover:border-primary/50'}
          {!isSupported ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}"
        disabled={!isSupported}
        onclick={() => handleSelect(option.type)}
      >
        <div class="mt-0.5">
          <option.icon class="size-5 {isSelected ? 'text-primary' : 'text-muted-foreground'}" />
        </div>
        <div class="flex-1 min-w-0">
          <div class="flex items-center gap-2">
            <span class="font-medium text-sm">{getStorageTypeName(option.type)}</span>
            {#if isCurrent}
              <span class="text-xs text-primary bg-primary/10 px-1.5 py-0.5 rounded">Current</span>
            {/if}
            {#if !isSupported}
              <span class="text-xs text-muted-foreground">(Not supported)</span>
            {/if}
          </div>
          <p class="text-xs text-muted-foreground mt-0.5">
            {getStorageTypeDescription(option.type)}
          </p>
        </div>
        {#if isSelected && isSupported}
          <Check class="size-4 text-primary mt-0.5" />
        {/if}
      </button>
    {/each}
  </div>

  <!-- Change Confirmation -->
  {#if needsRestart}
    <div class="flex items-start gap-2 p-3 rounded-lg bg-amber-500/10 border border-amber-500/30">
      <AlertCircle class="size-4 text-amber-600 mt-0.5 shrink-0" />
      <div class="flex-1 min-w-0">
        <p class="text-sm text-amber-700 dark:text-amber-400">
          Switching storage will reload the app. Your data will remain in the previous storage location.
        </p>
        <div class="flex gap-2 mt-2">
          <Button size="sm" onclick={applyChange} disabled={isChanging}>
            {#if isChanging}
              Switching...
            {:else}
              Switch Storage
            {/if}
          </Button>
          <Button size="sm" variant="ghost" onclick={cancelChange} disabled={isChanging}>
            Cancel
          </Button>
        </div>
      </div>
    </div>
  {/if}
</div>
