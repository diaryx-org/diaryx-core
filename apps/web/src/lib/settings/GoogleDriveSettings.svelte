<script lang="ts">
  /**
   * GoogleDriveSettings - Google Drive backup and sync configuration
   *
   * Self-contained component for Google Drive authentication, backup, and bidirectional sync.
   */
  import { Button } from "$lib/components/ui/button";
  import { Progress } from "$lib/components/ui/progress";
  import { Loader2, Check, AlertCircle, RefreshCw, Upload, Download } from "@lucide/svelte";
  import { getBackend } from "../backend";
  import {
    getGoogleDriveRefreshToken,
    storeGoogleDriveRefreshToken,
    getGoogleDriveCredentials,
    storeGoogleDriveCredentials,
    removeGoogleDriveCredentials,
  } from "../credentials";

  interface Props {
    workspacePath?: string | null;
    onCancel?: () => void;
  }

  let { workspacePath = null, onCancel }: Props = $props();

  // Google Drive state
  let gdConfig = $state({
    name: "Google Drive Backup",
    client_id: "",
    client_secret: "",
  });
  let gdIsAuthenticated = $state(false);
  let gdAuthLoading = $state(false);
  let gdIsBackingUp = $state(false);
  let gdBackupProgress = $state({ stage: "", percent: 0 });
  let gdBackupStatus: { success: boolean; message: string } | null = $state(null);
  let gdAccessToken: string | null = $state(null);
  let gdInitialLoading = $state(true);

  // Sync state
  let gdIsSyncing = $state(false);
  let gdSyncProgress = $state({ stage: "", percent: 0, message: "" });
  let gdSyncStatus: {
    success: boolean;
    message: string;
    uploaded?: number;
    downloaded?: number;
    deleted?: number;
    conflicts?: Array<{ path: string; local_modified: number | null; remote_modified: string | null }>;
  } | null = $state(null);

  // Load saved config on mount
  $effect(() => {
    loadSavedGoogleDriveConfig();
  });

  async function loadSavedGoogleDriveConfig() {
    gdInitialLoading = true;
    console.log("[GDrive] Starting loadSavedGoogleDriveConfig...");
    try {
      console.log("[GDrive] Getting backend...");
      const backend = await getBackend();
      console.log("[GDrive] Got backend, checking for invoke...");
      if ("getInvoke" in backend) {
        console.log("[GDrive] Getting stored refresh token...");
        const storedRefreshToken = await getGoogleDriveRefreshToken();
        console.log("[GDrive] Refresh token:", storedRefreshToken ? "found" : "not found");
        if (storedRefreshToken) {
          try {
            // Load saved credentials first
            console.log("[GDrive] Getting saved credentials...");
            const { clientId: savedClientId, clientSecret: savedClientSecret } =
              await getGoogleDriveCredentials();
            console.log("[GDrive] Saved credentials:", savedClientId ? "found" : "not found");

            // Also try to get from backend env vars
            console.log("[GDrive] Getting env credentials...");
            const invoke = (backend as any).getInvoke();
            const envCreds = await invoke("get_google_auth_config", {});
            console.log("[GDrive] Env credentials:", envCreds?.client_id ? "found" : "not found");

            const clientId = savedClientId || envCreds?.client_id;
            const clientSecret = savedClientSecret || envCreds?.client_secret;

            if (!clientId || !clientSecret) {
              console.warn("[GDrive] Missing OAuth credentials for token refresh");
              gdInitialLoading = false;
              return;
            }

            // Update config state
            if (clientId) gdConfig.client_id = clientId;
            if (clientSecret) gdConfig.client_secret = clientSecret;

            console.log("[GDrive] Calling refresh_token...");
            // Call invoke directly since the plugin's JS API has a bug
            const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
            const response = await tauriInvoke<{
              accessToken: string;
              refreshToken?: string;
              expiresAt?: number;
            }>("plugin:google-auth|refresh_token", {
              payload: {
                refreshToken: storedRefreshToken,
                clientId,
                clientSecret,
              },
            });
            console.log("[GDrive] Token refreshed successfully");
            gdAccessToken = response.accessToken;
            gdIsAuthenticated = true;

            // Store new refresh token if provided
            if (response.refreshToken) {
              await storeGoogleDriveRefreshToken(response.refreshToken);
            }
          } catch (e) {
            console.warn("[GDrive] Failed to refresh token:", e);
            gdIsAuthenticated = false;
          }
        }
      } else {
        console.log("[GDrive] Backend does not have getInvoke (not Tauri)");
      }
    } catch (e) {
      console.warn("[GDrive] Failed to load config:", e);
    } finally {
      console.log("[GDrive] Done loading, setting gdInitialLoading = false");
      gdInitialLoading = false;
    }
  }

  async function signInWithGoogle() {
    gdAuthLoading = true;
    try {
      const { signIn } =
        await import("@choochmeque/tauri-plugin-google-auth-api");

      const backend = await getBackend();
      let clientId = gdConfig.client_id;
      let clientSecret = gdConfig.client_secret;

      // Try to get credentials from backend if not manually entered
      if (!clientId || !clientSecret) {
        if ("getInvoke" in backend) {
          try {
            const invoke = (backend as any).getInvoke();
            const creds = await invoke("get_google_auth_config", {});
            if (creds) {
              clientId = creds.client_id || clientId;
              clientSecret = creds.client_secret || clientSecret;
            }
          } catch (e) {
            console.warn("Failed to get Google credentials from backend:", e);
          }
        }
      }

      const response = await signIn({
        clientId,
        clientSecret,
        scopes: ["https://www.googleapis.com/auth/drive.file"],
      });

      gdAccessToken = response.accessToken;

      // Store refresh token for later
      if (response.refreshToken) {
        await storeGoogleDriveRefreshToken(response.refreshToken);
      }
      // Store client credentials if user entered them
      if (gdConfig.client_id || gdConfig.client_secret) {
        await storeGoogleDriveCredentials(
          gdConfig.client_id,
          gdConfig.client_secret,
        );
      }

      gdIsAuthenticated = true;
    } catch (e) {
      console.error("Google Sign-In failed:", e);
      alert(
        "Google Sign-In failed: " +
          (e instanceof Error ? e.message : String(e)),
      );
    } finally {
      gdAuthLoading = false;
    }
  }

  async function disconnectGoogleDrive() {
    try {
      const { signOut } =
        await import("@choochmeque/tauri-plugin-google-auth-api");
      await signOut({ accessToken: gdAccessToken || undefined });
      await removeGoogleDriveCredentials();
      gdIsAuthenticated = false;
      gdAccessToken = null;
    } catch (e) {
      console.error("Google Sign-Out failed:", e);
    }
  }

  async function backupToGoogleDrive() {
    if (!gdAccessToken) {
      alert("Please sign in to Google Drive first");
      return;
    }
    gdIsBackingUp = true;
    gdBackupStatus = null;
    gdBackupProgress = { stage: "Preparing...", percent: 0 };
    let unlisten: (() => void) | null = null;
    try {
      const backend = await getBackend();
      if ("getInvoke" in backend) {
        const invoke = (backend as any).getInvoke();
        try {
          const { listen } = await import("@tauri-apps/api/event");
          unlisten = await listen<{
            stage: string;
            percent: number;
            message: string | null;
          }>("backup_progress", (event) => {
            const { stage, percent, message } = event.payload;
            gdBackupProgress = { stage: message || stage, percent };
          });
        } catch (e) {}
        const result = await invoke("backup_to_google_drive", {
          workspacePath: workspacePath
            ? workspacePath.substring(0, workspacePath.lastIndexOf("/"))
            : null,
          config: {
            name: gdConfig.name,
            access_token: gdAccessToken,
            folder_id: null,
          },
        });
        gdBackupProgress = { stage: "Complete!", percent: 100 };
        gdBackupStatus = {
          success: result.success,
          message: result.success
            ? `Backup complete! ${result.files_processed} files uploaded.`
            : result.error || "Backup failed",
        };
      }
    } catch (e) {
      gdBackupStatus = {
        success: false,
        message: e instanceof Error ? e.message : String(e),
      };
    } finally {
      if (unlisten) unlisten();
      gdIsBackingUp = false;
    }
  }

  async function syncToGoogleDrive() {
    if (!gdAccessToken) {
      alert("Please sign in to Google Drive first");
      return;
    }
    gdIsSyncing = true;
    gdSyncStatus = null;
    gdSyncProgress = { stage: "Starting...", percent: 0, message: "" };
    let unlisten: (() => void) | null = null;
    try {
      const backend = await getBackend();
      if ("getInvoke" in backend) {
        const invoke = (backend as any).getInvoke();

        // Listen for sync progress events
        try {
          const { listen } = await import("@tauri-apps/api/event");
          unlisten = await listen<{
            stage: string;
            current: number;
            total: number;
            percent: number;
            message: string | null;
          }>("sync_progress", (event) => {
            const { stage, percent, message } = event.payload;
            const stageLabels: Record<string, string> = {
              detecting_local: "Scanning local files",
              detecting_remote: "Fetching remote files",
              uploading: "Uploading",
              downloading: "Downloading",
              deleting: "Cleaning up",
              complete: "Complete",
              error: "Error",
            };
            gdSyncProgress = {
              stage: stageLabels[stage] || stage,
              percent,
              message: message || "",
            };
          });
        } catch (e) {
          console.warn("Failed to listen for sync progress:", e);
        }

        const result = await invoke("sync_to_google_drive", {
          workspacePath: workspacePath
            ? workspacePath.substring(0, workspacePath.lastIndexOf("/"))
            : null,
          config: {
            name: gdConfig.name,
            access_token: gdAccessToken,
            folder_id: null,
          },
        });

        if (result.success) {
          const parts = [];
          if (result.files_uploaded > 0) parts.push(`${result.files_uploaded} uploaded`);
          if (result.files_downloaded > 0) parts.push(`${result.files_downloaded} downloaded`);
          if (result.files_deleted > 0) parts.push(`${result.files_deleted} deleted`);
          const message = parts.length > 0 ? `Sync complete: ${parts.join(", ")}` : "Already in sync!";

          gdSyncStatus = {
            success: true,
            message,
            uploaded: result.files_uploaded,
            downloaded: result.files_downloaded,
            deleted: result.files_deleted,
            conflicts: result.conflicts,
          };
        } else {
          gdSyncStatus = {
            success: false,
            message: result.error || "Sync failed",
          };
        }
      } else {
        gdSyncStatus = {
          success: false,
          message: "Google Drive sync is only available in the desktop app",
        };
      }
    } catch (e) {
      gdSyncStatus = {
        success: false,
        message: e instanceof Error ? e.message : String(e),
      };
    } finally {
      if (unlisten) unlisten();
      gdIsSyncing = false;
    }
  }
