/**
 * Entry Controller
 *
 * Handles entry-level operations including:
 * - Opening entries
 * - Saving entries
 * - Creating entries
 * - Deleting entries
 * - Renaming entries
 * - Property changes
 */

import { tick } from 'svelte';
import type { EntryData, TreeNode, Api, CreateChildResult } from '../lib/backend';
import type { JsonValue } from '../lib/backend/generated/serde_json/JsonValue';
import { entryStore, uiStore, collaborationStore } from '../models/stores';
import {
  revokeBlobUrls,
  transformAttachmentPaths,
  reverseBlobUrlsToAttachmentPaths,
} from '../models/services';
import {
  ensureBodySync,
  closeBodySync,
  getBodyContentFromCrdt,
} from '../lib/crdt/workspaceCrdtBridge';
import { shareSessionStore } from '../models/stores/shareSessionStore.svelte';
// Note: CRDT sync for entry operations (save, create, delete, rename) is now handled by Rust.
// TypeScript only manages body sync bridges for real-time collaboration.

/**
 * Helper to handle mixed frontmatter types (Map from WASM vs Object from JSON/Tauri).
 */
function normalizeFrontmatter(frontmatter: any): Record<string, any> {
  if (!frontmatter) return {};
  if (frontmatter instanceof Map) {
    return Object.fromEntries(frontmatter.entries());
  }
  return frontmatter;
}

/**
 * Convert title to kebab-case filename.
 */
function slugifyTitle(title: string): string {
  return (
    title
      .toLowerCase()
      .replace(/[^a-z0-9\s-]/g, '')
      .replace(/\s+/g, '-')
      .replace(/-+/g, '-')
      .replace(/^-|-$/g, '') + '.md'
  );
}

/**
 * Open an entry for editing.
 */
export async function openEntry(
  api: Api,
  path: string,
  tree: TreeNode | null,
  collaborationEnabled: boolean,
  options?: {
    onBeforeOpen?: () => Promise<void>;
  }
): Promise<void> {
  // Call before open callback (e.g., save current entry)
  if (options?.onBeforeOpen) {
    await options.onBeforeOpen();
  }

  try {
    entryStore.setLoading(true);

    // Cleanup previous blob URLs
    revokeBlobUrls();

    const entry = await api.getEntry(path);
    // Normalize frontmatter to Object
    entry.frontmatter = normalizeFrontmatter(entry.frontmatter);
    entryStore.setCurrentEntry(entry);
    entryStore.setTitleError(null); // Clear any title error when switching files

    console.log('[EntryController] Loaded entry:', entry);

    // Eagerly create body sync bridge to receive remote body updates.
    // This is critical for new clients syncing from the server - without this,
    // files would appear empty because the body bridge wasn't created yet.
    await ensureBodySync(path);

    // For guests, body content is in CRDT (not on disk) - read from CRDT after sync
    if (shareSessionStore.isGuest) {
      const crdtContent = await getBodyContentFromCrdt(path);
      if (crdtContent !== null) {
        console.log('[EntryController] Guest: Using body content from CRDT, length:', crdtContent.length);
        entry.content = crdtContent;
      } else {
        console.log('[EntryController] Guest: No body content in CRDT for:', path);
      }
    }

    // Transform attachment paths to blob URLs for display
    if (entry) {
      const displayContent = await transformAttachmentPaths(
        entry.content,
        entry.path,
        api
      );
      entryStore.setDisplayContent(displayContent);

      // Calculate collaboration room path for tracking
      let workspaceDir = tree?.path || '';
      if (workspaceDir.endsWith('/')) {
        workspaceDir = workspaceDir.slice(0, -1);
      }
      if (
        workspaceDir.endsWith('README.md') ||
        workspaceDir.endsWith('index.md')
      ) {
        workspaceDir = workspaceDir.substring(0, workspaceDir.lastIndexOf('/'));
      }
      let newRelativePath = entry.path;
      if (workspaceDir && entry.path.startsWith(workspaceDir)) {
        newRelativePath = entry.path.substring(workspaceDir.length + 1);
      }

      // Update collaboration path tracking (sync happens at workspace level via workspaceCrdtBridge)
      const currentCollaborationPath = collaborationStore.currentCollaborationPath;
      if (currentCollaborationPath !== newRelativePath) {
        collaborationStore.clearCollaborationSession();
        await tick();
      }

      // Set the collaboration path for tracking which file is being edited
      // Note: Actual sync is handled by workspaceCrdtBridge, not per-document sessions
      if (collaborationEnabled) {
        collaborationStore.setCollaborationPath(newRelativePath);
        console.log('[EntryController] Collaboration path:', newRelativePath);
      }
    } else {
      entryStore.setDisplayContent('');
      collaborationStore.clearCollaborationSession();
    }

    entryStore.markClean();
    uiStore.clearError();
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
  } finally {
    entryStore.setLoading(false);
  }
}

