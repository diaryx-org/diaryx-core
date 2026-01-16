<script lang="ts">
  /**
   * SettingsDialog - Main settings dialog component
   *
   * Uses modular sub-components for different settings sections.
   * Renders as a Drawer on mobile and Dialog on desktop.
   */
  import * as Dialog from "$lib/components/ui/dialog";
  import * as Drawer from "$lib/components/ui/drawer";
  import { Button } from "$lib/components/ui/button";
  import { Settings } from "@lucide/svelte";
  import { getMobileState } from "./hooks/useMobile.svelte";

  // Import modular settings components
  import DisplaySettings from "./settings/DisplaySettings.svelte";
  import WorkspaceSettings from "./settings/WorkspaceSettings.svelte";
  import StorageSettings from "./settings/StorageSettings.svelte";
  import SyncSettings from "./settings/SyncSettings.svelte";
  import BackupSettings from "./settings/BackupSettings.svelte";
  import ImportSettings from "./settings/ImportSettings.svelte";
  import CloudBackupSettings from "./settings/CloudBackupSettings.svelte";
  import DebugInfo from "./settings/DebugInfo.svelte";

  interface Props {
    open?: boolean;
    showUnlinkedFiles?: boolean;
    showHiddenFiles?: boolean;
    showEditorTitle?: boolean;
    showEditorPath?: boolean;
    readableLineLength?: boolean;
    focusMode?: boolean;
    workspacePath?: string | null;
  }

  let {
    open = $bindable(),
    showUnlinkedFiles = $bindable(),
    showHiddenFiles = $bindable(false),
    showEditorTitle = $bindable(false),
    showEditorPath = $bindable(false),
    readableLineLength = $bindable(true),
    focusMode = $bindable(true),
    workspacePath = null,
  }: Props = $props();

  const mobileState = getMobileState();
</script>

{#snippet settingsContent()}
  <div class="space-y-4">
    <!-- Display Settings -->
    <DisplaySettings
      bind:showUnlinkedFiles
      bind:showHiddenFiles
      bind:showEditorTitle
      bind:showEditorPath
      bind:readableLineLength
      bind:focusMode
    />

    <!-- Workspace Settings -->
    <WorkspaceSettings />

    <!-- Storage Settings -->
    <StorageSettings />

    <!-- Collaboration Server Settings -->
    <SyncSettings />

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
{/snippet}

{#if mobileState.isMobile}
  <!-- Mobile: Use Drawer -->
  <Drawer.Root bind:open>
    <Drawer.Content>
      <div class="mx-auto w-full max-w-sm">
        <Drawer.Header>
          <Drawer.Title class="flex items-center gap-2">
            <Settings class="size-5" />
            Settings
          </Drawer.Title>
          <Drawer.Description>
            Configuration and debugging information.
          </Drawer.Description>
        </Drawer.Header>
        <div class="px-4 pb-8 overflow-y-auto max-h-[70vh]">
          {@render settingsContent()}
        </div>
      </div>
    </Drawer.Content>
  </Drawer.Root>
{:else}
  <!-- Desktop: Use Dialog -->
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
      <div class="py-4">
        {@render settingsContent()}
      </div>
    </Dialog.Content>
  </Dialog.Root>
{/if}
