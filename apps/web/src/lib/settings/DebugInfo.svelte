<script lang="ts">
  /**
   * DebugInfo - App paths and config debug information
   *
   * Extracted from SettingsDialog for modularity.
   */
  import { Info, Settings } from "@lucide/svelte";
  import { getBackend } from "../backend";

  // Config info state
  let config: Record<string, unknown> | null = $state(null);
  let appPaths: Record<string, string> | null = $state(null);

  // Load config on mount
  $effect(() => {
    loadConfig();
  });

  async function loadConfig() {
    try {
      const backend = await getBackend();
      if ("getInvoke" in backend) {
        const invoke = (backend as any).getInvoke();
        config = await invoke("get_config", {});
        appPaths = await invoke("get_app_paths", {});
      }
    } catch (e) {
      config = null;
      appPaths = null;
    }
  }
</script>

<!-- Path Info -->
{#if appPaths}
  <div class="space-y-2">
    <h3 class="font-medium flex items-center gap-2">
      <Info class="size-4" />App Paths
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
      <Settings class="size-4" />Config
    </h3>
    <div class="bg-muted rounded p-3 text-xs font-mono space-y-1">
      {#each Object.entries(config) as [key, value]}
        <div class="flex gap-2">
          <span class="text-muted-foreground min-w-[120px]">{key}:</span>
          <span class="break-all"
            >{typeof value === "object"
              ? JSON.stringify(value)
              : String(value ?? "null")}</span
          >
        </div>
      {/each}
    </div>
  </div>
{/if}
