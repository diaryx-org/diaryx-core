<script lang="ts">
  import type { TreeNode, EntryData, ValidationResultWithMeta, ValidationErrorWithMeta, ValidationWarningWithMeta, Api } from "./backend";
  import { Button } from "$lib/components/ui/button";
  import { toast } from "svelte-sonner";

  import * as ContextMenu from "$lib/components/ui/context-menu";
  import * as Popover from "$lib/components/ui/popover";
  import {
    ChevronRight,
    ChevronDown,
    FileText,
    Folder,
    FolderPlus,
    Loader2,
    PanelLeftClose,
    AlertCircle,
    AlertTriangle,
    Plus,
    Trash2,
    Clipboard,
    Download,
    Paperclip,
    Settings,
    Wrench,
    Eye,
    X,
    SearchCheck,
  } from "@lucide/svelte";

  interface Props {
    tree: TreeNode | null;
    currentEntry: EntryData | null;
    isLoading: boolean;
    error: string | null;
    expandedNodes: Set<string>;
    validationResult: ValidationResultWithMeta | null;
    collapsed: boolean;
    showUnlinkedFiles: boolean;
    api: Api | null;
    onOpenEntry: (path: string) => void;
    onToggleNode: (path: string) => void;
    onToggleCollapse: () => void;
    onOpenSettings: () => void;
    onMoveEntry: (fromPath: string, toParentPath: string) => void;
    onCreateChildEntry: (parentPath: string) => void;
    onDeleteEntry: (path: string) => void;
    onExport: (path: string) => void;
    onAddAttachment: (entryPath: string) => void;
    onMoveAttachment?: (sourceEntryPath: string, targetEntryPath: string, attachmentPath: string) => void;
    onRemoveBrokenPartOf?: (filePath: string) => void;
    onRemoveBrokenContentsRef?: (indexPath: string, target: string) => void;
    onAttachUnlinkedEntry?: (entryPath: string) => void;
    onValidationFix?: () => void;
    onLoadChildren?: (path: string) => Promise<void>;
    onValidate?: (path: string) => void;
  }

  let {
    tree,
    currentEntry,
    isLoading,
    error,
    expandedNodes,
    validationResult,
    collapsed,
    showUnlinkedFiles,
    api,
    onOpenEntry,
    onToggleNode,
    onToggleCollapse,
    onOpenSettings,
    onMoveEntry,
    onCreateChildEntry,
    onDeleteEntry,
    onExport,
    onAddAttachment,
    onMoveAttachment,
    onRemoveBrokenPartOf,
    onRemoveBrokenContentsRef,
    onAttachUnlinkedEntry,
    onValidationFix,
    onLoadChildren,
    onValidate,
  }: Props = $props();

  // Track which nodes are currently loading children
  let loadingNodes = $state(new Set<string>());

  // Check if a node has unloaded children (placeholder "... (N more)" node)
  function hasUnloadedChildren(node: TreeNode): boolean {
    return node.children.some((child) => child.name.startsWith("... ("));
  }

  // Handle expand with lazy loading
  async function handleToggleNode(path: string, node: TreeNode) {
    // If node is being expanded and has unloaded children, load them first
    if (!expandedNodes.has(path) && hasUnloadedChildren(node) && onLoadChildren) {
      loadingNodes.add(path);
      loadingNodes = new Set(loadingNodes);
      try {
        await onLoadChildren(path);
      } finally {
        loadingNodes.delete(path);
        loadingNodes = new Set(loadingNodes);
      }
    }
    onToggleNode(path);
  }

  // Check if a node is loading
  function isNodeLoading(path: string): boolean {
    return loadingNodes.has(path);
  }

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

  // =========================================================================
  // Inherited Warnings - orphan file warnings bubble up to nearest index
  // =========================================================================

  // Orphan warning types that should inherit to parent indexes
  const ORPHAN_WARNING_TYPES = ['OrphanFile', 'OrphanBinaryFile', 'MissingPartOf', 'UnlinkedEntry', 'UnlistedFile'];

  // Build a map of directory path -> index file path from the tree
  // Only includes actual index files (nodes with children), not leaf files
  function buildTreeIndexMap(node: TreeNode, map: Map<string, string> = new Map(), isRoot: boolean = true): Map<string, string> {
    // Only add to map if this is an index file:
    // - Has children (meaning it has contents property with entries)
    // - OR is the root node (root index is always valid even if empty)
    if (node.children.length > 0 || isRoot) {
      // Extract directory from the index path (e.g., "workspace/docs/README.md" -> "workspace/docs")
      const lastSlash = node.path.lastIndexOf('/');
      const dir = lastSlash >= 0 ? node.path.substring(0, lastSlash) : '';
      map.set(dir, node.path);
    }

    for (const child of node.children) {
      buildTreeIndexMap(child, map, false);
    }
    return map;
  }

  // Find the nearest index file in the tree for a given file path
  function findNearestIndex(filePath: string, treeIndexMap: Map<string, string>): string | null {
    // Get the file's directory
    const lastSlash = filePath.lastIndexOf('/');
    let dir = lastSlash >= 0 ? filePath.substring(0, lastSlash) : '';

    // Walk up the directory tree
    while (dir) {
      if (treeIndexMap.has(dir)) {
        return treeIndexMap.get(dir)!;
      }
      // Go up one level
      const parentSlash = dir.lastIndexOf('/');
      if (parentSlash < 0) break;
      dir = dir.substring(0, parentSlash);
    }

    // Check root level
    if (treeIndexMap.has('')) {
      return treeIndexMap.get('')!;
    }

    return null;
  }

  // Extract file path from any warning type
  function getWarningFilePath(warning: ValidationWarningWithMeta): string | null {
    switch (warning.type) {
      case 'OrphanFile':
      case 'OrphanBinaryFile':
      case 'MissingPartOf':
        return warning.file ?? null;
      case 'UnlinkedEntry':
        return warning.path ?? null;
      case 'UnlistedFile':
        return warning.file ?? null;
      case 'CircularReference': {
        // Return the first file in the cycle for viewing
        const files = (warning as { files?: string[] }).files;
        return files && files.length > 0 ? files[0] : null;
      }
      case 'NonPortablePath':
        return warning.file ?? null;
      case 'MultipleIndexes':
        return warning.directory ?? null;
      case 'InvalidContentsRef':
        return warning.index ?? null;
      default:
        return null;
    }
  }

  // Get filename from path for display
  function getFileName(path: string): string {
    const lastSlash = path.lastIndexOf('/');
    return lastSlash >= 0 ? path.substring(lastSlash + 1) : path;
  }

  // Get human-readable description for a warning (uses metadata from core)
  function getWarningDescription(warning: ValidationWarningWithMeta): string {
    return warning.description;
  }

  // Computed tree index map for reuse
  let treeIndexMap = $derived(() => {
    if (!tree) return new Map<string, string>();
    return buildTreeIndexMap(tree);
  });

  // Check if a warning is auto-fixable (uses metadata from core)
  function isWarningFixable(warning: ValidationWarningWithMeta): boolean {
    return warning.can_auto_fix;
  }

  // Check if a warning can be viewed (uses metadata from core)
  function isWarningViewable(warning: ValidationWarningWithMeta): boolean {
    return warning.is_viewable;
  }

  // Check if a warning supports "Choose parent..." option (uses metadata from core)
  function supportsParentPicker(warning: ValidationWarningWithMeta): boolean {
    return warning.supports_parent_picker;
  }

  // Derived state: map of index paths to their inherited warnings
  // Only active when showUnlinkedFiles is OFF (hierarchy mode)
  // When showUnlinkedFiles is ON, orphan files appear directly in the tree
  let inheritedWarnings = $derived(() => {
    const map = new Map<string, ValidationWarningWithMeta[]>();

    // Skip inherited warnings when showing all files - orphans are visible directly
    if (!tree || !validationResult?.warnings || showUnlinkedFiles) {
      return map;
    }

    // Build the tree index map once
    const treeIndexMap = buildTreeIndexMap(tree);

    // Process each orphan-type warning
    for (const warning of validationResult.warnings) {
      if (!ORPHAN_WARNING_TYPES.includes(warning.type)) continue;

      // Extract the file path from the warning
      const filePath = getWarningFilePath(warning);
      if (!filePath) continue;

      // Skip if this file is directly in the tree (will be shown as direct warning)
      // This happens when showUnlinkedFiles is ON
      if (treeIndexMap.has(filePath.substring(0, filePath.lastIndexOf('/')))) {
        // Check if the file itself is the index (not inherited)
        const dir = filePath.substring(0, filePath.lastIndexOf('/'));
        if (treeIndexMap.get(dir) === filePath) continue;
      }

      // Find the nearest index in the tree
      const nearestIndex = findNearestIndex(filePath, treeIndexMap);
      if (!nearestIndex) continue;

      // Don't inherit to self
      if (nearestIndex === filePath) continue;

      // Add to the map
      if (!map.has(nearestIndex)) {
        map.set(nearestIndex, []);
      }
      map.get(nearestIndex)!.push(warning);
    }

    return map;
  });

  // Check if an index has inherited warnings
  function hasInheritedWarnings(path: string): boolean {
    const warnings = inheritedWarnings().get(path);
    return warnings !== undefined && warnings.length > 0;
  }

  // Get inherited warnings for an index
  function getInheritedWarnings(path: string): ValidationWarningWithMeta[] {
    return inheritedWarnings().get(path) ?? [];
  }

  // =========================================================================
  // Problems Panel State
  // =========================================================================

  let problemsPanelOpen = $state(true);
  let isFixingAll = $state(false);

  // Parent picker state
  let showParentPicker = $state(false);
  let pendingWarningForParentPicker = $state<ValidationWarningWithMeta | null>(null);
  let availableParents = $state<string[]>([]);
  let isLoadingParents = $state(false);

  // Show parent picker for a warning
  async function handleChooseParent(warning: ValidationWarningWithMeta) {
    const filePath = getWarningFilePath(warning);
    if (!filePath || !api || !tree) return;

    isLoadingParents = true;
    try {
      availableParents = await api.getAvailableParentIndexes(filePath, tree.path);
      pendingWarningForParentPicker = warning;
      showParentPicker = true;
    } catch (e) {
      toast.error('Failed to load available parents');
    } finally {
      isLoadingParents = false;
    }
  }

  // Select a parent from the picker
  async function handleSelectParent(parentPath: string) {
    if (!pendingWarningForParentPicker || !api) return;

    const filePath = getWarningFilePath(pendingWarningForParentPicker);
    if (!filePath) return;

    try {
      let result;
      const warning = pendingWarningForParentPicker;

      if (warning.type === 'OrphanBinaryFile') {
        result = await api.fixOrphanBinaryFile(parentPath, filePath);
      } else if (warning.type === 'UnlinkedEntry') {
        const w = warning as { is_dir?: boolean; index_file?: string | null };
        if (w.is_dir && w.index_file) {
          result = await api.fixUnlistedFile(parentPath, w.index_file);
        } else {
          result = await api.fixUnlistedFile(parentPath, filePath);
        }
      } else {
        result = await api.fixUnlistedFile(parentPath, filePath);
      }

      if (result?.success) {
        toast.success(result.message);
        onValidationFix?.();
      } else if (result) {
        toast.error(result.message);
      }
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to fix issue');
    }

    showParentPicker = false;
    pendingWarningForParentPicker = null;
  }

  // Close parent picker
  function closeParentPicker() {
    showParentPicker = false;
    pendingWarningForParentPicker = null;
  }

  // Count total problems
  let totalProblems = $derived(() => {
    if (!validationResult) return 0;
    return validationResult.errors.length + validationResult.warnings.length;
  });

  // Count fixable problems
  let fixableCount = $derived(() => {
    if (!validationResult) return 0;
    let count = validationResult.errors.length; // All errors are fixable
    for (const warning of validationResult.warnings) {
      if (isWarningFixable(warning)) count++;
    }
    return count;
  });

  // Fix all fixable issues
  async function handleFixAll() {
    if (!api || !validationResult || isFixingAll) return;

    isFixingAll = true;
    try {
      // Cast to any to avoid type mismatch between interface and generated types
      const result = await api.fixAll(validationResult as any);
      const fixed = result.total_fixed;
      const failed = result.total_failed;

      if (fixed > 0 && failed === 0) {
        toast.success(`Fixed ${fixed} issue${fixed > 1 ? 's' : ''}`);
      } else if (fixed > 0 && failed > 0) {
        toast.warning(`Fixed ${fixed} issue${fixed > 1 ? 's' : ''}, ${failed} failed`);
      } else if (failed > 0) {
        toast.error(`Failed to fix ${failed} issue${failed > 1 ? 's' : ''}`);
      }

      onValidationFix?.();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to fix issues');
    } finally {
      isFixingAll = false;
    }
  }

  // Fix individual warning
  async function handleFixWarning(warning: ValidationWarningWithMeta) {
    if (!api) return;

    try {
      let result;
      switch (warning.type) {
        case 'OrphanBinaryFile': {
          const w = warning as { file: string; suggested_index?: string | null };
          if (w.suggested_index) {
            result = await api.fixOrphanBinaryFile(w.suggested_index, w.file);
          }
          break;
        }
        case 'MissingPartOf': {
          const w = warning as { file: string; suggested_index?: string | null };
          if (w.suggested_index) {
            result = await api.fixMissingPartOf(w.file, w.suggested_index);
          }
          break;
        }
        case 'UnlistedFile': {
          const w = warning as { index: string; file: string };
          result = await api.fixUnlistedFile(w.index, w.file);
          break;
        }
        case 'NonPortablePath': {
          const w = warning as { file: string; property: string; value: string; suggested: string };
          result = await api.fixNonPortablePath(w.file, w.property, w.value, w.suggested);
          break;
        }
        case 'OrphanFile': {
          // Use the backend's suggested_index
          const w = warning as { file: string; suggested_index: string | null };
          if (w.suggested_index) {
            result = await api.fixUnlistedFile(w.suggested_index, w.file);
          } else {
            toast.error('No parent index found to add this file to');
            return;
          }
          break;
        }
        case 'UnlinkedEntry': {
          // Use the backend's suggested_index and index_file for directories
          const w = warning as unknown as { path: string; is_dir: boolean; suggested_index: string | null; index_file: string | null };
          if (!w.suggested_index) {
            toast.error('No parent index found to add this entry to');
            return;
          }
          if (w.is_dir) {
            // For directories, link the index file inside
            if (w.index_file) {
              result = await api.fixUnlistedFile(w.suggested_index, w.index_file);
            } else {
              toast.error('Cannot link directory without an index file. Create an index.md or README.md first.');
              return;
            }
          } else {
            // For files, link directly
            result = await api.fixUnlistedFile(w.suggested_index, w.path);
          }
          break;
        }
        case 'CircularReference': {
          // Fix by removing the suggested contents reference
          const w = warning as { suggested_file?: string | null; suggested_remove_part_of?: string | null };
          if (w.suggested_file && w.suggested_remove_part_of) {
            result = await api.fixCircularReference(w.suggested_file, w.suggested_remove_part_of);
          } else {
            toast.error('Cannot auto-fix this circular reference. Manually edit one of the files involved.');
            return;
          }
          break;
        }
      }

      if (result?.success) {
        toast.success(result.message);
        onValidationFix?.();
      } else if (result) {
        toast.error(result.message);
      }
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to fix issue');
    }
  }

  // Navigate to a file from a warning
  function handleViewWarning(warning: ValidationWarningWithMeta) {
    const filePath = getWarningFilePath(warning);
    if (filePath && filePath.endsWith('.md')) {
      onOpenEntry(filePath);
    }
  }

  // Extract file path from any error type
  function getErrorFilePath(error: ValidationErrorWithMeta): string | null {
    switch (error.type) {
      case 'BrokenPartOf':
      case 'BrokenAttachment':
        return error.file ?? null;
      case 'BrokenContentsRef':
        return error.index ?? null;
      default:
        return null;
    }
  }

  // Extract target from errors that have it
  function getErrorTarget(error: ValidationErrorWithMeta): string | null {
    switch (error.type) {
      case 'BrokenPartOf':
      case 'BrokenContentsRef':
        return error.target ?? null;
      case 'BrokenAttachment':
        return error.attachment ?? null;
    }
  }

  // Check if an error can be viewed
  function isErrorViewable(error: ValidationErrorWithMeta): boolean {
    const filePath = getErrorFilePath(error);
    return filePath !== null && filePath.endsWith('.md');
  }

  // Navigate to a file from an error
  function handleViewError(error: ValidationErrorWithMeta) {
    const filePath = getErrorFilePath(error);
    if (filePath && filePath.endsWith('.md')) {
      onOpenEntry(filePath);
    }
  }

  // Fix individual error
  async function handleFixError(error: ValidationErrorWithMeta) {
    if (!api) return;

    try {
      let result;
      switch (error.type) {
        case 'BrokenPartOf':
          if (error.file) {
            result = await api.fixBrokenPartOf(error.file);
          }
          break;
        case 'BrokenContentsRef':
          if (error.index && error.target) {
            result = await api.fixBrokenContentsRef(error.index, error.target);
          }
          break;
        case 'BrokenAttachment':
          if (error.file && error.attachment) {
            result = await api.fixBrokenAttachment(error.file, error.attachment);
          }
          break;
      }

      if (result?.success) {
        toast.success(result.message);
        onValidationFix?.();
      } else if (result) {
        toast.error(result.message);
      }
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to fix issue');
    }
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
      e.dataTransfer.setData("application/vnd.diaryx.path", path);
    }
  }

  function handleDragOver(e: DragEvent, path: string) {
    // If we have an internal drag in progress, allow dropping
    if (draggedPath) {
      e.preventDefault();
      if (e.dataTransfer) {
        e.dataTransfer.dropEffect = "move";
      }
      dropTargetPath = path;
      return;
    }

    // Also allow if it matches our custom MIME type (for robustness if state is lost)
    if (e.dataTransfer && e.dataTransfer.types.includes("application/vnd.diaryx.path")) {
      e.preventDefault();
      e.dataTransfer.dropEffect = "move";
      dropTargetPath = path;
      return;
    }

    // Allow attachment drops from RightSidebar
    if (e.dataTransfer && e.dataTransfer.types.includes("text/x-diaryx-attachment")) {
      e.preventDefault();
      e.dataTransfer.dropEffect = "move";
      dropTargetPath = path;
    }
  }

  function handleDragLeave() {
    dropTargetPath = null;
  }

  function handleDrop(e: DragEvent, targetPath: string) {
    e.preventDefault();
    e.stopPropagation(); // Prevent bubbling to parent tree nodes

    // Handle entry move
    if (draggedPath && draggedPath !== targetPath) {
      onMoveEntry(draggedPath, targetPath);
      draggedPath = null;
      dropTargetPath = null;
      return;
    }

    // Handle attachment move from RightSidebar
    if (e.dataTransfer) {
      const attachmentPath = e.dataTransfer.getData("text/x-diaryx-attachment");
      const sourceEntryPath = e.dataTransfer.getData("text/x-diaryx-source-entry");

      if (attachmentPath && sourceEntryPath && sourceEntryPath !== targetPath) {
        onMoveAttachment?.(sourceEntryPath, targetPath, attachmentPath);
      }
    }

    draggedPath = null;
    dropTargetPath = null;
  }

  function handleDragEnd() {
    draggedPath = null;
    dropTargetPath = null;
  }

  // Get the file path associated with an error (for filtering)
  function getErrorAssociatedPath(error: ValidationErrorWithMeta): string | null {
    switch (error.type) {
      case 'BrokenPartOf':
      case 'BrokenAttachment':
        return error.file;
      case 'BrokenContentsRef':
        return error.index;
    }
  }

  // Check if a file has validation errors
  function hasValidationError(path: string): boolean {
    if (!validationResult) return false;
    return validationResult.errors.some(
      (err) => getErrorAssociatedPath(err) === path,
    );
  }

  // Get validation errors for a specific path
  function getValidationErrors(path: string): ValidationErrorWithMeta[] {
    if (!validationResult) return [];
    return validationResult.errors.filter(
      (err) => getErrorAssociatedPath(err) === path,
    );
  }

  // Get human-readable description for a validation error
  function getErrorDescription(error: ValidationErrorWithMeta): string {
    return error.description;
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

  <!-- Problems Panel -->
  {#if totalProblems() > 0}
    <div class="border-t border-sidebar-border shrink-0">
      <button
        type="button"
        class="w-full flex items-center justify-between px-4 py-2 hover:bg-sidebar-accent transition-colors"
        onclick={() => problemsPanelOpen = !problemsPanelOpen}
      >
        <div class="flex items-center gap-2">
          {#if problemsPanelOpen}
            <ChevronDown class="size-4 text-muted-foreground" />
          {:else}
            <ChevronRight class="size-4 text-muted-foreground" />
          {/if}
          <span class="text-sm font-medium">Problems</span>
          <span class="text-xs px-1.5 py-0.5 rounded-full bg-amber-500/20 text-amber-600 dark:text-amber-400">
            {totalProblems()}
          </span>
        </div>
      </button>
      {#if problemsPanelOpen}
        <div class="px-3 pb-3 max-h-64 overflow-y-auto space-y-2">
          <!-- Errors -->
          {#if validationResult && validationResult.errors.length > 0}
            <div class="space-y-1">
              <p class="text-xs font-medium text-destructive px-1">
                Errors ({validationResult.errors.length})
              </p>
              {#each validationResult.errors as error}
                <div class="text-xs p-2 bg-destructive/10 rounded flex items-start justify-between gap-2">
                  <div class="min-w-0 flex-1">
                    <span class="font-mono truncate block" title={getErrorFilePath(error) ?? ''}>
                      {getFileName(getErrorFilePath(error) ?? '')}
                    </span>
                    <span class="text-muted-foreground">
                      {getErrorDescription(error)}
                    </span>
                  </div>
                  <div class="flex gap-0.5 shrink-0">
                    {#if isErrorViewable(error)}
                      <Button
                        variant="ghost"
                        size="sm"
                        class="h-6 px-1.5"
                        title="View file"
                        onclick={() => handleViewError(error)}
                      >
                        <Eye class="size-3" />
                      </Button>
                    {/if}
                    {#if api}
                      <Button
                        variant="ghost"
                        size="sm"
                        class="h-6 px-1.5"
                        title="Fix issue"
                        onclick={() => handleFixError(error)}
                      >
                        <Wrench class="size-3" />
                      </Button>
                    {/if}
                  </div>
                </div>
              {/each}
            </div>
          {/if}

          <!-- Warnings -->
          {#if validationResult && validationResult.warnings.length > 0}
            <div class="space-y-1">
              <p class="text-xs font-medium text-amber-600 dark:text-amber-400 px-1">
                Warnings ({validationResult.warnings.length})
              </p>
              {#each validationResult.warnings as warning}
                {@const filePath = getWarningFilePath(warning)}
                <div class="text-xs p-2 bg-amber-500/10 rounded flex items-start justify-between gap-2">
                  <div class="min-w-0 flex-1">
                    <span class="font-mono truncate block" title={filePath ?? ''}>
                      {filePath ? getFileName(filePath) : 'Unknown'}
                    </span>
                    <span class="text-muted-foreground">
                      {getWarningDescription(warning)}
                    </span>
                  </div>
                  <div class="flex gap-0.5 shrink-0">
                    {#if isWarningViewable(warning)}
                      <Button
                        variant="ghost"
                        size="sm"
                        class="h-6 px-1.5"
                        title="View file"
                        onclick={() => handleViewWarning(warning)}
                      >
                        <Eye class="size-3" />
                      </Button>
                    {/if}
                    {#if isWarningFixable(warning) && api}
                      <Button
                        variant="ghost"
                        size="sm"
                        class="h-6 px-1.5"
                        title="Fix issue"
                        onclick={() => handleFixWarning(warning)}
                      >
                        <Wrench class="size-3" />
                      </Button>
                    {/if}
                    {#if supportsParentPicker(warning) && api}
                      <Button
                        variant="ghost"
                        size="sm"
                        class="h-6 px-1.5"
                        title="Choose parent..."
                        onclick={() => handleChooseParent(warning)}
                        disabled={isLoadingParents}
                      >
                        <FolderPlus class="size-3" />
                      </Button>
                    {/if}
                  </div>
                </div>
              {/each}
            </div>
          {/if}

          <!-- Fix All Button -->
          {#if fixableCount() > 0 && api}
            <Button
              variant="outline"
              size="sm"
              class="w-full gap-1.5 mt-2"
              onclick={handleFixAll}
              disabled={isFixingAll}
            >
              {#if isFixingAll}
                <Loader2 class="size-3 animate-spin" />
                Fixing...
              {:else}
                <Wrench class="size-3" />
                Fix All ({fixableCount()})
              {/if}
            </Button>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</aside>

<!-- Parent Picker Dialog -->
{#if showParentPicker}
  <div
    class="fixed inset-0 bg-black/50 z-50 flex items-center justify-center"
    role="dialog"
    aria-modal="true"
    aria-labelledby="parent-picker-title"
    onclick={closeParentPicker}
    onkeydown={(e) => e.key === 'Escape' && closeParentPicker()}
    tabindex={-1}
  >
    <div
      class="bg-background rounded-lg shadow-xl max-w-md w-full mx-4 max-h-[80vh] flex flex-col"
      onclick={(e) => e.stopPropagation()}
    >
      <div class="flex items-center justify-between p-4 border-b">
        <h2 id="parent-picker-title" class="text-lg font-semibold">Choose Parent Index</h2>
        <Button variant="ghost" size="sm" onclick={closeParentPicker}>
          <X class="size-4" />
        </Button>
      </div>
      <div class="p-4 overflow-y-auto flex-1">
        {#if availableParents.length === 0}
          <p class="text-muted-foreground text-sm">No available parent indexes found.</p>
        {:else}
          <p class="text-sm text-muted-foreground mb-3">
            Select which index to add this entry to:
          </p>
          <div class="space-y-2">
            {#each availableParents as parentPath}
              <Button
                variant="outline"
                class="w-full justify-start text-left h-auto py-2"
                onclick={() => handleSelectParent(parentPath)}
              >
                <div class="flex flex-col items-start gap-0.5 overflow-hidden">
                  <span class="font-medium truncate w-full">{getFileName(parentPath)}</span>
                  <span class="text-xs text-muted-foreground truncate w-full">{parentPath}</span>
                </div>
              </Button>
            {/each}
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}

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
                handleToggleNode(node.path, node);
              }}
              aria-label="Toggle folder"
              tabindex={-1}
              disabled={isNodeLoading(node.path)}
            >
              {#if isNodeLoading(node.path)}
                <Loader2 class="size-4 text-muted-foreground animate-spin" />
              {:else}
                <ChevronRight
                  class="size-4 text-muted-foreground transition-transform duration-200 {expandedNodes.has(
                    node.path,
                  )
                    ? 'rotate-90'
                    : ''}"
                />
              {/if}
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
                            <p class="font-mono text-xs mt-1 truncate" title={getErrorTarget(error) ?? ''}>
                              Target: {getErrorTarget(error)}
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
            {#if hasInheritedWarnings(node.path)}
              {@const inherited = getInheritedWarnings(node.path)}
              <Popover.Root>
                <Popover.Trigger
                  onclick={(e: MouseEvent) => e.stopPropagation()}
                  class="shrink-0 focus:outline-none"
                >
                  <span class="relative inline-flex items-center">
                    <AlertTriangle class="size-4 text-amber-500/70 hover:text-amber-500 transition-colors" />
                    <span class="absolute -top-1.5 -right-1.5 min-w-[14px] h-[14px] text-[9px] font-bold
                      bg-amber-500 text-white rounded-full flex items-center justify-center px-0.5">
                      {inherited.length}
                    </span>
                  </span>
                </Popover.Trigger>
                <Popover.Content class="w-80 p-3" side="right" align="start">
                  <div class="space-y-3">
                    <div class="flex items-start gap-2">
                      <AlertTriangle class="size-4 text-amber-500 shrink-0 mt-0.5" />
                      <div class="space-y-2">
                        <p class="text-sm font-medium">
                          {inherited.length} Issue{inherited.length > 1 ? 's' : ''} in Subtree
                        </p>
                        <p class="text-xs text-muted-foreground">
                          Files in this folder have validation warnings.
                        </p>
                        <div class="max-h-48 overflow-y-auto space-y-1.5">
                          {#each inherited as warning}
                            {@const filePath = getWarningFilePath(warning)}
                            <div class="text-xs p-2 bg-muted rounded flex items-start justify-between gap-2">
                              <div class="min-w-0 flex-1">
                                <span class="font-mono truncate block" title={filePath ?? ''}>
                                  {filePath ? getFileName(filePath) : 'Unknown'}
                                </span>
                                <span class="text-muted-foreground">
                                  {getWarningDescription(warning)}
                                </span>
                              </div>
                              <div class="flex gap-0.5 shrink-0">
                                {#if isWarningViewable(warning)}
                                  <Button
                                    variant="ghost"
                                    size="sm"
                                    class="h-6 px-1.5"
                                    title="View file"
                                    onclick={(e: MouseEvent) => {
                                      e.stopPropagation();
                                      handleViewWarning(warning);
                                    }}
                                  >
                                    <Eye class="size-3" />
                                  </Button>
                                {/if}
                                {#if isWarningFixable(warning) && api}
                                  <Button
                                    variant="ghost"
                                    size="sm"
                                    class="h-6 px-1.5"
                                    title="Fix issue"
                                    onclick={(e: MouseEvent) => {
                                      e.stopPropagation();
                                      handleFixWarning(warning);
                                    }}
                                  >
                                    <Wrench class="size-3" />
                                  </Button>
                                {/if}
                              </div>
                            </div>
                          {/each}
                        </div>
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
            {#each node.children.filter(c => !c.name.startsWith('... (')) as child}
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
      {#if onValidate}
        <ContextMenu.Item onclick={() => onValidate(node.path)}>
          <SearchCheck class="size-4 mr-2" />
          Validate
        </ContextMenu.Item>
      {/if}
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
