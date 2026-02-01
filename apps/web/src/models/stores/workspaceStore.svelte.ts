/**
 * Workspace Store - Manages workspace tree and validation state
 *
 * This store holds state related to the workspace hierarchy,
 * including the file tree, validation results, and expanded nodes.
 */

import type { TreeNode, ValidationResultWithMeta, Backend } from '$lib/backend';

// ============================================================================
// State
// ============================================================================

// Workspace tree
let tree = $state<TreeNode | null>(null);
let expandedNodes = $state(new Set<string>());

// Saved tree state for session restoration (when guest leaves a session)
let savedTree = $state<TreeNode | null>(null);
let savedExpandedNodes = $state<Set<string> | null>(null);

// Validation
let validationResult = $state<ValidationResultWithMeta | null>(null);

// Workspace CRDT state
let workspaceCrdtInitialized = $state(false);
let workspaceId = $state<string | null>(null);

// Backend reference
let backend = $state<Backend | null>(null);

// Display settings
let showUnlinkedFiles = $state(
  typeof window !== 'undefined'
    ? localStorage.getItem('diaryx-show-unlinked-files') === 'true'
    : false
);
let showHiddenFiles = $state(
  typeof window !== 'undefined'
    ? localStorage.getItem('diaryx-show-hidden-files') === 'true'
    : false
);

// Editor header display settings (default to false for minimal header)
let showEditorTitle = $state(
  typeof window !== 'undefined'
    ? localStorage.getItem('diaryx-show-editor-title') === 'true'
    : false
);
let showEditorPath = $state(
  typeof window !== 'undefined'
    ? localStorage.getItem('diaryx-show-editor-path') === 'true'
    : false
);

// Readable line length setting (default to true for better readability)
let readableLineLength = $state(
  typeof window !== 'undefined'
    ? localStorage.getItem('diaryx-readable-line-length') !== 'false'
    : true
);

// Focus mode setting (default to true on desktop, false on mobile)
// When enabled and both sidebars are closed, the editor header fades out
let focusMode = $state(
  typeof window !== 'undefined'
    ? localStorage.getItem('diaryx-focus-mode') !== null
      ? localStorage.getItem('diaryx-focus-mode') === 'true'
      : window.innerWidth >= 768 // Default: true on desktop, false on mobile
    : false
);

// ============================================================================
// Tree Merge Utilities
// ============================================================================

/**
 * Check if a node is a lazy-loading placeholder (e.g., "... (5 more)")
 */
function isPlaceholderNode(node: TreeNode): boolean {
  return node.name.startsWith('... (');
}

/**
 * Check if children array contains only placeholder nodes
 */
function hasOnlyPlaceholders(children: TreeNode[]): boolean {
  return children.length > 0 && children.every(isPlaceholderNode);
}

/**
 * Merge a new tree into an existing tree, preserving unchanged nodes.
 * This prevents unnecessary re-renders by keeping the same object references
 * for nodes that haven't changed.
 *
 * Important: If the old tree had children loaded (beyond depth limit) and the
 * new tree only has placeholders, we preserve the old children. This ensures
 * expanded folders stay expanded after tree refresh.
 */
