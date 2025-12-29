<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Switch } from "$lib/components/ui/switch";
  import { Label } from "$lib/components/ui/label";
  import { Settings, Info, Eye, Save, Check, AlertCircle } from "@lucide/svelte";
  import { getBackend } from "./backend";
  import type { BackupStatus } from "./backend/interface";

  interface Props {
    open: boolean;
    showUnlinkedFiles: boolean;
    showHiddenFiles?: boolean;
  }

  let { open = $bindable(), showUnlinkedFiles = $bindable(), showHiddenFiles = $bindable(false) }: Props = $props();
  
  // Config info state
  let config: Record<string, unknown> | null = $state(null);
  let appPaths: Record<string, string> | null = $state(null);
  let loadError: string | null = $state(null);
  
  // Backup state
  let backupTargets: string[] = $state([]);
  let backupStatus: BackupStatus[] | null = $state(null);
  let isBackingUp: boolean = $state(false);
  let backupError: string | null = $state(null);
  
  // Load config when dialog opens
  $effect(() => {
    if (open) {
      loadConfig();
      loadBackupTargets();
    }
  });
  
  async function loadConfig() {
    try {
      const backend = await getBackend();
      // Try to get config if available
      if ('getConfig' in backend && typeof backend.getConfig === 'function') {
        config = await (backend as any).getConfig();
      }
      // Try to get app paths if Tauri
      if ('getInvoke' in backend) {
        try {
          appPaths = await (backend as any).getInvoke()("get_app_paths", {});
        } catch (e) {
          // get_app_paths may not be available
        }
      }
      loadError = null;
    } catch (e) {
      loadError = e instanceof Error ? e.message : String(e);
    }
  }
  
  async function loadBackupTargets() {
    try {
      const backend = await getBackend();
      if ('getInvoke' in backend) {
        const invoke = (backend as any).getInvoke();
        backupTargets = await invoke("list_backup_targets", {});
      } else {
        // WASM backend - IndexedDB is implicit target
        backupTargets = ["IndexedDB (Local)"];
      }
    } catch (e) {
      backupTargets = [];
    }
  }
  
  async function performBackup() {
    isBackingUp = true;
    backupError = null;
    backupStatus = null;
    
    try {
      const backend = await getBackend();
      if ('getInvoke' in backend) {
        // Tauri backend
        const invoke = (backend as any).getInvoke();
        backupStatus = await invoke("backup_workspace", {});
      } else {
        // WASM backend - persist to IndexedDB
        await backend.persist();
        backupStatus = [{
          target_name: "IndexedDB (Local)",
          success: true,
          files_processed: 0, // We don't have exact count from persist()
          error: undefined,
        }];
      }
    } catch (e) {
      backupError = e instanceof Error ? e.message : String(e);
    } finally {
      isBackingUp = false;
    }
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="sm:max-w-[500px] max-h-[80vh] overflow-y-auto">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        <Settings class="size-5" />
        Settings
      </Dialog.Title>
      <Dialog.Description>
        Configuration and debugging information.
      </Dialog.Description>
    </Dialog.Header>
    
    <div class="py-4 space-y-4">
      <!-- Display Settings -->
      <div class="space-y-3">
        <h3 class="font-medium flex items-center gap-2">
          <Eye class="size-4" />
          Display
        </h3>
        
        <div class="flex items-center justify-between gap-4 px-1">
          <Label for="show-unlinked" class="text-sm cursor-pointer flex flex-col gap-0.5">
            <span>Show all files</span>
            <span class="font-normal text-xs text-muted-foreground">
              Switch to a filesystem view to see files not linked in hierarchy.
            </span>
          </Label>
          <Switch id="show-unlinked" bind:checked={showUnlinkedFiles} />
        </div>

        <div class="flex items-center justify-between gap-4 px-1">
          <Label for="show-hidden" class="text-sm cursor-pointer flex flex-col gap-0.5">
            <span>Show hidden files</span>
            <span class="font-normal text-xs text-muted-foreground">
              Show files starting with dot (.git, .DS_Store) in filesystem view.
            </span>
          </Label>
          <Switch id="show-hidden" bind:checked={showHiddenFiles} disabled={!showUnlinkedFiles} />
        </div>
      </div>

      <!-- Backup Section -->
      <div class="space-y-3">
        <h3 class="font-medium flex items-center gap-2">
          <Save class="size-4" />
          Backup
        </h3>
        
        {#if backupTargets.length > 0}
          <div class="text-sm text-muted-foreground px-1">
            Configured targets: {backupTargets.join(", ")}
          </div>
        {/if}
        
        <div class="flex items-center gap-2 px-1">
          <Button 
            variant="outline" 
            size="sm"
            onclick={performBackup}
            disabled={isBackingUp}
          >
            {isBackingUp ? 'Backing up...' : 'Backup Now'}
          </Button>
        </div>
        
        {#if backupStatus}
          <div class="space-y-1 px-1">
            {#each backupStatus as status}
              <div class="flex items-center gap-2 text-sm">
                {#if status.success}
                  <Check class="size-4 text-green-500" />
                  <span>{status.target_name}: {status.files_processed} files</span>
                {:else}
                  <AlertCircle class="size-4 text-destructive" />
                  <span class="text-destructive">{status.target_name}: {status.error}</span>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
        
        {#if backupError}
          <div class="text-destructive text-sm p-2 bg-destructive/10 rounded">
            Backup failed: {backupError}
          </div>
        {/if}
      </div>

      {#if loadError}
        <div class="text-destructive text-sm p-2 bg-destructive/10 rounded">
          Error loading config: {loadError}
        </div>
      {/if}
      
      {#if appPaths}
        <div class="space-y-2">
          <h3 class="font-medium flex items-center gap-2">
            <Info class="size-4" />
            App Paths
          </h3>
          <div class="bg-muted rounded p-3 text-xs font-mono space-y-1">
            {#each Object.entries(appPaths) as [key, value]}
              <div class="flex gap-2">
                <span class="text-muted-foreground min-w-[120px]">{key}:</span>
                <span class="break-all">{value}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}
      
      {#if config}
        <div class="space-y-2">
          <h3 class="font-medium flex items-center gap-2">
            <Settings class="size-4" />
            Config
          </h3>
          <div class="bg-muted rounded p-3 text-xs font-mono space-y-1">
            {#each Object.entries(config) as [key, value]}
              <div class="flex gap-2">
                <span class="text-muted-foreground min-w-[120px]">{key}:</span>
                <span class="break-all">{typeof value === 'object' ? JSON.stringify(value) : String(value ?? 'null')}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}
      
      <div class="flex justify-end pt-2">
        <Button variant="outline" onclick={() => open = false}>Close</Button>
      </div>
    </div>
  </Dialog.Content>
</Dialog.Root>

