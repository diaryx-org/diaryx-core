<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import {
    getBackend,
    startAutoPersist,
    stopAutoPersist,
    persistNow,
    type Backend,
    type TreeNode,
    type EntryData,
    type SearchResults,
  } from "./lib/backend";
  import Sidebar from "./lib/Sidebar.svelte";
  import NewEntryModal from "./lib/NewEntryModal.svelte";
  import { Button } from "$lib/components/ui/button";
  import { Save, Download } from "@lucide/svelte";

  // Dynamically import Editor to avoid SSR issues
  let Editor: typeof import("./lib/Editor.svelte").default | null =
    $state(null);

  // Backend instance
  let backend: Backend | null = $state(null);

  // State
  let tree: TreeNode | null = $state(null);
  let currentEntry: EntryData | null = $state(null);
  let isDirty = $state(false);
  let isLoading = $state(true);
  let error: string | null = $state(null);
  let searchQuery = $state("");
  let searchResults: SearchResults | null = $state(null);
  let isSearching = $state(false);
  let expandedNodes = $state(new Set<string>());
  let editorRef: any = $state(null);
  let showNewEntryModal = $state(false);

  // Load initial data
  onMount(async () => {
    try {
      // Dynamically import the Editor component
      const module = await import("./lib/Editor.svelte");
      Editor = module.default;

      // Initialize the backend (auto-detects Tauri vs WASM)
      backend = await getBackend();

      // Start auto-persist for WASM backend (no-op for Tauri)
      startAutoPersist(5000);

      tree = await backend.getWorkspaceTree();

      // Expand root by default
      if (tree) {
        expandedNodes.add(tree.path);
      }
    } catch (e) {
      console.error("[App] Initialization error:", e);
      error = e instanceof Error ? e.message : String(e);
    } finally {
      isLoading = false;
    }
  });

  onDestroy(() => {
    // Stop auto-persist and do a final persist
    stopAutoPersist();
    persistNow();
  });

  // Open an entry
  async function openEntry(path: string) {
    if (!backend) return;

    if (isDirty) {
      const confirm = window.confirm(
        "You have unsaved changes. Do you want to discard them?",
      );
      if (!confirm) return;
    }

    try {
      isLoading = true;
      currentEntry = await backend.getEntry(path);
      isDirty = false;
      error = null;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      isLoading = false;
    }
  }

  // Save current entry
  async function save() {
    if (!backend || !currentEntry || !editorRef) return;

    try {
      const markdown = editorRef.getMarkdown();
      await backend.saveEntry(currentEntry.path, markdown);
      isDirty = false;
      // Trigger persist for WASM backend
      await persistNow();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  // Handle content changes
  function handleContentChange(_markdown: string) {
    isDirty = true;
  }

  // Search
  async function handleSearch() {
    if (!backend || !searchQuery.trim()) {
      searchResults = null;
      return;
    }

    try {
      isSearching = true;
      searchResults = await backend.searchWorkspace(searchQuery);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      isSearching = false;
    }
  }

  function clearSearch() {
    searchQuery = "";
    searchResults = null;
  }

  // Toggle node expansion
  function toggleNode(path: string) {
    if (expandedNodes.has(path)) {
      expandedNodes.delete(path);
    } else {
      expandedNodes.add(path);
    }
    expandedNodes = new Set(expandedNodes); // Trigger reactivity
  }

  // Keyboard shortcuts
  function handleKeydown(event: KeyboardEvent) {
    if ((event.metaKey || event.ctrlKey) && event.key === "s") {
      event.preventDefault();
      save();
    }
  }

  function handleNewEntry() {
    showNewEntryModal = true;
  }

  async function createNewEntry(path: string, title: string) {
    if (!backend) return;
    try {
      const newPath = await backend.createEntry(path, { title });
      // Backend automatically adds entry to parent index's contents
      tree = await backend.getWorkspaceTree(); // Refresh tree
      await openEntry(newPath);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      showNewEntryModal = false;
    }
  }

  function exportEntry() {
    if (!currentEntry) return;
    const blob = new Blob([currentEntry.content], {
      type: "text/markdown;charset=utf-8",
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = currentEntry.path.split("/").pop() || "entry.md";
    a.click();
    URL.revokeObjectURL(url);
  }

  function getEntryTitle(entry: EntryData): string {
    return (
      entry.title ??
      entry.path.split("/").pop()?.replace(".md", "") ??
      "Untitled"
    );
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if showNewEntryModal}
  <NewEntryModal
    onSave={createNewEntry}
    onCancel={() => (showNewEntryModal = false)}
  />
{/if}

<div class="flex h-screen bg-background">
  <Sidebar
    {tree}
    {currentEntry}
    {isLoading}
    {error}
    bind:searchQuery
    {searchResults}
    {isSearching}
    {expandedNodes}
    onOpenEntry={openEntry}
    onSearch={handleSearch}
    onClearSearch={clearSearch}
    onToggleNode={toggleNode}
    onNewEntry={handleNewEntry}
  />

  <main class="flex-1 flex flex-col overflow-hidden">
    {#if currentEntry}
      <header
        class="flex items-center justify-between px-6 py-4 border-b border-border bg-card"
      >
        <div class="min-w-0 flex-1">
          <h2 class="text-xl font-semibold text-foreground truncate">
            {getEntryTitle(currentEntry)}
          </h2>
          <p class="text-sm text-muted-foreground truncate">
            {currentEntry.path}
          </p>
        </div>
        <div class="flex items-center gap-2 ml-4">
          {#if isDirty}
            <span
              class="px-2 py-1 text-xs font-medium rounded-md bg-amber-100 text-amber-800 dark:bg-amber-900/30 dark:text-amber-400"
            >
              Unsaved changes
            </span>
          {/if}
          <Button onclick={save} disabled={!isDirty} size="sm">
            <Save class="size-4" />
            Save
          </Button>
          <Button onclick={exportEntry} variant="outline" size="sm">
            <Download class="size-4" />
            Export
          </Button>
        </div>
      </header>

      <div class="flex-1 overflow-y-auto p-6">
        {#if Editor}
          <Editor
            bind:this={editorRef}
            content={currentEntry.content}
            onchange={handleContentChange}
            placeholder="Start writing..."
          />
        {:else}
          <div class="flex items-center justify-center h-full">
            <div
              class="animate-spin rounded-full h-8 w-8 border-b-2 border-primary"
            ></div>
          </div>
        {/if}
      </div>
    {:else}
      <div class="flex-1 flex items-center justify-center">
        <div class="text-center max-w-md px-4">
          <h2 class="text-2xl font-semibold text-foreground mb-2">
            Welcome to Diaryx
          </h2>
          <p class="text-muted-foreground">
            Select an entry from the sidebar to start editing, or create a new
            one.
          </p>
        </div>
      </div>
    {/if}
  </main>
</div>
