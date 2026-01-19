<script lang="ts">
  /**
   * SyncSetupWizard - Multi-step wizard for sync setup
   *
   * Steps:
   * 1. Server URL - Enter and validate sync server URL
   * 2. Authentication - Enter email for magic link
   * 3. Verification - Wait for email or enter code manually
   *
   * After verification, the wizard closes and sync progress shows in SyncStatusIndicator.
   */
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { collaborationStore } from "@/models/stores/collaborationStore.svelte";
  import {
    setServerUrl,
    requestMagicLink,
    verifyMagicLink,
  } from "$lib/auth";
  import {
    Server,
    Mail,
    Link,
    Loader2,
    AlertCircle,
    ArrowRight,
    ArrowLeft,
  } from "@lucide/svelte";
  import { toast } from "svelte-sonner";

  interface Props {
    open?: boolean;
    onOpenChange?: (open: boolean) => void;
    onComplete?: () => void;
  }

  let {
    open = $bindable(false),
    onOpenChange,
    onComplete,
  }: Props = $props();

  // Wizard state
  let step = $state(1);
  let serverUrl = $state(
    typeof window !== "undefined"
      ? localStorage.getItem("diaryx_sync_server_url") || "https://sync.diaryx.org"
      : "https://sync.diaryx.org"
  );
  let email = $state("");
  let verificationCode = $state("");
  let devLink = $state<string | null>(null);

  // Loading states
  let isValidatingServer = $state(false);
  let isSendingMagicLink = $state(false);
  let isVerifying = $state(false);

  // Error state
  let error = $state<string | null>(null);

  // Step 1: Validate and apply server URL
  async function handleApplyServer() {
    let url = serverUrl.trim();
    if (!url) {
      error = "Please enter a server URL";
      return;
    }

    // Ensure proper protocol
    if (!url.startsWith("http://") && !url.startsWith("https://")) {
      url = "https://" + url;
      serverUrl = url;
    }

    isValidatingServer = true;
    error = null;

    try {
      // Validate by making a test request
      const response = await fetch(`${url}/health`, {
        method: "GET",
        signal: AbortSignal.timeout(5000),
      });

      if (!response.ok) {
        throw new Error("Server returned an error");
      }

      // Apply the server URL
      setServerUrl(url);
      collaborationStore.setServerUrl(toWebSocketUrl(url));
      collaborationStore.setSyncStatus('idle');

      // Move to next step
      step = 2;
    } catch (e) {
      if (e instanceof Error && e.name === "TimeoutError") {
        error = "Connection timed out. Check the URL and try again.";
      } else {
        error = "Could not connect to server. Please check the URL.";
      }
    } finally {
      isValidatingServer = false;
    }
  }

  // Step 2: Send magic link
  async function handleSendMagicLink() {
    if (!email.trim()) {
      error = "Please enter your email address";
      return;
    }

    isSendingMagicLink = true;
    error = null;
    devLink = null;

    try {
      const result = await requestMagicLink(email.trim());
      devLink = result.devLink || null;
      step = 3;
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to send magic link";
    } finally {
      isSendingMagicLink = false;
    }
  }

  // Step 3: Verify token (manual code entry)
  async function handleVerifyToken(token: string) {
    if (!token.trim()) {
      error = "Please enter the verification code";
      return;
    }

    isVerifying = true;
    error = null;

    try {
      await verifyMagicLink(token.trim());

      // Show success toast
      toast.success("Signed in successfully", {
        description: "Your workspace is now syncing.",
      });

      // Close the wizard - progress will show in SyncStatusIndicator
      handleClose();
      onComplete?.();
    } catch (e) {
      error = e instanceof Error ? e.message : "Verification failed";
    } finally {
      isVerifying = false;
    }
  }

  // Handle dialog close
  function handleClose() {
    open = false;
    onOpenChange?.(false);
  }

  // Go back to previous step
  function handleBack() {
    if (step > 1) {
      step = step - 1;
      error = null;
    }
  }

  // Convert HTTP URL to WebSocket URL
  function toWebSocketUrl(httpUrl: string): string {
    return httpUrl
      .replace(/^https:\/\//, "wss://")
      .replace(/^http:\/\//, "ws://")
      + "/sync";
  }
</script>

<Dialog.Root bind:open onOpenChange={(o) => onOpenChange?.(o)}>
  <Dialog.Content class="sm:max-w-[450px]">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        {#if step === 1}
          <Server class="size-5" />
          Connect to Sync Server
        {:else if step === 2}
          <Mail class="size-5" />
          Sign In
        {:else}
          <Link class="size-5" />
          Verify Your Email
        {/if}
      </Dialog.Title>
      <Dialog.Description>
        {#if step === 1}
          Enter your sync server URL to get started.
        {:else if step === 2}
          Enter your email to receive a sign-in link.
        {:else}
          Check your email and click the sign-in link, or enter the code below.
        {/if}
      </Dialog.Description>
    </Dialog.Header>

    <div class="py-4 space-y-4">
      <!-- Error message -->
      {#if error}
        <div class="flex items-center gap-2 text-destructive text-sm p-3 bg-destructive/10 rounded-md">
          <AlertCircle class="size-4 shrink-0" />
          <span>{error}</span>
        </div>
      {/if}

      <!-- Step 1: Server URL -->
      {#if step === 1}
        <div class="space-y-3">
          <div class="space-y-2">
            <Label for="server-url" class="text-sm">Server URL</Label>
            <Input
              id="server-url"
              type="text"
              bind:value={serverUrl}
              placeholder="https://sync.diaryx.org"
              disabled={isValidatingServer}
              onkeydown={(e) => e.key === "Enter" && handleApplyServer()}
            />
          </div>
          <p class="text-xs text-muted-foreground">
            Use the official Diaryx sync server or your own self-hosted instance.
          </p>
        </div>
      {/if}

      <!-- Step 2: Email -->
      {#if step === 2}
        <div class="space-y-3">
          <div class="space-y-2">
            <Label for="email" class="text-sm">Email Address</Label>
            <Input
              id="email"
              type="email"
              bind:value={email}
              placeholder="you@example.com"
              disabled={isSendingMagicLink}
              onkeydown={(e) => e.key === "Enter" && handleSendMagicLink()}
            />
          </div>
          <p class="text-xs text-muted-foreground">
            We'll send you a sign-in link. No password required.
          </p>
        </div>
      {/if}

      <!-- Step 3: Verification -->
      {#if step === 3}
        <div class="space-y-4">
          {#if devLink}
            <!-- Dev mode: show link directly -->
            <div class="space-y-2 p-3 bg-amber-500/10 rounded-md">
              <p class="text-xs text-amber-700 dark:text-amber-400 font-medium">
                Development mode: Email not configured
              </p>
              <a
                href={devLink}
                class="text-xs text-primary hover:underline flex items-center gap-1 break-all"
                onclick={() => handleVerifyToken(new URL(devLink!).searchParams.get("token") || "")}
              >
                <Link class="size-3 shrink-0" />
                Click here to verify
              </a>
            </div>
          {:else}
            <div class="space-y-2">
              <p class="text-sm">
                Check your email at <strong>{email}</strong>
              </p>
              <p class="text-xs text-muted-foreground">
                Click the link in the email, or paste the verification code below:
              </p>
            </div>
            <div class="space-y-2">
              <Label for="verification-code" class="text-sm">Verification Code</Label>
              <Input
                id="verification-code"
                type="text"
                bind:value={verificationCode}
                placeholder="Enter code from email"
                disabled={isVerifying}
                onkeydown={(e) => e.key === "Enter" && handleVerifyToken(verificationCode)}
              />
            </div>
          {/if}
        </div>
      {/if}

    </div>

    <!-- Footer with navigation buttons -->
    <div class="flex justify-between pt-4 border-t">
      {#if step > 1}
        <Button variant="ghost" size="sm" onclick={handleBack}>
          <ArrowLeft class="size-4 mr-1" />
          Back
        </Button>
      {:else}
        <div></div>
      {/if}

      {#if step === 1}
        <Button onclick={handleApplyServer} disabled={isValidatingServer}>
          {#if isValidatingServer}
            <Loader2 class="size-4 mr-2 animate-spin" />
            Connecting...
          {:else}
            Continue
            <ArrowRight class="size-4 ml-1" />
          {/if}
        </Button>
      {:else if step === 2}
        <Button onclick={handleSendMagicLink} disabled={isSendingMagicLink || !email.trim()}>
          {#if isSendingMagicLink}
            <Loader2 class="size-4 mr-2 animate-spin" />
            Sending...
          {:else}
            <Mail class="size-4 mr-2" />
            Send Sign-in Link
          {/if}
        </Button>
      {:else if step === 3}
        {#if !devLink}
          <Button onclick={() => handleVerifyToken(verificationCode)} disabled={isVerifying || !verificationCode.trim()}>
            {#if isVerifying}
              <Loader2 class="size-4 mr-2 animate-spin" />
              Verifying...
            {:else}
              Verify
              <ArrowRight class="size-4 ml-1" />
            {/if}
          </Button>
        {:else}
          <div></div>
        {/if}
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>
