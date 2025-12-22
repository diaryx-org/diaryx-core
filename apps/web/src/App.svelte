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
    type ValidationResult,
  } from "./lib/backend";
  import LeftSidebar from "./lib/LeftSidebar.svelte";
  import RightSidebar from "./lib/RightSidebar.svelte";
  import NewEntryModal from "./lib/NewEntryModal.svelte";
  import { Button } from "$lib/components/ui/button";
  import { Save, Download, PanelLeft, PanelRight, Menu } from "@lucide/svelte";

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
  let validationResult: ValidationResult | null = $state(null);
  let titleError: string | null = $state(null);

  // Sidebar states - collapsed by default on mobile
  let leftSidebarCollapsed = $state(true);
  let rightSidebarCollapsed = $state(true);

  // Check if we're on desktop and expand sidebars by default
  onMount(async () => {
    // Expand sidebars on desktop
    if (window.innerWidth >= 768) {
      leftSidebarCollapsed = false;
      rightSidebarCollapsed = false;
    }

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

      // Run initial validation
      await runValidation();
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
      titleError = null; // Clear any title error when switching files
      console.log("[App] Loaded entry:", currentEntry);
      console.log("[App] Frontmatter:", currentEntry?.frontmatter);
      console.log(
        "[App] Frontmatter keys:",
        Object.keys(currentEntry?.frontmatter ?? {}),
      );
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

  // Sidebar toggles
  function toggleLeftSidebar() {
    leftSidebarCollapsed = !leftSidebarCollapsed;
  }

  function toggleRightSidebar() {
    rightSidebarCollapsed = !rightSidebarCollapsed;
  }

  // Keyboard shortcuts
  function handleKeydown(event: KeyboardEvent) {
    if ((event.metaKey || event.ctrlKey) && event.key === "s") {
      event.preventDefault();
      save();
    }
    // Toggle left sidebar with Cmd/Ctrl + B
    if ((event.metaKey || event.ctrlKey) && event.key === "b") {
      event.preventDefault();
      toggleLeftSidebar();
    }
    // Toggle right sidebar with Cmd/Ctrl + I (for Info)
    if (
      (event.metaKey || event.ctrlKey) &&
      event.shiftKey &&
      event.key === "I"
    ) {
      event.preventDefault();
      toggleRightSidebar();
    }
  }

  async function handleCreateChildEntry(parentPath: string) {
    if (!backend) return;
    try {
      const newPath = await backend.createChildEntry(parentPath);
      await persistNow();
      tree = await backend.getWorkspaceTree();
      await openEntry(newPath);
      await runValidation();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function createNewEntry(path: string, title: string) {
    if (!backend) return;
    try {
      const newPath = await backend.createEntry(path, { title });
      tree = await backend.getWorkspaceTree();
      await openEntry(newPath);
      await runValidation();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      showNewEntryModal = false;
    }
  }

  async function handleDeleteEntry(path: string) {
    if (!backend) return;
    const confirm = window.confirm(
      `Are you sure you want to delete "${path.split('/').pop()?.replace('.md', '')}"?`
    );
    if (!confirm) return;

    try {
      await backend.deleteEntry(path);
      await persistNow();
      tree = await backend.getWorkspaceTree();
      await runValidation();
      // If we deleted the currently open entry, clear it
      if (currentEntry?.path === path) {
        currentEntry = null;
        isDirty = false;
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  // Run workspace validation
  async function runValidation() {
    if (!backend) return;
    try {
      validationResult = await backend.validateWorkspace();
    } catch (e) {
      console.error("[App] Validation error:", e);
    }
  }

  // Handle drag-drop: attach entry to new parent
  async function handleMoveEntry(entryPath: string, newParentPath: string) {
    if (!backend) return;
    if (entryPath === newParentPath) return; // Can't attach to self
    
    console.log(`[Drag-Drop] entryPath="${entryPath}" -> newParentPath="${newParentPath}"`);
    
    try {
      // Attach the entry to the new parent
      // This will:
      // - Add entry to newParent's `contents`
      // - Set entry's `part_of` to point to newParent
      await backend.attachEntryToParent(entryPath, newParentPath);
      await persistNow();
      tree = await backend.getWorkspaceTree();
      await runValidation();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  // Handle frontmatter property changes
  async function handlePropertyChange(key: string, value: unknown) {
    if (!backend || !currentEntry) return;
    try {
      // Special handling for title: need to check rename first
      if (key === "title" && typeof value === "string" && value.trim()) {
        const newFilename = backend.slugifyTitle(value);
        const currentFilename = currentEntry.path.split("/").pop() || "";
        
        // Only rename if the filename would actually change
        // For index files (have contents property), compare the directory name
        const isIndex = Array.isArray(currentEntry.frontmatter?.contents);
        const pathParts = currentEntry.path.split("/");
        const currentDir = isIndex 
          ? pathParts.slice(-2, -1)[0] || ""
          : currentFilename.replace(/\.md$/, "");
        const newDir = newFilename.replace(/\.md$/, "");
        
        if (currentDir !== newDir) {
          // Try rename FIRST, before updating frontmatter
          try {
            const newPath = await backend.renameEntry(currentEntry.path, newFilename);
            // Rename succeeded, now update title in frontmatter (at new path)
            await backend.setFrontmatterProperty(newPath, key, value);
            await persistNow();
            // Update current entry path and refresh tree
            currentEntry = { ...currentEntry, path: newPath, frontmatter: { ...currentEntry.frontmatter, [key]: value } };
            tree = await backend.getWorkspaceTree();
            titleError = null; // Clear any previous error
          } catch (renameError) {
            // Rename failed (e.g., target exists), show user-friendly error near title input
            // DON'T update the title - leave frontmatter unchanged
            const errorMsg = renameError instanceof Error ? renameError.message : String(renameError);
            if (errorMsg.includes("already exists") || errorMsg.includes("Destination")) {
              titleError = `A file named "${newFilename.replace('.md', '')}" already exists. Choose a different title.`;
            } else {
              titleError = `Could not rename: ${errorMsg}`;
            }
            // Don't update anything - input will show original value
          }
        } else {
          // No rename needed, just update title
          await backend.setFrontmatterProperty(currentEntry.path, key, value);
          await persistNow();
          currentEntry = { ...currentEntry, frontmatter: { ...currentEntry.frontmatter, [key]: value } };
          titleError = null;
        }
      } else {
        // Non-title properties: update normally
        await backend.setFrontmatterProperty(currentEntry.path, key, value);
        await persistNow();
        currentEntry = { ...currentEntry, frontmatter: { ...currentEntry.frontmatter, [key]: value } };
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function handlePropertyRemove(key: string) {
    if (!backend || !currentEntry) return;
    try {
      await backend.removeFrontmatterProperty(currentEntry.path, key);
      await persistNow();
      // Update local state
      const newFrontmatter = { ...currentEntry.frontmatter };
      delete newFrontmatter[key];
      currentEntry = { ...currentEntry, frontmatter: newFrontmatter };
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function handlePropertyAdd(key: string, value: unknown) {
    if (!backend || !currentEntry) return;
    try {
      await backend.setFrontmatterProperty(currentEntry.path, key, value);
      await persistNow();
      // Update local state
      currentEntry = { ...currentEntry, frontmatter: { ...currentEntry.frontmatter, [key]: value } };
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  function exportEntry() {
    if (!currentEntry) return;

    // Reconstruct full markdown with frontmatter
    let fullContent = "";
    if (
      currentEntry.frontmatter &&
      Object.keys(currentEntry.frontmatter).length > 0
    ) {
      const yamlLines = ["---"];
      for (const [key, value] of Object.entries(currentEntry.frontmatter)) {
        if (Array.isArray(value)) {
          yamlLines.push(`${key}:`);
          for (const item of value) {
            yamlLines.push(`  - ${item}`);
          }
        } else if (typeof value === "string" && value.includes("\n")) {
          // Multi-line string
          yamlLines.push(`${key}: |`);
          for (const line of value.split("\n")) {
            yamlLines.push(`  ${line}`);
          }
        } else if (typeof value === "string") {
          // Quote strings that might need it
          const needsQuotes = /[:#{}[\],&*?|<>=!%@`]/.test(value);
          yamlLines.push(
            `${key}: ${needsQuotes ? `"${value.replace(/"/g, '\\"')}"` : value}`,
          );
        } else {
          yamlLines.push(`${key}: ${JSON.stringify(value)}`);
        }
      }
      yamlLines.push("---");
      fullContent = yamlLines.join("\n") + "\n" + currentEntry.content;
    } else {
      fullContent = currentEntry.content;
    }

    const blob = new Blob([fullContent], {
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
    // Prioritize frontmatter.title for live updates, fall back to cached title
    const frontmatterTitle = entry.frontmatter?.title as string | undefined;
    return (
      frontmatterTitle ??
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

<div class="flex h-screen bg-background overflow-hidden">
  <!-- Left Sidebar -->
  <LeftSidebar
    {tree}
    {currentEntry}
    {isLoading}
    {error}
    bind:searchQuery
    {searchResults}
    {isSearching}
    {expandedNodes}
    {validationResult}
    collapsed={leftSidebarCollapsed}
    onOpenEntry={openEntry}
    onSearch={handleSearch}
    onClearSearch={clearSearch}
    onToggleNode={toggleNode}
    onToggleCollapse={toggleLeftSidebar}
    onMoveEntry={handleMoveEntry}
    onCreateChildEntry={handleCreateChildEntry}
    onDeleteEntry={handleDeleteEntry}
  />

  <!-- Main Content Area -->
  <main class="flex-1 flex flex-col overflow-hidden min-w-0">
    {#if currentEntry}
      <header
        class="flex items-center justify-between px-4 md:px-6 py-3 md:py-4 border-b border-border bg-card shrink-0"
      >
        <!-- Left side: toggle + title -->
        <div class="flex items-center gap-2 min-w-0 flex-1">
          <!-- Mobile menu button -->
          <Button
            variant="ghost"
            size="icon"
            onclick={toggleLeftSidebar}
            class="size-8 md:hidden shrink-0"
            aria-label="Toggle navigation"
          >
            <Menu class="size-4" />
          </Button>

          <!-- Desktop left sidebar toggle -->
          <Button
            variant="ghost"
            size="icon"
            onclick={toggleLeftSidebar}
            class="size-8 hidden md:flex shrink-0"
            aria-label="Toggle navigation sidebar"
          >
            <PanelLeft class="size-4" />
          </Button>

          <div class="min-w-0 flex-1">
            <h2
              class="text-lg md:text-xl font-semibold text-foreground truncate"
            >
              {getEntryTitle(currentEntry)}
            </h2>
            <p
              class="text-xs md:text-sm text-muted-foreground truncate hidden sm:block"
            >
              {currentEntry.path}
            </p>
          </div>
        </div>

        <!-- Right side: actions -->
        <div class="flex items-center gap-1 md:gap-2 ml-2 shrink-0">
          {#if isDirty}
            <span
              class="hidden sm:inline-flex px-2 py-1 text-xs font-medium rounded-md bg-amber-100 text-amber-800 dark:bg-amber-900/30 dark:text-amber-400"
            >
              Unsaved
            </span>
          {/if}
          <Button
            onclick={save}
            disabled={!isDirty}
            size="sm"
            class="gap-1 md:gap-2"
          >
            <Save class="size-4" />
            <span class="hidden sm:inline">Save</span>
          </Button>
          <Button
            onclick={exportEntry}
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
            onclick={toggleRightSidebar}
            class="size-8"
            aria-label="Toggle properties panel"
          >
            <PanelRight class="size-4" />
          </Button>
        </div>
      </header>

      <div class="flex-1 overflow-y-auto p-4 md:p-6">
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
      <!-- Empty state with sidebar toggles -->
      <header
        class="flex items-center justify-between px-4 py-3 border-b border-border bg-card shrink-0 md:hidden"
      >
        <Button
          variant="ghost"
          size="icon"
          onclick={toggleLeftSidebar}
          class="size-8"
          aria-label="Toggle navigation"
        >
          <Menu class="size-4" />
        </Button>
        <span class="text-lg font-semibold">Diaryx</span>
        <div class="size-8"></div>
      </header>

      <div class="flex-1 flex items-center justify-center">
        <div class="text-center max-w-md px-4">
          <!-- Desktop sidebar toggle when no entry -->
          <div class="hidden md:flex justify-center mb-4">
            {#if leftSidebarCollapsed}
              <Button
                variant="outline"
                size="sm"
                onclick={toggleLeftSidebar}
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
            Select an entry from the sidebar to start editing, or create a new
            one.
          </p>
        </div>
      </div>
    {/if}
  </main>

  <!-- Right Sidebar (Properties) -->
  <RightSidebar
    entry={currentEntry}
    collapsed={rightSidebarCollapsed}
    onToggleCollapse={toggleRightSidebar}
    onPropertyChange={handlePropertyChange}
    onPropertyRemove={handlePropertyRemove}
    onPropertyAdd={handlePropertyAdd}
    {titleError}
    onTitleErrorClear={() => titleError = null}
  />
</div>
