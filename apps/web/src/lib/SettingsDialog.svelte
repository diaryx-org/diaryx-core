<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Settings, Info } from "@lucide/svelte";
  import { getBackend } from "./backend";

  interface Props {
    open: boolean;
  }

  let { open = $bindable() }: Props = $props();
  
  // Config info state
  let config: Record<string, unknown> | null = $state(null);
  let appPaths: Record<string, string> | null = $state(null);
  let loadError: string | null = $state(null);
  
  // Load config when dialog opens
  $effect(() => {
    if (open) {
      loadConfig();
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
      
      {#if !config && !appPaths && !loadError}
        <p class="text-muted-foreground text-center">Loading config...</p>
      {/if}
    </div>
    
    <Dialog.Footer>
      <Button variant="outline" onclick={() => open = false}>
        Close
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
