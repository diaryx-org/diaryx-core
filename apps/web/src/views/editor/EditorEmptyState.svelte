<script lang="ts">
  /**
   * EditorEmptyState - Welcome screen shown when no entry is selected
   * 
   * A pure presentational component that displays:
   * - Mobile header with menu toggle
   * - Desktop sidebar toggle button (when collapsed)
   * - Welcome message
   */
  
  import { Button } from "$lib/components/ui/button";
  import { PanelLeft, Menu } from "@lucide/svelte";

  interface Props {
    leftSidebarCollapsed: boolean;
    onToggleLeftSidebar: () => void;
  }

  let {
    leftSidebarCollapsed,
    onToggleLeftSidebar,
  }: Props = $props();
</script>

<!-- Mobile header -->
<header
  class="flex items-center justify-between px-4 py-3 border-b border-border bg-card shrink-0 md:hidden"
>
  <Button
    variant="ghost"
    size="icon"
    onclick={onToggleLeftSidebar}
    class="size-8"
    aria-label="Toggle navigation"
  >
    <Menu class="size-4" />
  </Button>
  <span class="text-lg font-semibold">Diaryx</span>
  <div class="size-8"></div>
</header>

<!-- Welcome content -->
<div class="flex-1 flex items-center justify-center">
  <div class="text-center max-w-md px-4">
    <!-- Desktop sidebar toggle when no entry -->
    <div class="hidden md:flex justify-center mb-4">
      {#if leftSidebarCollapsed}
        <Button
          variant="outline"
          size="sm"
          onclick={onToggleLeftSidebar}
          class="gap-2"
        >
          <PanelLeft class="size-4" />
          Show Sidebar
        </Button>
      {/if}
    </div>
    <h2 class="text-2xl font-semibold text-foreground mb-2">
      Welcome to Diaryx
    </h2>
    <p class="text-muted-foreground">
      Select an entry from the sidebar to start editing, or create a new one.
    </p>
  </div>
</div>
