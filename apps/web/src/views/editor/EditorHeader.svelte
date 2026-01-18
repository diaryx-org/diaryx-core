<script lang="ts">
  /**
   * EditorHeader - Header bar for the editor with actions and sidebar toggles
   *
   * A pure presentational component that displays:
   * - Sidebar toggle buttons (only when sidebar is closed)
   * - Entry title and path (configurable via settings)
   * - Unsaved indicator badge
   * - Save button with keyboard shortcut tooltip
   * - Command palette button with keyboard shortcut tooltip
   */

  import { Button } from "$lib/components/ui/button";
  import * as Tooltip from "$lib/components/ui/tooltip";
  import { getMobileState } from "$lib/hooks/useMobile.svelte";
  import {
    Save,
    PanelLeft,
    PanelRight,
    Menu,
    Loader2,
    Search,
  } from "@lucide/svelte";

  // Mobile state for hiding keyboard shortcut tooltips
  const mobileState = getMobileState();

  interface Props {
    title: string;
    path: string;
    isDirty: boolean;
    isSaving: boolean;
    showTitle: boolean;
    showPath: boolean;
    leftSidebarOpen: boolean;
    rightSidebarOpen: boolean;
    focusMode?: boolean;
    readonly?: boolean;
    onSave: () => void;
    onToggleLeftSidebar: () => void;
    onToggleRightSidebar: () => void;
    onOpenCommandPalette: () => void;
  }

  let {
    title,
    path,
    isDirty,
    isSaving,
    showTitle,
    showPath,
    leftSidebarOpen,
    rightSidebarOpen,
    focusMode = false,
    readonly = false,
    onSave,
    onToggleLeftSidebar,
    onToggleRightSidebar,
    onOpenCommandPalette,
  }: Props = $props();

  // Focus mode: header is invisible when both sidebars are closed
  let bothSidebarsClosed = $derived(!leftSidebarOpen && !rightSidebarOpen);
  let shouldFade = $derived(focusMode && bothSidebarsClosed);
  let isHovered = $state(false);

  // Detect platform for keyboard shortcut display
  const isMac =
    typeof navigator !== "undefined" &&
    navigator.platform.toUpperCase().indexOf("MAC") >= 0;
  const modKey = isMac ? "⌘" : "Ctrl+";
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<header
  class="flex items-center justify-between px-4 md:px-6 py-3 md:py-4 border-b border-border bg-background shrink-0
    transition-opacity duration-300 ease-in-out
    {shouldFade && !isHovered ? 'opacity-0' : 'opacity-100'}"
  onmouseenter={() => isHovered = true}
  onmouseleave={() => isHovered = false}
>
  <!-- Left side: toggle + title -->
  <div class="flex items-center gap-2 min-w-0 flex-1">
    <!-- Mobile menu button (always show on mobile for navigation) -->
    <Button
      variant="ghost"
      size="icon"
      onclick={onToggleLeftSidebar}
      class="size-8 md:hidden shrink-0"
      aria-label="Toggle navigation"
    >
      <Menu class="size-4" />
    </Button>

    <!-- Desktop left sidebar toggle (only when sidebar is closed) -->
    {#if !leftSidebarOpen}
      <Tooltip.Root>
        <Tooltip.Trigger>
          <Button
            variant="ghost"
            size="icon"
            onclick={onToggleLeftSidebar}
            class="size-8 hidden md:flex shrink-0"
            aria-label="Open navigation sidebar"
          >
            <PanelLeft class="size-4" />
          </Button>
        </Tooltip.Trigger>
        {#if !mobileState.isMobile}
          <Tooltip.Content>Open sidebar ({modKey}[)</Tooltip.Content>
        {/if}
      </Tooltip.Root>
    {/if}

    <!-- Title and path (conditional based on settings) -->
    {#if showTitle}
      <div class="min-w-0 flex-1">
        <h2 class="text-lg md:text-xl font-semibold text-foreground truncate">
          {title}
        </h2>
        {#if showPath}
          <p
            class="text-xs md:text-sm text-muted-foreground truncate hidden sm:block"
          >
            {path}
          </p>
        {/if}
      </div>
    {/if}
  </div>

  <!-- Right side: actions -->
  <div class="flex items-center gap-1 md:gap-2 ml-2 shrink-0">
    {#if readonly}
      <!-- View-only indicator for read-only mode -->
      <span
        class="inline-flex items-center gap-1 px-2 py-1 text-xs font-medium rounded-md bg-amber-100 text-amber-800 dark:bg-amber-900/30 dark:text-amber-400"
      >
        View only
      </span>
    {:else}
      {#if isDirty && !isSaving}
        <span
          class="hidden sm:inline-flex px-2 py-1 text-xs font-medium rounded-md bg-amber-100 text-amber-800 dark:bg-amber-900/30 dark:text-amber-400"
        >
          Unsaved
        </span>
      {/if}

      <!-- Save button with tooltip -->
      <Tooltip.Root>
        <Tooltip.Trigger>
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
        </Tooltip.Trigger>
        {#if !mobileState.isMobile}
          <Tooltip.Content>Save ({modKey}S)</Tooltip.Content>
        {/if}
      </Tooltip.Root>
    {/if}

    <!-- Command palette button with tooltip -->
    <Tooltip.Root>
      <Tooltip.Trigger>
        <Button
          variant="ghost"
          size="icon"
          onclick={onOpenCommandPalette}
          class="size-8"
          aria-label="Open command palette"
        >
          <Search class="size-4" />
        </Button>
      </Tooltip.Trigger>
      {#if !mobileState.isMobile}
        <Tooltip.Content>Search ({modKey}K)</Tooltip.Content>
      {/if}
    </Tooltip.Root>

    <!-- Right sidebar toggle (only when sidebar is closed) -->
    {#if !rightSidebarOpen}
      <Tooltip.Root>
        <Tooltip.Trigger>
          <Button
            variant="ghost"
            size="icon"
            onclick={onToggleRightSidebar}
            class="size-8"
            aria-label="Open properties panel"
          >
            <PanelRight class="size-4" />
          </Button>
        </Tooltip.Trigger>
        {#if !mobileState.isMobile}
          <Tooltip.Content>Open properties ({modKey}])</Tooltip.Content>
        {/if}
      </Tooltip.Root>
    {/if}
  </div>
</header>
