<script lang="ts">
  /**
   * MainLayout - The main application layout container
   * 
   * Provides the flex container structure for left sidebar, main content, and right sidebar.
   * This is a slot-based layout component.
   */
  
  import type { Snippet } from 'svelte';

  interface Props {
    leftSidebar?: Snippet;
    mainContent?: Snippet;
    rightSidebar?: Snippet;
    hiddenElements?: Snippet;
  }

  let {
    leftSidebar,
    mainContent,
    rightSidebar,
    hiddenElements,
  }: Props = $props();
</script>

<div class="flex h-screen bg-background overflow-hidden">
  {#if leftSidebar}
    {@render leftSidebar()}
  {/if}

  {#if hiddenElements}
    {@render hiddenElements()}
  {/if}

  <main class="flex-1 flex flex-col overflow-hidden min-w-0">
    {#if mainContent}
      {@render mainContent()}
    {/if}
  </main>

  {#if rightSidebar}
    {@render rightSidebar()}
  {/if}
</div>
