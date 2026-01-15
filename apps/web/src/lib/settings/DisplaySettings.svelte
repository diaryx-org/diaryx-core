<script lang="ts">
  /**
   * DisplaySettings - Display and theme settings section
   * 
   * Extracted from SettingsDialog for modularity.
   */
  import { Switch } from "$lib/components/ui/switch";
  import { Label } from "$lib/components/ui/label";
  import { Eye, Sun, Moon, Monitor } from "@lucide/svelte";
  import { getThemeStore, type ThemeMode } from "../stores/theme.svelte";

  interface Props {
    showUnlinkedFiles?: boolean;
    showHiddenFiles?: boolean;
    showEditorTitle?: boolean;
    showEditorPath?: boolean;
    readableLineLength?: boolean;
  }

  let {
    showUnlinkedFiles = $bindable(false),
    showHiddenFiles = $bindable(false),
    showEditorTitle = $bindable(false),
    showEditorPath = $bindable(false),
    readableLineLength = $bindable(true),
  }: Props = $props();

  const themeStore = getThemeStore();
</script>

<div class="space-y-3">
  <h3 class="font-medium flex items-center gap-2">
    <Eye class="size-4" />
    Display
  </h3>

  <!-- Theme Selection -->
  <div class="flex items-center justify-between gap-4 px-1">
    <Label for="theme-mode" class="text-sm cursor-pointer flex flex-col gap-0.5">
      <span>Theme</span>
      <span class="font-normal text-xs text-muted-foreground">
        Choose light, dark, or follow system preference.
      </span>
    </Label>
    <select
      id="theme-mode"
      class="w-auto px-2 py-1 text-sm border rounded bg-background"
      value={themeStore.mode}
      onchange={(e) => themeStore.setMode((e.target as HTMLSelectElement).value as ThemeMode)}
    >
      <option value="system">
        <Monitor class="size-3" /> System
      </option>
      <option value="light">
        <Sun class="size-3" /> Light
      </option>
      <option value="dark">
        <Moon class="size-3" /> Dark
      </option>
    </select>
  </div>

  <!-- Editor Header: Show Title -->
  <div class="flex items-center justify-between gap-4 px-1">
    <Label for="show-editor-title" class="text-sm cursor-pointer flex flex-col gap-0.5">
      <span>Show title in editor</span>
      <span class="font-normal text-xs text-muted-foreground">
        Display the entry title in the editor header.
      </span>
    </Label>
    <Switch id="show-editor-title" bind:checked={showEditorTitle} />
  </div>

  <!-- Editor Header: Show Path -->
  <div class="flex items-center justify-between gap-4 px-1">
    <Label for="show-editor-path" class="text-sm cursor-pointer flex flex-col gap-0.5">
      <span>Show filename in editor</span>
      <span class="font-normal text-xs text-muted-foreground">
        Display the file path below the title.
      </span>
    </Label>
    <Switch
      id="show-editor-path"
      bind:checked={showEditorPath}
      disabled={!showEditorTitle}
    />
  </div>

  <!-- Readable Line Length -->
  <div class="flex items-center justify-between gap-4 px-1">
    <Label for="readable-line-length" class="text-sm cursor-pointer flex flex-col gap-0.5">
      <span>Readable line length</span>
      <span class="font-normal text-xs text-muted-foreground">
        Limit text width for comfortable reading.
      </span>
    </Label>
    <Switch id="readable-line-length" bind:checked={readableLineLength} />
  </div>

  <!-- Show Unlinked Files (temporarily disabled) -->
  <div class="flex items-center justify-between gap-4 px-1 opacity-50">
    <Label for="show-unlinked" class="text-sm flex flex-col gap-0.5">
      <span>Show all files</span>
      <span class="font-normal text-xs text-muted-foreground">
        Temporarily disabled. Will show files not linked in hierarchy.
      </span>
    </Label>
    <Switch id="show-unlinked" bind:checked={showUnlinkedFiles} disabled />
  </div>

  <!-- Show Hidden Files -->
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
