import type { RustCrdtApi } from '$lib/crdt/rustCrdtApi';
import {
  initWorkspace,
  setWorkspaceId,
  setInitializing,
  updateFileMetadata as updateFileInCrdt,
  addToContents,
  getWorkspaceStats,
  type FileMetadata,
  type BinaryRef,
} from '$lib/crdt/workspaceCrdtBridge';
import { setCollaborationWorkspaceId } from '$lib/crdt/collaborationBridge';
import type { JsonValue } from '$lib/backend/generated/serde_json/JsonValue';

// ============================================================================
// Types
// ============================================================================

export interface WorkspaceCrdtCallbacks {
  onFilesChange?: (files: Map<string, FileMetadata>) => void;
  onConnectionChange?: (connected: boolean) => void;
  onRemoteFileSync?: (created: string[], deleted: string[]) => Promise<void>;
}

export interface WorkspaceCrdtStats {
  activeFiles: number;
  totalAttachments: number;
}

// ============================================================================
// State for tracking initialization
// ============================================================================

let isInitialized = false;

// ============================================================================
// Public API
// ============================================================================

/**
 * Initialize the workspace CRDT system.
 *
 * @param workspaceId - Unique workspace identifier (null for simple room names)
 * @param _workspacePath - Path to workspace root (for display, currently unused)
 * @param serverUrl - Collaboration server URL (null for offline mode)
 * @param collaborationEnabled - Whether collaboration is enabled
 * @param rustApi - Rust CRDT API instance
 * @param callbacks - Event callbacks
 * @returns Whether initialization succeeded
 */
export async function initializeWorkspaceCrdt(
  workspaceId: string | null,
  _workspacePath: string | null,
  serverUrl: string | null,
  collaborationEnabled: boolean,
  rustApi: RustCrdtApi,
  _callbacks: WorkspaceCrdtCallbacks,
): Promise<boolean> {
  try {
    // Set workspace ID for per-file document room naming
    setWorkspaceId(workspaceId);
    setCollaborationWorkspaceId(workspaceId);

    // Initialize workspace CRDT (bridge is created internally if collaboration is enabled)
    setInitializing(true);
    try {
      await initWorkspace({
        rustApi,
        serverUrl: collaborationEnabled && serverUrl ? serverUrl : undefined,
        workspaceId: workspaceId ?? undefined,
        onReady: () => {
          console.log('[WorkspaceCrdtService] Workspace CRDT ready');
        },
      });
    } finally {
      setInitializing(false);
    }

    const stats = await getWorkspaceStats();
    console.log(
      `[WorkspaceCrdtService] Initialized: ${stats.activeFiles} active, ${stats.deletedFiles} deleted files`,
    );

    isInitialized = true;
    return true;
  } catch (e) {
    console.error('[WorkspaceCrdtService] Failed to initialize:', e);
    isInitialized = false;
    return false;
  }
}

/**
 * Check if CRDT is initialized.
 */
export function isCrdtInitialized(): boolean {
  return isInitialized;
}

/**
 * Reset initialization state.
 */
export function resetCrdtState(): void {
  isInitialized = false;
}

/**
 * Update file metadata in the CRDT.
 */
export async function updateCrdtFileMetadata(
  path: string,
  frontmatter: Record<string, unknown>,
): Promise<void> {
  if (!isInitialized) return;

  try {
    // Build extra fields with proper typing
    const extraFields: Record<string, JsonValue | undefined> = {};
    for (const [key, value] of Object.entries(frontmatter)) {
      if (!['title', 'part_of', 'contents', 'attachments', 'audience', 'description'].includes(key)) {
        extraFields[key] = value as JsonValue;
      }
    }

    await updateFileInCrdt(path, {
      title: (frontmatter.title as string) ?? null,
      part_of: (frontmatter.part_of as string) ?? null,
      // Keep contents as relative paths (as stored in frontmatter)
      // The CRDT uses relative paths consistently - don't convert to full paths
      contents: frontmatter.contents
        ? (frontmatter.contents as string[])
        : null,
      audience: (frontmatter.audience as string[]) ?? null,
      description: (frontmatter.description as string) ?? null,
      extra: extraFields,
    });
  } catch (e) {
    console.error('[WorkspaceCrdtService] Failed to update metadata:', e);
  }
}

/**
 * Add a new file to the CRDT.
 */
export async function addFileToCrdt(
  path: string,
  frontmatter: Record<string, unknown>,
  parentPath: string | null,
): Promise<void> {
  if (!isInitialized) return;

  try {
    // Build extra fields with proper typing
    const extraFields: Record<string, JsonValue | undefined> = {};
    for (const [key, value] of Object.entries(frontmatter)) {
      if (!['title', 'part_of', 'contents', 'attachments', 'audience', 'description'].includes(key)) {
        extraFields[key] = value as JsonValue;
      }
    }

    const metadata: FileMetadata = {
      title: (frontmatter.title as string) ?? null,
      part_of: parentPath ?? (frontmatter.part_of as string) ?? null,
      // Keep contents as relative paths (as stored in frontmatter)
      // The CRDT uses relative paths consistently - don't convert to full paths
      contents: frontmatter.contents
        ? (frontmatter.contents as string[])
        : null,
      attachments: ((frontmatter.attachments as string[]) ?? []).map((p): BinaryRef => ({
        path: p,
        source: 'local',
        hash: '',
        mime_type: '',
        size: BigInt(0),
        uploaded_at: null,
        deleted: false,
      })),
      deleted: false,
      audience: (frontmatter.audience as string[]) ?? null,
      description: (frontmatter.description as string) ?? null,
      extra: extraFields,
      modified_at: BigInt(Date.now()),
    };

    await updateFileInCrdt(path, metadata);

    // Add to parent's contents if parent exists
    if (parentPath) {
      // Calculate relative path (just the filename if inside parent dir, or fully relative)
      let relativePath = path;
      if (path.startsWith(parentPath + '/')) {
        relativePath = path.substring(parentPath.length + 1);
      } else {
        const lastSlash = path.lastIndexOf('/');
        relativePath = lastSlash >= 0 ? path.substring(lastSlash + 1) : path;
      }
      await addToContents(parentPath, relativePath);
    }
  } catch (e) {
    console.error('[WorkspaceCrdtService] Failed to add file:', e);
  }
}

/**
 * Create an attachment reference for CRDT tracking.
 */
export function createAttachmentRef(
  attachmentPath: string,
  file: File,
): BinaryRef {
  return {
    path: attachmentPath,
    source: 'local',
    hash: '',
    mime_type: file.type,
    size: BigInt(file.size),
    uploaded_at: BigInt(Date.now()),
    deleted: false,
  };
}

/**
 * Get workspace statistics.
 */
export async function getCrdtStats(): Promise<WorkspaceCrdtStats> {
  const stats = await getWorkspaceStats();
  return {
    activeFiles: stats.activeFiles,
    totalAttachments: 0, // TODO: count attachments from all files
  };
}
