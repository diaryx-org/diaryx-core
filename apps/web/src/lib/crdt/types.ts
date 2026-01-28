/**
 * CRDT-specific command and response types.
 *
 * These extend the generated Command/Response types with CRDT operations
 * that are implemented in the Rust backend but not yet in the generated types.
 */

import type { Command as GeneratedCommand, Response as GeneratedResponse } from '../backend/generated';
import type { CrdtHistoryEntry, FileDiff, FileMetadata } from '../backend/generated';
import type { JsonValue } from '../backend/generated/serde_json/JsonValue';

// CRDT-specific commands
export type CrdtCommand =
  | GeneratedCommand
  // Workspace CRDT operations
  | { type: 'GetSyncState'; params: { doc_name: string } }
  | { type: 'ApplyRemoteUpdate'; params: { doc_name: string; update: number[] } }
  | { type: 'GetMissingUpdates'; params: { doc_name: string; remote_state_vector: number[] } }
  | { type: 'GetFullState'; params: { doc_name: string } }
  // History operations
  | { type: 'GetHistory'; params: { doc_name: string; limit: number | null } }
  | { type: 'GetFileHistory'; params: { file_path: string; limit: number | null } }
  | { type: 'RestoreVersion'; params: { doc_name: string; update_id: bigint } }
  | { type: 'GetVersionDiff'; params: { doc_name: string; from_id: bigint; to_id: bigint } }
  | { type: 'GetStateAt'; params: { doc_name: string; update_id: bigint } }
  // File metadata operations
  | { type: 'GetCrdtFile'; params: { path: string } }
  | { type: 'SetCrdtFile'; params: { path: string; metadata: JsonValue } }
  | { type: 'ListCrdtFiles'; params: { include_deleted: boolean } }
  | { type: 'SaveCrdtState'; params: { doc_name: string } }
  // Body document operations
  | { type: 'GetBodyContent'; params: { doc_name: string } }
  | { type: 'SetBodyContent'; params: { doc_name: string; content: string } }
  | { type: 'GetBodySyncState'; params: { doc_name: string } }
  | { type: 'GetBodyFullState'; params: { doc_name: string } }
  | { type: 'ApplyBodyUpdate'; params: { doc_name: string; update: number[] } }
  | { type: 'GetBodyMissingUpdates'; params: { doc_name: string; remote_state_vector: number[] } }
  | { type: 'SaveBodyDoc'; params: { doc_name: string } }
  | { type: 'SaveAllBodyDocs' }
  | { type: 'ListLoadedBodyDocs' }
  | { type: 'UnloadBodyDoc'; params: { doc_name: string } }
  // Sync protocol operations
  | { type: 'CreateSyncStep1'; params: { doc_name: string } }
  | { type: 'HandleSyncMessage'; params: { doc_name: string; message: number[]; write_to_disk: boolean } }
  | { type: 'CreateUpdateMessage'; params: { doc_name: string; update: number[] } };

// CRDT-specific responses
export type CrdtResponse =
  | GeneratedResponse
  | { type: 'Binary'; data: number[] }
  | { type: 'UpdateId'; data: bigint | null }
  | { type: 'CrdtHistory'; data: CrdtHistoryEntry[] }
  | { type: 'VersionDiff'; data: FileDiff[] }
  | { type: 'CrdtFile'; data: FileMetadata | null }
  | { type: 'CrdtFiles'; data: [string, FileMetadata][] };
