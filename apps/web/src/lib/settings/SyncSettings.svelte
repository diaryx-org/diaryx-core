<script lang="ts">
  /**
   * SyncSettings - Collaboration and sync server settings
   * 
   * Extracted from SettingsDialog for modularity.
   */
  import { Button } from "$lib/components/ui/button";
  import { Switch } from "$lib/components/ui/switch";
  import { Label } from "$lib/components/ui/label";
  import { Input } from "$lib/components/ui/input";
  import { Wifi, WifiOff, RefreshCw, Loader2, Trash2, Check, AlertCircle } from "@lucide/svelte";
  import {
    setCollaborationServer,
    getCollaborationServer,
    clearAllDocumentCache,
  } from "../collaborationUtils";
  import { setWorkspaceServer } from "../workspaceCrdt";

  interface Props {
    collaborationEnabled?: boolean;
    collaborationConnected?: boolean;
    onCollaborationToggle?: (enabled: boolean) => void;
    onCollaborationReconnect?: () => void;
  }

  let {
    collaborationEnabled = $bindable(false),
    collaborationConnected = false,
    onCollaborationToggle,
    onCollaborationReconnect,
  }: Props = $props();

  // Server URL state
  let syncServerUrl = $state(
    typeof window !== "undefined"
      ? getCollaborationServer() || ""
      : "",
  );
  let isApplyingServer = $state(false);
  let isClearingCache = $state(false);

  function applySyncServer() {
    const url = syncServerUrl.trim();
    if (url) {
      // Validate URL format
      try {
        new URL(url);
      } catch {
        // Allow partial URLs like "localhost:1234" by prepending ws://
        if (!url.startsWith("ws://") && !url.startsWith("wss://")) {
          syncServerUrl = "wss://" + url;
        }
      }
      isApplyingServer = true;
      setCollaborationServer(syncServerUrl);
      setWorkspaceServer(syncServerUrl);
      // Save to localStorage
      if (typeof window !== "undefined") {
        localStorage.setItem("diaryx-collab-server-url", syncServerUrl);
      }
      // Enable collaboration
      if (onCollaborationToggle) {
        onCollaborationToggle(true);
      }
      setTimeout(() => {
        isApplyingServer = false;
      }, 1000);
    }
  }

  function clearSyncServer() {
    syncServerUrl = "";
    if (typeof window !== "undefined") {
      localStorage.removeItem("diaryx-collab-server-url");
    }
    setCollaborationServer("ws://localhost:1234");
    setWorkspaceServer(null);
    // Disable collaboration when clearing server
    if (onCollaborationToggle) {
      onCollaborationToggle(false);
    }
  }

  async function handleClearDocumentCache() {
    isClearingCache = true;
    try {
      const count = await clearAllDocumentCache();
      console.log(`[SyncSettings] Cleared ${count} document caches`);
    } catch (e) {
      console.error("Failed to clear document cache:", e);
    } finally {
      isClearingCache = false;
    }
  }
</script>

<div class="space-y-3">
  <h3 class="font-medium flex items-center gap-2">
    {#if collaborationConnected}
      <Wifi class="size-4 text-green-500" />
    {:else}
      <WifiOff class="size-4 text-muted-foreground" />
    {/if}
    Live Sync
  </h3>

  <!-- Sync Toggle -->
  <div class="flex items-center justify-between gap-4 px-1">
    <Label for="collab-toggle" class="text-sm cursor-pointer flex flex-col gap-0.5">
      <span>Enable Live Sync</span>
      <span class="font-normal text-xs text-muted-foreground">
        Sync changes in real-time across devices.
      </span>
    </Label>
    <div class="flex items-center gap-2">
      {#if collaborationEnabled && !collaborationConnected}
        <Button
          variant="ghost"
          size="icon"
          class="size-7"
          onclick={onCollaborationReconnect}
          title="Reconnect"
        >
          <RefreshCw class="size-3" />
        </Button>
      {/if}
      <Switch
        id="collab-toggle"
        checked={collaborationEnabled}
        onCheckedChange={(checked) => onCollaborationToggle?.(checked)}
      />
    </div>
  </div>

  <!-- Sync Status -->
  {#if collaborationEnabled}
    <div class="flex items-center gap-2 px-1 text-sm">
      {#if collaborationConnected}
        <span class="flex items-center gap-1 text-green-600">
          <Check class="size-3" />
          Connected
        </span>
      {:else}
        <span class="flex items-center gap-1 text-amber-600">
          <AlertCircle class="size-3" />
          Connecting...
        </span>
      {/if}
    </div>
  {/if}

  <!-- Server URL -->
  <div class="space-y-2 px-1">
    <Label for="sync-server" class="text-xs text-muted-foreground">
      Sync Server URL
    </Label>
    <div class="flex gap-2">
      <Input
        id="sync-server"
        type="text"
        bind:value={syncServerUrl}
        placeholder="wss://sync.example.com"
        class="text-sm"
      />
      <Button
        variant="secondary"
        size="sm"
        onclick={applySyncServer}
        disabled={isApplyingServer || !syncServerUrl.trim()}
      >
        {#if isApplyingServer}
          <Loader2 class="size-4 animate-spin" />
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

    <Button
      variant="ghost"
      size="sm"
      class="text-xs text-muted-foreground h-7 gap-1"
      onclick={handleClearDocumentCache}
      disabled={isClearingCache}
      title="Clear cached document content to fix sync issues"
    >
      {#if isClearingCache}
        <Loader2 class="size-3 animate-spin" />
      {:else}
        <Trash2 class="size-3" />
      {/if}
      Clear document cache
    </Button>
  </div>
</div>
