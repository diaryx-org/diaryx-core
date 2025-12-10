<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import {
    getBackend,
    startAutoPersist,
    stopAutoPersist,
    persistNow,
    type Backend,
    type Config,
    type TreeNode,
    type EntryData,
    type SearchResults,
  } from "./lib/backend";

  // Dynamically import Editor to avoid SSR issues
  let Editor: typeof import("./lib/Editor.svelte").default | null =
    $state(null);

  // Backend instance
  let backend: Backend | null = $state(null);

  // State
  let config: Config | null = $state(null);
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
  let isBrowser = $state(false);

  // Load initial data
  onMount(async () => {
    // We're in the browser now
    isBrowser = true;
    console.log("[App] onMount started");

    try {
      // Dynamically import the Editor component
      console.log("[App] Loading Editor component...");
      const module = await import("./lib/Editor.svelte");
      Editor = module.default;
      console.log("[App] Editor component loaded");

      // Initialize the backend (auto-detects Tauri vs WASM)
      console.log("[App] Initializing backend...");
      backend = await getBackend();
      console.log("[App] Backend initialized");

      // Start auto-persist for WASM backend (no-op for Tauri)
      startAutoPersist(5000);

      console.log("[App] Getting config...");
      config = await backend.getConfig();
      console.log("[App] Config loaded:", config);

      console.log("[App] Getting workspace tree...");
      tree = await backend.getWorkspaceTree();
      console.log("[App] Workspace tree loaded:", tree);

      // Expand root by default
      if (tree) {
        expandedNodes.add(tree.path);
      }
      console.log("[App] Initialization complete");
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

  // Get filename from path
  function getFileName(path: string): string {
    return path.split("/").pop()?.replace(".md", "") ?? path;
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="app">
  <!-- Sidebar -->
  <aside class="sidebar">
    <div class="sidebar-header">
      <h1 class="logo">Diaryx</h1>
    </div>

    <!-- Search -->
    <div class="search-container">
      <input
        type="text"
        placeholder="Search..."
        bind:value={searchQuery}
        onkeydown={(e) => e.key === "Enter" && handleSearch()}
      />
      <button onclick={handleSearch} disabled={isSearching}>
        {isSearching ? "..." : "🔍"}
      </button>
    </div>

    <!-- Search Results -->
    {#if searchResults}
      <div class="search-results">
        <div class="search-results-header">
          <span>{searchResults.files.length} results</span>
          <button onclick={() => (searchResults = null)}>✕</button>
        </div>
        {#each searchResults.files as result}
          <button
            class="search-result-item"
            onclick={() => openEntry(result.path)}
          >
            <span class="result-title"
              >{result.title ?? getFileName(result.path)}</span
            >
            <span class="result-matches">{result.matches.length} matches</span>
          </button>
        {/each}
      </div>
    {:else}
      <!-- Tree View -->
      <nav class="tree-view">
        {#if tree}
          {@render treeNode(tree, 0)}
        {:else if isLoading}
          <div class="loading">Loading workspace...</div>
        {:else if error}
          <div class="error">
            <p><strong>Error:</strong></p>
            <p>{error}</p>
            <p style="font-size: 11px; margin-top: 8px;">
              Check browser console for details
            </p>
          </div>
        {:else}
          <div class="loading">No workspace found</div>
        {/if}
      </nav>
    {/if}
  </aside>

  <!-- Show error prominently during loading if it occurred -->
  {#if isLoading && error}
    <div class="loading-error">
      <h2>Failed to load Diaryx</h2>
      <p>{error}</p>
      <p style="font-size: 12px; color: #666;">
        Check browser console for details
      </p>
    </div>
  {/if}

  <!-- Main Content -->
  <main class="content">
    {#if currentEntry}
      <header class="content-header">
        <div class="entry-info">
          <h2 class="entry-title">
            {currentEntry.title ?? getFileName(currentEntry.path)}
          </h2>
          <span class="entry-path">{currentEntry.path}</span>
        </div>
        <div class="header-actions">
          {#if isDirty}
            <span class="unsaved-indicator">Unsaved changes</span>
          {/if}
          <button class="save-button" onclick={save} disabled={!isDirty}>
            Save
          </button>
        </div>
      </header>

      <div class="editor-area">
        {#if Editor}
          <Editor
            bind:this={editorRef}
            content={currentEntry.content}
            onchange={handleContentChange}
            placeholder="Start writing..."
          />
        {:else}
          <div class="loading">Loading editor...</div>
        {/if}
      </div>
    {:else}
      <div class="empty-state">
        <div class="empty-state-content">
          <h2>Welcome to Diaryx</h2>
          <p>Select an entry from the sidebar to start editing</p>
        </div>
      </div>
    {/if}
  </main>
</div>

<!-- Tree Node Snippet -->
{#snippet treeNode(node: TreeNode, depth: number)}
  <div class="tree-item" style="--depth: {depth}">
    <button
      class="tree-item-button"
      class:active={currentEntry?.path === node.path}
      onclick={() => openEntry(node.path)}
    >
      {#if node.children.length > 0}
        <button
          type="button"
          class="expand-icon"
          class:expanded={expandedNodes.has(node.path)}
          onclick={(e) => {
            e.stopPropagation();
            toggleNode(node.path);
          }}
          aria-label="Toggle expand"
        >
          ▶
        </button>
      {:else}
        <span class="expand-icon placeholder"></span>
      {/if}
      <span class="node-name">{node.name}</span>
    </button>

    {#if node.children.length > 0 && expandedNodes.has(node.path)}
      <div class="tree-children">
        {#each node.children as child}
          {@render treeNode(child, depth + 1)}
        {/each}
      </div>
    {/if}
  </div>
{/snippet}

<style>
  .app {
    display: flex;
    height: 100vh;
    overflow: hidden;
  }

  /* Sidebar */
  .sidebar {
    width: 280px;
    min-width: 280px;
    background: var(--sidebar-bg, #f9fafb);
    border-right: 1px solid var(--border-color, #e5e7eb);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .sidebar-header {
    padding: 16px;
    border-bottom: 1px solid var(--border-color, #e5e7eb);
  }

  .logo {
    font-size: 1.25rem;
    font-weight: 600;
    margin: 0;
    color: var(--accent-color, #2563eb);
  }

  /* Search */
  .search-container {
    padding: 12px 16px;
    display: flex;
    gap: 8px;
    border-bottom: 1px solid var(--border-color, #e5e7eb);
  }

  .search-container input {
    flex: 1;
    padding: 8px 12px;
    border: 1px solid var(--border-color, #e5e7eb);
    border-radius: 6px;
    font-size: 14px;
    background: var(--input-bg, #ffffff);
    color: var(--text-color, #1a1a1a);
  }

  .search-container input:focus {
    outline: none;
    border-color: var(--accent-color, #2563eb);
  }

  .search-container button {
    padding: 8px 12px;
    border: none;
    border-radius: 6px;
    background: var(--accent-color, #2563eb);
    color: white;
    cursor: pointer;
  }

  /* Search Results */
  .search-results {
    flex: 1;
    overflow-y: auto;
  }

  .search-results-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 16px;
    font-size: 12px;
    color: var(--muted-color, #6b7280);
    border-bottom: 1px solid var(--border-color, #e5e7eb);
  }

  .search-results-header button {
    background: none;
    border: none;
    cursor: pointer;
    padding: 4px;
    color: var(--muted-color, #6b7280);
  }

  .search-result-item {
    display: flex;
    flex-direction: column;
    width: 100%;
    padding: 10px 16px;
    background: none;
    border: none;
    border-bottom: 1px solid var(--border-color, #e5e7eb);
    cursor: pointer;
    text-align: left;
    color: var(--text-color, #1a1a1a);
  }

  .search-result-item:hover {
    background: var(--hover-bg, #f3f4f6);
  }

  .result-title {
    font-weight: 500;
  }

  .result-matches {
    font-size: 12px;
    color: var(--muted-color, #6b7280);
  }

  /* Tree View */
  .tree-view {
    flex: 1;
    overflow-y: auto;
    padding: 8px 0;
  }

  .tree-item {
    padding-left: calc(var(--depth) * 16px);
  }

  .tree-item-button {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    padding: 6px 16px;
    background: none;
    border: none;
    cursor: pointer;
    text-align: left;
    color: var(--text-color, #1a1a1a);
    font-size: 14px;
  }

  .tree-item-button:hover {
    background: var(--hover-bg, #f3f4f6);
  }

  .tree-item-button.active {
    background: var(--active-bg, #dbeafe);
    color: var(--accent-color, #2563eb);
  }

  .expand-icon {
    width: 16px;
    height: 16px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 10px;
    transition: transform 0.15s;
    color: var(--muted-color, #6b7280);
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
  }

  .expand-icon.expanded {
    transform: rotate(90deg);
  }

  .loading-error {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    background: #fff;
    border: 1px solid #dc2626;
    border-radius: 8px;
    padding: 24px 32px;
    text-align: center;
    z-index: 1000;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  }

  .loading-error h2 {
    color: #dc2626;
    margin: 0 0 12px 0;
    font-size: 1.25rem;
  }

  .loading-error p {
    margin: 0 0 8px 0;
    color: #1a1a1a;
  }

  @media (prefers-color-scheme: dark) {
    .loading-error {
      background: #1f2937;
      border-color: #f87171;
    }

    .loading-error h2 {
      color: #f87171;
    }

    .loading-error p {
      color: #e5e5e5;
    }
  }

  .expand-icon.placeholder {
    visibility: hidden;
  }

  .tree-children {
    margin-left: 0;
  }

  .node-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .loading,
  .error {
    padding: 16px;
    text-align: center;
    color: var(--muted-color, #6b7280);
  }

  .error {
    color: #dc2626;
  }

  /* Main Content */
  .content {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .content-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px 24px;
    border-bottom: 1px solid var(--border-color, #e5e7eb);
    background: var(--header-bg, #ffffff);
  }

  .entry-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .entry-title {
    font-size: 1.125rem;
    font-weight: 600;
    margin: 0;
    color: var(--text-color, #1a1a1a);
  }

  .entry-path {
    font-size: 12px;
    color: var(--muted-color, #6b7280);
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .unsaved-indicator {
    font-size: 12px;
    color: #f59e0b;
  }

  .save-button {
    padding: 8px 16px;
    border: none;
    border-radius: 6px;
    background: var(--accent-color, #2563eb);
    color: white;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: opacity 0.15s;
  }

  .save-button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .save-button:not(:disabled):hover {
    opacity: 0.9;
  }

  .editor-area {
    flex: 1;
    padding: 24px;
    overflow: hidden;
  }

  .empty-state {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .empty-state-content {
    text-align: center;
    color: var(--muted-color, #6b7280);
  }

  .empty-state-content h2 {
    font-size: 1.5rem;
    font-weight: 600;
    margin: 0 0 8px 0;
    color: var(--text-color, #1a1a1a);
  }

  .empty-state-content p {
    margin: 0;
  }

  /* Dark mode support */
  @media (prefers-color-scheme: dark) {
    .app {
      --sidebar-bg: #111827;
      --border-color: #374151;
      --text-color: #e5e5e5;
      --muted-color: #9ca3af;
      --hover-bg: #1f2937;
      --active-bg: #1e3a5f;
      --accent-color: #60a5fa;
      --input-bg: #1f2937;
      --header-bg: #111827;
    }
  }
</style>
