<script lang="ts">
  /**
   * SyncSettings - Device sync and collaboration settings
   *
   * Provides two sync modes:
   * 1. P2P Device Sync - Direct peer-to-peer sync between devices (no server required)
   * 2. Server Sync - Traditional Hocuspocus server-based sync (advanced option)
   */
  import { Button } from "$lib/components/ui/button";
  import { Switch } from "$lib/components/ui/switch";
  import { Label } from "$lib/components/ui/label";
  import { Input } from "$lib/components/ui/input";
  import {
    Smartphone,
    Wifi,
    WifiOff,
    RefreshCw,
    Loader2,
    Trash2,
    Check,
    AlertCircle,
    Copy,
    ChevronDown,
    ChevronRight,
    Server,
    Users,
  } from "@lucide/svelte";
  import {
    setCollaborationServer,
    getCollaborationServer,
    releaseAllDocuments,
    refreshP2PProviders,
  } from "$lib/crdt/collaborationBridge";
  import { setWorkspaceServer, refreshWorkspaceP2P, getWorkspaceId } from "$lib/crdt/workspaceCrdtBridge";
  import {
    initP2PSync,
    enableP2PSync,
    disableP2PSync,
    joinWithSyncCode,
    validateSyncCode,
    getP2PState,
    onP2PStatusChange,
    onSyncProgress,
    initiateFileSync,
    isSyncInProgress,
    setConflictHandler,
    type P2PState,
    type SyncProgress,
    type ConflictInfo,
  } from "$lib/crdt/p2pSyncBridge";
  import { onMount, onDestroy } from "svelte";
  import SyncConflictDialog from "./SyncConflictDialog.svelte";

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

  // P2P state
  let p2pState: P2PState = $state(getP2PState());
  let joinCode = $state("");
  let joinError = $state("");
  let isCopied = $state(false);
  let unsubscribeP2P: (() => void) | null = null;
  let unsubscribeSyncProgress: (() => void) | null = null;

  // Sync progress state
  let syncProgress: SyncProgress | null = $state(null);

  // Conflict resolution state
  let pendingConflicts: ConflictInfo[] = $state([]);
  let conflictResolver: ((resolutions: Map<string, 'local' | 'remote' | 'both'>) => void) | null = null;

  // Server URL state (advanced)
  let showAdvanced = $state(false);
  let syncServerUrl = $state(
    typeof window !== "undefined" ? getCollaborationServer() || "" : ""
  );
  let isApplyingServer = $state(false);
  let isClearingCache = $state(false);

  onMount(() => {
    initP2PSync();
    p2pState = getP2PState();

    // Subscribe to P2P status changes
    unsubscribeP2P = onP2PStatusChange((state) => {
      p2pState = state;
    });

    // Subscribe to sync progress
    unsubscribeSyncProgress = onSyncProgress((progress) => {
      syncProgress = progress;
      
      // Clear progress after a delay when complete
      if (progress.phase === 'complete') {
        setTimeout(() => {
          if (syncProgress?.phase === 'complete') {
            syncProgress = null;
          }
        }, 5000);
      }
    });

    // Set up conflict handler
    setConflictHandler(async (conflicts) => {
      return new Promise((resolve) => {
        pendingConflicts = conflicts;
        conflictResolver = resolve;
      });
    });
  });

  onDestroy(() => {
    unsubscribeP2P?.();
    unsubscribeSyncProgress?.();
  });

  // P2P handlers
  async function handleEnableP2P() {
    const workspaceId = getWorkspaceId();
    enableP2PSync(workspaceId ?? undefined);
    await refreshP2PProviders();
    await refreshWorkspaceP2P();
  }

  async function handleDisableP2P() {
    disableP2PSync();
    await refreshP2PProviders();
    await refreshWorkspaceP2P();
  }

  async function handleJoinSync() {
    joinError = "";
    const code = joinCode.trim().toUpperCase();

    if (!validateSyncCode(code)) {
      joinError = "Invalid code format. Expected: XXXXXXXX-XXXXXXXX";
      return;
    }

    try {
      joinWithSyncCode(code);
      enableP2PSync();
      await refreshP2PProviders();
      await refreshWorkspaceP2P();
      joinCode = "";
    } catch (e) {
      joinError = e instanceof Error ? e.message : "Failed to join";
    }
  }

  function handleCopyCode() {
    if (p2pState.syncCode) {
      navigator.clipboard.writeText(p2pState.syncCode);
      isCopied = true;
      setTimeout(() => {
        isCopied = false;
      }, 2000);
    }
  }

  async function handleManualSync() {
    if (isSyncInProgress()) return;
    
    try {
      await initiateFileSync();
    } catch (e) {
      console.error('Manual sync failed:', e);
    }
  }

  // Helper to format sync phase for display
  function formatSyncPhase(phase: SyncProgress['phase']): string {
    switch (phase) {
      case 'comparing': return 'Comparing files...';
      case 'downloading': return 'Downloading files...';
      case 'uploading': return 'Uploading files...';
      case 'complete': return 'Sync complete';
      case 'error': return 'Sync failed';
      default: return '';
    }
  }

  // Conflict resolution handlers
  function handleConflictResolve(resolutions: Map<string, 'local' | 'remote' | 'both'>) {
    if (conflictResolver) {
      conflictResolver(resolutions);
      conflictResolver = null;
      pendingConflicts = [];
    }
  }

  function handleConflictCancel() {
    if (conflictResolver) {
      // Cancel by returning empty resolutions (skip all conflicts)
      conflictResolver(new Map());
      conflictResolver = null;
      pendingConflicts = [];
    }
  }

  // Server sync handlers (advanced)
  async function applySyncServer() {
    const url = syncServerUrl.trim();
    if (url) {
      try {
        new URL(url);
      } catch {
        if (!url.startsWith("ws://") && !url.startsWith("wss://")) {
          syncServerUrl = "wss://" + url;
        }
      }
      isApplyingServer = true;
      setCollaborationServer(syncServerUrl);
      await setWorkspaceServer(syncServerUrl);
      if (typeof window !== "undefined") {
        localStorage.setItem("diaryx-collab-server-url", syncServerUrl);
      }
      onCollaborationToggle?.(true);
      setTimeout(() => {
        isApplyingServer = false;
      }, 1000);
    }
  }

  async function clearSyncServer() {
    syncServerUrl = "";
    if (typeof window !== "undefined") {
      localStorage.removeItem("diaryx-collab-server-url");
    }
    setCollaborationServer(null);
    await setWorkspaceServer(null);
    onCollaborationToggle?.(false);
  }

  async function handleClearDocumentCache() {
    isClearingCache = true;
    try {
      await releaseAllDocuments();
      console.log(`[SyncSettings] Released all document sessions`);
    } catch (e) {
      console.error("Failed to clear document cache:", e);
    } finally {
      isClearingCache = false;
    }
  }


