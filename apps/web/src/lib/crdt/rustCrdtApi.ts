/**
 * Type-safe API wrapper for Rust CRDT commands.
 *
 * This module provides ergonomic access to the CRDT functionality
 * implemented in the Rust backend (diaryx_core/crdt).
 */

import type { Backend } from '../backend/interface';
import type {
  CrdtHistoryEntry,
  FileDiff,
  FileMetadata,
} from '../backend/generated';
import type { JsonValue } from '../backend/generated/serde_json/JsonValue';
import type { CrdtCommand, CrdtResponse } from './types';

// Helper to extract response data with type checking
function expectResponse<T extends CrdtResponse['type']>(
  response: CrdtResponse,
  expectedType: T
): Extract<CrdtResponse, { type: T }> {
  if (response.type !== expectedType) {
    throw new Error(`Expected response type '${expectedType}', got '${response.type}'`);
  }
  return response as Extract<CrdtResponse, { type: T }>;
}

// Type-safe execute helper that accepts CRDT commands
async function executeCrdt(backend: Backend, command: CrdtCommand): Promise<CrdtResponse> {
  // Cast to any since the Backend interface only knows about generated Command type
  return await backend.execute(command as any) as CrdtResponse;
}

/**
 * CRDT API wrapper providing type-safe access to Rust CRDT operations.
 */
export class RustCrdtApi {
  constructor(private backend: Backend) {}

  // ===========================================================================
  // Workspace CRDT Operations
  // ===========================================================================

  /**
   * Get the sync state vector for the workspace CRDT.
   * Used to initiate sync with a remote peer.
   */
  async getSyncState(docName: string = 'workspace'): Promise<Uint8Array> {
    const response = await executeCrdt(this.backend, {
      type: 'GetSyncState',
      params: { doc_name: docName },
    });
    const data = expectResponse(response, 'Binary').data;
    return new Uint8Array(data);
  }

  /**
   * Apply an update received from a remote peer.
   * Returns the update ID assigned to this update.
   */
  async applyRemoteUpdate(
    update: Uint8Array,
    docName: string = 'workspace'
  ): Promise<bigint | null> {
    const response = await executeCrdt(this.backend, {
      type: 'ApplyRemoteUpdate',
      params: { doc_name: docName, update: Array.from(update) },
    });
    return expectResponse(response, 'UpdateId').data;
  }

  /**
   * Get updates that a remote peer is missing.
   * Send these updates to sync the remote peer.
   */
  async getMissingUpdates(
    remoteStateVector: Uint8Array,
    docName: string = 'workspace'
  ): Promise<Uint8Array> {
    const response = await executeCrdt(this.backend, {
      type: 'GetMissingUpdates',
      params: { doc_name: docName, remote_state_vector: Array.from(remoteStateVector) },
    });
    const data = expectResponse(response, 'Binary').data;
    return new Uint8Array(data);
  }

  /**
   * Get the full CRDT state as an update.
   * Can be used to initialize a new peer.
   */
  async getFullState(docName: string = 'workspace'): Promise<Uint8Array> {
    const response = await executeCrdt(this.backend, {
      type: 'GetFullState',
      params: { doc_name: docName },
    });
    const data = expectResponse(response, 'Binary').data;
    return new Uint8Array(data);
  }

  // ===========================================================================
  // History Operations
  // ===========================================================================

  /**
   * Get the version history for a document.
   */
  async getHistory(docName: string = 'workspace', limit?: number): Promise<CrdtHistoryEntry[]> {
    console.log('[RustCrdtApi] getHistory:', docName, 'limit:', limit);
    const response = await executeCrdt(this.backend, {
      type: 'GetHistory',
      params: { doc_name: docName, limit: limit ?? null },
    });
    const history = expectResponse(response, 'CrdtHistory').data;
    console.log('[RustCrdtApi] getHistory result:', history.length, 'entries');
    return history;
  }