/**
 * Save the current entry.
 */
export async function saveEntry(
  api: Api,
  currentEntry: EntryData | null,
  editorRef: any
): Promise<void> {
  if (!currentEntry || !editorRef) return;
  if (entryStore.isSaving) return; // Prevent concurrent saves

  try {
    entryStore.setSaving(true);
    const markdownWithBlobUrls = editorRef.getMarkdown();
    // Reverse-transform blob URLs back to attachment paths
    const markdown = reverseBlobUrlsToAttachmentPaths(markdownWithBlobUrls || '');

    // Note: saveEntry expects only the body content, not frontmatter.
    // Frontmatter is preserved by the backend's save_content() method.
    await api.saveEntry(currentEntry.path, markdown);
    entryStore.markClean();
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
  } finally {
    entryStore.setSaving(false);
  }
}

/**
 * Create a child entry under a parent.
 * Note: CRDT sync is handled by Rust CreateEntry command.
 */
export async function createChildEntry(
  api: Api,
  parentPath: string,
  onSuccess?: () => Promise<void>
): Promise<CreateChildResult | null> {
  try {
    const result = await api.createChildEntry(parentPath);

    if (onSuccess) {
      await onSuccess();
    }

    return result;
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return null;
  }
}

/**
 * Create a new entry at a specific path.
 * Note: CRDT sync is handled by Rust CreateEntry command.
 */
export async function createEntry(
  api: Api,
  path: string,
  options: { title: string },
  onSuccess?: () => Promise<void>
): Promise<string | null> {
  try {
    const newPath = await api.createEntry(path, options);

    if (onSuccess) {
      await onSuccess();
    }

    return newPath;
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return null;
  } finally {
    uiStore.closeNewEntryModal();
  }
}

/**
 * Create or open today's daily entry.
 */
export async function ensureDailyEntry(
  api: Api,
  workspacePath: string,
  dailyEntryFolder?: string,
  onSuccess?: (path: string) => Promise<void>
): Promise<string | null> {
  try {
    const path = await api.ensureDailyEntry(workspacePath, dailyEntryFolder);
    if (onSuccess) {
      await onSuccess(path);
    }
    return path;
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return null;
  }
}

/**
 * Delete an entry.
 */
export async function deleteEntry(
  api: Api,
  path: string,
  currentEntryPath: string | null,
  onSuccess?: () => Promise<void>
): Promise<boolean> {
  const confirm = window.confirm(
    `Are you sure you want to delete "${path.split('/').pop()?.replace('.md', '')}"?`
  );
  if (!confirm) return false;

  try {
    await api.deleteEntry(path);

    // If we deleted the currently open entry, clear it
    if (currentEntryPath === path) {
      entryStore.setCurrentEntry(null);
      entryStore.markClean();
    }

    if (onSuccess) {
      // Try to refresh - might fail if workspace state is temporarily inconsistent
      try {
        await onSuccess();
      } catch (refreshError) {
        console.warn('[EntryController] Error refreshing after delete:', refreshError);
        // Try again after a short delay
        setTimeout(async () => {
          try {
            if (onSuccess) await onSuccess();
          } catch (e) {
            console.error('[EntryController] Retry refresh failed:', e);
          }
        }, 500);
      }
    }

    return true;
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return false;
  }
}

/**
 * Move an entry to a new parent (attach entry to parent).
 */
export async function moveEntry(
  api: Api,
  entryPath: string,
  newParentPath: string,
  onSuccess?: () => Promise<void>
): Promise<boolean> {
  if (entryPath === newParentPath) return false; // Can't attach to self

  console.log(
    `[EntryController] entryPath="${entryPath}" -> newParentPath="${newParentPath}"`
  );

  try {
    await api.attachEntryToParent(entryPath, newParentPath);

    if (onSuccess) {
      await onSuccess();
    }

    return true;
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return false;
  }
}

/**
 * Handle property change on the current entry.
 */
