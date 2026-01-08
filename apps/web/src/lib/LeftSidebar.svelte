<script lang="ts">
  import type { TreeNode, EntryData, ValidationResult, ValidationError } from "./backend";
  import { Button } from "$lib/components/ui/button";

  import * as ContextMenu from "$lib/components/ui/context-menu";
  import * as Popover from "$lib/components/ui/popover";
  import {
    ChevronRight,
    FileText,
    Folder,
    Loader2,
    PanelLeftClose,
    AlertCircle,
    Plus,
    Trash2,
    Clipboard,
    Download,
    Paperclip,
    Settings,
    Wrench,
  } from "@lucide/svelte";

  interface Props {
    tree: TreeNode | null;
    currentEntry: EntryData | null;
    isLoading: boolean;
    error: string | null;
    expandedNodes: Set<string>;
    validationResult: ValidationResult | null;
    collapsed: boolean;
    showUnlinkedFiles: boolean;
    onOpenEntry: (path: string) => void;
    onToggleNode: (path: string) => void;
    onToggleCollapse: () => void;
    onOpenSettings: () => void;
    onMoveEntry: (fromPath: string, toParentPath: string) => void;
    onCreateChildEntry: (parentPath: string) => void;
    onDeleteEntry: (path: string) => void;
    onExport: (path: string) => void;
    onAddAttachment: (entryPath: string) => void;
    onRemoveBrokenPartOf?: (filePath: string) => void;
    onRemoveBrokenContentsRef?: (indexPath: string, target: string) => void;
    onAttachUnlinkedEntry?: (entryPath: string) => void;
  }

  let {
    tree,
    currentEntry,
    isLoading,
    error,
    expandedNodes,
    validationResult,
    collapsed,
    onOpenEntry,
    onToggleNode,
    onToggleCollapse,
    onOpenSettings,
    onMoveEntry,
    onCreateChildEntry,
    onDeleteEntry,
    onExport,
    onAddAttachment,
    onRemoveBrokenPartOf,
    onRemoveBrokenContentsRef,
    onAttachUnlinkedEntry,
  }: Props = $props();

  // Extract unlinked entries (files/directories not in hierarchy) from validation result
  let unlinkedPaths = $derived(() => {
    const paths = new Set<string>();
    if (validationResult?.warnings) {
      for (const warning of validationResult.warnings) {
        if (warning.type === "UnlinkedEntry" && warning.path) {
          paths.add(warning.path);
        }
      }
    }
    return paths;
  });

  // Check if a path is unlinked
  function isUnlinked(path: string): boolean {
    return unlinkedPaths().has(path);
  }

  // Drag state
  let draggedPath: string | null = $state(null);
  let dropTargetPath: string | null = $state(null);

  function handleEntryClick(path: string) {
    onOpenEntry(path);
    // On mobile, collapse after selection
    if (window.innerWidth < 768) {
      onToggleCollapse();
    }
  }

  // Drag handlers
  function handleDragStart(e: DragEvent, path: string) {
    e.stopPropagation(); // Prevent parent nodes from overwriting draggedPath
    draggedPath = path;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = "move";
      e.dataTransfer.setData("text/plain", path);
    }
  }

  function handleDragOver(e: DragEvent, path: string) {
    e.preventDefault();
    if (e.dataTransfer) {
      e.dataTransfer.dropEffect = "move";
    }
    dropTargetPath = path;
  }

  function handleDragLeave() {
    dropTargetPath = null;
  }

  function handleDrop(e: DragEvent, targetPath: string) {
    e.preventDefault();
    e.stopPropagation(); // Prevent bubbling to parent tree nodes
    if (draggedPath && draggedPath !== targetPath) {
      onMoveEntry(draggedPath, targetPath);
    }
    draggedPath = null;
    dropTargetPath = null;
  }

  function handleDragEnd() {
    draggedPath = null;
    dropTargetPath = null;
  }

  // Check if a file has validation errors
  function hasValidationError(path: string): boolean {
    if (!validationResult) return false;
    return validationResult.errors.some(
      (err) => err.file === path || err.index === path,
    );
  }

  // Get validation errors for a specific path
  function getValidationErrors(path: string): ValidationError[] {
    if (!validationResult) return [];
    return validationResult.errors.filter(
      (err) => err.file === path || err.index === path,
    );
  }

  // Get human-readable description for a validation error
  function getErrorDescription(error: ValidationError): string {
    switch (error.type) {
      case "BrokenPartOf":
        return `This file's "part_of" references a file that doesn't exist`;
      case "BrokenContentsRef":
        return `This index's "contents" references a file that doesn't exist`;
      default:
        return "Unknown validation error";
    }
  }


  // Copy path to clipboard
  async function copyPathToClipboard(path: string) {
    try {
      await navigator.clipboard.writeText(path);
    } catch (e) {
      console.error("Failed to copy path:", e);
    }
  }
