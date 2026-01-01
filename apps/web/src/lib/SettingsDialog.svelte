<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Switch } from "$lib/components/ui/switch";
  import { Label } from "$lib/components/ui/label";
  import { Progress } from "$lib/components/ui/progress";
  import {
    Settings,
    Info,
    Eye,
    Save,
    Check,
    AlertCircle,
    Loader2,
    Upload,
  } from "@lucide/svelte";
  import { getBackend } from "./backend";
  import {
    getS3Config,
    storeS3Config,
    removeS3Credentials,
    getGoogleDriveRefreshToken,
    storeGoogleDriveRefreshToken,
    getGoogleDriveFolderId,
    storeGoogleDriveFolderId,
    getGoogleDriveCredentials,
    storeGoogleDriveCredentials,
    removeGoogleDriveCredentials,
  } from "./credentials";
  import type { BackupStatus } from "./backend/interface";

  interface Props {
    open?: boolean;
    showUnlinkedFiles?: boolean;
    showHiddenFiles?: boolean;
    workspacePath?: string | null;
  }

  let {
    open = $bindable(),
    showUnlinkedFiles = $bindable(),
    showHiddenFiles = $bindable(false),
    workspacePath = null,
  }: Props = $props();

  // Config info state
  let config: Record<string, unknown> | null = $state(null);
  let appPaths: Record<string, string> | null = $state(null);

  // Backup state
  let backupTargets: string[] = $state([]);
  let backupStatus: BackupStatus[] | null = $state(null);
  let isBackingUp: boolean = $state(false);
  let backupError: string | null = $state(null);

  // Import state
  let isImporting: boolean = $state(false);
  let importResult: {
    success: boolean;
    files_imported: number;
    error?: string;
  } | null = $state(null);

  // Load config when dialog opens
  $effect(() => {
    if (open) {
      loadConfig();
      loadBackupTargets();
      loadSavedS3Config();
      loadSavedGoogleDriveConfig();
    }
  });

  async function loadConfig() {
    try {
      const backend = await getBackend();
      if ("getConfig" in backend && typeof backend.getConfig === "function") {
        config = await (backend as any).getConfig();
      }
      if ("getInvoke" in backend) {
        try {
          appPaths = await (backend as any).getInvoke()("get_app_paths", {});
        } catch (e) {}
      }
      // no-op
    } catch (e) {
      console.warn("Failed to load config:", e);
    }
  }

  async function loadBackupTargets() {
    try {
      const backend = await getBackend();
      if ("getInvoke" in backend) {
        const invoke = (backend as any).getInvoke();
        backupTargets = await invoke("list_backup_targets", {});
      } else {
        backupTargets = ["IndexedDB (Local)"];
      }
    } catch (e) {
      backupTargets = [];
    }
  }

  async function performBackup() {
    isBackingUp = true;
    backupError = null;
    backupStatus = null;
    try {
      const backend = await getBackend();
      if ("getInvoke" in backend) {
        const invoke = (backend as any).getInvoke();
        backupStatus = await invoke("backup_workspace", {});
      } else {
        await backend.persist();
        backupStatus = [
          {
            target_name: "IndexedDB (Local)",
            success: true,
            files_processed: 0,
            error: undefined,
          },
        ];
      }
    } catch (e) {
      backupError = e instanceof Error ? e.message : String(e);
    } finally {
      isBackingUp = false;
    }
  }

  // Modular Backup Provider state
  type BackupProvider = "s3" | "google_drive" | "webdav" | null;
  let selectedProvider: BackupProvider = $state(null);
  let showNewBackupForm: boolean = $state(false);

  // S3 Cloud Backup state
  let s3Config = $state({
    name: "S3 Backup",
    bucket: "",
    region: "us-east-1",
    prefix: "",
    endpoint: "",
    access_key: "",
    secret_key: "",
  });
  let s3Testing: boolean = $state(false);
  let s3TestResult: { success: boolean; message: string } | null = $state(null);
  let s3BackupStatus: { success: boolean; message: string } | null =
    $state(null);
  let s3IsBackingUp: boolean = $state(false);
  let s3BackupProgress: { stage: string; percent: number } = $state({
    stage: "",
    percent: 0,
  });
  let s3ConfigSaved: boolean = $state(false);

  async function loadSavedS3Config() {
    try {
      const backend = await getBackend();
      if ("getInvoke" in backend) {
        const savedConfig = await getS3Config();
        if (savedConfig) {
          s3Config = {
            ...savedConfig,
            prefix: savedConfig.prefix ?? "",
            endpoint: savedConfig.endpoint ?? "",
          };
          s3ConfigSaved = true;
          showNewBackupForm = true;
          selectedProvider = "s3";
        }
      }
    } catch (e) {
      console.warn("Failed to load saved S3 config:", e);
    }
  }

  async function saveS3ConfigToVault() {
    try {
      await storeS3Config(s3Config);
      s3ConfigSaved = true;
    } catch (e) {
      console.error("Failed to save S3 config:", e);
    }
  }

  async function clearSavedS3Config() {
    try {
      await removeS3Credentials();
      s3ConfigSaved = false;
      s3Config = {
        name: "S3 Backup",
        bucket: "",
        region: "us-east-1",
        prefix: "",
        endpoint: "",
        access_key: "",
        secret_key: "",
      };
    } catch (e) {
      console.error("Failed to clear S3 config:", e);
    }
  }

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
  let gdBackupProgress: { stage: string; percent: number } = $state({
    stage: "",
    percent: 0,
  });
  let gdBackupStatus: { success: boolean; message: string } | null =
    $state(null);
  let gdAccessToken: string | null = null;

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

            const { clientId, clientSecret } =
              await getGoogleDriveCredentials();
            if (clientId) gdConfig.client_id = clientId;
            if (clientSecret) gdConfig.client_secret = clientSecret;

            showNewBackupForm = true;
            selectedProvider = "google_drive";
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

      // Fetch from backend if not already provided in state
      if ("getInvoke" in backend && (!clientId || !clientSecret)) {
        const invoke = (backend as any).getInvoke();
        const config = await invoke("get_google_auth_config", {});
        if (!clientId) clientId = config.client_id;
        if (!clientSecret) clientSecret = config.client_secret;
      }

      if (!clientId) {
        alert(
          "Google Client ID not found. Please ensure it is set in the backend environment.",
        );
        return;
      }

      const response = await signIn({
        clientId,
        ...(clientSecret ? { clientSecret } : {}),
        scopes: ["https://www.googleapis.com/auth/drive.file"],
      });

      gdAccessToken = response.accessToken;
      if (response.refreshToken) {
        await storeGoogleDriveRefreshToken(response.refreshToken);
      }

      // Save any manually entered credentials for next time
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

  // Live Sync functions

  // Reference to hidden file input
  let fileInputRef: HTMLInputElement | null = $state(null);

  function triggerFileInput() {
    fileInputRef?.click();
  }

  async function handleFileSelected(event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];

    if (!file) return;

    isImporting = true;
    importResult = null;

    try {
      const backend = await getBackend();

      // Use backend's importFromZip method (handles chunked upload internally)
      // Note: workspacePath is the workspace index file path (e.g., Diaryx/README.md)
      // We need just the directory path for import
      const workspaceDir = workspacePath
        ? workspacePath.substring(0, workspacePath.lastIndexOf("/"))
        : undefined;

      const result = await backend.importFromZip(
        file,
        workspaceDir,
        (uploaded, total) => {
          // Could update progress UI here if desired
          if (uploaded % (10 * 1024 * 1024) < 1024 * 1024) {
            console.log(
              `[Import] Progress: ${(uploaded / 1024 / 1024).toFixed(1)} / ${(total / 1024 / 1024).toFixed(1)} MB`,
            );
          }
        },
      );

      importResult = result;

      // Notify that files were imported - might need refresh
      if (result.success) {
        window.dispatchEvent(
          new CustomEvent("import:complete", { detail: result }),
        );
      }
    } catch (e) {
      console.error("Import failed:", e);
      importResult = {
        success: false,
        files_imported: 0,
        error: e instanceof Error ? e.message : String(e),
      };
    } finally {
      isImporting = false;
      // Reset file input so same file can be selected again
      if (input) input.value = "";
    }
  }

  function selectProvider(provider: BackupProvider) {
    selectedProvider = provider;
  }

  function cancelNewBackup() {
    showNewBackupForm = false;
    selectedProvider = null;
  }

  async function testS3Connection() {
    s3Testing = true;
    s3TestResult = null;
    try {
      const backend = await getBackend();
      if ("getInvoke" in backend) {
        const invoke = (backend as any).getInvoke();
        const result = await invoke("test_s3_connection", {
          config: {
            name: s3Config.name,
            bucket: s3Config.bucket,
            region: s3Config.region,
            prefix: s3Config.prefix || null,
            endpoint: s3Config.endpoint || null,
            access_key: s3Config.access_key,
            secret_key: s3Config.secret_key,
          },
        });
        s3TestResult = {
          success: result,
          message: result ? "Connection successful!" : "Connection failed",
        };
      } else {
        s3TestResult = {
          success: false,
          message: "S3 backup is only available in the desktop app",
        };
      }
    } catch (e) {
      s3TestResult = {
        success: false,
        message: e instanceof Error ? e.message : String(e),
      };
    } finally {
      s3Testing = false;
    }
  }

  async function backupToS3() {
    s3IsBackingUp = true;
    s3BackupStatus = null;
    s3BackupProgress = { stage: "Preparing backup...", percent: 5 };
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
            s3BackupProgress = {
              stage:
                message ||
                (stage === "zipping"
                  ? "Creating zip archive..."
                  : stage === "uploading"
                    ? "Uploading to S3..."
                    : stage === "complete"
                      ? "Complete!"
                      : stage === "preparing"
                        ? "Preparing backup..."
                        : stage),
              percent,
            };
          });
        } catch (e) {}
        const result = await invoke("backup_to_s3", {
          workspacePath: workspacePath
            ? workspacePath.substring(0, workspacePath.lastIndexOf("/"))
            : null,
          config: {
            name: s3Config.name,
            bucket: s3Config.bucket,
            region: s3Config.region,
            prefix: s3Config.prefix || null,
            endpoint: s3Config.endpoint || null,
            access_key: s3Config.access_key,
            secret_key: s3Config.secret_key,
          },
        });
        s3BackupProgress = { stage: "Complete!", percent: 100 };
        s3BackupStatus = {
          success: result.success,
          message: result.success
            ? `Backup complete! ${result.files_processed} files uploaded.`
            : result.error || "Backup failed",
        };
        if (result.success) {
          await saveS3ConfigToVault();
        }
      }
    } catch (e) {
      s3BackupStatus = {
        success: false,
        message: e instanceof Error ? e.message : String(e),
      };
    } finally {
      if (unlisten) unlisten();
      s3IsBackingUp = false;
      s3BackupProgress = { stage: "", percent: 0 };
    }
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="sm:max-w-[500px] max-h-[80vh] overflow-y-auto">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        <Settings class="size-5" />
        Settings
      </Dialog.Title>
      <Dialog.Description>
        Configuration and debugging information.
      </Dialog.Description>
    </Dialog.Header>

    <div class="py-4 space-y-4">
      <!-- Display Settings -->
      <div class="space-y-3">
        <h3 class="font-medium flex items-center gap-2">
          <Eye class="size-4" />
          Display
        </h3>

        <div class="flex items-center justify-between gap-4 px-1">
          <Label
            for="show-unlinked"
            class="text-sm cursor-pointer flex flex-col gap-0.5"
          >
            <span>Show all files</span>
            <span class="font-normal text-xs text-muted-foreground">
              Switch to a filesystem view to see files not linked in hierarchy.
            </span>
          </Label>
          <Switch id="show-unlinked" bind:checked={showUnlinkedFiles} />
        </div>

        <div class="flex items-center justify-between gap-4 px-1">
          <Label
            for="show-hidden"
            class="text-sm cursor-pointer flex flex-col gap-0.5"
          >
            <span>Show hidden files</span>
            <span class="font-normal text-xs text-muted-foreground">
              Show files starting with dot (.git, .DS_Store) in filesystem view.
            </span>
          </Label>
          <Switch
            id="show-hidden"
            bind:checked={showHiddenFiles}
            disabled={!showUnlinkedFiles}
          />
        </div>
      </div>

      <!-- Backup Section -->
      <div class="space-y-3">
        <h3 class="font-medium flex items-center gap-2">
          <Save class="size-4" />
          Backup
        </h3>

        {#if backupTargets.length > 0}
          <div class="text-sm text-muted-foreground px-1">
            Configured targets: {backupTargets.join(", ")}
          </div>
        {/if}

        <div class="flex items-center gap-2 px-1">
          <Button
            variant="outline"
            size="sm"
            onclick={performBackup}
            disabled={isBackingUp}
          >
            {isBackingUp ? "Backing up..." : "Backup Now"}
          </Button>
        </div>

        {#if backupStatus}
          <div class="space-y-1 px-1">
            {#each backupStatus as status}
              <div class="flex items-center gap-2 text-sm">
                {#if status.success}
                  <Check class="size-4 text-green-500" />
                  <span
                    >{status.target_name}: {status.files_processed} files</span
                  >
                {:else}
                  <AlertCircle class="size-4 text-destructive" />
                  <span class="text-destructive"
                    >{status.target_name}: {status.error}</span
                  >
                {/if}
              </div>
            {/each}
          </div>
        {/if}

        {#if backupError}
          <div class="text-destructive text-sm p-2 bg-destructive/10 rounded">
            Backup failed: {backupError}
          </div>
        {/if}
      </div>

      <!-- Import Section -->
      <div class="space-y-3">
        <h3 class="font-medium flex items-center gap-2">
          <Upload class="size-4" />
          Import
        </h3>

        <div class="space-y-2 px-1">
          <p class="text-xs text-muted-foreground">
            Import files from a backup zip archive.
          </p>

          <!-- Hidden file input for iOS-compatible picker -->
          <input
            type="file"
            accept=".zip,application/zip"
            class="hidden"
            bind:this={fileInputRef}
            onchange={handleFileSelected}
          />

          <Button
            variant="outline"
            size="sm"
            onclick={triggerFileInput}
            disabled={isImporting}
          >
            {#if isImporting}
              <Loader2 class="size-4 mr-2 animate-spin" />
              Importing...
            {:else}
              Select Zip File...
            {/if}
          </Button>

          {#if importResult}
            {#if importResult.success}
              <div
                class="flex items-center gap-2 text-sm text-green-600 bg-green-50 dark:bg-green-950/20 p-2 rounded"
              >
                <Check class="size-4" />
                <span
                  >Imported {importResult.files_imported} files. Refresh to see changes.</span
                >
              </div>
            {:else}
              <div
                class="flex items-center gap-2 text-sm text-destructive bg-destructive/10 p-2 rounded"
              >
                <AlertCircle class="size-4" />
                <span>{importResult.error || "Import failed"}</span>
              </div>
            {/if}
          {/if}
        </div>
      </div>

      <!-- Cloud Backup Section -->
      <div class="space-y-3">
        <h3 class="font-medium flex items-center gap-2">
          <Save class="size-4" />
          Cloud Backup
        </h3>

        {#if !showNewBackupForm}
          <Button
            variant="outline"
            size="sm"
            onclick={() => (showNewBackupForm = true)}
          >
            + New Backup
          </Button>
        {:else}
          <div class="space-y-3 p-3 border rounded-lg bg-muted/30">
            {#if !selectedProvider}
              <!-- Provider Selection -->
              <div class="space-y-2">
                <Label for="backup-provider" class="text-sm font-medium"
                  >Select Backup Provider</Label
                >
                <select
                  id="backup-provider"
                  class="w-full px-3 py-2 text-sm border rounded bg-background"
                  onchange={(e) =>
                    selectProvider(
                      (e.target as HTMLSelectElement).value as BackupProvider,
                    )}
                >
                  <option value="">Choose a provider...</option>
                  <option value="s3">Amazon S3 / S3-Compatible</option>
                  <option value="google_drive">Google Drive</option>
                  <option value="webdav" disabled>WebDAV (coming soon)</option>
                </select>
              </div>
              <Button variant="ghost" size="sm" onclick={cancelNewBackup}
                >Cancel</Button
              >
            {:else if selectedProvider === "s3"}
              <!-- S3 Configuration Form -->
              <div class="space-y-3">
                <div class="flex items-center justify-between">
                  <span class="text-sm font-medium"
                    >S3 / S3-Compatible Storage</span
                  >
                  <Button variant="ghost" size="sm" onclick={cancelNewBackup}
                    >Cancel</Button
                  >
                </div>
                <!-- S3 form content same as before ... -->
                <div class="grid grid-cols-2 gap-2">
                  <div class="space-y-1">
                    <Label for="s3-bucket" class="text-xs">Bucket *</Label>
                    <input
                      id="s3-bucket"
                      type="text"
                      bind:value={s3Config.bucket}
                      placeholder="my-backup-bucket"
                      class="w-full px-2 py-1 text-sm border rounded bg-background"
                    />
                  </div>
                  <div class="space-y-1">
                    <Label for="s3-region" class="text-xs">Region *</Label>
                    <input
                      id="s3-region"
                      type="text"
                      bind:value={s3Config.region}
                      placeholder="us-east-1"
                      class="w-full px-2 py-1 text-sm border rounded bg-background"
                    />
                  </div>
                </div>
                <div class="space-y-1">
                  <Label for="s3-prefix" class="text-xs"
                    >Prefix (optional)</Label
                  >
                  <input
                    id="s3-prefix"
                    type="text"
                    bind:value={s3Config.prefix}
                    placeholder="backups/diaryx"
                    class="w-full px-2 py-1 text-sm border rounded bg-background"
                  />
                </div>
                <div class="space-y-1">
                  <Label for="s3-endpoint" class="text-xs"
                    >Custom Endpoint (for MinIO, etc.)</Label
                  >
                  <input
                    id="s3-endpoint"
                    type="text"
                    bind:value={s3Config.endpoint}
                    placeholder="https://minio.example.com"
                    class="w-full px-2 py-1 text-sm border rounded bg-background"
                  />
                </div>
                <div class="grid grid-cols-2 gap-2">
                  <div class="space-y-1">
                    <Label for="s3-access-key" class="text-xs"
                      >Access Key *</Label
                    >
                    <input
                      id="s3-access-key"
                      type="password"
                      bind:value={s3Config.access_key}
                      placeholder="AKIA..."
                      class="w-full px-2 py-1 text-sm border rounded bg-background"
                    />
                  </div>
                  <div class="space-y-1">
                    <Label for="s3-secret-key" class="text-xs"
                      >Secret Key *</Label
                    >
                    <input
                      id="s3-secret-key"
                      type="password"
                      bind:value={s3Config.secret_key}
                      placeholder="••••••••"
                      class="w-full px-2 py-1 text-sm border rounded bg-background"
                    />
                  </div>
                </div>
                <div class="flex items-center gap-2 pt-2">
                  <Button
                    variant="outline"
                    size="sm"
                    onclick={testS3Connection}
                    disabled={s3Testing ||
                      !s3Config.bucket ||
                      !s3Config.access_key ||
                      !s3Config.secret_key}
                    >{s3Testing ? "Testing..." : "Test Connection"}</Button
                  >
                  <Button
                    variant="default"
                    size="sm"
                    onclick={backupToS3}
                    disabled={s3IsBackingUp ||
                      !s3Config.bucket ||
                      !s3Config.access_key ||
                      !s3Config.secret_key}
                  >
                    {#if s3IsBackingUp}<Loader2
                        class="mr-2 size-4 animate-spin"
                      />Backing up...{:else}Backup Now{/if}
                  </Button>
                </div>
                {#if s3IsBackingUp && s3BackupProgress.stage}
                  <div class="space-y-1">
                    <div
                      class="flex items-center justify-between text-xs text-muted-foreground"
                    >
                      <span>{s3BackupProgress.stage}</span><span
                        >{s3BackupProgress.percent}%</span
                      >
                    </div>
                    <Progress value={s3BackupProgress.percent} class="h-2" />
                  </div>
                {/if}
                {#if s3TestResult}<div class="flex items-center gap-2 text-sm">
                    {#if s3TestResult.success}<Check
                        class="size-4 text-green-500"
                      /><span class="text-green-600"
                        >{s3TestResult.message}</span
                      >{:else}<AlertCircle
                        class="size-4 text-destructive"
                      /><span class="text-destructive"
                        >{s3TestResult.message}</span
                      >{/if}
                  </div>{/if}
                {#if s3BackupStatus}<div
                    class="flex items-center gap-2 text-sm"
                  >
                    {#if s3BackupStatus.success}<Check
                        class="size-4 text-green-500"
                      /><span class="text-green-600"
                        >{s3BackupStatus.message}</span
                      >{:else}<AlertCircle
                        class="size-4 text-destructive"
                      /><span class="text-destructive"
                        >{s3BackupStatus.message}</span
                      >{/if}
                  </div>{/if}
                {#if s3ConfigSaved}
                  <div
                    class="flex items-center justify-between border-t pt-3 mt-3"
                  >
                    <span
                      class="text-xs text-muted-foreground flex items-center gap-1"
                      ><Check class="size-3" />Credentials saved securely</span
                    >
                    <Button
                      variant="ghost"
                      size="sm"
                      onclick={clearSavedS3Config}
                      class="text-xs text-muted-foreground hover:text-destructive"
                      >Clear saved</Button
                    >
                  </div>
                {/if}
              </div>
            {:else if selectedProvider === "google_drive"}
              <!-- Google Drive Configuration Form -->
              <div class="space-y-3">
                <div class="flex items-center justify-between">
                  <span class="text-sm font-medium">Google Drive Backup</span>
                  <Button variant="ghost" size="sm" onclick={cancelNewBackup}
                    >Cancel</Button
                  >
                </div>

                {#if !gdIsAuthenticated}
                  <div class="py-4 flex flex-col items-center gap-4">
                    <p class="text-sm text-center text-muted-foreground px-4">
                      Connect your Google account to back up your diary to
                      Google Drive.
                    </p>

                    <Button
                      variant="outline"
                      onclick={signInWithGoogle}
                      disabled={gdAuthLoading}
                      class="w-full"
                    >
                      {#if gdAuthLoading}<Loader2
                          class="mr-2 size-4 animate-spin"
                        />{:else}<span class="mr-2">G</span>{/if}
                      Sign in with Google
                    </Button>

                    <p
                      class="text-[10px] text-muted-foreground text-center px-4"
                    >
                      Note: Your diary files will be stored in a dedicated
                      folder in your Google Drive. Diaryx only accesses files it
                      creates.
                    </p>
                  </div>
                {:else}
                  <div class="space-y-3">
                    <div class="flex items-center justify-between">
                      <span
                        class="text-xs font-medium text-green-600 flex items-center gap-1"
                      >
                        <Check class="size-3" /> Connected to Google Drive
                      </span>
                      <Button
                        variant="ghost"
                        size="sm"
                        onclick={disconnectGoogleDrive}
                        class="text-[10px] h-6 text-muted-foreground"
                        >Disconnect</Button
                      >
                    </div>

                    <div class="space-y-1">
                      <Label for="gd-folder" class="text-xs"
                        >Target Folder ID (optional)</Label
                      >
                      <input
                        id="gd-folder"
                        type="text"
                        bind:value={gdConfig.folder_id}
                        placeholder="Leave blank for root"
                        class="w-full px-2 py-1 text-sm border rounded bg-background"
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
                        {#if gdIsBackingUp}<Loader2
                            class="mr-2 size-4 animate-spin"
                          />Backing up...{:else}Backup Now{/if}
                      </Button>
                    </div>

                    {#if gdIsBackingUp && gdBackupProgress.stage}
                      <div class="space-y-1">
                        <div
                          class="flex items-center justify-between text-xs text-muted-foreground"
                        >
                          <span>{gdBackupProgress.stage}</span>
                          <span>{gdBackupProgress.percent}%</span>
                        </div>
                        <Progress
                          value={gdBackupProgress.percent}
                          class="h-2"
                        />
                      </div>
                    {/if}

                    {#if gdBackupStatus}
                      <div class="flex items-center gap-2 text-sm pt-1">
                        {#if gdBackupStatus.success}
                          <Check class="size-4 text-green-500" />
                          <span class="text-green-600 text-xs"
                            >{gdBackupStatus.message}</span
                          >
                        {:else}
                          <AlertCircle class="size-4 text-destructive" />
                          <span class="text-destructive text-xs"
                            >{gdBackupStatus.message}</span
                          >
                        {/if}
                      </div>
                    {/if}
                  </div>
                {/if}
              </div>
            {/if}
          </div>
        {/if}
      </div>

      <!-- Path Info -->
      {#if appPaths}
        <div class="space-y-2">
          <h3 class="font-medium flex items-center gap-2">
            <Info class="size-4" />App Paths
          </h3>
          <div class="bg-muted rounded p-3 text-xs font-mono space-y-1">
            {#each Object.entries(appPaths) as [key, value]}
              <div class="flex gap-2">
                <span class="text-muted-foreground min-w-[120px]">{key}:</span
                ><span class="break-all">{value}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      {#if config}
        <div class="space-y-2">
          <h3 class="font-medium flex items-center gap-2">
            <Settings class="size-4" />Config
          </h3>
          <div class="bg-muted rounded p-3 text-xs font-mono space-y-1">
            {#each Object.entries(config) as [key, value]}
              <div class="flex gap-2">
                <span class="text-muted-foreground min-w-[120px]">{key}:</span
                ><span class="break-all"
                  >{typeof value === "object"
                    ? JSON.stringify(value)
                    : String(value ?? "null")}</span
                >
              </div>
            {/each}
          </div>
        </div>
      {/if}

      <div class="flex justify-end pt-2">
        <Button variant="outline" onclick={() => (open = false)}>Close</Button>
      </div>
    </div>
  </Dialog.Content>
</Dialog.Root>
