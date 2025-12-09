<script lang="ts">
  import { onMount } from "svelte";
  import {
    getConfig,
    getWorkspaceTree,
    getEntry,
    saveEntry,
    searchWorkspace,
  } from "./lib/api";
  import type { Config, TreeNode, EntryData, SearchResults } from "./lib/types";

  // Dynamically import Editor to avoid SSR issues
  let Editor: typeof import("./lib/Editor.svelte").default | null =
    $state(null);

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

    // Dynamically import the Editor component
    const module = await import("./lib/Editor.svelte");
    Editor = module.default;

    try {
      config = await getConfig();
      tree = await getWorkspaceTree();
      // Expand root by default
      if (tree) {
        expandedNodes.add(tree.path);
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      isLoading = false;
    }
  });

  // Open an entry
  async function openEntry(path: string) {
    if (isDirty) {
      const confirm = window.confirm(
        "You have unsaved changes. Do you want to discard them?",
      );
      if (!confirm) return;
    }

    try {
      isLoading = true;
      currentEntry = await getEntry(path);
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
    if (!currentEntry || !editorRef) return;

    try {
      const markdown = editorRef.getMarkdown();
      await saveEntry(currentEntry.path, markdown);
      isDirty = false;
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
    if (!searchQuery.trim()) {
      searchResults = null;
      return;
    }

    try {
      isSearching = true;
      searchResults = await searchWorkspace(searchQuery);
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
          <div class="loading">Loading...</div>
        {:else if error}
          <div class="error">{error}</div>
        {/if}
      </nav>
    {/if}
  </aside>

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
</style>
