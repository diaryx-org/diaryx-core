<script lang="ts">
  /**
   * EditorHeader - Header bar for the editor with title, actions, and sidebar toggles
   * 
   * A pure presentational component that displays:
   * - Sidebar toggle buttons (left/right)
   * - Entry title and path
   * - Unsaved indicator badge
   * - Save and Export buttons
   */
  
  import { Button } from "$lib/components/ui/button";
  import { Save, Download, PanelLeft, PanelRight, Menu, Loader2, Search } from "@lucide/svelte";

  interface Props {
    title: string;
    path: string;
    isDirty: boolean;
    isSaving: boolean;
    onSave: () => void;
    onExport: () => void;
    onToggleLeftSidebar: () => void;
    onToggleRightSidebar: () => void;
    onOpenCommandPalette: () => void;
  }

  let {
    title,
    path,
    isDirty,
    isSaving,
    onSave,
    onExport,
    onToggleLeftSidebar,
    onToggleRightSidebar,
    onOpenCommandPalette,
  }: Props = $props();
</script>

<header
  class="flex items-center justify-between px-4 md:px-6 py-3 md:py-4 border-b border-border bg-card shrink-0"
>
  <!-- Left side: toggle + title -->
  <div class="flex items-center gap-2 min-w-0 flex-1">
    <!-- Mobile menu button -->
    <Button
      variant="ghost"
      size="icon"
      onclick={onToggleLeftSidebar}
      class="size-8 md:hidden shrink-0"
      aria-label="Toggle navigation"
    >
      <Menu class="size-4" />
    </Button>

    <!-- Desktop left sidebar toggle -->
    <Button
      variant="ghost"
      size="icon"
      onclick={onToggleLeftSidebar}
      class="size-8 hidden md:flex shrink-0"
      aria-label="Toggle navigation sidebar"
    >
      <PanelLeft class="size-4" />
    </Button>

    <div class="min-w-0 flex-1">
      <h2 class="text-lg md:text-xl font-semibold text-foreground truncate">
        {title}
      </h2>
      <p class="text-xs md:text-sm text-muted-foreground truncate hidden sm:block">
        {path}
      </p>
    </div>
  </div>

  <!-- Right side: actions -->
  <div class="flex items-center gap-1 md:gap-2 ml-2 shrink-0">
    {#if isDirty && !isSaving}
      <span
        class="hidden sm:inline-flex px-2 py-1 text-xs font-medium rounded-md bg-amber-100 text-amber-800 dark:bg-amber-900/30 dark:text-amber-400"
      >
        Unsaved
      </span>
    {/if}

    <Button
      onclick={onSave}
      disabled={!isDirty || isSaving}
      variant={!isDirty && !isSaving ? "ghost" : "default"}
      size="sm"
      class="gap-1 md:gap-2 min-w-[80px]"
    >
      {#if isSaving}
        <Loader2 class="size-4 animate-spin" />
        <span class="hidden sm:inline">Saving...</span>
      {:else if !isDirty}
        <Save class="size-4 opacity-50" />
        <span class="hidden sm:inline opacity-50">Saved</span>
      {:else}
        <Save class="size-4" />
        <span class="hidden sm:inline">Save</span>
      {/if}
    </Button>

    <Button
      variant="ghost"
      size="icon"
      onclick={onOpenCommandPalette}
      class="size-8"
      aria-label="Open command palette"
    >
      <Search class="size-4" />
    </Button>

    <Button
      onclick={onExport}
      variant="outline"
      size="sm"
      class="gap-1 md:gap-2 hidden sm:flex"
    >
      <Download class="size-4" />
      <span class="hidden md:inline">Export</span>
    </Button>

    <!-- Properties panel toggle -->
    <Button
      variant="ghost"
      size="icon"
      onclick={onToggleRightSidebar}
      class="size-8"
      aria-label="Toggle properties panel"
    >
      <PanelRight class="size-4" />
    </Button>
  </div>
</header>
