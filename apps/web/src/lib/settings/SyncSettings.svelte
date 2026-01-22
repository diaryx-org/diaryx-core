<script lang="ts">
  /**
   * SyncSettings - Multi-device sync status and setup
   *
   * Features:
   * - Button to open sync setup wizard (when not authenticated)
   * - Sync status display (when authenticated)
   */
  import { Button } from "$lib/components/ui/button";
  import { Server, Wifi, WifiOff, Loader2 } from "@lucide/svelte";
  import { getAuthState, initAuth } from "$lib/auth";
  import { collaborationStore } from "@/models/stores/collaborationStore.svelte";
  import { onMount } from "svelte";

  interface Props {
    /** Callback to open the sync setup wizard */
    onOpenWizard?: () => void;
  }

  let { onOpenWizard }: Props = $props();

  // Get auth state reactively
  let authState = $derived(getAuthState());

  // Get collaboration state
  let syncStatus = $derived(collaborationStore.syncStatus);
  let isEnabled = $derived(collaborationStore.collaborationEnabled);

  // Get server URL for display
  let serverUrl = $derived(
    typeof window !== "undefined"
      ? localStorage.getItem("diaryx_sync_server_url") || ""
      : ""
  );

  // Initialize auth on mount
  onMount(() => {
    initAuth();
  });
</script>

<div class="space-y-4">
  <!-- Header -->
  <h3 class="font-medium flex items-center gap-2">
    <Server class="size-4" />
    Multi-Device Sync
  </h3>

  <p class="text-xs text-muted-foreground px-1">
    Sync your workspace across devices with our cloud server.
  </p>

  {#if authState.isAuthenticated}
    <!-- Authenticated: Show sync status -->
    <div class="space-y-3">
      <!-- Server URL -->
      {#if serverUrl}
        <div class="text-xs text-muted-foreground px-1">
          Server: <span class="font-mono">{serverUrl}</span>
        </div>
      {/if}

      <!-- Sync Status -->
      <div class="flex items-center gap-2 p-2 bg-muted/50 rounded-md">
        {#if syncStatus === "syncing" || syncStatus === "connecting"}
          <Loader2 class="size-4 text-primary animate-spin" />
          <span class="text-sm">{syncStatus === "connecting" ? "Connecting..." : "Syncing..."}</span>
        {:else if syncStatus === "synced" || (isEnabled && syncStatus === "idle")}
          <Wifi class="size-4 text-green-500" />
          <span class="text-sm text-green-600 dark:text-green-400">Connected</span>
        {:else if syncStatus === "error"}
          <WifiOff class="size-4 text-destructive" />
          <span class="text-sm text-destructive">Connection Error</span>
        {:else}
          <WifiOff class="size-4 text-muted-foreground" />
          <span class="text-sm text-muted-foreground">Not Connected</span>
        {/if}
      </div>

      <p class="text-xs text-muted-foreground">
        Your workspace is syncing across all your devices. Manage your account in the Account tab.
      </p>
    </div>
  {:else}
    <!-- Not Authenticated: Show setup button -->
    {#if onOpenWizard}
      <Button
        variant="default"
        size="sm"
        class="w-full"
        onclick={onOpenWizard}
      >
        <Server class="size-4 mr-2" />
        Set Up Sync
      </Button>
      <p class="text-xs text-muted-foreground px-1">
        Sign in with your email to sync your notes across devices.
      </p>
    {/if}
  {/if}
</div>
