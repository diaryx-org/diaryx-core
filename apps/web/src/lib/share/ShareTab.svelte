<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import * as Alert from "$lib/components/ui/alert";
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
  } from "@lucide/svelte";
  import { shareSessionStore } from "@/models/stores/shareSessionStore.svelte";
  import {
    createShareSession,
    joinShareSession,
    endShareSession,
  } from "@/models/services/shareService";
  import { workspaceStore } from "@/models/stores/workspaceStore.svelte";
  import { entryStore } from "@/models/stores/entryStore.svelte";

  // Props
  interface Props {
    onSessionStart?: () => void;
    onSessionEnd?: () => void;
    /** Called before hosting to populate CRDT with current files */
    onBeforeHost?: () => Promise<void>;
  }

  let { onSessionStart, onSessionEnd, onBeforeHost }: Props = $props();

  // Local state
  let joinCodeInput = $state("");
  let copied = $state(false);
  let isCreating = $state(false);
  let isJoining = $state(false);

  // Get current state from store
  let mode = $derived(shareSessionStore.mode);
  let joinCode = $derived(shareSessionStore.joinCode);
  let connected = $derived(shareSessionStore.connected);
  let connecting = $derived(shareSessionStore.connecting);
  let error = $derived(shareSessionStore.error);
  let peerCount = $derived(shareSessionStore.peerCount);

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

    isCreating = true;
    try {
      // Populate CRDT with current files before creating session
      if (onBeforeHost) {
        console.log("[ShareTab] Populating CRDT before hosting...");
        await onBeforeHost();
      }
      await createShareSession(wsId);
      onSessionStart?.();
    } catch (e) {
      console.error("[ShareTab] Failed to create session:", e);
    } finally {
      isCreating = false;
    }
  }

  // Handle joining a session
  async function handleJoinSession() {
    if (!joinCodeInput.trim()) {
      shareSessionStore.setError("Please enter a join code");
      return;
    }

    isJoining = true;
    try {
      // Save current tree state before joining so we can restore it when leaving
      workspaceStore.saveTreeState();

      await joinShareSession(joinCodeInput.trim());
      joinCodeInput = "";
      onSessionStart?.();
    } catch (e) {
      console.error("[ShareTab] Failed to join session:", e);
      // Clear saved state if join failed
      workspaceStore.clearSavedTreeState();
    } finally {
      isJoining = false;
    }
  }

  // Handle ending session
  async function handleEndSession() {
    // Check if we were a guest before ending (mode will be cleared after)
    const wasGuest = mode === "guest";

    await endShareSession();

    // Restore previous workspace tree if we were a guest
    if (wasGuest) {
      workspaceStore.restoreTreeState();
      // Clear the current entry since the guest path is no longer valid
      entryStore.setCurrentEntry(null);
      entryStore.setDisplayContent("");
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
        <h3 class="font-medium text-sm">Live Collaboration</h3>
        <p class="text-xs text-muted-foreground">
          Share your workspace for real-time editing
        </p>
      </div>

      <!-- Host Session Button -->
      <Button
        variant="default"
        class="w-full"
        onclick={handleCreateSession}
        disabled={isCreating || connecting}
      >
        {#if isCreating || connecting}
          <Loader2 class="size-4 mr-2 animate-spin" />
          Creating session...
        {:else}
          <Link class="size-4 mr-2" />
          Host a Session
        {/if}
      </Button>

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
        <label class="text-xs font-medium text-muted-foreground">
          Join a Session
        </label>
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
        <label class="text-xs font-medium text-muted-foreground">
          Share this code
        </label>
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
        <label class="text-xs font-medium text-muted-foreground">
          Session Code
        </label>
        <div class="p-3 rounded-md bg-muted border border-border">
          <code class="text-sm font-mono">
            {joinCode ? formatJoinCode(joinCode) : "---"}
          </code>
        </div>
      </div>

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
        Changes sync in real-time. Your local workspace is not affected.
      </p>
    </div>
  {/if}
</div>