</script>

<div class="space-y-3">
  <div class="flex items-center justify-between">
    <span class="text-sm font-medium">Google Drive Backup</span>
    <Button variant="ghost" size="sm" onclick={onCancel}>Cancel</Button>
  </div>

  {#if gdInitialLoading}
    <div class="py-8 flex flex-col items-center gap-2">
      <Loader2 class="size-6 animate-spin text-muted-foreground" />
      <p class="text-sm text-muted-foreground">Checking authentication...</p>
    </div>
  {:else if !gdIsAuthenticated}
    <div class="py-4 flex flex-col items-center gap-4">
      <p class="text-sm text-center text-muted-foreground px-4">
        Connect your Google account to back up your diary to Google Drive.
      </p>

      <Button
        variant="outline"
        onclick={signInWithGoogle}
        disabled={gdAuthLoading}
        class="w-full"
      >
        {#if gdAuthLoading}
          <Loader2 class="mr-2 size-4 animate-spin" />
        {:else}
          <span class="mr-2">G</span>
        {/if}
        Sign in with Google
      </Button>

      <p class="text-[10px] text-muted-foreground text-center px-4">
        Note: Your diary files will be stored in a dedicated folder in your
        Google Drive. Diaryx only accesses files it creates.
      </p>
    </div>
  {:else}
    <div class="space-y-3">
      <div class="flex items-center justify-between">
        <span class="text-xs font-medium text-green-600 flex items-center gap-1">
          <Check class="size-3" /> Connected to Google Drive
        </span>
        <Button
          variant="ghost"
          size="sm"
          onclick={disconnectGoogleDrive}
          class="text-[10px] h-6 text-muted-foreground"
        >
          Disconnect
        </Button>
      </div>

      <p class="text-xs text-muted-foreground">
        Files will sync to a <strong>diaryx-sync</strong> folder in your Google Drive root.
      </p>

      <div class="flex gap-2 pt-2">
        <Button
          variant="default"
          size="sm"
          onclick={syncToGoogleDrive}
          disabled={gdIsSyncing || gdIsBackingUp}
          class="flex-1"
        >
          {#if gdIsSyncing}
            <RefreshCw class="mr-2 size-4 animate-spin" />Syncing...
          {:else}
            <RefreshCw class="mr-2 size-4" />Sync Now
          {/if}
        </Button>
        <Button
          variant="outline"
          size="sm"
          onclick={backupToGoogleDrive}
          disabled={gdIsBackingUp || gdIsSyncing}
          class="flex-1"
        >
          {#if gdIsBackingUp}
            <Loader2 class="mr-2 size-4 animate-spin" />Backing up...
          {:else}
            <Upload class="mr-2 size-4" />Full Backup
          {/if}
        </Button>
      </div>

      <p class="text-xs text-muted-foreground">
        <strong>Sync</strong> keeps local and cloud files in sync. <strong>Full Backup</strong> creates a one-time ZIP archive.
      </p>

      {#if gdIsBackingUp && gdBackupProgress.stage}
        <div class="space-y-1">
          <div class="flex items-center justify-between text-xs text-muted-foreground">
            <span>{gdBackupProgress.stage}</span>
            <span>{gdBackupProgress.percent}%</span>
          </div>
          <Progress value={gdBackupProgress.percent} class="h-2" />
        </div>
      {/if}

      {#if gdIsSyncing && gdSyncProgress.stage}
        <div class="space-y-1">
          <div class="flex items-center justify-between text-xs text-muted-foreground">
            <span>{gdSyncProgress.message || gdSyncProgress.stage}</span>
            <span>{gdSyncProgress.percent}%</span>
          </div>
          <Progress value={gdSyncProgress.percent} class="h-2" />
        </div>
      {/if}

      {#if gdBackupStatus}
        <div class="flex items-center gap-2 text-sm pt-1">
          {#if gdBackupStatus.success}
            <Check class="size-4 text-green-500" />
            <span class="text-green-600 text-xs">{gdBackupStatus.message}</span>
          {:else}
            <AlertCircle class="size-4 text-destructive" />
            <span class="text-destructive text-xs">{gdBackupStatus.message}</span>
          {/if}
        </div>
      {/if}

      <!-- Sync Status -->
      {#if gdSyncStatus}
        <div class="space-y-2">
          <div class="flex items-center gap-2 text-sm">
            {#if gdSyncStatus.success}
              <Check class="size-4 text-green-500" />
              <span class="text-green-600 text-xs">{gdSyncStatus.message}</span>
            {:else}
              <AlertCircle class="size-4 text-destructive" />
              <span class="text-destructive text-xs">{gdSyncStatus.message}</span>
            {/if}
          </div>

          {#if gdSyncStatus.success && (gdSyncStatus.uploaded || gdSyncStatus.downloaded)}
            <div class="flex items-center gap-4 text-xs text-muted-foreground">
              {#if gdSyncStatus.uploaded}
                <span class="flex items-center gap-1">
                  <Upload class="size-3" />{gdSyncStatus.uploaded} uploaded
                </span>
              {/if}
              {#if gdSyncStatus.downloaded}
                <span class="flex items-center gap-1">
                  <Download class="size-3" />{gdSyncStatus.downloaded} downloaded
                </span>
              {/if}
            </div>
          {/if}

          {#if gdSyncStatus.conflicts && gdSyncStatus.conflicts.length > 0}
            <div class="p-2 border border-yellow-500 rounded bg-yellow-50 dark:bg-yellow-950">
              <p class="text-xs font-medium text-yellow-700 dark:text-yellow-400">
                {gdSyncStatus.conflicts.length} conflict(s) detected:
              </p>
              <ul class="mt-1 text-xs text-yellow-600 dark:text-yellow-500">
                {#each gdSyncStatus.conflicts as conflict}
                  <li>{conflict.path}</li>
                {/each}
              </ul>
            </div>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>
