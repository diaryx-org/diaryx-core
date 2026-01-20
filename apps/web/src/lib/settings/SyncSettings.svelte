<script lang="ts">
  /**
   * SyncSettings - Multi-device sync settings
   *
   * Features:
   * - Button to open sync setup wizard (when not authenticated)
   * - Account management (when logged in)
   * - Device management
   */
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import { Separator } from "$lib/components/ui/separator";
  import {
    Server,
    Loader2,
    LogOut,
    User,
    Smartphone,
    Trash2,
    RefreshCw,
    AlertCircle,
  } from "@lucide/svelte";
  import {
    getAuthState,
    logout,
    deleteDevice,
    refreshUserInfo,
    initAuth,
  } from "$lib/auth";
  import { onMount } from "svelte";

  interface Props {
    /** Callback to open the sync setup wizard */
    onOpenWizard?: () => void;
  }

  let { onOpenWizard }: Props = $props();

  // State
  let isLoggingOut = $state(false);
  let error = $state<string | null>(null);

  // Get auth state reactively
  let authState = $derived(getAuthState());

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

  // Logout
  async function handleLogout() {
    isLoggingOut = true;
    try {
      await logout();
    } finally {
      isLoggingOut = false;
    }
  }

  // Delete device
  async function handleDeleteDevice(deviceId: string) {
    try {
      await deleteDevice(deviceId);
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to delete device";
    }
  }

  // Refresh user info
  async function handleRefresh() {
    try {
      await refreshUserInfo();
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to refresh";
    }
  }
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

  <!-- Error Message -->
  {#if error}
    <div
      class="flex items-center gap-2 text-destructive text-sm p-2 bg-destructive/10 rounded-md"
    >
      <AlertCircle class="size-4 shrink-0" />
      <span>{error}</span>
    </div>
  {/if}

  {#if authState.isAuthenticated && authState.user}
    <!-- Logged In State -->
    <div class="space-y-4">
      <!-- Server URL (read-only) -->
      {#if serverUrl}
        <div class="text-xs text-muted-foreground px-1">
          Connected to: <span class="font-mono">{serverUrl}</span>
        </div>
      {/if}

      <Separator />

      <div class="flex items-center justify-between">
        <div class="flex items-center gap-2">
          <User class="size-4 text-muted-foreground" />
          <span class="text-sm font-medium">{authState.user.email}</span>
        </div>
        <div class="flex gap-2">
          <Button variant="ghost" size="sm" onclick={handleRefresh}>
            <RefreshCw class="size-4" />
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onclick={handleLogout}
            disabled={isLoggingOut}
          >
            {#if isLoggingOut}
              <Loader2 class="size-4 animate-spin" />
            {:else}
              <LogOut class="size-4" />
            {/if}
          </Button>
        </div>
      </div>

      <!-- Devices -->
      {#if authState.devices.length > 0}
        <div class="space-y-2">
          <Label class="text-xs text-muted-foreground">Your Devices</Label>
          <div class="space-y-1">
            {#each authState.devices as device}
              <div
                class="flex items-center justify-between text-sm p-2 bg-muted/50 rounded-md"
              >
                <div class="flex items-center gap-2">
                  <Smartphone class="size-4 text-muted-foreground" />
                  <span>{device.name || "Unknown Device"}</span>
                </div>
                <Button
                  variant="ghost"
                  size="sm"
                  class="h-7 w-7 p-0"
                  onclick={() => handleDeleteDevice(device.id)}
                >
                  <Trash2 class="size-3 text-muted-foreground" />
                </Button>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      <p class="text-xs text-muted-foreground">
        Your workspace is syncing across all your devices.
      </p>
    </div>
  {:else}
    <!-- Not Authenticated - Show Setup Button -->
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