export async function handlePropertyChange(
  api: Api,
  currentEntry: EntryData,
  key: string,
  value: unknown,
  expandedNodes: Set<string>,
  onRefreshTree?: () => Promise<void>
): Promise<{ success: boolean; newPath?: string }> {
  try {
    // Special handling for title: need to check rename first
    if (key === 'title' && typeof value === 'string' && value.trim()) {
      const newFilename = slugifyTitle(value);
      const currentFilename = currentEntry.path.split('/').pop() || '';

      // For index files (have contents property), compare the directory name
      const isIndex = Array.isArray(currentEntry.frontmatter?.contents);
      const pathParts = currentEntry.path.split('/');
      const currentDir = isIndex
        ? pathParts.slice(-2, -1)[0] || ''
        : currentFilename.replace(/\.md$/, '');
      const newDir = newFilename.replace(/\.md$/, '');

      if (currentDir !== newDir) {
        // Try rename FIRST, before updating frontmatter
        try {
          const oldPath = currentEntry.path;
          const newPath = await api.renameEntry(oldPath, newFilename);
          // Rename succeeded, now update title in frontmatter (at new path)
          await api.setFrontmatterProperty(newPath, key, value);

          // Transfer expanded state from old path to new path
          if (expandedNodes.has(oldPath)) {
            expandedNodes.delete(oldPath);
            expandedNodes.add(newPath);
          }

          // Update current entry
          const newFrontmatter = { ...currentEntry.frontmatter, [key]: value };
          entryStore.setCurrentEntry({
            ...currentEntry,
            path: newPath,
            frontmatter: newFrontmatter,
          });

          // Note: CRDT sync is handled by Rust RenameEntry command

          if (onRefreshTree) {
            await onRefreshTree();
          }

          entryStore.setTitleError(null);
          return { success: true, newPath };
        } catch (renameError) {
          // Rename failed (e.g., target exists), show user-friendly error
          const errorMsg =
            renameError instanceof Error
              ? renameError.message
              : String(renameError);
          if (
            errorMsg.includes('already exists') ||
            errorMsg.includes('Destination')
          ) {
            entryStore.setTitleError(
              `A file named "${newFilename.replace('.md', '')}" already exists. Choose a different title.`
            );
          } else {
            entryStore.setTitleError(`Could not rename: ${errorMsg}`);
          }
          return { success: false };
        }
      } else {
        // No rename needed, just update title
        const newFrontmatter = { ...currentEntry.frontmatter, [key]: value };
        await api.setFrontmatterProperty(currentEntry.path, key, value);
        entryStore.setCurrentEntry({
          ...currentEntry,
          frontmatter: newFrontmatter,
        });

        // Note: CRDT sync for frontmatter is handled by Rust SetFrontmatterProperty
        entryStore.setTitleError(null);

        if (onRefreshTree) {
          await onRefreshTree();
        }

        return { success: true };
      }
    } else {
      // Non-title properties: update normally
      const newFrontmatter = { ...currentEntry.frontmatter, [key]: value };
      await api.setFrontmatterProperty(currentEntry.path, key, value as JsonValue);
      entryStore.setCurrentEntry({
        ...currentEntry,
        frontmatter: newFrontmatter,
      });

      // Note: CRDT sync for frontmatter is handled by Rust SetFrontmatterProperty

      // Refresh tree if contents or part_of changed (affects hierarchy)
      if ((key === 'contents' || key === 'part_of') && onRefreshTree) {
        await onRefreshTree();
      }

      return { success: true };
    }
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return { success: false };
  }
}

/**
 * Remove a property from the current entry.
 * Note: CRDT sync is handled by Rust RemoveFrontmatterProperty command.
 */
export async function removeProperty(
  api: Api,
  currentEntry: EntryData,
  key: string
): Promise<boolean> {
  try {
    await api.removeFrontmatterProperty(currentEntry.path, key);

    // Update local state
    const newFrontmatter = { ...currentEntry.frontmatter };
    delete newFrontmatter[key];
    entryStore.setCurrentEntry({ ...currentEntry, frontmatter: newFrontmatter });

    return true;
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return false;
  }
}

/**
 * Add a property to the current entry.
 * Note: CRDT sync is handled by Rust SetFrontmatterProperty command.
 */
export async function addProperty(
  api: Api,
  currentEntry: EntryData,
  key: string,
  value: unknown
): Promise<boolean> {
  try {
    await api.setFrontmatterProperty(currentEntry.path, key, value as JsonValue);

    // Update local state
    entryStore.setCurrentEntry({
      ...currentEntry,
      frontmatter: { ...currentEntry.frontmatter, [key]: value },
    });

    return true;
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return false;
  }
}

/**
 * Rename an entry.
 * Note: CRDT sync is handled by Rust RenameEntry command.
 *
 * @param api - API instance
 * @param path - Current path of the entry
 * @param newFilename - New filename (with .md extension)
 * @param onSuccess - Callback after successful rename (e.g., refresh tree)
 * @returns The new path of the renamed entry
 */
export async function renameEntry(
  api: Api,
  path: string,
  newFilename: string,
  onSuccess?: () => Promise<void>
): Promise<string> {
  // Close old body sync bridge before rename
  closeBodySync(path);

  const newPath = await api.renameEntry(path, newFilename);

  // Create new body sync bridge for the new path
  await ensureBodySync(newPath);

  if (onSuccess) {
    await onSuccess();
  }

  return newPath;
}

/**
 * Duplicate an entry.
 * Note: CRDT sync for the new file should be added via Rust DuplicateEntry command.
 *
 * @param api - API instance
 * @param path - Path of the entry to duplicate
 * @param onSuccess - Callback after successful duplication (e.g., refresh tree)
 * @returns The path of the new duplicated entry
 */
