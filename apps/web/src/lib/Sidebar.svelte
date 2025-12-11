<script lang="ts">
  import type { TreeNode, SearchResults, EntryData } from "./backend";
  import { Button } from "$lib/components/ui/button";
  import {
    Plus,
    Search,
    X,
    ChevronRight,
    FileText,
    Folder,
    Loader2,
  } from "@lucide/svelte";

  interface Props {
    tree: TreeNode | null;
    currentEntry: EntryData | null;
    isLoading: boolean;
    error: string | null;
    searchQuery: string;
    searchResults: SearchResults | null;
    isSearching: boolean;
    expandedNodes: Set<string>;
    onOpenEntry: (path: string) => void;
    onSearch: () => void;
    onClearSearch: () => void;
    onToggleNode: (path: string) => void;
    onNewEntry: () => void;
  }

  let {
    tree,
    currentEntry,
    isLoading,
    error,
    searchQuery = $bindable(),
    searchResults,
    isSearching,
    expandedNodes,
    onOpenEntry,
    onSearch,
    onClearSearch,
    onToggleNode,
    onNewEntry,
  }: Props = $props();

  function getFileName(path: string): string {
    return path.split("/").pop()?.replace(".md", "") ?? path;
  }

  function handleSearchKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      onSearch();
    } else if (e.key === "Escape") {
      onClearSearch();
    }
  }
</script>

<aside
  class="flex flex-col w-72 h-screen border-r border-border bg-sidebar text-sidebar-foreground"
>
  <!-- Header -->
  <div
    class="flex items-center justify-between px-4 py-4 border-b border-sidebar-border"
  >
    <a
      href="/"
      class="text-xl font-semibold text-sidebar-foreground hover:text-sidebar-foreground/80 transition-colors"
    >
      Diaryx
    </a>
  </div>

  <!-- New Entry Button -->
  <div class="p-3">
    <Button onclick={onNewEntry} class="w-full justify-center gap-2">
      <Plus class="size-4" />
      New Entry
    </Button>
  </div>

  <!-- Search -->
  <div class="px-3 pb-3">
    <div class="relative flex items-center">
      <input
        type="text"
        class="w-full h-9 pl-9 pr-9 rounded-md border border-input bg-background text-sm placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 focus:ring-offset-background disabled:cursor-not-allowed disabled:opacity-50"
        placeholder="Search entries..."
        bind:value={searchQuery}
        onkeydown={handleSearchKeydown}
      />
      {#if isSearching}
        <Loader2
          class="absolute left-3 size-4 text-muted-foreground pointer-events-none animate-spin"
        />
      {:else}
        <Search
          class="absolute left-3 size-4 text-muted-foreground pointer-events-none"
        />
      {/if}
      {#if searchQuery}
        <button
          type="button"
          class="absolute right-2 p-1 rounded-sm hover:bg-accent text-muted-foreground hover:text-foreground transition-colors"
          onclick={onClearSearch}
          aria-label="Clear search"
        >
          <X class="size-3" />
        </button>
      {/if}
    </div>
  </div>

  <!-- Content Area -->
  <div class="flex-1 overflow-y-auto px-3 pb-3">
    {#if searchResults}
      <!-- Search Results -->
      <div class="space-y-1">
        <div class="flex items-center justify-between px-2 py-1">
          <span class="text-xs text-muted-foreground">
            {searchResults.files.length} result{searchResults.files.length !== 1
              ? "s"
              : ""}
          </span>
        </div>
        {#each searchResults.files as result}
          <button
            class="w-full flex items-center gap-2 px-2 py-1.5 rounded-md text-sm text-left hover:bg-sidebar-accent hover:text-sidebar-accent-foreground transition-colors {currentEntry?.path ===
            result.path
              ? 'bg-sidebar-accent text-sidebar-accent-foreground'
              : ''}"
            onclick={() => onOpenEntry(result.path)}
          >
            <FileText class="size-4 shrink-0 text-muted-foreground" />
            <span class="truncate"
              >{result.title ?? getFileName(result.path)}</span
            >
          </button>
        {/each}
      </div>
    {:else if isLoading}
      <!-- Loading State -->
      <div class="flex items-center justify-center py-8">
        <Loader2 class="size-6 animate-spin text-muted-foreground" />
      </div>
    {:else if error}
      <!-- Error State -->
      <div
        class="rounded-md bg-destructive/10 border border-destructive/20 p-3"
      >
        <p class="text-sm text-destructive">{error}</p>
      </div>
    {:else if tree}
      <!-- Tree View -->
      <nav class="space-y-0.5">
        {@render treeNode(tree, 0)}
      </nav>
    {:else}
      <!-- Empty State -->
      <div class="flex flex-col items-center justify-center py-8 text-center">
        <Folder class="size-8 text-muted-foreground mb-2" />
        <p class="text-sm text-muted-foreground">No workspace found</p>
      </div>
    {/if}
  </div>
</aside>

{#snippet treeNode(node: TreeNode, depth: number)}
  <div class="select-none">
    <div
      class="group flex items-center gap-1 rounded-md hover:bg-sidebar-accent transition-colors"
      style="padding-left: {depth * 12}px"
    >
      {#if node.children.length > 0}
        <button
          type="button"
          class="p-1 rounded-sm hover:bg-sidebar-accent-foreground/10 transition-colors"
          onclick={(e) => {
            e.stopPropagation();
            onToggleNode(node.path);
          }}
          aria-expanded={expandedNodes.has(node.path)}
          aria-label="Toggle folder"
        >
          <ChevronRight
            class="size-4 text-muted-foreground transition-transform duration-200 {expandedNodes.has(
              node.path,
            )
              ? 'rotate-90'
              : ''}"
          />
        </button>
      {:else}
        <span class="w-6"></span>
      {/if}
      <button
        type="button"
        class="flex-1 flex items-center gap-2 py-1.5 pr-2 text-sm text-left rounded-md transition-colors {currentEntry?.path ===
        node.path
          ? 'text-sidebar-primary font-medium'
          : 'text-sidebar-foreground'}"
        onclick={() => onOpenEntry(node.path)}
      >
        {#if node.children.length > 0}
          <Folder class="size-4 shrink-0 text-muted-foreground" />
        {:else}
          <FileText class="size-4 shrink-0 text-muted-foreground" />
        {/if}
        <span class="truncate">{node.name.replace(".md", "")}</span>
      </button>
    </div>

    {#if node.children.length > 0 && expandedNodes.has(node.path)}
      <div class="mt-0.5">
        {#each node.children as child}
          {@render treeNode(child, depth + 1)}
        {/each}
      </div>
    {/if}
  </div>
{/snippet}