  /**
   * Get the version history for a specific file, combining body and workspace changes.
   */
  async getFileHistory(filePath: string, limit?: number): Promise<CrdtHistoryEntry[]> {
    console.log('[RustCrdtApi] getFileHistory:', filePath, 'limit:', limit);
    const response = await executeCrdt(this.backend, {
      type: 'GetFileHistory',
      params: { file_path: filePath, limit: limit ?? null },
    });
    const history = expectResponse(response, 'CrdtHistory').data;
    console.log('[RustCrdtApi] getFileHistory result:', history.length, 'entries');
    return history;
  }

  /**
   * Restore a document to a previous version.
   */
  async restoreVersion(updateId: bigint, docName: string = 'workspace'): Promise<void> {
    await executeCrdt(this.backend, {
      type: 'RestoreVersion',
      params: { doc_name: docName, update_id: updateId },
    });
  }

  /**
   * Get the diff between two versions of a document.
   */
  async getVersionDiff(
    fromId: bigint,
    toId: bigint,
    docName: string = 'workspace'
  ): Promise<FileDiff[]> {
    const response = await executeCrdt(this.backend, {
      type: 'GetVersionDiff',
      params: { doc_name: docName, from_id: fromId, to_id: toId },
    });
    return expectResponse(response, 'VersionDiff').data;
  }

  /**
   * Get the state of a document at a specific point in history.
   */
  async getStateAt(updateId: bigint, docName: string = 'workspace'): Promise<Uint8Array | null> {
    const response = await executeCrdt(this.backend, {
      type: 'GetStateAt',
      params: { doc_name: docName, update_id: updateId },
    });
    if (response.type === 'Ok') {
      return null;
    }
    const data = expectResponse(response, 'Binary').data;
    return new Uint8Array(data);
  }

  // ===========================================================================
  // File Metadata Operations
  // ===========================================================================

  /**
   * Get file metadata from the CRDT.
   */
  async getFile(path: string): Promise<FileMetadata | null> {
    const response = await executeCrdt(this.backend, {
      type: 'GetCrdtFile',
      params: { path },
    });
    return expectResponse(response, 'CrdtFile').data;
  }

  /**
   * Set file metadata in the CRDT.
   */
  async setFile(path: string, metadata: FileMetadata): Promise<void> {
    console.log('[RustCrdtApi] setFile:', path);
    await executeCrdt(this.backend, {
      type: 'SetCrdtFile',
      params: { path, metadata: metadata as unknown as JsonValue },
    });
    console.log('[RustCrdtApi] setFile complete:', path);
  }

  /**
   * List all files in the CRDT.
   */
  async listFiles(includeDeleted: boolean = false): Promise<[string, FileMetadata][]> {
    const response = await executeCrdt(this.backend, {
      type: 'ListCrdtFiles',
      params: { include_deleted: includeDeleted },
    });
    return expectResponse(response, 'CrdtFiles').data;
  }

  /**
   * Save CRDT state to persistent storage.
   */
  async saveCrdtState(docName: string = 'workspace'): Promise<void> {
    await executeCrdt(this.backend, {
      type: 'SaveCrdtState',
      params: { doc_name: docName },
    });
  }

  // ===========================================================================
  // Body Document Operations
  // ===========================================================================

  /**
   * Get body content from a document CRDT.
   */
  async getBodyContent(docName: string): Promise<string> {
    const response = await executeCrdt(this.backend, {
      type: 'GetBodyContent',
      params: { doc_name: docName },
    });
    return expectResponse(response, 'String').data;
  }

  /**
   * Set body content in a document CRDT.
   */
  async setBodyContent(docName: string, content: string): Promise<void> {
    await executeCrdt(this.backend, {
      type: 'SetBodyContent',
      params: { doc_name: docName, content },
    });
  }

  /**
   * Get sync state (state vector) for a body document.
   */
  async getBodySyncState(docName: string): Promise<Uint8Array> {
    const response = await executeCrdt(this.backend, {
      type: 'GetBodySyncState',
      params: { doc_name: docName },
    });
    const data = expectResponse(response, 'Binary').data;
    return new Uint8Array(data);
  }