</script>

<!-- Mobile overlay backdrop -->
{#if !collapsed}
  <button
    type="button"
    class="fixed inset-0 bg-black/50 z-30 md:hidden"
    onclick={onToggleCollapse}
    aria-label="Close sidebar"
  ></button>
{/if}

<aside
  class="flex flex-col h-screen border-r border-border bg-sidebar text-sidebar-foreground transition-all duration-300 ease-in-out
    {collapsed ? 'w-0 opacity-0 overflow-hidden md:w-0' : 'w-72'}
    fixed md:relative z-40 md:z-auto"
>
  <!-- Header -->
  <div
    class="flex items-center justify-between px-4 py-4 border-b border-sidebar-border shrink-0"
  >
    <a
      href="/"
      class="text-xl font-semibold text-sidebar-foreground hover:text-sidebar-foreground/80 transition-colors"
    >
      Diaryx
    </a>
    <div class="flex items-center gap-1">
      <Button
        variant="ghost"
        size="icon"
        onclick={() => tree && onExport(tree.path)}
        class="size-8"
        aria-label="Export workspace"
        disabled={!tree}
      >
        <Download class="size-4" />
      </Button>
      <Button
        variant="ghost"
        size="icon"
        onclick={onOpenSettings}
        class="size-8"
        aria-label="Open settings"
      >
        <Settings class="size-4" />
      </Button>
      <Button
        variant="ghost"
        size="icon"
        onclick={onToggleCollapse}
        class="size-8"
        aria-label="Collapse sidebar"
      >
        <PanelLeftClose class="size-4" />
      </Button>
    </div>
  </div>

  <!-- Content Area -->
  <div class="flex-1 overflow-y-auto px-3 pb-3">
    {#if isLoading}
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
      <div class="space-y-0.5" role="tree" aria-label="Workspace entries">
        {@render treeNode(tree, 0)}
      </div>
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
  <ContextMenu.Root>
    <ContextMenu.Trigger>
      <div
        class="select-none"
        role="treeitem"
        tabindex={0}
        aria-selected={currentEntry?.path === node.path}
        aria-expanded={node.children.length > 0
          ? expandedNodes.has(node.path)
          : undefined}
        aria-level={depth + 1}
        draggable="true"
        ondragstart={(e) => handleDragStart(e, node.path)}
        ondragend={handleDragEnd}
      >
        <div
          class="group flex items-center gap-1 rounded-md hover:bg-sidebar-accent transition-colors
            {dropTargetPath === node.path
            ? 'bg-primary/20 ring-2 ring-primary'
            : ''}"
          style="padding-left: {depth * 12}px"
          role="presentation"
          ondragover={(e) => handleDragOver(e, node.path)}
          ondragleave={handleDragLeave}
          ondrop={(e) => handleDrop(e, node.path)}
        >
          {#if node.children.length > 0}
            <button
              type="button"
              class="p-1 rounded-sm hover:bg-sidebar-accent-foreground/10 transition-colors"
              onclick={(e) => {
                e.stopPropagation();
                onToggleNode(node.path);
              }}
              aria-label="Toggle folder"
              tabindex={-1}
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
            onclick={() => handleEntryClick(node.path)}
          >
            {#if node.children.length > 0}
              <Folder class="size-4 shrink-0 text-muted-foreground" />
            {:else}
              <FileText class="size-4 shrink-0 text-muted-foreground" />
            {/if}
            <span class="truncate flex-1">{node.name.replace(".md", "")}</span>
            {#if hasValidationError(node.path)}
              {@const errors = getValidationErrors(node.path)}
              <Popover.Root>
                <Popover.Trigger
                  onclick={(e: MouseEvent) => e.stopPropagation()}
                  class="shrink-0 focus:outline-none"
                >
                  <AlertCircle class="size-4 text-destructive hover:text-destructive/80 transition-colors" />
                </Popover.Trigger>
                <Popover.Content class="w-80 p-3" side="right" align="start">
                  <div class="space-y-3">
                    <div class="flex items-start gap-2">
                      <AlertCircle class="size-4 text-destructive shrink-0 mt-0.5" />
                      <div class="space-y-1">
                        <p class="text-sm font-medium">Validation Error</p>
                        {#each errors as error}
                          <div class="text-sm text-muted-foreground">
                            <p>{getErrorDescription(error)}</p>
                            <p class="font-mono text-xs mt-1 truncate" title={error.target}>
                              Target: {error.target}
                            </p>
                          </div>
                          {#if error.type === "BrokenPartOf" && onRemoveBrokenPartOf}
                            <Button
                              variant="outline"
                              size="sm"
                              class="mt-2 gap-1.5"
                              onclick={(e: MouseEvent) => {
                                e.stopPropagation();
                                if (error.file) onRemoveBrokenPartOf(error.file);
                              }}
                            >
                              <Wrench class="size-3" />
                              Remove broken reference
                            </Button>
                          {/if}
                          {#if error.type === "BrokenContentsRef" && onRemoveBrokenContentsRef}
                            <Button
                              variant="outline"
                              size="sm"
                              class="mt-2 gap-1.5"
                              onclick={(e: MouseEvent) => {
                                e.stopPropagation();
                                if (error.index && error.target) onRemoveBrokenContentsRef(error.index, error.target);
                              }}
                            >
                              <Wrench class="size-3" />
                              Remove from contents
                            </Button>
                          {/if}
                        {/each}
                      </div>
                    </div>
                  </div>
                </Popover.Content>
              </Popover.Root>
            {/if}
            {#if isUnlinked(node.path)}
              <Popover.Root>
                <Popover.Trigger
                  onclick={(e: MouseEvent) => e.stopPropagation()}
                  class="shrink-0 focus:outline-none"
                >
                  <AlertCircle class="size-4 text-amber-500 hover:text-amber-400 transition-colors" />
                </Popover.Trigger>
                <Popover.Content class="w-72 p-3" side="right" align="start">
                  <div class="space-y-3">
                    <div class="flex items-start gap-2">
                      <AlertCircle class="size-4 text-amber-500 shrink-0 mt-0.5" />
                      <div class="space-y-1">
                        <p class="text-sm font-medium">Unlinked Entry</p>
                        <p class="text-sm text-muted-foreground">
                          This entry is not part of the workspace hierarchy. Drag it onto a parent entry to link it.
                        </p>
                        {#if onAttachUnlinkedEntry}
                          <Button
                            variant="outline"
                            size="sm"
                            class="mt-2 gap-1.5"
                            onclick={(e: MouseEvent) => {
                              e.stopPropagation();
                              onAttachUnlinkedEntry(node.path);
                            }}
                          >
                            <Wrench class="size-3" />
                            Add to workspace root
                          </Button>
                        {/if}
                      </div>
                    </div>
                  </div>
                </Popover.Content>
              </Popover.Root>
            {/if}
          </button>
        </div>

        {#if node.children.length > 0 && expandedNodes.has(node.path)}
          <div class="mt-0.5" role="group">
            {#each node.children as child}
              {@render treeNode(child, depth + 1)}
            {/each}
          </div>
        {/if}
      </div>
    </ContextMenu.Trigger>

    <ContextMenu.Content class="w-48">
      <ContextMenu.Item onclick={() => onCreateChildEntry(node.path)}>
        <Plus class="size-4 mr-2" />
        New Entry Here
      </ContextMenu.Item>
      <ContextMenu.Item onclick={() => copyPathToClipboard(node.path)}>
        <Clipboard class="size-4 mr-2" />
        Copy Path
      </ContextMenu.Item>
      <ContextMenu.Item onclick={() => onExport(node.path)}>
        <Download class="size-4 mr-2" />
        Export...
      </ContextMenu.Item>
      <ContextMenu.Item onclick={() => onAddAttachment(node.path)}>
        <Paperclip class="size-4 mr-2" />
        Add Attachment...
      </ContextMenu.Item>
      <ContextMenu.Separator />
      <ContextMenu.Item
        variant="destructive"
        onclick={() => onDeleteEntry(node.path)}
      >
        <Trash2 class="size-4 mr-2" />
        Delete
      </ContextMenu.Item>
    </ContextMenu.Content>
  </ContextMenu.Root>
{/snippet}
