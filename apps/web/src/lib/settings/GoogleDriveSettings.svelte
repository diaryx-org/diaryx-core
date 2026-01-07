<script lang="ts">
  /**
   * GoogleDriveSettings - Google Drive backup configuration
   * 
   * Self-contained component for Google Drive authentication and backup.
   */
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import { Progress } from "$lib/components/ui/progress";
  import { Loader2, Check, AlertCircle } from "@lucide/svelte";
  import { getBackend } from "../backend";
  import {
    getGoogleDriveRefreshToken,
    storeGoogleDriveRefreshToken,
    getGoogleDriveFolderId,
    storeGoogleDriveFolderId,
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
    folder_id: "",
    client_id: "",
    client_secret: "",
  });
  let gdIsAuthenticated = $state(false);
  let gdAuthLoading = $state(false);
  let gdIsBackingUp = $state(false);
  let gdBackupProgress = $state({ stage: "", percent: 0 });
  let gdBackupStatus: { success: boolean; message: string } | null = $state(null);
  let gdAccessToken: string | null = $state(null);

  // Load saved config on mount
  $effect(() => {
    loadSavedGoogleDriveConfig();
  });

  async function loadSavedGoogleDriveConfig() {
    try {
      const backend = await getBackend();
      if ("getInvoke" in backend) {
        const refreshToken = await getGoogleDriveRefreshToken();
        if (refreshToken) {
          try {
            const { refreshToken: refresh } =
              await import("@choochmeque/tauri-plugin-google-auth-api");
            const response = await refresh();
            gdAccessToken = response.accessToken;
            gdIsAuthenticated = true;
            const folderId = await getGoogleDriveFolderId();
            if (folderId) gdConfig.folder_id = folderId;

            const { clientId, clientSecret } = await getGoogleDriveCredentials();
            if (clientId) gdConfig.client_id = clientId;
            if (clientSecret) gdConfig.client_secret = clientSecret;
          } catch (e) {
            console.warn("Failed to refresh Google token:", e);
            gdIsAuthenticated = false;
          }
        }
      }
    } catch (e) {
      console.warn("Failed to load Google Drive config:", e);
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
            const creds = await invoke("get_google_credentials", {});
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
            folder_id: gdConfig.folder_id || null,
          },
        });
        gdBackupProgress = { stage: "Complete!", percent: 100 };
        gdBackupStatus = {
          success: result.success,
          message: result.success
            ? `Backup complete! ${result.files_processed} files uploaded.`
            : result.error || "Backup failed",
        };
        if (gdConfig.folder_id) {
          await storeGoogleDriveFolderId(gdConfig.folder_id);
        }
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
</script>

<div class="space-y-3">
  <div class="flex items-center justify-between">
    <span class="text-sm font-medium">Google Drive Backup</span>
    <Button variant="ghost" size="sm" onclick={onCancel}>Cancel</Button>
  </div>

  {#if !gdIsAuthenticated}
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

      <div class="space-y-1">
        <Label for="gd-folder" class="text-xs">Target Folder ID (optional)</Label>
        <input
          id="gd-folder"
          type="text"
          bind:value={gdConfig.folder_id}
          placeholder="Leave blank for root"
          class="w-full px-2 py-1 text-base md:text-sm border rounded bg-background"
        />
      </div>

      <div class="pt-2">
        <Button
          variant="default"
          size="sm"
          onclick={backupToGoogleDrive}
          disabled={gdIsBackingUp}
          class="w-full"
        >
          {#if gdIsBackingUp}
            <Loader2 class="mr-2 size-4 animate-spin" />Backing up...
          {:else}
            Backup Now
          {/if}
        </Button>
      </div>

      {#if gdIsBackingUp && gdBackupProgress.stage}
        <div class="space-y-1">
          <div class="flex items-center justify-between text-xs text-muted-foreground">
            <span>{gdBackupProgress.stage}</span>
            <span>{gdBackupProgress.percent}%</span>
          </div>
          <Progress value={gdBackupProgress.percent} class="h-2" />
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
    </div>
  {/if}
</div>