export async function duplicateEntry(
  api: Api,
  path: string,
  onSuccess?: () => Promise<void>
): Promise<string> {
  const newPath = await api.duplicateEntry(path);

  // Ensure body sync bridge is created for the new file
  await ensureBodySync(newPath);

  if (onSuccess) {
    await onSuccess();
  }

  return newPath;
}

/**
 * Delete an entry with CRDT sync support.
 * Note: CRDT sync (soft delete) is now handled by Rust DeleteEntry command.
 *
 * @param api - API instance
 * @param path - Path of the entry to delete
 * @param currentEntryPath - Path of the currently open entry (to clear if same)
 * @param onSuccess - Callback after successful deletion (e.g., refresh tree)
 * @returns True if deletion was confirmed and completed
 */
export async function deleteEntryWithSync(
  api: Api,
  path: string,
  currentEntryPath: string | null,
  onSuccess?: () => Promise<void>
): Promise<boolean> {
  const confirm = window.confirm(
    `Are you sure you want to delete "${path.split('/').pop()?.replace('.md', '')}"?`
  );
  if (!confirm) return false;

  try {
    // Close body sync bridge for the deleted file
    closeBodySync(path);

    // Delete via Rust - handles CRDT soft delete automatically
    await api.deleteEntry(path);

    // If we deleted the currently open entry, clear it
    if (currentEntryPath === path) {
      entryStore.setCurrentEntry(null);
      entryStore.markClean();
    }

    if (onSuccess) {
      // Try to refresh - might fail if workspace state is temporarily inconsistent
      try {
        await onSuccess();
      } catch (refreshError) {
        console.warn('[EntryController] Error refreshing after delete:', refreshError);
        // Try again after a short delay
        setTimeout(async () => {
          try {
            if (onSuccess) await onSuccess();
          } catch (e) {
            console.error('[EntryController] Retry refresh failed:', e);
          }
        }, 500);
      }
    }

    return true;
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return false;
  }
}

/**
 * Create a child entry with CRDT sync support.
 * Note: CRDT sync is now handled by Rust CreateEntry command.
 *
 * @param api - API instance
 * @param parentPath - Path of the parent entry
 * @param onSuccess - Callback after successful creation. Receives the full result
 *                    including both child path and (possibly new) parent path.
 * @returns The CreateChildResult with paths and conversion info, or null on failure
 */
export async function createChildEntryWithSync(
  api: Api,
  parentPath: string,
  onSuccess?: (result: CreateChildResult) => Promise<void>
): Promise<CreateChildResult | null> {
  try {
    // Create via Rust - handles CRDT sync automatically
    // Returns detailed result with child path, parent path, and conversion info
    const result = await api.createChildEntry(parentPath);

    // Ensure body sync bridge is created for the new file
    await ensureBodySync(result.child_path);

    if (onSuccess) {
      await onSuccess(result);
    }

    return result;
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return null;
  }
}

/**
 * Create a new entry with CRDT sync support.
 * Note: CRDT sync is now handled by Rust CreateEntry command.
 *
 * @param api - API instance
 * @param path - Path for the new entry
 * @param options - Options including title and template
 * @param onSuccess - Callback after successful creation
 * @returns The path of the new entry, or null on failure
 */
export async function createEntryWithSync(
  api: Api,
  path: string,
  options: { title: string; template?: string },
  onSuccess?: () => Promise<void>
): Promise<string | null> {
  try {
    // Create via Rust - handles CRDT sync automatically
    const newPath = await api.createEntry(path, options);

    // Ensure body sync bridge is created for the new file
    await ensureBodySync(newPath);

    if (onSuccess) {
      await onSuccess();
    }

    return newPath;
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return null;
  } finally {
    uiStore.closeNewEntryModal();
  }
}

/**
 * Save an entry with CRDT sync support.
 * Note: CRDT sync is now handled by Rust SaveEntry command.
 *
 * @param api - API instance
 * @param currentEntry - The current entry being saved
 * @param editorRef - Reference to the editor component
 */
export async function saveEntryWithSync(
  api: Api,
  currentEntry: EntryData | null,
  editorRef: any
): Promise<void> {
  if (!currentEntry || !editorRef) return;
  if (entryStore.isSaving) return; // Prevent concurrent saves

  try {
    entryStore.setSaving(true);
    const markdownWithBlobUrls = editorRef.getMarkdown();
    // Reverse-transform blob URLs back to attachment paths
    const markdown = reverseBlobUrlsToAttachmentPaths(markdownWithBlobUrls || '');

    // Save to backend - Rust handles CRDT sync automatically
    await api.saveEntry(currentEntry.path, markdown);
    entryStore.markClean();
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
  } finally {
    entryStore.setSaving(false);
  }
}
