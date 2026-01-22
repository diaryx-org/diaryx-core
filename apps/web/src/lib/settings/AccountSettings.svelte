<script lang="ts">
  /**
   * AccountSettings - Account management settings
   *
   * Features:
   * - Account info (email)
   * - Device management
   * - Delete server data
   * - Logout
   */
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import { Separator } from "$lib/components/ui/separator";
  import * as Dialog from "$lib/components/ui/dialog";
  import {
    User,
    Loader2,
    LogOut,
    Smartphone,
    Trash2,
    RefreshCw,
    AlertCircle,
    AlertTriangle,
  } from "@lucide/svelte";
  import {
    getAuthState,
    logout,
    deleteDevice,
    deleteAccount,
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
  let isDeleting = $state(false);
  let showDeleteConfirm = $state(false);
  let error = $state<string | null>(null);

  // Get auth state reactively
  let authState = $derived(getAuthState());

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

  // Delete account
  async function handleDeleteAccount() {
    isDeleting = true;
    error = null;
    try {
      await deleteAccount();
      showDeleteConfirm = false;
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to delete account";
    } finally {
      isDeleting = false;
    }
  }
</script>

<div class="space-y-4">
  <!-- Header -->
  <h3 class="font-medium flex items-center gap-2">
    <User class="size-4" />
    Account
  </h3>

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
      <!-- Email -->
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-2">
          <User class="size-4 text-muted-foreground" />
          <span class="text-sm font-medium">{authState.user.email}</span>
        </div>
        <Button variant="ghost" size="sm" onclick={handleRefresh}>
          <RefreshCw class="size-4" />
        </Button>
      </div>

      <Separator />

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

        <Separator />
      {/if}

      <!-- Account Actions -->
      <div class="space-y-2">
        <Label class="text-xs text-muted-foreground">Actions</Label>

        <Button
          variant="outline"
          size="sm"
          class="w-full justify-start"
          onclick={handleLogout}
          disabled={isLoggingOut}
        >
          {#if isLoggingOut}
            <Loader2 class="size-4 mr-2 animate-spin" />
          {:else}
            <LogOut class="size-4 mr-2" />
          {/if}
          Sign Out
        </Button>

        <Button
          variant="destructive"
          size="sm"
          class="w-full justify-start"
          onclick={() => (showDeleteConfirm = true)}
        >
          <Trash2 class="size-4 mr-2" />
          Delete All Server Data
        </Button>
      </div>

      <p class="text-xs text-muted-foreground">
        Deleting server data will remove all synced data from our servers but keep your local files.
      </p>
    </div>
  {:else}
    <!-- Not Authenticated -->
    <div class="space-y-4">
      <p class="text-sm text-muted-foreground">
        Sign in to sync your workspace across devices.
      </p>
      {#if onOpenWizard}
        <Button
          variant="default"
          size="sm"
          class="w-full"
          onclick={onOpenWizard}
        >
          <User class="size-4 mr-2" />
          Sign In
        </Button>
      {/if}
    </div>
  {/if}
</div>

<!-- Delete Confirmation Dialog -->
<Dialog.Root bind:open={showDeleteConfirm}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2 text-destructive">
        <AlertTriangle class="size-5" />
        Delete All Server Data
      </Dialog.Title>
      <Dialog.Description>
        This will permanently delete all your data from our sync servers, including:
      </Dialog.Description>
    </Dialog.Header>

    <ul class="list-disc list-inside text-sm text-muted-foreground space-y-1 py-2">
      <li>Your synced workspace data</li>
      <li>All linked devices</li>
      <li>Your account information</li>
    </ul>

    <p class="text-sm font-medium">
      Your local files will NOT be deleted.
    </p>

    <Dialog.Footer class="gap-2 sm:gap-0">
      <Button variant="outline" onclick={() => (showDeleteConfirm = false)}>
        Cancel
      </Button>
      <Button
        variant="destructive"
        onclick={handleDeleteAccount}
        disabled={isDeleting}
      >
        {#if isDeleting}
          <Loader2 class="size-4 mr-2 animate-spin" />
        {/if}
        Delete Everything
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
