/**
 * UI Store - Manages UI state
 * 
 * This store holds state related to UI elements like sidebars,
 * modals, loading states, and error messages.
 */

// ============================================================================
// State
// ============================================================================

// Sidebar states - collapsed by default on mobile
let leftSidebarCollapsed = $state(true);
let rightSidebarCollapsed = $state(true);

// Modal states
let showCommandPalette = $state(false);
let showSettingsDialog = $state(false);
let showExportDialog = $state(false);
let showNewEntryModal = $state(false);

// Export state
let exportPath = $state('');

// Error state
let error = $state<string | null>(null);

// Editor reference (for accessing editor methods)
let editorRef = $state<any>(null);

// ============================================================================
// Store Factory
// ============================================================================

/**
 * Get the UI store singleton.
 */
export function getUIStore() {
  return {
    // Sidebar getters
    get leftSidebarCollapsed() { return leftSidebarCollapsed; },
    get rightSidebarCollapsed() { return rightSidebarCollapsed; },
    
    // Modal getters
    get showCommandPalette() { return showCommandPalette; },
    get showSettingsDialog() { return showSettingsDialog; },
    get showExportDialog() { return showExportDialog; },
    get showNewEntryModal() { return showNewEntryModal; },
    
    // Other getters
    get exportPath() { return exportPath; },
    get error() { return error; },
    get editorRef() { return editorRef; },
    
    // Sidebar actions
    toggleLeftSidebar() {
      leftSidebarCollapsed = !leftSidebarCollapsed;
    },
    
    toggleRightSidebar() {
      rightSidebarCollapsed = !rightSidebarCollapsed;
    },
    
    setLeftSidebarCollapsed(collapsed: boolean) {
      leftSidebarCollapsed = collapsed;
    },
    
    setRightSidebarCollapsed(collapsed: boolean) {
      rightSidebarCollapsed = collapsed;
    },
    
    // Expand sidebars (for desktop)
    expandSidebarsForDesktop() {
      if (typeof window !== 'undefined' && window.innerWidth >= 768) {
        leftSidebarCollapsed = false;
        rightSidebarCollapsed = false;
      }
    },
    
    // Modal actions
    openCommandPalette() { showCommandPalette = true; },
    closeCommandPalette() { showCommandPalette = false; },
    toggleCommandPalette() { showCommandPalette = !showCommandPalette; },
    
    openSettingsDialog() { showSettingsDialog = true; },
    closeSettingsDialog() { showSettingsDialog = false; },
    setShowSettingsDialog(show: boolean) { showSettingsDialog = show; },
    
    openExportDialog(path: string = '') {
      exportPath = path;
      showExportDialog = true;
    },
    closeExportDialog() { showExportDialog = false; },
    setShowExportDialog(show: boolean) { showExportDialog = show; },
    
    openNewEntryModal() { showNewEntryModal = true; },
    closeNewEntryModal() { showNewEntryModal = false; },
    setShowNewEntryModal(show: boolean) { showNewEntryModal = show; },
    
    // Export path
    setExportPath(path: string) { exportPath = path; },
    
    // Error management
    setError(err: string | null) { error = err; },
    clearError() { error = null; },
    
    // Editor reference
    setEditorRef(ref: any) { editorRef = ref; },
  };
}

// ============================================================================
// Convenience export
// ============================================================================

export const uiStore = getUIStore();
