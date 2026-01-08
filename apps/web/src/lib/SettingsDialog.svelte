<script lang="ts">
  /**
   * SettingsDialog - Main settings dialog component
   *
   * Uses modular sub-components for different settings sections.
   */
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Settings } from "@lucide/svelte";

  // Import modular settings components
  import DisplaySettings from "./settings/DisplaySettings.svelte";
  import SyncSettings from "./settings/SyncSettings.svelte";
  import BackupSettings from "./settings/BackupSettings.svelte";
  import ImportSettings from "./settings/ImportSettings.svelte";
  import CloudBackupSettings from "./settings/CloudBackupSettings.svelte";
  import DebugInfo from "./settings/DebugInfo.svelte";

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
      <DisplaySettings
        bind:showUnlinkedFiles
        bind:showHiddenFiles
      />

      <!-- Live Sync Settings -->
      <SyncSettings
        bind:collaborationEnabled
        {collaborationConnected}
        {onCollaborationToggle}
        {onCollaborationReconnect}
      />

      <!-- Backup Section -->
      <BackupSettings {workspacePath} />

      <!-- Import Section -->
      <ImportSettings {workspacePath} />

      <!-- Cloud Backup Section -->
      <CloudBackupSettings {workspacePath} />

      <!-- Debug Info (App Paths & Config) -->
      <DebugInfo />

      <div class="flex justify-end pt-2">
        <Button variant="outline" onclick={() => (open = false)}>Close</Button>
      </div>
    </div>
  </Dialog.Content>
</Dialog.Root>