function mergeTree(oldNode: TreeNode | null, newNode: TreeNode | null): TreeNode | null {
  // If either is null, just return the new one
  if (!oldNode || !newNode) {
    return newNode;
  }

  // If paths don't match, this is a different node - use the new one
  if (oldNode.path !== newNode.path) {
    return newNode;
  }

  // Check if the node itself has changed (name change)
  const nodeChanged = oldNode.name !== newNode.name;

  // IMPORTANT: If old node had real children loaded but new node only has placeholders,
  // preserve the old children. This happens when tree is refreshed with depth limit
  // but user had expanded folders beyond that limit.
  const oldHasRealChildren = oldNode.children.length > 0 && !hasOnlyPlaceholders(oldNode.children);
  const newHasOnlyPlaceholders = hasOnlyPlaceholders(newNode.children);

  if (oldHasRealChildren && newHasOnlyPlaceholders) {
    // Keep old children - they were loaded via lazy loading and shouldn't be lost
    console.log('[mergeTree] Preserving loaded children for:', oldNode.path);
    if (!nodeChanged) {
      return oldNode; // Nothing changed, keep old node entirely
    }
    // Name changed but keep old children
    return {
      ...newNode,
      children: oldNode.children,
    };
  }

  // Build a map of old children by path for quick lookup
  const oldChildrenMap = new Map<string, TreeNode>();
  for (const child of oldNode.children) {
    if (!isPlaceholderNode(child)) {
      oldChildrenMap.set(child.path, child);
    }
  }

  // Check if children have changed
  let childrenChanged = oldNode.children.length !== newNode.children.length;

  // Merge children, preserving unchanged ones
  const mergedChildren: TreeNode[] = [];
  for (const newChild of newNode.children) {
    // Skip placeholder nodes in comparison
    if (isPlaceholderNode(newChild)) {
      // If new tree has a placeholder but old tree had real children at this level,
      // we need to check if any old children should be preserved
      continue;
    }

    const oldChild = oldChildrenMap.get(newChild.path);
    if (oldChild) {
      // Recursively merge the child
      const mergedChild = mergeTree(oldChild, newChild);
      if (mergedChild !== oldChild) {
        childrenChanged = true;
      }
      mergedChildren.push(mergedChild!);
    } else {
      // New child - this is a change
      childrenChanged = true;
      mergedChildren.push(newChild);
    }
  }

  // Check if new tree has any placeholders (meaning depth was limited)
  const newTreeHasPlaceholders = newNode.children.some(isPlaceholderNode);

  // Add back any old children that weren't in the new tree but should be preserved
  // ONLY preserve if:
  // 1. The new tree has placeholders (depth-limited fetch) - AND
  // 2. No sibling in the same directory was fetched in the new tree
  //    (if a sibling was fetched, the old child was likely moved/renamed, not hidden)
  for (const oldChild of oldNode.children) {
    if (isPlaceholderNode(oldChild)) continue;

    const inNewTree = newNode.children.some((c) => c.path === oldChild.path);
    const alreadyMerged = mergedChildren.some((c) => c.path === oldChild.path);

    if (!inNewTree && !alreadyMerged && newTreeHasPlaceholders) {
      // Check if old child's parent directory has siblings in new tree
      // If ANY sibling from the same directory was fetched in the new tree,
      // the old child was likely moved/renamed - don't preserve it
      const oldChildDir = oldChild.path.substring(0, oldChild.path.lastIndexOf('/'));
      const siblingInNewTree = newNode.children.some((c) => {
        if (isPlaceholderNode(c)) return false;
        const cDir = c.path.substring(0, c.path.lastIndexOf('/'));
        return cDir === oldChildDir && c.path !== oldChild.path;
      });

      if (!siblingInNewTree) {
        // This child was loaded via lazy loading but not in new tree's depth
        // No siblings were fetched, so it might still exist behind placeholders
        console.log('[mergeTree] Preserving lazy-loaded child:', oldChild.path);
        mergedChildren.push(oldChild);
        childrenChanged = true;
      } else {
        console.log('[mergeTree] NOT preserving (sibling exists in new tree):', oldChild.path);
      }
    }
  }

  // Re-add placeholder if new tree had one and we have preserved children
  const newPlaceholder = newNode.children.find(isPlaceholderNode);
  if (newPlaceholder && mergedChildren.length > 0) {
    // Don't add placeholder if we have all the children
    // Only add if there might be more children not yet loaded
  }

  // Check for removed children (old children not in new tree and not preserved)
  if (!childrenChanged) {
    for (const oldChild of oldNode.children) {
      if (isPlaceholderNode(oldChild)) continue;
      const stillExists = mergedChildren.some(c => c.path === oldChild.path);
      if (!stillExists) {
        childrenChanged = true;
        break;
      }
    }
  }

  // If nothing changed, return the old node to preserve reference
  if (!nodeChanged && !childrenChanged) {
    return oldNode;
  }

  // Something changed - create a new node with merged children
  return {
    ...newNode,
    children: mergedChildren,
  };
}

// ============================================================================
// Tree Query Utilities
// ============================================================================

/**
 * Find the parent node path for a given file path in the tree.
 * Returns null if not found.
 */
function findParentNodePath(treeNode: TreeNode | null, targetPath: string): string | null {
  if (!treeNode) return null;

  // Check if any of this node's children match the target path
  for (const child of treeNode.children) {
    if (child.path === targetPath) {
      return treeNode.path; // This node is the parent
    }
  }

  // Recursively search in children
  for (const child of treeNode.children) {
    const result = findParentNodePath(child, targetPath);
    if (result) return result;
  }

  return null;
}