  /**
   * Get full state of a body document as an update.
   */
  async getBodyFullState(docName: string): Promise<Uint8Array> {
    const response = await executeCrdt(this.backend, {
      type: 'GetBodyFullState',
      params: { doc_name: docName },
    });
    const data = expectResponse(response, 'Binary').data;
    return new Uint8Array(data);
  }

  /**
   * Apply an update to a body document.
   * Returns the update ID assigned to this update.
   */
  async applyBodyUpdate(docName: string, update: Uint8Array): Promise<bigint | null> {
    const response = await executeCrdt(this.backend, {
      type: 'ApplyBodyUpdate',
      params: { doc_name: docName, update: Array.from(update) },
    });
    return expectResponse(response, 'UpdateId').data;
  }

  /**
   * Get updates needed by a remote peer for a body document.
   */
  async getBodyMissingUpdates(
    docName: string,
    remoteStateVector: Uint8Array
  ): Promise<Uint8Array> {
    const response = await executeCrdt(this.backend, {
      type: 'GetBodyMissingUpdates',
      params: { doc_name: docName, remote_state_vector: Array.from(remoteStateVector) },
    });
    const data = expectResponse(response, 'Binary').data;
    return new Uint8Array(data);
  }

  /**
   * Save a body document to storage.
   */
  async saveBodyDoc(docName: string): Promise<void> {
    await executeCrdt(this.backend, {
      type: 'SaveBodyDoc',
      params: { doc_name: docName },
    });
  }

  /**
   * Save all body documents to storage.
   */
  async saveAllBodyDocs(): Promise<void> {
    await executeCrdt(this.backend, { type: 'SaveAllBodyDocs' });
  }

  /**
   * Get list of loaded body documents.
   */
  async listLoadedBodyDocs(): Promise<string[]> {
    const response = await executeCrdt(this.backend, { type: 'ListLoadedBodyDocs' });
    return expectResponse(response, 'Strings').data;
  }

  /**
   * Unload a body document from memory.
   */
  async unloadBodyDoc(docName: string): Promise<void> {
    await executeCrdt(this.backend, {
      type: 'UnloadBodyDoc',
      params: { doc_name: docName },
    });
  }

  // ===========================================================================
  // Sync Protocol Operations
  // ===========================================================================

  /**
   * Create a SyncStep1 message for initiating sync.
   * Returns the encoded message that should be sent to the sync server.
   */
  async createSyncStep1(docName: string = 'workspace'): Promise<Uint8Array> {
    const response = await executeCrdt(this.backend, {
      type: 'CreateSyncStep1',
      params: { doc_name: docName },
    });
    const data = expectResponse(response, 'Binary').data;
    return new Uint8Array(data);
  }

  /**
   * Handle an incoming sync message.
   * Returns an optional response message to send back, or null if no response needed.
   */
  async handleSyncMessage(
    message: Uint8Array,
    docName: string = 'workspace'
  ): Promise<Uint8Array | null> {
    const response = await executeCrdt(this.backend, {
      type: 'HandleSyncMessage',
      params: { doc_name: docName, message: Array.from(message) },
    });
    if (response.type === 'Ok') {
      return null;
    }
    const data = expectResponse(response, 'Binary').data;
    return data.length > 0 ? new Uint8Array(data) : null;
  }

  /**
   * Create an update message to broadcast local changes.
   */
  async createUpdateMessage(update: Uint8Array, docName: string = 'workspace'): Promise<Uint8Array> {
    const response = await executeCrdt(this.backend, {
      type: 'CreateUpdateMessage',
      params: { doc_name: docName, update: Array.from(update) },
    });
    const data = expectResponse(response, 'Binary').data;
    return new Uint8Array(data);
  }
}

/**
 * Create a CRDT API wrapper for a backend instance.
 */
export function createCrdtApi(backend: Backend): RustCrdtApi {
  return new RustCrdtApi(backend);
}
