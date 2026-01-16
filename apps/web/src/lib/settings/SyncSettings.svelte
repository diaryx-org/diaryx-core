<script lang="ts">
  /**
   * SyncSettings - Collaboration server configuration
   *
   * Provides a simple way to configure a custom sync server URL
   * for live collaboration features.
   */
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import { Input } from "$lib/components/ui/input";
  import {
    Server,
    Loader2,
    Check,
  } from "@lucide/svelte";
  import { collaborationStore } from "@/models/stores/collaborationStore.svelte";

  // Server URL state
  let syncServerUrl = $state(
    typeof window !== "undefined"
      ? localStorage.getItem("diaryx-collab-server-url") || ""
      : ""
  );
  let isApplyingServer = $state(false);
  let applied = $state(false);

  // Apply server URL
  async function applySyncServer() {
    let url = syncServerUrl.trim();

    if (url) {
      // Auto-prefix with wss:// if no protocol specified
      if (!url.startsWith("ws://") && !url.startsWith("wss://")) {
        url = "wss://" + url;
        syncServerUrl = url;
      }

      isApplyingServer = true;

      // Store in localStorage and collaborationStore
      if (typeof window !== "undefined") {
        localStorage.setItem("diaryx-collab-server-url", url);
      }
      collaborationStore.setServerUrl(url);

      // Show success feedback
      setTimeout(() => {
        isApplyingServer = false;
        applied = true;
        setTimeout(() => {
          applied = false;
        }, 2000);
      }, 500);
    }
  }

  // Clear server URL
  function clearSyncServer() {
    syncServerUrl = "";
    if (typeof window !== "undefined") {
      localStorage.removeItem("diaryx-collab-server-url");
    }
    collaborationStore.setServerUrl(null);
  }
</script>

<div class="space-y-3">
  <!-- Header -->
  <h3 class="font-medium flex items-center gap-2">
    <Server class="size-4" />
    Collaboration Server
  </h3>

  <p class="text-xs text-muted-foreground px-1">
    Configure a custom sync server for live collaboration. Leave empty to use the default server.
  </p>

  <!-- Server URL -->
  <div class="space-y-2 px-1">
    <Label for="sync-server" class="text-xs text-muted-foreground">
      Server URL
    </Label>
    <div class="flex gap-2">
      <Input
        id="sync-server"
        type="text"
        bind:value={syncServerUrl}
        placeholder="wss://sync.example.com or ws://localhost:1234"
        class="text-sm"
        onkeydown={(e) => e.key === "Enter" && applySyncServer()}
      />
      <Button
        variant="secondary"
        size="sm"
        onclick={applySyncServer}
        disabled={isApplyingServer || !syncServerUrl.trim()}
      >
        {#if isApplyingServer}
          <Loader2 class="size-4 animate-spin" />
        {:else if applied}
          <Check class="size-4 text-green-600" />
        {:else}
          Apply
        {/if}
      </Button>
    </div>

    {#if syncServerUrl}
      <Button
        variant="ghost"
        size="sm"
        class="text-xs text-muted-foreground h-7"
        onclick={clearSyncServer}
      >
        Clear server URL
      </Button>
    {/if}
  </div>
</div>
