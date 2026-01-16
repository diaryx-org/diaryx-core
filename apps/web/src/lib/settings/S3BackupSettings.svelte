<script lang="ts">
  /**
   * S3BackupSettings - S3/S3-Compatible storage backup and sync configuration
   *
   * Self-contained component for S3 backup configuration, execution, and bidirectional sync.
   */
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import { Progress } from "$lib/components/ui/progress";
  import { Loader2, Check, AlertCircle, RefreshCw, Upload, Download } from "@lucide/svelte";
  import { getBackend } from "../backend";
  import { getS3Config, storeS3Config, removeS3Credentials } from "../credentials";

  interface Props {
    workspacePath?: string | null;
    onCancel?: () => void;
  }

  let { workspacePath = null, onCancel }: Props = $props();

  // S3 Configuration state
  let s3Config = $state({
    name: "S3 Backup",
    bucket: "",
    region: "us-east-1",
    prefix: "",
    endpoint: "",
    access_key: "",
    secret_key: "",
  });

  let s3Testing = $state(false);
  let s3TestResult: { success: boolean; message: string } | null = $state(null);
  let s3BackupStatus: { success: boolean; message: string } | null = $state(null);
  let s3IsBackingUp = $state(false);
  let s3BackupProgress = $state({ stage: "", percent: 0 });
  let s3ConfigSaved = $state(false);

  // Sync state
  let s3IsSyncing = $state(false);
  let s3SyncProgress = $state({ stage: "", percent: 0, message: "" });
  let s3SyncStatus: {
    success: boolean;
    message: string;
    uploaded?: number;
    downloaded?: number;
    deleted?: number;
    conflicts?: Array<{ path: string; local_modified: number | null; remote_modified: string | null }>;
  } | null = $state(null);

  // Load saved config on mount
  $effect(() => {
    loadSavedS3Config();
  });

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

  $effect(() => {
    // Check if valid config to enable backup
  });

  const isConfigValid = $derived(
    s3Config.bucket && s3Config.access_key && s3Config.secret_key
  );

  async function syncToS3() {
    s3IsSyncing = true;
    s3SyncStatus = null;
    s3SyncProgress = { stage: "Starting...", percent: 0, message: "" };
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
            s3SyncProgress = {
              stage: stageLabels[stage] || stage,
              percent,
              message: message || "",
            };
          });
        } catch (e) {
          console.warn("Failed to listen for sync progress:", e);
        }

        const result = await invoke("sync_to_s3", {
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

        if (result.success) {
          const parts = [];
          if (result.files_uploaded > 0) parts.push(`${result.files_uploaded} uploaded`);
          if (result.files_downloaded > 0) parts.push(`${result.files_downloaded} downloaded`);
          if (result.files_deleted > 0) parts.push(`${result.files_deleted} deleted`);
          const message = parts.length > 0 ? `Sync complete: ${parts.join(", ")}` : "Already in sync!";

          s3SyncStatus = {
            success: true,
            message,
            uploaded: result.files_uploaded,
            downloaded: result.files_downloaded,
            deleted: result.files_deleted,
            conflicts: result.conflicts,
          };
          await saveS3ConfigToVault();
        } else {
          s3SyncStatus = {
            success: false,
            message: result.error || "Sync failed",
          };
        }
      } else {
        s3SyncStatus = {
          success: false,
          message: "S3 sync is only available in the desktop app",
        };
      }
    } catch (e) {
      s3SyncStatus = {
        success: false,
        message: e instanceof Error ? e.message : String(e),
      };
    } finally {
      if (unlisten) unlisten();
      s3IsSyncing = false;
    }
  }
</script>

