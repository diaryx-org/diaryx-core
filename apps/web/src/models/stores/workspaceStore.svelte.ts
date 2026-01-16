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
      tree = newTree;
    },

    // Update a subtree (for lazy loading)
    updateSubtree(nodePath: string, subtree: TreeNode) {
      if (!tree) return;

      // Find the node in the tree and replace its children
      function findAndUpdate(node: TreeNode): boolean {
        if (node.path === nodePath) {
          node.children = subtree.children;
          return true;
        }
        for (const child of node.children) {
          if (findAndUpdate(child)) return true;
        }
        return false;
      }

      findAndUpdate(tree);
      tree = { ...tree }; // Trigger reactivity
    },
    
    // Node expansion
    toggleNode(path: string) {
      if (expandedNodes.has(path)) {
        expandedNodes.delete(path);
      } else {
        expandedNodes.add(path);
      }
      expandedNodes = new Set(expandedNodes); // Trigger reactivity
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
