/**
 * Entry Store - Manages current entry state
 * 
 * This store holds state related to the currently open entry (document),
 * including content, dirty state, and save status.
 */

import type { EntryData } from '$lib/backend';

// ============================================================================
// State
// ============================================================================

let currentEntry = $state<EntryData | null>(null);
let displayContent = $state('');
let isDirty = $state(false);
let isSaving = $state(false);
let isLoading = $state(true);
let titleError = $state<string | null>(null);

// Auto-save timer reference (not reactive, just for cleanup)
let autoSaveTimer: ReturnType<typeof setTimeout> | null = null;
const AUTO_SAVE_DELAY_MS = 2500;

// ============================================================================
// Store Factory
// ============================================================================

/**
 * Get the entry store singleton.
 * Uses module-level state so all consumers share the same store.
 */
export function getEntryStore() {
  return {
    // Getters (reactive)
    get currentEntry() { return currentEntry; },
    get displayContent() { return displayContent; },
    get isDirty() { return isDirty; },
    get isSaving() { return isSaving; },
    get isLoading() { return isLoading; },
    get titleError() { return titleError; },
    
    // Setters
    setEntry(entry: EntryData | null) {
      currentEntry = entry;
      isDirty = false;
      titleError = null;
    },
    
    setDisplayContent(content: string) {
      displayContent = content;
    },
    
    markDirty() {
      isDirty = true;
    },
    
    markClean() {
      isDirty = false;
    },
    
    setSaving(saving: boolean) {
      isSaving = saving;
    },
    
    setLoading(loading: boolean) {
      isLoading = loading;
    },
    
    setTitleError(error: string | null) {
      titleError = error;
    },
    
    // Auto-save helpers
    scheduleAutoSave(saveCallback: () => void) {
      this.cancelAutoSave();
      autoSaveTimer = setTimeout(() => {
        autoSaveTimer = null;
        if (isDirty) {
          saveCallback();
        }
      }, AUTO_SAVE_DELAY_MS);
    },
    
    cancelAutoSave() {
      if (autoSaveTimer) {
        clearTimeout(autoSaveTimer);
        autoSaveTimer = null;
      }
    },
  };
}

// ============================================================================
// Convenience export for direct state access (read-only)
// ============================================================================

export const entryStore = getEntryStore();
