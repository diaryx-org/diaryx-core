/**
 * Workspace Controller
 *
 * Handles workspace-level operations including:
 * - Tree loading and refresh
 * - Lazy loading of children
 * - Validation
 * - Workspace CRDT initialization
 */

import type { TreeNode, Api, ValidationResultWithMeta } from '../lib/backend';
import type { Backend } from '../lib/backend/interface';
import type { RustCrdtApi } from '../lib/crdt/rustCrdtApi';
import { workspaceStore } from '../models/stores';
import { initializeWorkspaceCrdt } from '../models/services';
import { setWorkspaceId } from '../lib/crdt/workspaceCrdtBridge';
import { toast } from 'svelte-sonner';

// Depth limit for initial tree loading (lazy loading)
const TREE_INITIAL_DEPTH = 2;

/**
 * Refresh the workspace tree.
 * Uses either filesystem tree or hierarchy tree based on showUnlinkedFiles setting.
 */
export async function refreshTree(
  api: Api,
  backend: Backend,
  showUnlinkedFiles: boolean,
  showHiddenFiles: boolean
): Promise<void> {
  try {
    // Get the workspace directory from the backend
    const workspaceDir = backend
      .getWorkspacePath()
      .replace(/\/index\.md$/, '')
      .replace(/\/README\.md$/, '');

    if (showUnlinkedFiles) {
      // "Show All Files" mode - use filesystem tree with depth limit
      workspaceStore.setTree(
        await api.getFilesystemTree(workspaceDir, showHiddenFiles, TREE_INITIAL_DEPTH)
      );
    } else {
      // Normal mode - find the actual root index and use hierarchy tree with depth limit
      try {
        const rootIndexPath = await api.findRootIndex(workspaceDir);
        workspaceStore.setTree(
          await api.getWorkspaceTree(rootIndexPath, TREE_INITIAL_DEPTH)
        );
      } catch (e) {
        console.warn('[WorkspaceController] Could not find root index for tree:', e);
        // Fall back to filesystem tree if no root index found
        workspaceStore.setTree(
          await api.getFilesystemTree(workspaceDir, showHiddenFiles, TREE_INITIAL_DEPTH)
        );
      }
    }
  } catch (e) {
    console.error('[WorkspaceController] Error refreshing tree:', e);
  }
}

/**
 * Load children for a node (lazy loading when user expands).
 */
export async function loadNodeChildren(
  api: Api,
  nodePath: string,
  showUnlinkedFiles: boolean,
  showHiddenFiles: boolean
): Promise<void> {
  console.log('[WorkspaceController] loadNodeChildren called for:', nodePath);
  try {
    let subtree: TreeNode;

    if (showUnlinkedFiles) {
      // Filesystem tree mode - need directory path
      // If nodePath ends with .md, it's an index file - use parent directory
      const dirPath = nodePath.endsWith('.md')
        ? nodePath.substring(0, nodePath.lastIndexOf('/'))
        : nodePath;
      subtree = await api.getFilesystemTree(dirPath, showHiddenFiles, TREE_INITIAL_DEPTH);
    } else {
      // Workspace tree mode - use index file path directly
      subtree = await api.getWorkspaceTree(nodePath, TREE_INITIAL_DEPTH);
    }

    console.log('[WorkspaceController] loadNodeChildren subtree result:', {
      path: subtree.path,
      name: subtree.name,
      childrenCount: subtree.children.length,
      childPaths: subtree.children.map(c => c.path),
    });

    // Merge into existing tree
    workspaceStore.updateSubtree(nodePath, subtree);
  } catch (e) {
    console.error('[WorkspaceController] Error loading children for', nodePath, e);
  }
}

/**
 * Run workspace validation.
 */
export async function runValidation(
  api: Api,
  backend: Backend,
  tree: TreeNode | null
): Promise<void> {
  try {
    // Pass the actual workspace root path for validation
    // tree?.path is the root index file path (e.g., "/Users/.../workspace/index.md")
    // This is required for Tauri which uses absolute filesystem paths
    // Fall back to backend.getWorkspacePath() if tree is not yet loaded
    const rootPath = tree?.path ?? backend.getWorkspacePath();
    const result = await api.validateWorkspace(rootPath);
    workspaceStore.setValidationResult(result);
    console.log('[WorkspaceController] Validation result:', result);
  } catch (e) {
    console.error('[WorkspaceController] Validation error:', e);
  }
}