</script>

<div class="space-y-4">
  <!-- Device Sync Header -->
  <h3 class="font-medium flex items-center gap-2">
    <Smartphone class="size-4" />
    Device Sync
  </h3>

  <p class="text-xs text-muted-foreground px-1">
    Sync this workspace across your devices using peer-to-peer connections.
  </p>

  <!-- P2P Sync Section -->
  {#if !p2pState.enabled}
    <!-- Not enabled: show enable/join options -->
    <div class="space-y-3 px-1">
      <Button variant="default" class="w-full" onclick={handleEnableP2P}>
        Enable Device Sync
      </Button>

      <div class="flex items-center gap-2 text-xs text-muted-foreground">
        <div class="flex-1 h-px bg-border"></div>
        <span>or join existing</span>
        <div class="flex-1 h-px bg-border"></div>
      </div>

      <div class="space-y-2">
        <div class="flex gap-2">
          <Input
            type="text"
            bind:value={joinCode}
            placeholder="Enter sync code"
            class="text-sm font-mono uppercase"
            onkeydown={(e) => e.key === "Enter" && handleJoinSync()}
          />
          <Button
            variant="secondary"
            size="sm"
            onclick={handleJoinSync}
            disabled={!joinCode.trim()}
          >
            Join
          </Button>
        </div>
        {#if joinError}
          <p class="text-xs text-destructive">{joinError}</p>
        {/if}
      </div>
    </div>
  {:else}
    <!-- Enabled: show status and sync code -->
    <div class="space-y-3 px-1">
      <!-- Status -->
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-2 text-sm">
          {#if p2pState.status === "connected" && p2pState.connectedPeers > 0}
            <span class="flex items-center gap-1 text-green-600">
              <Users class="size-3" />
              Connected ({p2pState.connectedPeers + 1} devices)
            </span>
          {:else if p2pState.status === "connecting"}
            <span class="flex items-center gap-1 text-amber-600">
              <Loader2 class="size-3 animate-spin" />
              Connecting...
            </span>
          {:else}
            <span class="flex items-center gap-1 text-muted-foreground">
              <AlertCircle class="size-3" />
              Ready to connect
            </span>
          {/if}
        </div>
        <div class="flex items-center gap-1">
          {#if p2pState.status === "connected" && p2pState.connectedPeers > 0}
            <Button
              variant="ghost"
              size="sm"
              class="text-xs h-7"
              onclick={handleManualSync}
              disabled={syncProgress !== null && syncProgress.phase !== 'idle' && syncProgress.phase !== 'complete' && syncProgress.phase !== 'error'}
            >
              {#if syncProgress && syncProgress.phase !== 'idle' && syncProgress.phase !== 'complete' && syncProgress.phase !== 'error'}
                <Loader2 class="size-3 animate-spin mr-1" />
              {:else}
                <RefreshCw class="size-3 mr-1" />
              {/if}
              Sync Files
            </Button>
          {/if}
          <Button
            variant="ghost"
            size="sm"
            class="text-xs text-destructive h-7"
            onclick={handleDisableP2P}
          >
            Disable
          </Button>
        </div>
      </div>

      <!-- Sync Progress -->
      {#if syncProgress && syncProgress.phase !== 'idle'}
        <div class="rounded-md border bg-muted/30 p-3 space-y-2">
          <div class="flex items-center justify-between text-sm">
            <span class="text-muted-foreground">{formatSyncPhase(syncProgress.phase)}</span>
            {#if syncProgress.filesTotal > 0}
              <span class="text-xs font-mono">
                {syncProgress.filesComplete}/{syncProgress.filesTotal}
              </span>
            {/if}
          </div>
          
          {#if syncProgress.filesTotal > 0 && syncProgress.phase !== 'complete' && syncProgress.phase !== 'error'}
            <!-- Progress bar -->
            <div class="w-full h-1.5 bg-muted rounded-full overflow-hidden">
              <div 
                class="h-full bg-primary transition-all duration-300"
                style="width: {Math.round((syncProgress.filesComplete / syncProgress.filesTotal) * 100)}%"
              ></div>
            </div>
          {/if}

          {#if syncProgress.currentFile}
            <p class="text-xs text-muted-foreground truncate">
              {syncProgress.currentFile}
            </p>
          {/if}

          {#if syncProgress.phase === 'complete'}
            <p class="text-xs text-green-600 flex items-center gap-1">
              <Check class="size-3" />
              Files synchronized successfully
            </p>
          {/if}

          {#if syncProgress.phase === 'error'}
            <p class="text-xs text-destructive flex items-center gap-1">
              <AlertCircle class="size-3" />
              Sync encountered errors
            </p>
          {/if}
        </div>
      {/if}

      <!-- Sync Code -->
      {#if p2pState.syncCode}
        <div class="space-y-1">
          <Label class="text-xs text-muted-foreground">Your Sync Code</Label>
          <div class="flex gap-2">
            <div
              class="flex-1 bg-muted rounded-md px-3 py-2 font-mono text-sm tracking-wider select-all"
            >
              {p2pState.syncCode}
            </div>
            <Button
              variant="secondary"
              size="icon"
              class="shrink-0"
              onclick={handleCopyCode}
              title="Copy sync code"
            >
              {#if isCopied}
                <Check class="size-4 text-green-600" />
              {:else}
                <Copy class="size-4" />
              {/if}
            </Button>
          </div>
          <p class="text-xs text-muted-foreground">
            Enter this code on your other devices to sync.
          </p>
        </div>
      {/if}
    </div>
  {/if}

  <!-- Divider -->
  <div class="border-t pt-3 mt-3"></div>

  <!-- Advanced: Server Sync -->
  <button
    class="flex items-center gap-2 text-sm text-muted-foreground hover:text-foreground w-full px-1"
    onclick={() => (showAdvanced = !showAdvanced)}
  >
    {#if showAdvanced}
      <ChevronDown class="size-4" />
    {:else}
      <ChevronRight class="size-4" />
    {/if}
    <Server class="size-4" />
    <span>Advanced: Custom Server</span>
  </button>

  {#if showAdvanced}
    <div class="space-y-3 px-1 pl-7">
      <p class="text-xs text-muted-foreground">
        Use a Hocuspocus server for always-on sync and multi-user collaboration.
      </p>

      <!-- Server Toggle -->
      <div class="flex items-center justify-between gap-4">
        <Label for="server-toggle" class="text-sm cursor-pointer flex items-center gap-2">
          {#if collaborationConnected}
            <Wifi class="size-4 text-green-500" />
          {:else}
            <WifiOff class="size-4 text-muted-foreground" />
          {/if}
          Server Sync
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
            id="server-toggle"
            checked={collaborationEnabled}
            onCheckedChange={(checked) => onCollaborationToggle?.(checked)}
          />
        </div>
      </div>

      <!-- Server URL -->
      <div class="space-y-2">
        <Label for="sync-server" class="text-xs text-muted-foreground">
          Server URL
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
      </div>

      <!-- Clear Cache -->
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
  {/if}
</div>

<!-- Conflict Resolution Dialog -->
{#if pendingConflicts.length > 0}
  <SyncConflictDialog
    conflicts={pendingConflicts}
    onResolve={handleConflictResolve}
    onCancel={handleConflictCancel}
  />
{/if}
