<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import * as Alert from "$lib/components/ui/alert";
  import { Switch } from "$lib/components/ui/switch";
  import NativeSelect from "$lib/components/ui/native-select/native-select.svelte";
  import {
    Users,
    Link,
    Copy,
    Check,
    Loader2,
    AlertCircle,
    LogOut,
    Radio,
    UserPlus,
    Lock,
    LockOpen,
    ChevronDown,
    ChevronUp,
    Server,
  } from "@lucide/svelte";
  import { shareSessionStore } from "@/models/stores/shareSessionStore.svelte";
  import {
    createShareSession,
    joinShareSession,
    endShareSession,
    setSessionReadOnly,
    setShareServerUrl,
    getShareServerUrl,
  } from "@/models/services/shareService";
  import { workspaceStore } from "@/models/stores/workspaceStore.svelte";
  import { entryStore } from "@/models/stores/entryStore.svelte";
  import { getAuthState } from "$lib/auth";
  import type { Api } from "$lib/backend/api";
  import { toast } from "svelte-sonner";

  // Props
  interface Props {
    onSessionStart?: () => void;
    onSessionEnd?: () => void;
    /** Called before hosting to populate CRDT with current files */
    onBeforeHost?: (audience: string | null) => Promise<void>;
    /** Called to open an entry by path */
    onOpenEntry?: (path: string) => Promise<void>;
    /** API instance for loading audiences */
    api: Api | null;
    /** When true, automatically starts a hosting session */
    triggerStart?: boolean;
    /** Called after triggerStart is consumed */
    onTriggerStartConsumed?: () => void;
  }

  let { onSessionStart, onSessionEnd, onBeforeHost, onOpenEntry, api, triggerStart = false, onTriggerStartConsumed }: Props = $props();

  // Local state
  let joinCodeInput = $state("");
  let copied = $state(false);
  let isCreating = $state(false);
  let isJoining = $state(false);

  // Pre-session config
  let preSessionReadOnly = $state(false);
  let selectedAudience = $state("all");
  let audiences = $state<string[]>([]);
  let showAdvanced = $state(false);
  let customServerUrl = $state(getShareServerUrl());

  // Get current state from store
  let mode = $derived(shareSessionStore.mode);
  let joinCode = $derived(shareSessionStore.joinCode);
  let connected = $derived(shareSessionStore.connected);
  let connecting = $derived(shareSessionStore.connecting);
  let error = $derived(shareSessionStore.error);
  let peerCount = $derived(shareSessionStore.peerCount);
  let readOnly = $derived(shareSessionStore.readOnly);
  let sessionAudience = $derived(shareSessionStore.audience);
  let authState = $derived(getAuthState());

  // Sync custom server URL to shareService
  $effect(() => {
    const trimmed = customServerUrl.trim();
    setShareServerUrl(trimmed || null);
  });

  // Load available audiences when component mounts or tree changes
  $effect(() => {
    loadAudiences();
  });

  // Handle external trigger to start session
  $effect(() => {
    if (triggerStart && mode === 'idle' && !isCreating) {
      handleCreateSession();
      onTriggerStartConsumed?.();
    }
  });

  async function loadAudiences() {
    if (!api || !workspaceStore.tree) {
      audiences = [];
      return;
    }

    try {
      const available = await api.getAvailableAudiences(workspaceStore.tree.path);
      audiences = available;
    } catch (e) {
      console.warn("[ShareTab] Failed to load audiences:", e);
      audiences = [];
    }
  }

  // Generate a unique workspace ID if none exists
  function getOrCreateWorkspaceId(): string {
    const existingId = workspaceStore.workspaceId;
    if (existingId) return existingId;

    // Generate a random ID based on timestamp and random bytes
    const timestamp = Date.now().toString(36);
    const random = Math.random().toString(36).substring(2, 8);
    return `ws-${timestamp}-${random}`;
  }

  // Handle creating a session
  async function handleCreateSession() {
    const wsId = getOrCreateWorkspaceId();
    const audienceToUse = selectedAudience === "all" ? null : selectedAudience;

    isCreating = true;
    try {
      // Populate CRDT with current files before creating session (with audience filter)
      if (onBeforeHost) {
        console.log("[ShareTab] Populating CRDT before hosting with audience:", audienceToUse);
        await onBeforeHost(audienceToUse);
      }
      await createShareSession(wsId, preSessionReadOnly, audienceToUse);
      toast.success("Share session started", { description: "Others can now join with your code" });
      onSessionStart?.();
    } catch (e) {
      console.error("[ShareTab] Failed to create session:", e);
      toast.error("Failed to start session", { description: e instanceof Error ? e.message : String(e) });
    } finally {
      isCreating = false;
    }
  }

  // Handle toggling read-only during session (host only)
  function handleReadOnlyToggle() {
    setSessionReadOnly(!readOnly);
  }

  // Handle joining a session
  async function handleJoinSession() {
    if (!joinCodeInput.trim()) {
      shareSessionStore.setError("Please enter a join code");
      return;
    }

    isJoining = true;
    try {
      await joinShareSession(joinCodeInput.trim());
      joinCodeInput = "";
      onSessionStart?.();
    } catch (e) {
      console.error("[ShareTab] Failed to join session:", e);
    } finally {
      isJoining = false;
    }
  }

  // Handle ending session
  async function handleEndSession() {
    // Check if we were a guest before ending (mode will be cleared after)
    const wasGuest = mode === "guest";

    await endShareSession();

    // Clean up guest UI state (endShareSession already restores the backend and tree)
    if (wasGuest) {
      // Clear the current entry since the guest path is no longer valid
      entryStore.setCurrentEntry(null);
      entryStore.setDisplayContent("");

      // Open the root entry of the restored tree
      const restoredTree = workspaceStore.tree;
      if (restoredTree && onOpenEntry) {
        await onOpenEntry(restoredTree.path);
      }
    }

    onSessionEnd?.();
  }

  // Handle copying join code
  async function handleCopyCode() {
    if (!joinCode) return;
    try {
      await navigator.clipboard.writeText(joinCode);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch (e) {
      console.error("[ShareTab] Failed to copy:", e);
    }
  }

  // Format join code for display (add spaces for readability)
  function formatJoinCode(code: string): string {
    return code;
  }
</script>

<div class="p-3 space-y-4">
  <!-- Error Alert -->
  {#if error}
    <Alert.Root variant="destructive" class="py-2">
      <AlertCircle class="size-4" />
      <Alert.Description class="text-xs">{error}</Alert.Description>
    </Alert.Root>
  {/if}

  {#if mode === "idle"}
    <!-- Idle State: Show options to host or join -->
    <div class="space-y-4">
      <!-- Header -->
      <div class="text-center space-y-1">
        <Users class="size-8 mx-auto text-muted-foreground" />
        <h3 class="font-medium text-sm">Live Collaboration <span class="text-[10px] font-semibold uppercase px-1.5 py-0.5 rounded-full bg-amber-500/15 text-amber-600 dark:text-amber-400">Alpha</span></h3>
        <p class="text-xs text-muted-foreground">
          Share your workspace for real-time editing
        </p>
      </div>

      <!-- Session Options -->
      <div class="space-y-3 p-3 rounded-md bg-muted/50 border border-border">
        <!-- Read-only toggle -->
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2">
            {#if preSessionReadOnly}
              <Lock class="size-4 text-muted-foreground" />
            {:else}
              <LockOpen class="size-4 text-muted-foreground" />
            {/if}
            <span class="text-xs font-medium">Read-only mode</span>
          </div>
          <Switch bind:checked={preSessionReadOnly} />
        </div>

        <!-- Audience picker (only show if audiences exist) -->
        {#if audiences.length > 0}
          <div class="space-y-1.5">
            <label for="share-audience-select" class="text-xs font-medium text-muted-foreground">
              Share audience
            </label>
            <NativeSelect bind:value={selectedAudience} class="w-full" id="share-audience-select">
              <option value="all">All files (no filter)</option>
              {#each audiences as audience}
                <option value={audience}>{audience}</option>
              {/each}
            </NativeSelect>
          </div>
        {/if}
      </div>

      <!-- Advanced settings -->
      <div>
        <Button
          variant="ghost"
          size="sm"
          class="w-full justify-between"
          onclick={() => showAdvanced = !showAdvanced}
        >
          <span class="text-xs">Advanced</span>
          {#if showAdvanced}
            <ChevronUp class="size-4" />
          {:else}
            <ChevronDown class="size-4" />
          {/if}
        </Button>
        {#if showAdvanced}
          <div class="space-y-1.5 mt-1 px-1">
            <label for="share-server-url" class="text-xs font-medium text-muted-foreground flex items-center gap-1.5">
              <Server class="size-3" />
              Server URL
            </label>
            <Input
              id="share-server-url"
              type="text"
              bind:value={customServerUrl}
              placeholder="https://sync.diaryx.org"
              class="h-8 text-xs font-mono"
            />
          </div>
        {/if}
      </div>

      <!-- Host Session Button -->
      <Button
        variant="default"
        class="w-full"
        onclick={handleCreateSession}
        disabled={!authState.isAuthenticated || isCreating || connecting}
      >
        {#if isCreating || connecting}
          <Loader2 class="size-4 mr-2 animate-spin" />
          Creating session...
        {:else}
          <Link class="size-4 mr-2" />
          Host a Session
        {/if}
      </Button>
      {#if !authState.isAuthenticated}
        <p class="text-xs text-muted-foreground text-center">
          Sign in from Settings to host sessions.
        </p>
      {/if}

      <!-- Divider -->
      <div class="relative">
        <div class="absolute inset-0 flex items-center">
          <span class="w-full border-t border-border"></span>
        </div>
        <div class="relative flex justify-center text-xs uppercase">
          <span class="bg-sidebar px-2 text-muted-foreground">or</span>
        </div>
      </div>

      <!-- Join Session -->
      <div class="space-y-2">
        <span class="text-xs font-medium text-muted-foreground">
          Join a Session
        </span>
        <div class="flex gap-2">
          <Input
            type="text"
            bind:value={joinCodeInput}
            placeholder="XXXX-XXXX"
            class="h-9 text-base md:text-sm font-mono uppercase"
            maxlength={17}
            onkeydown={(e) => e.key === "Enter" && handleJoinSession()}
          />
          <Button
            variant="secondary"
            size="sm"
            class="h-9 px-3"
            onclick={handleJoinSession}
            disabled={isJoining || connecting || !joinCodeInput.trim()}
          >
            {#if isJoining || connecting}
              <Loader2 class="size-4 animate-spin" />
            {:else}
              <UserPlus class="size-4" />
            {/if}
          </Button>
        </div>
        <p class="text-xs text-muted-foreground">No account needed to join.</p>
      </div>
    </div>
  {:else if mode === "hosting"}
    <!-- Hosting State: Show join code and peers -->
    <div class="space-y-4">
      <!-- Status Badge -->
      <div
        class="flex items-center gap-2 px-3 py-2 rounded-md bg-primary/10 border border-primary/20"
      >
        <Radio class="size-4 text-primary animate-pulse" />
        <span class="text-sm font-medium text-primary">Hosting Session</span>
      </div>

      <!-- Join Code Display -->
      <div class="space-y-2">
        <span class="text-xs font-medium text-muted-foreground">
          Share this code
        </span>
        <div
          class="flex items-center gap-2 p-3 rounded-md bg-muted border border-border"
        >
          <code class="flex-1 text-lg font-mono font-bold tracking-wider">
            {joinCode ? formatJoinCode(joinCode) : "---"}
          </code>
          <Button
            variant="ghost"
            size="icon"
            class="size-8 shrink-0"
            onclick={handleCopyCode}
          >
            {#if copied}
              <Check class="size-4 text-green-500" />
            {:else}
              <Copy class="size-4" />
            {/if}
          </Button>
        </div>
      </div>

      <!-- Audience info (if filtered) -->
      {#if sessionAudience}
        <div class="flex items-center gap-2 px-3 py-2 rounded-md bg-secondary/50 border border-border">
          <span class="text-xs text-muted-foreground">Sharing:</span>
          <span class="text-xs font-medium">"{sessionAudience}" audience only</span>
        </div>
      {/if}

      <!-- Read-only toggle (host can change during session) -->
      <div class="flex items-center justify-between px-1">
        <div class="flex items-center gap-2">
          {#if readOnly}
            <Lock class="size-4 text-muted-foreground" />
          {:else}
            <LockOpen class="size-4 text-muted-foreground" />
          {/if}
          <span class="text-xs text-muted-foreground">Read-only mode</span>
        </div>
        <Switch checked={readOnly} onCheckedChange={handleReadOnlyToggle} />
      </div>

      <!-- Peer Count -->
      <div class="flex items-center justify-between px-1">
        <span class="text-xs text-muted-foreground">Connected peers</span>
        <span class="text-sm font-medium">{peerCount}</span>
      </div>

      <!-- Connection Status -->
      <div class="flex items-center gap-2 px-1">
        <div
          class="size-2 rounded-full {connected
            ? 'bg-green-500'
            : 'bg-yellow-500 animate-pulse'}"
        ></div>
        <span class="text-xs text-muted-foreground">
          {connected ? "Connected" : "Reconnecting..."}
        </span>
      </div>

      <!-- Stop Sharing Button -->
      <Button variant="outline" class="w-full" onclick={handleEndSession}>
        <LogOut class="size-4 mr-2" />
        Stop Sharing
      </Button>
    </div>
  {:else if mode === "guest"}
    <!-- Guest State: Show session info and leave button -->
    <div class="space-y-4">
      <!-- Status Badge -->
      <div
        class="flex items-center gap-2 px-3 py-2 rounded-md bg-secondary border border-border"
      >
        <Radio class="size-4 text-secondary-foreground" />
        <span class="text-sm font-medium">Connected to Session</span>
      </div>

      <!-- Session Info -->
      <div class="space-y-2">
        <span class="text-xs font-medium text-muted-foreground">
          Session Code
        </span>
        <div class="p-3 rounded-md bg-muted border border-border">
          <code class="text-sm font-mono">
            {joinCode ? formatJoinCode(joinCode) : "---"}
          </code>
        </div>
      </div>

      <!-- Read-only indicator -->
      {#if readOnly}
        <div class="flex items-center gap-2 px-3 py-2 rounded-md bg-amber-500/10 border border-amber-500/20">
          <Lock class="size-4 text-amber-600" />
          <span class="text-sm text-amber-600">View-only session</span>
        </div>
      {/if}

      <!-- Connection Status -->
      <div class="flex items-center gap-2 px-1">
        <div
          class="size-2 rounded-full {connected
            ? 'bg-green-500'
            : 'bg-yellow-500 animate-pulse'}"
        ></div>
        <span class="text-xs text-muted-foreground">
          {connected ? "Synced" : "Syncing..."}
        </span>
      </div>

      <!-- Leave Session Button -->
      <Button variant="outline" class="w-full" onclick={handleEndSession}>
        <LogOut class="size-4 mr-2" />
        Leave Session
      </Button>

      <!-- Guest Mode Notice -->
      <p class="text-xs text-muted-foreground text-center">
        {#if readOnly}
          View-only mode. You can browse but not edit.
        {:else}
          Changes sync in real-time. Your local workspace is not affected.
        {/if}
      </p>
    </div>
  {/if}
</div>
