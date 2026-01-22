<script lang="ts">
  /**
   * ClearDataSettings - Clear all locally stored data
   *
   * Allows users to wipe all local data including:
   * - OPFS storage (workspace files, CRDT data)
   * - IndexedDB databases
   * - localStorage (settings, auth tokens)
   *
   * After clearing, the page is refreshed to start fresh.
   */
  import { Button } from "$lib/components/ui/button";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Trash2, AlertTriangle, Loader2 } from "@lucide/svelte";
  import { isTauri } from "$lib/backend/interface";

  // State
  let showConfirmDialog = $state(false);
  let isClearing = $state(false);
  let error = $state<string | null>(null);

  /**
   * Clear all OPFS data by deleting the diaryx directory.
   */
  async function clearOpfs(): Promise<void> {
    if (!navigator.storage?.getDirectory) return;

    try {
      const root = await navigator.storage.getDirectory();
      // Try to remove the diaryx directory
      await root.removeEntry("diaryx", { recursive: true });
    } catch (e) {
      // Directory might not exist, that's fine
      if ((e as Error).name !== "NotFoundError") {
        console.warn("[ClearData] Failed to clear OPFS:", e);
      }
    }
  }

  /**
   * Clear all IndexedDB databases used by the app.
   */
  async function clearIndexedDb(): Promise<void> {
    const dbNames = [
      "diaryx-fs-handles",
      // Add any other IndexedDB databases used by the app
    ];

    for (const name of dbNames) {
      try {
        await new Promise<void>((resolve, reject) => {
          const request = indexedDB.deleteDatabase(name);
          request.onsuccess = () => resolve();
          request.onerror = () => reject(request.error);
          request.onblocked = () => {
            console.warn(`[ClearData] Database ${name} is blocked`);
            resolve(); // Continue anyway
          };
        });
      } catch (e) {
        console.warn(`[ClearData] Failed to delete IndexedDB ${name}:`, e);
      }
    }
  }

  /**
   * Clear all localStorage keys used by the app.
   */
  function clearLocalStorage(): void {
    const keysToRemove = [
      // Storage type
      "diaryx-storage-type",
      // Auth
      "diaryx_auth_token",
      "diaryx_sync_server_url",
      "diaryx_user",
      // Sync
      "diaryx-sync-server",
      // Display settings
      "diaryx-show-unlinked-files",
      "diaryx-show-hidden-files",
      "diaryx-show-editor-title",
      "diaryx-show-editor-path",
      "diaryx-readable-line-length",
      "diaryx-focus-mode",
      // Device
      "diaryx-device-id",
      "diaryx-device-name",
      // Theme
      "diaryx-theme",
      // Cloud backup credentials
      "diaryx_s3_access_key",
      "diaryx_s3_secret_key",
      "diaryx_s3_config",
      "diaryx_gd_refresh_token",
      "diaryx_gd_folder_id",
      "diaryx_gd_client_id",
      "diaryx_gd_client_secret",
      "diaryx_sync_enabled",
    ];

    for (const key of keysToRemove) {
      try {
        localStorage.removeItem(key);
      } catch (e) {
        console.warn(`[ClearData] Failed to remove localStorage key ${key}:`, e);
      }
    }
  }

  /**
   * Clear all local data and refresh the page.
   */
  async function handleClearData() {
    isClearing = true;
    error = null;

    try {
      // Clear all storage types
      await clearOpfs();
      await clearIndexedDb();
      clearLocalStorage();

      // Close the dialog before refreshing
      showConfirmDialog = false;

      // Small delay to let the dialog close animation complete
      await new Promise((resolve) => setTimeout(resolve, 100));

      // Refresh the page
      window.location.reload();
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to clear data";
      isClearing = false;
    }
  }
</script>

<div class="space-y-3">
  <h3 class="font-medium flex items-center gap-2">
    <Trash2 class="size-4" />
    Clear Local Data
  </h3>

  <div class="px-1 space-y-2">
    {#if isTauri()}
      <p class="text-xs text-muted-foreground">
        This option is not available in the desktop app. Your files are stored directly on your device.
      </p>
    {:else}
      <p class="text-xs text-muted-foreground">
        Delete all locally stored data including your workspace files, settings, and credentials.
        This will reset the app to its initial state.
      </p>

      <Button
        variant="destructive"
        size="sm"
        onclick={() => (showConfirmDialog = true)}
      >
        <Trash2 class="size-4 mr-2" />
        Clear All Local Data
      </Button>
    {/if}
  </div>
</div>

<!-- Confirmation Dialog -->
<Dialog.Root bind:open={showConfirmDialog}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2 text-destructive">
        <AlertTriangle class="size-5" />
        Clear All Local Data
      </Dialog.Title>
      <Dialog.Description>
        This will permanently delete all data stored in your browser:
      </Dialog.Description>
    </Dialog.Header>

    <ul class="list-disc list-inside text-sm text-muted-foreground space-y-1 py-2">
      <li>All workspace files and notes</li>
      <li>CRDT sync data</li>
      <li>App settings and preferences</li>
      <li>Login credentials and tokens</li>
      <li>Cloud backup configurations</li>
    </ul>

    <p class="text-sm font-medium text-destructive">
      This action cannot be undone. Make sure to export your data first if you want to keep it.
    </p>

    {#if error}
      <p class="text-sm text-destructive bg-destructive/10 p-2 rounded">
        {error}
      </p>
    {/if}

    <Dialog.Footer class="gap-2 sm:gap-0">
      <Button
        variant="outline"
        onclick={() => (showConfirmDialog = false)}
        disabled={isClearing}
      >
        Cancel
      </Button>
      <Button
        variant="destructive"
        onclick={handleClearData}
        disabled={isClearing}
      >
        {#if isClearing}
          <Loader2 class="size-4 mr-2 animate-spin" />
          Clearing...
        {:else}
          Clear Everything
        {/if}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