// ============================================================================
// Store Factory
// ============================================================================

/**
 * Get the workspace store singleton.
 */
export function getWorkspaceStore() {
  return {
    // Getters
    get tree() { return tree; },
    get expandedNodes() { return expandedNodes; },
    get validationResult() { return validationResult; },
    get workspaceCrdtInitialized() { return workspaceCrdtInitialized; },
    get workspaceId() { return workspaceId; },
    get backend() { return backend; },
    get showUnlinkedFiles() { return showUnlinkedFiles; },
    get showHiddenFiles() { return showHiddenFiles; },
    get showEditorTitle() { return showEditorTitle; },
    get showEditorPath() { return showEditorPath; },
    get readableLineLength() { return readableLineLength; },
    get focusMode() { return focusMode; },

    // Tree management
    setTree(newTree: TreeNode | null) {
      // Debug: log expanded nodes and tree paths
      console.log('[WorkspaceStore] setTree called');
      console.log('[WorkspaceStore] expandedNodes:', [...expandedNodes]);
      if (newTree) {
        const paths: string[] = [];
        function collectPaths(node: TreeNode) {
          paths.push(node.path);
          node.children.forEach(collectPaths);
        }
        collectPaths(newTree);
        console.log('[WorkspaceStore] tree paths:', paths);
      }

      // Merge new tree into existing tree to preserve unchanged nodes
      // This prevents unnecessary re-renders and maintains DOM stability
      tree = mergeTree(tree, newTree);
    },

    // Update a subtree (for lazy loading)
    updateSubtree(nodePath: string, subtree: TreeNode) {
      if (!tree) return;

      // Recursively find and update the node, preserving unchanged parts
      function findAndMerge(node: TreeNode): TreeNode {
        if (node.path === nodePath) {
          // Found the target node - merge the subtree children
          return mergeTree(node, subtree) ?? node;
        }

        // Check if any child paths could contain the target
        let childrenChanged = false;
        const updatedChildren: TreeNode[] = [];
        for (const child of node.children) {
          const updated = findAndMerge(child);
          if (updated !== child) {
            childrenChanged = true;
          }
          updatedChildren.push(updated);
        }

        // If no children changed, return the original node
        if (!childrenChanged) {
          return node;
        }

        // Return new node with updated children
        return { ...node, children: updatedChildren };
      }

      tree = findAndMerge(tree);
    },

    /**
     * Find the parent node path for a given file path in the current tree.
     * Returns null if not found.
     */
    getParentNodePath(targetPath: string): string | null {
      return findParentNodePath(tree, targetPath);
    },

    // Node expansion
    toggleNode(path: string) {
      console.log('[WorkspaceStore] toggleNode:', path);
      if (expandedNodes.has(path)) {
        expandedNodes.delete(path);
      } else {
        expandedNodes.add(path);
      }
      expandedNodes = new Set(expandedNodes); // Trigger reactivity
      console.log('[WorkspaceStore] expandedNodes after toggle:', [...expandedNodes]);
    },

    expandNode(path: string) {
      expandedNodes.add(path);
      expandedNodes = new Set(expandedNodes);
    },

    collapseNode(path: string) {
      expandedNodes.delete(path);
      expandedNodes = new Set(expandedNodes);
    },

    setExpandedNodes(nodes: Set<string>) {
      expandedNodes = nodes;
    },

    /**
     * Reveal a file in the tree by expanding all parent folders.
     * This makes the file visible in the sidebar.
     *
     * Note: Tree nodes use index file paths (e.g., "workspace/README.md"),
     * not directory paths. We need to find ancestor nodes in the tree.
     */
    revealPath(filePath: string) {
      console.log('[WorkspaceStore] revealPath called with:', filePath);

      if (!tree) {
        console.log('[WorkspaceStore] revealPath - no tree loaded');
        return;
      }

      // Find all ancestor nodes in the tree that contain the target file
      const ancestorPaths: string[] = [];

      function findAncestors(node: TreeNode, targetPath: string): boolean {
        // Check if any child matches the target or contains it
        for (const child of node.children) {
          if (child.path === targetPath) {
            // Found the target - this node is a direct parent
            ancestorPaths.push(node.path);
            return true;
          }
          // Check if target is nested under this child
          // Compare directory prefixes to see if target is under this subtree
          const childDir = child.path.substring(0, child.path.lastIndexOf('/'));
          const targetDir = targetPath.substring(0, targetPath.lastIndexOf('/'));

          if (targetDir.startsWith(childDir) || targetPath.startsWith(childDir + '/')) {
            // Target might be under this child's subtree
            if (findAncestors(child, targetPath)) {
              ancestorPaths.push(node.path);
              return true;
            }
          }
        }
        return false;
      }

      findAncestors(tree, filePath);

      console.log('[WorkspaceStore] revealPath - ancestor node paths to expand:', ancestorPaths);
      console.log('[WorkspaceStore] revealPath - current expandedNodes:', [...expandedNodes]);

      // Expand all ancestor nodes
      if (ancestorPaths.length > 0) {
        for (const path of ancestorPaths) {
          expandedNodes.add(path);
        }
        expandedNodes = new Set(expandedNodes); // Trigger reactivity
        console.log('[WorkspaceStore] revealPath - expandedNodes after:', [...expandedNodes]);
      }
    },

    // Validation
    setValidationResult(result: ValidationResultWithMeta | null) {
      validationResult = result;
    },

    // Workspace CRDT
    setWorkspaceCrdtInitialized(initialized: boolean) {
      workspaceCrdtInitialized = initialized;
    },

    setWorkspaceId(id: string | null) {
      workspaceId = id;
    },

    // Backend
    setBackend(newBackend: Backend | null) {
      backend = newBackend;
    },

    // Display settings
    setShowUnlinkedFiles(show: boolean) {
      showUnlinkedFiles = show;
      if (typeof window !== 'undefined') {
        localStorage.setItem('diaryx-show-unlinked-files', String(show));
      }
    },

    setShowHiddenFiles(show: boolean) {
      showHiddenFiles = show;
      if (typeof window !== 'undefined') {
        localStorage.setItem('diaryx-show-hidden-files', String(show));
      }
    },

    setShowEditorTitle(show: boolean) {
      showEditorTitle = show;
      if (typeof window !== 'undefined') {
        localStorage.setItem('diaryx-show-editor-title', String(show));
      }
    },

    setShowEditorPath(show: boolean) {
      showEditorPath = show;
      if (typeof window !== 'undefined') {
        localStorage.setItem('diaryx-show-editor-path', String(show));
      }
    },

    setReadableLineLength(enabled: boolean) {
      readableLineLength = enabled;
      if (typeof window !== 'undefined') {
        localStorage.setItem('diaryx-readable-line-length', String(enabled));
      }
    },

    setFocusMode(enabled: boolean) {
      focusMode = enabled;
      if (typeof window !== 'undefined') {
        localStorage.setItem('diaryx-focus-mode', String(enabled));
      }
    },

    // Persist display settings
    persistDisplaySettings() {
      if (typeof window !== 'undefined') {
        localStorage.setItem('diaryx-show-unlinked-files', String(showUnlinkedFiles));
        localStorage.setItem('diaryx-show-hidden-files', String(showHiddenFiles));
      }
    },

    // Session state management (for guest sessions)
    /**
     * Save the current tree state before joining a session.
     * Call this before joining a share session as a guest.
     */
    saveTreeState() {
      console.log('[WorkspaceStore] Saving tree state before session');
      savedTree = tree;
      savedExpandedNodes = new Set(expandedNodes);
    },

    /**
     * Restore the previously saved tree state.
     * Call this when leaving a share session as a guest.
     * Returns true if state was restored, false if no saved state.
     */
    restoreTreeState(): boolean {
      if (savedTree) {
        console.log('[WorkspaceStore] Restoring saved tree state');
        tree = savedTree;
        expandedNodes = savedExpandedNodes ?? new Set();
        savedTree = null;
        savedExpandedNodes = null;
        return true;
      }
      console.log('[WorkspaceStore] No saved tree state to restore');
      return false;
    },

    /**
     * Clear saved tree state without restoring.
     * Call this if the session ended abnormally.
     */
    clearSavedTreeState() {
      savedTree = null;
      savedExpandedNodes = null;
    },

    /**
     * Check if there's a saved tree state.
     */
    hasSavedTreeState(): boolean {
      return savedTree !== null;
    },
  };
}

// ============================================================================
// Convenience export
// ============================================================================

export const workspaceStore = getWorkspaceStore();
