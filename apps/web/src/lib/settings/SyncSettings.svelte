<script lang="ts">
  /**
   * SyncSettings - Multi-device sync with magic link authentication
   *
   * Features:
   * - Server URL configuration
   * - Magic link login
   * - Account management (logged-in state)
   * - Device management
   */
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import { Input } from "$lib/components/ui/input";
  import { Separator } from "$lib/components/ui/separator";
  import {
    Server,
    Loader2,
    Check,
    Mail,
    LogOut,
    User,
    Smartphone,
    Trash2,
    RefreshCw,
    AlertCircle,
    Link,
  } from "@lucide/svelte";
  import { collaborationStore } from "@/models/stores/collaborationStore.svelte";
  import {
    getAuthState,
    setServerUrl,
    requestMagicLink,
    verifyMagicLink,
    logout,
    deleteDevice,
    refreshUserInfo,
    initAuth,
  } from "$lib/auth";
  import { onMount } from "svelte";

  // State
  let syncServerUrl = $state(
    typeof window !== "undefined"
      ? localStorage.getItem("diaryx_sync_server_url") || ""
      : ""
  );
  let email = $state("");
  let isApplyingServer = $state(false);
  let isSendingMagicLink = $state(false);
  let isLoggingOut = $state(false);
  let applied = $state(false);
  let magicLinkSent = $state(false);
  let devLink = $state<string | null>(null);
  let error = $state<string | null>(null);

  // Get auth state reactively
  let authState = $derived(getAuthState());

  // Initialize auth on mount
  onMount(() => {
    initAuth();

    // Check for magic link token in URL
    const params = new URLSearchParams(window.location.search);
    const token = params.get("token");
    if (token) {
      handleMagicLinkVerification(token);
    }
  });

  // Apply server URL
  async function applySyncServer() {
    let url = syncServerUrl.trim();

    if (url) {
      // Ensure proper protocol for HTTP endpoints
      if (!url.startsWith("http://") && !url.startsWith("https://")) {
        url = "https://" + url;
        syncServerUrl = url;
      }

      isApplyingServer = true;
      error = null;

      try {
        setServerUrl(url);
        collaborationStore.setServerUrl(toWebSocketUrl(url));

        // Show success feedback
        setTimeout(() => {
          isApplyingServer = false;
          applied = true;
          setTimeout(() => {
            applied = false;
          }, 2000);
        }, 500);
      } catch (e) {
        error = e instanceof Error ? e.message : "Failed to apply server URL";
        isApplyingServer = false;
      }
    }
  }

  // Clear server URL
  function clearSyncServer() {
    syncServerUrl = "";
    setServerUrl(null);
    collaborationStore.setServerUrl(null);
  }

  // Convert HTTP URL to WebSocket URL
  function toWebSocketUrl(httpUrl: string): string {
    return httpUrl
      .replace(/^https:\/\//, "wss://")
      .replace(/^http:\/\//, "ws://")
      + "/sync";
  }

  // Send magic link
  async function handleSendMagicLink() {
    if (!email.trim()) return;

    isSendingMagicLink = true;
    error = null;
    devLink = null;

    try {
      const result = await requestMagicLink(email.trim());
      magicLinkSent = true;
      devLink = result.devLink || null;
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to send magic link";
    } finally {
      isSendingMagicLink = false;
    }
  }

  // Verify magic link
  async function handleMagicLinkVerification(token: string) {
    error = null;

    try {
      await verifyMagicLink(token);
      // Clear the token from URL
      const url = new URL(window.location.href);
      url.searchParams.delete("token");
      window.history.replaceState({}, "", url.toString());
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to verify magic link";
    }
  }

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

  // Reset to login form
  function resetToLogin() {
    magicLinkSent = false;
    devLink = null;
    error = null;
  }
</script>

<div class="space-y-4">
  <!-- Header -->
  <h3 class="font-medium flex items-center gap-2">
    <Server class="size-4" />
    Multi-Device Sync
  </h3>

  <p class="text-xs text-muted-foreground px-1">
    Sync your workspace across devices with our cloud server. Sign in with your
    email to get started.
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

  <!-- Server URL Configuration -->
  <div class="space-y-2 px-1">
    <Label for="sync-server" class="text-xs text-muted-foreground">
      Server URL
    </Label>
    <div class="flex gap-2">
      <Input
        id="sync-server"
        type="text"
        bind:value={syncServerUrl}
        placeholder="https://sync.diaryx.org"
        class="text-sm"
        disabled={authState.isAuthenticated}
        onkeydown={(e) => e.key === "Enter" && applySyncServer()}
      />
      {#if !authState.isAuthenticated}
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
      {/if}
    </div>

    {#if syncServerUrl && !authState.isAuthenticated}
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

  {#if syncServerUrl}
    <Separator class="my-4" />

    <!-- Auth Section -->
    {#if authState.isAuthenticated && authState.user}
      <!-- Logged In State -->
      <div class="space-y-4">
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
    {:else if magicLinkSent}
      <!-- Magic Link Sent -->
      <div class="space-y-3">
        <div class="flex items-center gap-2 text-sm">
          <Mail class="size-4 text-muted-foreground" />
          <span>Check your email for a sign-in link!</span>
        </div>

        {#if devLink}
          <!-- Dev mode: show link directly -->
          <div class="space-y-2 p-3 bg-amber-500/10 rounded-md">
            <p class="text-xs text-amber-700">
              Development mode: Email not configured. Use this link:
            </p>
            <a
              href={devLink}
              class="text-xs text-primary hover:underline flex items-center gap-1 break-all"
            >
              <Link class="size-3 shrink-0" />
              {devLink}
            </a>
          </div>
        {/if}

        <p class="text-xs text-muted-foreground">
          Sent to: <strong>{email}</strong>
        </p>

        <Button variant="ghost" size="sm" onclick={resetToLogin}>
          Use a different email
        </Button>
      </div>
    {:else}
      <!-- Login Form -->
      <div class="space-y-3">
        <Label for="email" class="text-xs text-muted-foreground">
          Email Address
        </Label>
        <div class="flex gap-2">
          <Input
            id="email"
            type="email"
            bind:value={email}
            placeholder="you@example.com"
            class="text-sm"
            onkeydown={(e) => e.key === "Enter" && handleSendMagicLink()}
          />
          <Button
            variant="default"
            size="sm"
            onclick={handleSendMagicLink}
            disabled={isSendingMagicLink || !email.trim()}
          >
            {#if isSendingMagicLink}
              <Loader2 class="size-4 animate-spin" />
            {:else}
              <Mail class="size-4" />
            {/if}
          </Button>
        </div>
        <p class="text-xs text-muted-foreground">
          We'll send you a sign-in link. No password required.
        </p>
      </div>
    {/if}
  {/if}
</div>
