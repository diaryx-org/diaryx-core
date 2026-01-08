<script lang="ts">
  /**
   * CloudBackupSettings - Cloud backup provider selection and configuration
   *
   * Extracted from SettingsDialog for modularity.
   */
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import { Save } from "@lucide/svelte";
  import S3BackupSettings from "./S3BackupSettings.svelte";
  import GoogleDriveSettings from "./GoogleDriveSettings.svelte";

  interface Props {
    workspacePath?: string | null;
  }

  let { workspacePath = null }: Props = $props();

  // Cloud backup state
  type BackupProvider = "s3" | "google_drive" | "webdav" | null;
  let showNewBackupForm = $state(false);
  let selectedProvider: BackupProvider = $state(null);

  function selectProvider(provider: BackupProvider) {
    selectedProvider = provider;
  }

  function cancelNewBackup() {
    showNewBackupForm = false;
    selectedProvider = null;
  }
</script>

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
        <Button variant="ghost" size="sm" onclick={cancelNewBackup}
          >Cancel</Button
        >
      {:else if selectedProvider === "s3"}
        <S3BackupSettings {workspacePath} onCancel={cancelNewBackup} />
      {:else if selectedProvider === "google_drive"}
        <GoogleDriveSettings {workspacePath} onCancel={cancelNewBackup} />
      {/if}
    </div>
  {/if}
</div>
