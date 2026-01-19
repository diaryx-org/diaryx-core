<script lang="ts">
  /**
   * SyncStatusIndicator - Shows current sync status with visual feedback
   *
   * Displays a small indicator showing:
   * - Connected & synced (green dot)
   * - Syncing (yellow dot with animation)
   * - Disconnected/Error (red dot)
   * - Not configured (gray dot)
   *
   * Click opens Settings dialog on the Sync tab.
   */
  import { Button } from "$lib/components/ui/button";
  import * as Popover from "$lib/components/ui/popover";
  import { Progress } from "$lib/components/ui/progress";
  import { collaborationStore, type SyncStatus } from "@/models/stores/collaborationStore.svelte";
  import { getAuthState } from "$lib/auth";
  import {
    Cloud,
    CloudOff,
    RefreshCw,
    AlertCircle,
    CheckCircle,
  } from "@lucide/svelte";

  interface Props {
    onOpenSettings?: () => void;
  }

  let { onOpenSettings }: Props = $props();

  // Reactive state from stores
  let syncStatus = $derived(collaborationStore.syncStatus);
  let syncProgress = $derived(collaborationStore.syncProgress);
  let syncError = $derived(collaborationStore.syncError);
  let authState = $derived(getAuthState());

  // Status display config
  const statusConfig: Record<SyncStatus, {
    icon: typeof Cloud;
    color: string;
    dotColor: string;
    label: string;
    animate?: boolean;
  }> = {
    'not_configured': {
      icon: CloudOff,
      color: 'text-muted-foreground',
      dotColor: 'bg-muted-foreground',
      label: 'Sync not configured',
    },
    'idle': {
      icon: Cloud,
      color: 'text-muted-foreground',
      dotColor: 'bg-muted-foreground',
      label: 'Sync idle',
    },
    'connecting': {
      icon: RefreshCw,
      color: 'text-amber-500',
      dotColor: 'bg-amber-500',
      label: 'Connecting...',
      animate: true,
    },
    'syncing': {
      icon: RefreshCw,
      color: 'text-amber-500',
      dotColor: 'bg-amber-500',
      label: 'Syncing...',
      animate: true,
    },
    'synced': {
      icon: CheckCircle,
      color: 'text-green-500',
      dotColor: 'bg-green-500',
      label: 'Synced',
    },
    'error': {
      icon: AlertCircle,
      color: 'text-destructive',
      dotColor: 'bg-destructive',
      label: 'Sync error',
    },
  };

  let config = $derived(statusConfig[syncStatus]);
  let StatusIcon = $derived(config.icon);

  // Show progress percentage if syncing
  let progressPercent = $derived(
    syncProgress && syncProgress.total > 0
      ? Math.round((syncProgress.completed / syncProgress.total) * 100)
      : null
  );
</script>

<Popover.Root>
  <Popover.Trigger>
    <Button
      variant="ghost"
      size="sm"
      class="h-8 gap-1.5 px-2 {config.color}"
      aria-label="Sync status"
    >
      <!-- Status dot -->
      <span
        class="relative flex h-2 w-2"
      >
        {#if config.animate}
          <span class="animate-ping absolute inline-flex h-full w-full rounded-full {config.dotColor} opacity-75"></span>
        {/if}
        <span class="relative inline-flex rounded-full h-2 w-2 {config.dotColor}"></span>
      </span>

      <!-- Icon -->
      <svelte:component
        this={StatusIcon}
        class="size-4 {config.animate ? 'animate-spin' : ''}"
      />

      <!-- Label (hidden on mobile) -->
      <span class="hidden sm:inline text-xs">
        {#if syncStatus === 'syncing' && progressPercent !== null}
          {progressPercent}%
        {:else if authState.isAuthenticated}
          Synced
        {:else}
          Sync
        {/if}
      </span>
    </Button>
  </Popover.Trigger>

  <Popover.Content class="w-64 p-3" align="end">
    <div class="space-y-3">
      <!-- Status header -->
      <div class="flex items-center gap-2">
        <svelte:component
          this={StatusIcon}
          class="size-5 {config.color} {config.animate ? 'animate-spin' : ''}"
        />
        <span class="font-medium text-sm">{config.label}</span>
      </div>

      <!-- Progress bar when syncing -->
      {#if syncStatus === 'syncing' && syncProgress}
        <div class="space-y-1">
          <Progress value={progressPercent || 0} class="h-2" />
          <p class="text-xs text-muted-foreground">
            {syncProgress.completed} of {syncProgress.total} files
          </p>
        </div>
      {/if}

      <!-- Error message -->
      {#if syncError}
        <p class="text-xs text-destructive bg-destructive/10 p-2 rounded-md">
          {syncError}
        </p>
      {/if}

      <!-- Account info when authenticated -->
      {#if authState.isAuthenticated && authState.user}
        <div class="text-xs text-muted-foreground border-t pt-2">
          <p>Signed in as <strong>{authState.user.email}</strong></p>
          <p class="mt-1">{authState.devices.length} device(s) connected</p>
        </div>
      {:else if syncStatus === 'not_configured'}
        <p class="text-xs text-muted-foreground">
          Set up sync to access your notes from any device.
        </p>
      {/if}

      <!-- Action button -->
      <Button
        variant="outline"
        size="sm"
        class="w-full text-xs"
        onclick={onOpenSettings}
      >
        {#if authState.isAuthenticated}
          Manage sync settings
        {:else}
          Set up sync
        {/if}
      </Button>
    </div>
  </Popover.Content>
</Popover.Root>