/**
 * Validate a specific path (file or subtree).
 */
export async function validatePath(
  api: Api,
  path: string
): Promise<void> {
  try {
    // Determine if this is an index file (validate subtree) or regular file
    const isIndex =
      path.endsWith('/index.md') ||
      path.endsWith('\\index.md') ||
      path.match(/[/\\]index\.[^/\\]+$/);

    let result: ValidationResultWithMeta;
    if (isIndex) {
      // Validate from this index down
      result = await api.validateWorkspace(path);
    } else {
      // Validate just this file
      result = await api.validateFile(path);
    }

    // Update the validation result
    workspaceStore.setValidationResult(result);

    // Show a summary toast
    const errorCount = result.errors.length;
    const warningCount = result.warnings.length;
    if (errorCount === 0 && warningCount === 0) {
      toast.success('No issues found');
    } else {
      toast.info(
        `Found ${errorCount} error${errorCount !== 1 ? 's' : ''} and ${warningCount} warning${warningCount !== 1 ? 's' : ''}`
      );
    }
  } catch (e) {
    toast.error(e instanceof Error ? e.message : 'Validation failed');
    console.error('[WorkspaceController] Validation error:', e);
  }
}

/**
 * Setup workspace CRDT for collaboration.
 *
 * Gets workspace ID from the auth store (server is source of truth).
 * When authenticated, the server generates and stores the workspace UUID.
 * For local-only mode (not signed in), we use null.
 */
export async function setupWorkspaceCrdt(
  api: Api,
  backend: Backend,
  rustApi: RustCrdtApi,
  collaborationServerUrl: string | null,
  collaborationEnabled: boolean,
  serverWorkspaceId: string | null,
  onConnectionChange: (connected: boolean) => void
): Promise<{ workspaceId: string | null; initialized: boolean }> {
  try {
    const sharedWorkspaceId = serverWorkspaceId;

    if (sharedWorkspaceId) {
      console.log('[WorkspaceController] Using workspace_id from server:', sharedWorkspaceId);
    } else {
      console.log('[WorkspaceController] No authenticated workspace, using local-only mode');
    }

    // Get the workspace directory from the backend, then find the actual root index
    const workspaceDir = backend
      .getWorkspacePath()
      .replace(/\/index\.md$/, '')
      .replace(/\/README\.md$/, '');
    console.log('[WorkspaceController] Workspace directory:', workspaceDir);

    let workspacePath: string;
    try {
      workspacePath = await api.findRootIndex(workspaceDir);
      console.log('[WorkspaceController] Found root index at:', workspacePath);
    } catch (e) {
      console.warn('[WorkspaceController] Could not find root index:', e);
      workspacePath = `${workspaceDir}/index.md`;
    }

    // Ensure local workspace exists (creates index.md if needed)
    try {
      await api.getWorkspaceTree(workspacePath);
    } catch (e) {
      const errStr = e instanceof Error ? e.message : String(e);
      if (
        errStr.includes('No workspace found') ||
        errStr.includes('NotFoundError') ||
        errStr.includes('The object can not be found here')
      ) {
        console.log('[WorkspaceController] Default workspace missing, creating...');
        try {
          await api.createWorkspace('workspace', 'My Journal');
        } catch (createErr) {
          console.error('[WorkspaceController] Failed to create default workspace:', createErr);
        }
      }
    }

    // Set workspace ID for per-file document room naming
    // If null, rooms will be "doc:{path}" instead of "{id}:doc:{path}"
    setWorkspaceId(sharedWorkspaceId);
    workspaceStore.setWorkspaceId(sharedWorkspaceId);

    // Initialize workspace CRDT using service with Rust API
    const initialized = await initializeWorkspaceCrdt(
      sharedWorkspaceId,
      workspacePath,
      collaborationServerUrl,
      collaborationEnabled,
      rustApi,
      {
        onConnectionChange: (connected: boolean) => {
          console.log(
            '[WorkspaceController] Workspace CRDT connection:',
            connected ? 'online' : 'offline'
          );
          onConnectionChange(connected);
        },
      }
    );

    workspaceStore.setWorkspaceCrdtInitialized(initialized);
    return { workspaceId: sharedWorkspaceId, initialized };
  } catch (e) {
    console.error('[WorkspaceController] Failed to initialize workspace CRDT:', e);
    workspaceStore.setWorkspaceCrdtInitialized(false);
    return { workspaceId: null, initialized: false };
  }
}
