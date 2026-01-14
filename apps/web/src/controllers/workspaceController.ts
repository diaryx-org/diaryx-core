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
import {
  setWorkspaceId,
  setWorkspaceServer,
} from '../lib/crdt/workspaceCrdtBridge';
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
 * Generate a UUID for workspace identification.
 */
function generateUUID(): string {
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    const v = c === 'x' ? r : (r & 0x3) | 0x8;
    return v.toString(16);
  });
}

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
 * Setup workspace CRDT for collaboration.
 */
export async function setupWorkspaceCrdt(
  api: Api,
  backend: Backend,
  rustApi: RustCrdtApi,
  collaborationServerUrl: string | null,
  collaborationEnabled: boolean,
  envWorkspaceId: string | null,
  onConnectionChange: (connected: boolean) => void,
  retryCount = 0
): Promise<{ workspaceId: string | null; initialized: boolean }> {
  try {
    // Workspace ID priority:
    // 1. Environment variable VITE_WORKSPACE_ID (best for multi-device, avoids bootstrap issue)
    // 2. workspace_id from root index frontmatter (should persist in the workspace index)
    // 3. null (no prefix - uses simple room names like "doc:path/to/file.md")
    let sharedWorkspaceId: string | null = envWorkspaceId;

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
      // Fall back to default - will trigger workspace creation
      workspacePath = `${workspaceDir}/index.md`;
    }

    if (sharedWorkspaceId) {
      console.log(
        '[WorkspaceController] Using workspace_id from environment:',
        sharedWorkspaceId
      );
    } else {
      // Try to get/create workspace_id from root index frontmatter
      try {
        const rootTree = await api.getWorkspaceTree(workspacePath);
        console.log('[WorkspaceController] Workspace tree root path:', rootTree?.path);

        if (rootTree?.path) {
          const rootFrontmatter = await api.getFrontmatter(rootTree.path);

          sharedWorkspaceId =
            (rootFrontmatter.workspace_id as string) ?? null;

          // If no workspace_id exists, generate one and save it
          if (!sharedWorkspaceId) {
            sharedWorkspaceId = generateUUID();
            console.log(
              '[WorkspaceController] workspace_id missing in index; generating:',
              sharedWorkspaceId
            );

            await api.setFrontmatterProperty(
              rootTree.path,
              'workspace_id',
              sharedWorkspaceId
            );

            console.log(
              '[WorkspaceController] Wrote workspace_id to index, persisting...',
              sharedWorkspaceId
            );

            // Re-read to confirm it actually persisted (especially important in WASM mode)
            const verifyFrontmatter = await api.getFrontmatter(rootTree.path);
            console.log(
              '[WorkspaceController] Verified workspace_id after write:',
              normalizeFrontmatter(verifyFrontmatter)?.workspace_id
            );
          } else {
            console.log(
              '[WorkspaceController] Using workspace_id from index:',
              sharedWorkspaceId
            );
          }
        }
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
            // Recursively try again to setup workspace_id and CRDT
            return await setupWorkspaceCrdt(
              api,
              backend,
              rustApi,
              collaborationServerUrl,
              collaborationEnabled,
              envWorkspaceId,
              onConnectionChange,
              retryCount
            );
          } catch (createErr) {
            const createErrStr = String(createErr);
            if (createErrStr.includes('Workspace already exists')) {
              if (retryCount >= 3) {
                console.error(
                  '[WorkspaceController] Max retries reached for workspace setup. Stopping to prevent infinite loop.'
                );
                return { workspaceId: null, initialized: false };
              }
              console.log(
                `[WorkspaceController] Workspace existed but wasn't found initially. Retrying setup (attempt ${retryCount + 1})...`
              );
              await new Promise((resolve) => setTimeout(resolve, 500));
              return await setupWorkspaceCrdt(
                api,
                backend,
                rustApi,
                collaborationServerUrl,
                collaborationEnabled,
                envWorkspaceId,
                onConnectionChange,
                retryCount + 1
              );
            }
            console.error(
              '[WorkspaceController] Failed to create default workspace:',
              createErr
            );
          }
        }

        console.warn(
          '[WorkspaceController] Could not get/set workspace_id from index:',
          e
        );
        // Fall back to null - will use simple room names without workspace prefix
        console.log(
          '[WorkspaceController] Using no workspace_id prefix (simple room names)'
        );
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
