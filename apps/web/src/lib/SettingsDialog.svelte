<script lang="ts">
  /**
   * SettingsDialog - Main settings dialog component
   * 
   * Uses modular sub-components for different settings sections.
   */
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Switch } from "$lib/components/ui/switch";
  import { Label } from "$lib/components/ui/label";
  import {
    Settings,
    Info,
    Eye,
    Save,
    Check,
    AlertCircle,
    Loader2,
    Upload,
    Sun,
    Moon,
    Monitor,
    Wifi,
    WifiOff,
    RefreshCw,
    Trash2,
  } from "@lucide/svelte";
  import { Input } from "$lib/components/ui/input";
  import {
    setCollaborationServer,
    getCollaborationServer,
    clearAllDocumentCache,
  } from "./collaborationUtils";
  import { setWorkspaceServer } from "./workspaceCrdt";
  import { getThemeStore } from "./stores/theme.svelte";
  import { getBackend } from "./backend";
  import type { BackupStatus } from "./backend/interface";
  
  // Import modular settings components
  import S3BackupSettings from "./settings/S3BackupSettings.svelte";
  import GoogleDriveSettings from "./settings/GoogleDriveSettings.svelte";

  interface Props {
    open?: boolean;
    showUnlinkedFiles?: boolean;
    showHiddenFiles?: boolean;
    workspacePath?: string | null;
    collaborationEnabled?: boolean;
    collaborationConnected?: boolean;
    onCollaborationToggle?: (enabled: boolean) => void;
    onCollaborationReconnect?: () => void;
  }

  let {
    open = $bindable(),
    showUnlinkedFiles = $bindable(),
    showHiddenFiles = $bindable(false),
    workspacePath = null,
    collaborationEnabled = $bindable(false),
    collaborationConnected = false,
    onCollaborationToggle,
    onCollaborationReconnect,
  }: Props = $props();

  // Config info state
  let config: Record<string, unknown> | null = $state(null);
  let appPaths: Record<string, string> | null = $state(null);

  // Theme state
  const themeStore = getThemeStore();

  // Backup state
  let backupTargets: string[] = $state([]);
  let backupStatus: BackupStatus[] | null = $state(null);
  let isBackingUp: boolean = $state(false);
  let backupError: string | null = $state(null);

  // Sync server state
  let syncServerUrl = $state(
    typeof window !== "undefined"
      ? localStorage.getItem("diaryx-collab-server-url") ||
          getCollaborationServer() ||
          ""
      : "",
  );
  let isApplyingServer = $state(false);
  let isClearingCache = $state(false);

  // Cloud backup state
  type BackupProvider = "s3" | "google_drive" | "webdav" | null;
  let showNewBackupForm = $state(false);
  let selectedProvider: BackupProvider = $state(null);

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
      // Load current server URL from the collaboration module
      const currentServer = getCollaborationServer();
      if (currentServer && currentServer !== "ws://localhost:1234") {
        syncServerUrl = currentServer;
      }
    }
  });

  function applySyncServer() {
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
      setWorkspaceServer(syncServerUrl);
      if (typeof window !== "undefined") {
        localStorage.setItem("diaryx-collab-server-url", syncServerUrl);
      }
      if (onCollaborationToggle) {
        onCollaborationToggle(true);
      }
      setTimeout(() => {
        isApplyingServer = false;
      }, 1000);
    }
  }

  function clearSyncServer() {
    syncServerUrl = "";
    if (typeof window !== "undefined") {
      localStorage.removeItem("diaryx-collab-server-url");
    }
    setCollaborationServer("ws://localhost:1234");
    setWorkspaceServer(null);
    if (onCollaborationToggle) {
      onCollaborationToggle(false);
    }
  }

  async function handleClearDocumentCache() {
    isClearingCache = true;
    try {
      const count = await clearAllDocumentCache();
      console.log(`[Settings] Cleared ${count} document caches`);
    } catch (e) {
      console.error("Failed to clear document cache:", e);
    } finally {
      isClearingCache = false;
    }
  }

  async function loadConfig() {
    try {
      const backend = await getBackend();
      if ("getInvoke" in backend) {
        const invoke = (backend as any).getInvoke();
        config = await invoke("get_config", {});
        appPaths = await invoke("get_app_paths", {});
      }
    } catch (e) {
      config = null;
      appPaths = null;
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
        backupStatus = await invoke("run_backups", {});
      }
    } catch (e) {
      backupError = e instanceof Error ? e.message : String(e);
    } finally {
      isBackingUp = false;
    }
  }

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
      const workspaceDir = workspacePath
        ? workspacePath.substring(0, workspacePath.lastIndexOf("/"))
        : undefined;

      const result = await backend.importFromZip(
        file,
        workspaceDir,
        (uploaded, total) => {
          if (uploaded % (10 * 1024 * 1024) < 1024 * 1024) {
            console.log(
              `[Import] Progress: ${(uploaded / 1024 / 1024).toFixed(1)} / ${(total / 1024 / 1024).toFixed(1)} MB`,
            );
          }
        },
      );

      importResult = result;

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
          <Label for="theme-mode" class="text-sm cursor-pointer flex flex-col gap-0.5">
            <span>Theme</span>
            <span class="font-normal text-xs text-muted-foreground">
              Choose light, dark, or follow system preference.
            </span>
          </Label>
          <div class="flex items-center gap-1">
            <Button
              variant={themeStore.mode === "light" ? "default" : "ghost"}
              size="sm"
              class="h-8 w-8 p-0"
              onclick={() => themeStore.setMode("light")}
              title="Light mode"
            >
              <Sun class="size-4" />
            </Button>
            <Button
              variant={themeStore.mode === "dark" ? "default" : "ghost"}
              size="sm"
              class="h-8 w-8 p-0"
              onclick={() => themeStore.setMode("dark")}
              title="Dark mode"
            >
              <Moon class="size-4" />
            </Button>
            <Button
              variant={themeStore.mode === "system" ? "default" : "ghost"}
              size="sm"
              class="h-8 w-8 p-0"
              onclick={() => themeStore.setMode("system")}
              title="System preference"
            >
              <Monitor class="size-4" />
            </Button>
          </div>
        </div>

        <!-- Live Sync -->
        <div class="space-y-3">
          <h3 class="font-medium flex items-center gap-2">
            {#if collaborationConnected}
              <Wifi class="size-4 text-green-500" />
            {:else}
              <WifiOff class="size-4" />
            {/if}
            Live Sync
          </h3>

          <div class="space-y-2 px-1">
            <p class="text-xs text-muted-foreground">
              Connect to a Hocuspocus server for real-time collaboration and multi-device sync.
            </p>

            <div class="space-y-1">
              <Label for="sync-server" class="text-xs">Server URL</Label>
              <div class="flex gap-2">
                <Input
                  id="sync-server"
                  type="text"
                  bind:value={syncServerUrl}
                  placeholder="wss://your-server.com or leave empty"
                  class="h-8 text-sm flex-1"
                  onkeydown={(e) => {
                    if (e.key === "Enter") {
                      applySyncServer();
                    }
                  }}
                />
                <Button
                  variant="outline"
                  size="sm"
                  class="h-8 px-3"
                  onclick={applySyncServer}
                  disabled={isApplyingServer}
                >
                  {#if isApplyingServer}
                    <Loader2 class="size-3 animate-spin" />
                  {:else}
                    Apply
                  {/if}
                </Button>
              </div>
              <p class="text-xs text-muted-foreground">
                Use <code class="bg-muted px-1 rounded">ws://</code> for local
                or <code class="bg-muted px-1 rounded">wss://</code> for secure connections.
              </p>
            </div>

            <div class="flex items-center justify-between gap-4 pt-2">
              <div class="flex flex-col gap-0.5">
                <Label for="collab-enabled" class="text-sm cursor-pointer">
                  Enable sync
                </Label>
                <span class="text-xs text-muted-foreground">
                  {#if collaborationConnected}
                    <span class="text-green-600">Connected</span>
                  {:else if collaborationEnabled}
                    <span class="text-yellow-600">Connecting...</span>
                  {:else}
                    Disabled
                  {/if}
                </span>
              </div>
              <div class="flex items-center gap-2">
                {#if collaborationEnabled}
                  <Button
                    variant="ghost"
                    size="sm"
                    class="h-8 w-8 p-0"
                    onclick={() => onCollaborationReconnect?.()}
                    title="Reconnect"
                  >
                    <RefreshCw class="size-4" />
                  </Button>
                {/if}
                <Switch
                  id="collab-enabled"
                  checked={collaborationEnabled}
                  onCheckedChange={(checked) => onCollaborationToggle?.(checked)}
                />
              </div>
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
        </div>

        <div class="flex items-center justify-between gap-4 px-1">
          <Label for="show-unlinked" class="text-sm cursor-pointer flex flex-col gap-0.5">
            <span>Show all files</span>
            <span class="font-normal text-xs text-muted-foreground">
              Switch to a filesystem view to see files not linked in hierarchy.
            </span>
          </Label>
          <Switch id="show-unlinked" bind:checked={showUnlinkedFiles} />
        </div>

        <div class="flex items-center justify-between gap-4 px-1">
          <Label for="show-hidden" class="text-sm cursor-pointer flex flex-col gap-0.5">
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
            {#if isBackingUp}
              <Loader2 class="mr-2 size-4 animate-spin" />
            {/if}
            Run Backup
          </Button>
        </div>

        {#if backupError}
          <div class="flex items-center gap-2 text-sm text-destructive bg-destructive/10 p-2 rounded">
            <AlertCircle class="size-4" />
            <span>{backupError}</span>
          </div>
        {/if}

        {#if backupStatus}
          <div class="space-y-1 px-1">
            {#each backupStatus as status}
              <div class="flex items-center gap-2 text-sm">
                {#if status.success}
                  <Check class="size-4 text-green-500" />
                {:else}
                  <AlertCircle class="size-4 text-destructive" />
                {/if}
                <span>{status.target_name}: {status.success ? `${status.files_processed} files` : status.error || 'Failed'}</span>
              </div>
            {/each}
          </div>
        {/if}
      </div>

      <!-- Import Section -->
      <div class="space-y-3">
        <h3 class="font-medium flex items-center gap-2">
          <Upload class="size-4" />
          Import
        </h3>
        <div class="px-1 space-y-2">
          <p class="text-xs text-muted-foreground">
            Import entries from a zip backup.
          </p>
          <input
            type="file"
            accept=".zip"
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
              <div class="flex items-center gap-2 text-sm text-green-600 bg-green-50 dark:bg-green-950/20 p-2 rounded">
                <Check class="size-4" />
                <span>Imported {importResult.files_imported} files. Refresh to see changes.</span>
              </div>
            {:else}
              <div class="flex items-center gap-2 text-sm text-destructive bg-destructive/10 p-2 rounded">
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
                <Label for="backup-provider" class="text-sm font-medium">
                  Select Backup Provider
                </Label>
                <select
                  id="backup-provider"
                  class="w-full px-3 py-2 text-base md:text-sm border rounded bg-background"
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
              <Button variant="ghost" size="sm" onclick={cancelNewBackup}>Cancel</Button>
            {:else if selectedProvider === "s3"}
              <S3BackupSettings workspacePath={workspacePath} onCancel={cancelNewBackup} />
            {:else if selectedProvider === "google_drive"}
              <GoogleDriveSettings workspacePath={workspacePath} onCancel={cancelNewBackup} />
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
                <span class="text-muted-foreground min-w-[120px]">{key}:</span>
                <span class="break-all">{value}</span>
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
                <span class="text-muted-foreground min-w-[120px]">{key}:</span>
                <span class="break-all">{typeof value === "object"
                    ? JSON.stringify(value)
                    : String(value ?? "null")}</span>
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
