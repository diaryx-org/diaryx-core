<script lang="ts">
  /**
   * CloudBackupSettings - Cloud sync provider selection and configuration
   *
   * Supports bidirectional sync with S3-compatible storage and Google Drive.
   */
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import { Cloud } from "@lucide/svelte";
  import S3BackupSettings from "./S3BackupSettings.svelte";
  import GoogleDriveSettings from "./GoogleDriveSettings.svelte";

  interface Props {
    workspacePath?: string | null;
  }

  let { workspacePath = null }: Props = $props();

  // Cloud sync state
  type SyncProvider = "s3" | "google_drive" | "webdav" | null;
  let showNewSyncForm = $state(false);
  let selectedProvider: SyncProvider = $state(null);

  function selectProvider(provider: SyncProvider) {
    selectedProvider = provider;
  }

  function cancelNewSync() {
    showNewSyncForm = false;
    selectedProvider = null;
  }
</script>

<div class="space-y-3">
  <h3 class="font-medium flex items-center gap-2">
    <Cloud class="size-4" />
    Cloud Sync
  </h3>

  <div class="px-1">
    <p class="text-xs text-muted-foreground mb-2">
      Sync your workspace with cloud storage for backup and cross-device access.
    </p>

    {#if !showNewSyncForm}
      <Button
        variant="outline"
        size="sm"
        onclick={() => (showNewSyncForm = true)}
      >
        + Add Cloud Provider
      </Button>
    {:else}
      <div class="space-y-3 p-3 border rounded-lg bg-muted/30">
        {#if !selectedProvider}
          <!-- Provider Selection -->
          <div class="space-y-2">
            <Label for="sync-provider" class="text-sm font-medium">
              Select Cloud Provider
            </Label>
            <select
              id="sync-provider"
              class="w-full px-3 py-2 text-base md:text-sm border rounded bg-background"
              onchange={(e) =>
                selectProvider(
                  (e.target as HTMLSelectElement).value as SyncProvider,
                )}
            >
              <option value="">Choose a provider...</option>
              <option value="s3">Amazon S3 / S3-Compatible (R2, MinIO)</option>
              <option value="google_drive">Google Drive</option>
              <option value="webdav" disabled>WebDAV (coming soon)</option>
            </select>
          </div>
          <Button variant="ghost" size="sm" onclick={cancelNewSync}
            >Cancel</Button
          >
        {:else if selectedProvider === "s3"}
          <S3BackupSettings {workspacePath} onCancel={cancelNewSync} />
        {:else if selectedProvider === "google_drive"}
          <GoogleDriveSettings {workspacePath} onCancel={cancelNewSync} />
        {/if}
      </div>
    {/if}
  </div>
</div>