<div class="space-y-3">
  <div class="flex items-center justify-between">
    <span class="text-sm font-medium">S3 / S3-Compatible Storage</span>
    <Button variant="ghost" size="sm" onclick={onCancel}>Cancel</Button>
  </div>

  <!-- Bucket and Region -->
  <div class="grid grid-cols-2 gap-2">
    <div class="space-y-1">
      <Label for="s3-bucket" class="text-xs">Bucket *</Label>
      <input
        id="s3-bucket"
        type="text"
        bind:value={s3Config.bucket}
        placeholder="my-backup-bucket"
        class="w-full px-2 py-1 text-base md:text-sm border rounded bg-background"
      />
    </div>
    <div class="space-y-1">
      <Label for="s3-region" class="text-xs">Region *</Label>
      <input
        id="s3-region"
        type="text"
        bind:value={s3Config.region}
        placeholder="us-east-1"
        class="w-full px-2 py-1 text-base md:text-sm border rounded bg-background"
      />
    </div>
  </div>

  <!-- Prefix -->
  <div class="space-y-1">
    <Label for="s3-prefix" class="text-xs">Prefix (optional)</Label>
    <input
      id="s3-prefix"
      type="text"
      bind:value={s3Config.prefix}
      placeholder="backups/diaryx"
      class="w-full px-2 py-1 text-base md:text-sm border rounded bg-background"
    />
  </div>

  <!-- Custom Endpoint -->
  <div class="space-y-1">
    <Label for="s3-endpoint" class="text-xs">Custom Endpoint (for MinIO, etc.)</Label>
    <input
      id="s3-endpoint"
      type="text"
      bind:value={s3Config.endpoint}
      placeholder="https://minio.example.com"
      class="w-full px-2 py-1 text-base md:text-sm border rounded bg-background"
    />
  </div>

  <!-- Credentials -->
  <div class="grid grid-cols-2 gap-2">
    <div class="space-y-1">
      <Label for="s3-access-key" class="text-xs">Access Key *</Label>
      <input
        id="s3-access-key"
        type="password"
        bind:value={s3Config.access_key}
        placeholder="AKIA..."
        class="w-full px-2 py-1 text-base md:text-sm border rounded bg-background"
      />
    </div>
    <div class="space-y-1">
      <Label for="s3-secret-key" class="text-xs">Secret Key *</Label>
      <input
        id="s3-secret-key"
        type="password"
        bind:value={s3Config.secret_key}
        placeholder="••••••••"
        class="w-full px-2 py-1 text-base md:text-sm border rounded bg-background"
      />
    </div>
  </div>

  <!-- Actions -->
  <div class="flex flex-wrap items-center gap-2 pt-2">
    <Button
      variant="outline"
      size="sm"
      onclick={testS3Connection}
      disabled={s3Testing || !isConfigValid}
    >
      {s3Testing ? "Testing..." : "Test Connection"}
    </Button>
    <Button
      variant="default"
      size="sm"
      onclick={syncToS3}
      disabled={s3IsSyncing || s3IsBackingUp || !isConfigValid}
    >
      {#if s3IsSyncing}
        <RefreshCw class="mr-2 size-4 animate-spin" />Syncing...
      {:else}
        <RefreshCw class="mr-2 size-4" />Sync Now
      {/if}
    </Button>
    <Button
      variant="outline"
      size="sm"
      onclick={backupToS3}
      disabled={s3IsBackingUp || s3IsSyncing || !isConfigValid}
    >
      {#if s3IsBackingUp}
        <Loader2 class="mr-2 size-4 animate-spin" />Backing up...
      {:else}
        <Upload class="mr-2 size-4" />Full Backup
      {/if}
    </Button>
  </div>

  <!-- Sync explanation -->
  <p class="text-xs text-muted-foreground">
    <strong>Sync</strong> keeps local and cloud files in sync (bidirectional). <strong>Full Backup</strong> creates a one-time ZIP archive.
  </p>

  <!-- Backup Progress -->
  {#if s3IsBackingUp && s3BackupProgress.stage}
    <div class="space-y-1">
      <div class="flex items-center justify-between text-xs text-muted-foreground">
        <span>{s3BackupProgress.stage}</span>
        <span>{s3BackupProgress.percent}%</span>
      </div>
      <Progress value={s3BackupProgress.percent} class="h-2" />
    </div>
  {/if}

  <!-- Sync Progress -->
  {#if s3IsSyncing && s3SyncProgress.stage}
    <div class="space-y-1">
      <div class="flex items-center justify-between text-xs text-muted-foreground">
        <span>{s3SyncProgress.message || s3SyncProgress.stage}</span>
        <span>{s3SyncProgress.percent}%</span>
      </div>
      <Progress value={s3SyncProgress.percent} class="h-2" />
    </div>
  {/if}

  <!-- Test Result -->
  {#if s3TestResult}
    <div class="flex items-center gap-2 text-sm">
      {#if s3TestResult.success}
        <Check class="size-4 text-green-500" />
        <span class="text-green-600">{s3TestResult.message}</span>
      {:else}
        <AlertCircle class="size-4 text-destructive" />
        <span class="text-destructive">{s3TestResult.message}</span>
      {/if}
    </div>
  {/if}

  <!-- Backup Status -->
  {#if s3BackupStatus}
    <div class="flex items-center gap-2 text-sm">
      {#if s3BackupStatus.success}
        <Check class="size-4 text-green-500" />
        <span class="text-green-600">{s3BackupStatus.message}</span>
      {:else}
        <AlertCircle class="size-4 text-destructive" />
        <span class="text-destructive">{s3BackupStatus.message}</span>
      {/if}
    </div>
  {/if}

  <!-- Sync Status -->
  {#if s3SyncStatus}
    <div class="space-y-2">
      <div class="flex items-center gap-2 text-sm">
        {#if s3SyncStatus.success}
          <Check class="size-4 text-green-500" />
          <span class="text-green-600">{s3SyncStatus.message}</span>
        {:else}
          <AlertCircle class="size-4 text-destructive" />
          <span class="text-destructive">{s3SyncStatus.message}</span>
        {/if}
      </div>

      {#if s3SyncStatus.success && (s3SyncStatus.uploaded || s3SyncStatus.downloaded)}
        <div class="flex items-center gap-4 text-xs text-muted-foreground">
          {#if s3SyncStatus.uploaded}
            <span class="flex items-center gap-1">
              <Upload class="size-3" />{s3SyncStatus.uploaded} uploaded
            </span>
          {/if}
          {#if s3SyncStatus.downloaded}
            <span class="flex items-center gap-1">
              <Download class="size-3" />{s3SyncStatus.downloaded} downloaded
            </span>
          {/if}
        </div>
      {/if}

      {#if s3SyncStatus.conflicts && s3SyncStatus.conflicts.length > 0}
        <div class="p-2 border border-yellow-500 rounded bg-yellow-50 dark:bg-yellow-950">
          <p class="text-xs font-medium text-yellow-700 dark:text-yellow-400">
            {s3SyncStatus.conflicts.length} conflict(s) detected:
          </p>
          <ul class="mt-1 text-xs text-yellow-600 dark:text-yellow-500">
            {#each s3SyncStatus.conflicts as conflict}
              <li>{conflict.path}</li>
            {/each}
          </ul>
        </div>
      {/if}
    </div>
  {/if}

  <!-- Saved Config Actions -->
  {#if s3ConfigSaved}
    <div class="flex items-center justify-between border-t pt-3 mt-3">
      <span class="text-xs text-muted-foreground flex items-center gap-1">
        <Check class="size-3" />Credentials saved securely
      </span>
      <Button
        variant="ghost"
        size="sm"
        onclick={clearSavedS3Config}
        class="text-xs text-muted-foreground hover:text-destructive"
      >
        Clear saved
      </Button>
    </div>
  {/if}
</div>
