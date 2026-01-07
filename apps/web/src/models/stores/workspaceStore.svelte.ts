/**
 * Workspace Store - Manages workspace tree and validation state
 * 
 * This store holds state related to the workspace hierarchy,
 * including the file tree, validation results, and expanded nodes.
 */

import type { TreeNode, ValidationResult, Backend } from '$lib/backend';

// ============================================================================
// State
// ============================================================================

// Workspace tree
let tree = $state<TreeNode | null>(null);
let expandedNodes = $state(new Set<string>());

// Validation
let validationResult = $state<ValidationResult | null>(null);

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
    
    // Tree management
    setTree(newTree: TreeNode | null) {
      tree = newTree;
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
    setValidationResult(result: ValidationResult | null) {
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
    
    // Persist display settings
    persistDisplaySettings() {
      if (typeof window !== 'undefined') {
        localStorage.setItem('diaryx-show-unlinked-files', String(showUnlinkedFiles));
        localStorage.setItem('diaryx-show-hidden-files', String(showHiddenFiles));
      }
    },
  };
}

// ============================================================================
// Convenience export
// ============================================================================

export const workspaceStore = getWorkspaceStore();
