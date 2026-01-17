/**
 * Typed API wrapper for the unified execute() command pattern.
 * 
 * This module provides ergonomic, type-safe functions that wrap execute() calls.
 * Usage: `const api = createApi(backend); await api.getEntry(path);`
 */

import type { Backend } from './interface';
import type {
  Response,
  EntryData,
  TreeNode,
  SearchResults,
  ValidationResult,
  ValidationResultWithMeta,
  FixResult,
  FixSummary,
  ExportPlan,
  ExportedFile,
  BinaryExportFile,
  TemplateInfo,
  StorageInfo,
  CreateEntryOptions,
  SearchOptions,
  AncestorAttachmentsResult,
} from './generated';
import type { JsonValue } from './generated/serde_json/JsonValue';

// Helper to extract response data with type checking
function expectResponse<T extends Response['type']>(
  response: Response,
  expectedType: T
): Extract<Response, { type: T }> {
  if (response.type !== expectedType) {
    throw new Error(`Expected response type '${expectedType}', got '${response.type}'`);
  }
  return response as Extract<Response, { type: T }>;
}

/**
 * Create a typed API wrapper around a Backend instance.
 */
export function createApi(backend: Backend) {
  return {
    // =========================================================================
    // Entry Operations
    // =========================================================================

    /** Get an entry's content and metadata. */
    async getEntry(path: string): Promise<EntryData> {
      const response = await backend.execute({ type: 'GetEntry', params: { path } });
      return expectResponse(response, 'Entry').data;
    },

    /** Save an entry's content. */
    async saveEntry(path: string, content: string): Promise<void> {
      await backend.execute({ type: 'SaveEntry', params: { path, content } });
    },

    /** Create a new entry. Returns the path to the created entry. */
    async createEntry(path: string, options?: { title?: string | null; template?: string | null; part_of?: string | null }): Promise<string> {
      const fullOptions: CreateEntryOptions = {
        title: options?.title ?? null,
        part_of: options?.part_of ?? null,
        template: options?.template ?? null,
      };
      const response = await backend.execute({
        type: 'CreateEntry',
        params: { path, options: fullOptions },
      });
      return expectResponse(response, 'String').data;
    },

    /** Delete an entry. */
    async deleteEntry(path: string): Promise<void> {
      await backend.execute({ type: 'DeleteEntry', params: { path } });
    },

    /** Move/rename an entry from one path to another. */
    async moveEntry(from: string, to: string): Promise<void> {
      await backend.execute({ type: 'MoveEntry', params: { from, to } });
    },

    /** Rename an entry file. Returns the new path. */
    async renameEntry(path: string, newFilename: string): Promise<string> {
      const response = await backend.execute({
        type: 'RenameEntry',
        params: { path, new_filename: newFilename },
      });
      return expectResponse(response, 'String').data;
    },

    /** Duplicate an entry, creating a copy. Returns the new path. */
    async duplicateEntry(path: string): Promise<string> {
      const response = await backend.execute({
        type: 'DuplicateEntry',
        params: { path },
      });
      return expectResponse(response, 'String').data;
    },

    /** Convert a leaf file to an index file with a directory. */
    async convertToIndex(path: string): Promise<string> {
      const response = await backend.execute({ type: 'ConvertToIndex', params: { path } });
      return expectResponse(response, 'String').data;
    },

    /** Convert an empty index file back to a leaf file. */
    async convertToLeaf(path: string): Promise<string> {
      const response = await backend.execute({ type: 'ConvertToLeaf', params: { path } });
      return expectResponse(response, 'String').data;
    },

    /** Create a new child entry under a parent. Returns the path to the created entry. */
    async createChildEntry(parentPath: string): Promise<string> {
      const response = await backend.execute({
        type: 'CreateChildEntry',
        params: { parent_path: parentPath },
      });
      return expectResponse(response, 'String').data;
    },

    /** Attach an existing entry to a parent index. */
    async attachEntryToParent(entryPath: string, parentPath: string): Promise<void> {
      await backend.execute({
        type: 'AttachEntryToParent',
        params: { entry_path: entryPath, parent_path: parentPath },
      });
    },

    /** Ensure today's daily entry exists. Returns the path to the daily entry. */
    async ensureDailyEntry(): Promise<string> {
      const response = await backend.execute({ type: 'EnsureDailyEntry' });
      return expectResponse(response, 'String').data;
    },

    // =========================================================================
    // Workspace Operations
    // =========================================================================

    /** Find the root index file in a directory. */
    async findRootIndex(directory: string): Promise<string> {
      const response = await backend.execute({
        type: 'FindRootIndex',
        params: { directory },
      });
      return expectResponse(response, 'String').data;
    },

    /** Get the workspace tree structure. */
    async getWorkspaceTree(path?: string, depth?: number): Promise<TreeNode> {
      const response = await backend.execute({
        type: 'GetWorkspaceTree',
        params: { path: path ?? null, depth: depth ?? null },
      });
      return expectResponse(response, 'Tree').data;
    },

    /** Get the filesystem tree (for "Show All Files" mode). */
    async getFilesystemTree(path?: string, showHidden = false, depth?: number): Promise<TreeNode> {
      const response = await backend.execute({
        type: 'GetFilesystemTree',
        params: { path: path ?? null, show_hidden: showHidden, depth: depth ?? null },
      });
      return expectResponse(response, 'Tree').data;
    },

    /** Create a new workspace. */
    async createWorkspace(path?: string, name?: string): Promise<void> {
      await backend.execute({
        type: 'CreateWorkspace',
        params: { path: path ?? null, name: name ?? null },
      });
    },

    // =========================================================================
    // Frontmatter Operations
    // =========================================================================

    /** Get all frontmatter properties for an entry. */
    async getFrontmatter(path: string): Promise<Record<string, JsonValue | undefined>> {
      const response = await backend.execute({ type: 'GetFrontmatter', params: { path } });
      return expectResponse(response, 'Frontmatter').data;
    },

    /** Set a frontmatter property. */
    async setFrontmatterProperty(path: string, key: string, value: JsonValue): Promise<void> {
      await backend.execute({
        type: 'SetFrontmatterProperty',
        params: { path, key, value },
      });
    },

    /** Remove a frontmatter property. */
    async removeFrontmatterProperty(path: string, key: string): Promise<void> {
      await backend.execute({
        type: 'RemoveFrontmatterProperty',
        params: { path, key },
      });
    },

    // =========================================================================
    // Search
    // =========================================================================

    /** Search the workspace for entries. */
    async searchWorkspace(pattern: string, options?: Partial<SearchOptions>): Promise<SearchResults> {
      const fullOptions: SearchOptions = {
        workspace_path: options?.workspace_path ?? null,
        search_frontmatter: options?.search_frontmatter ?? false,
        property: options?.property ?? null,
        case_sensitive: options?.case_sensitive ?? false,
      };
      const response = await backend.execute({
        type: 'SearchWorkspace',
        params: { pattern, options: fullOptions },
      });
      return expectResponse(response, 'SearchResults').data;
    },

    // =========================================================================
    // Validation
    // =========================================================================

    /** Validate workspace links. Returns result with computed metadata. */
    async validateWorkspace(path?: string): Promise<ValidationResultWithMeta> {
      const response = await backend.execute({
        type: 'ValidateWorkspace',
        params: { path: path ?? null },
      });
      return expectResponse(response, 'ValidationResult').data;
    },

    /** Validate a single file's links. Returns result with computed metadata. */
    async validateFile(path: string): Promise<ValidationResultWithMeta> {
      const response = await backend.execute({ type: 'ValidateFile', params: { path } });
      return expectResponse(response, 'ValidationResult').data;
    },

    /** Fix a broken part_of reference. */
    async fixBrokenPartOf(path: string): Promise<FixResult> {
      const response = await backend.execute({ type: 'FixBrokenPartOf', params: { path } });
      return expectResponse(response, 'FixResult').data;
    },

    /** Fix a broken contents reference. */
    async fixBrokenContentsRef(indexPath: string, target: string): Promise<FixResult> {
      const response = await backend.execute({
        type: 'FixBrokenContentsRef',
        params: { index_path: indexPath, target },
      });
      return expectResponse(response, 'FixResult').data;
    },

    /** Fix a broken attachment reference. */
    async fixBrokenAttachment(path: string, attachment: string): Promise<FixResult> {
      const response = await backend.execute({
        type: 'FixBrokenAttachment',
        params: { path, attachment },
      });
      return expectResponse(response, 'FixResult').data;
    },

    /** Fix a non-portable path. */
    async fixNonPortablePath(
      path: string,
      property: string,
      oldValue: string,
      newValue: string
    ): Promise<FixResult> {
      const response = await backend.execute({
        type: 'FixNonPortablePath',
        params: { path, property, old_value: oldValue, new_value: newValue },
      });
      return expectResponse(response, 'FixResult').data;
    },

    /** Add an unlisted file to an index's contents. */
    async fixUnlistedFile(indexPath: string, filePath: string): Promise<FixResult> {
      const response = await backend.execute({
        type: 'FixUnlistedFile',
        params: { index_path: indexPath, file_path: filePath },
      });
      return expectResponse(response, 'FixResult').data;
    },

    /** Add an orphan binary file to an index's attachments. */
    async fixOrphanBinaryFile(indexPath: string, filePath: string): Promise<FixResult> {
      const response = await backend.execute({
        type: 'FixOrphanBinaryFile',
        params: { index_path: indexPath, file_path: filePath },
      });
      return expectResponse(response, 'FixResult').data;
    },

    /** Fix a missing part_of reference. */
    async fixMissingPartOf(filePath: string, indexPath: string): Promise<FixResult> {
      const response = await backend.execute({
        type: 'FixMissingPartOf',
        params: { file_path: filePath, index_path: indexPath },
      });
      return expectResponse(response, 'FixResult').data;
    },

    /** Fix all validation issues. */
    async fixAll(validationResult: ValidationResult): Promise<FixSummary> {
      const response = await backend.execute({
        type: 'FixAll',
        params: { validation_result: validationResult },
      });
      return expectResponse(response, 'FixSummary').data;
    },

    /** Fix a circular reference by removing a contents reference. */
    async fixCircularReference(filePath: string, contentsRefToRemove: string): Promise<FixResult> {
      const response = await backend.execute({
        type: 'FixCircularReference',
        params: { file_path: filePath, part_of_value: contentsRefToRemove },
      });
      return expectResponse(response, 'FixResult').data;
    },

    /** Get available parent indexes for a file (for "Choose parent" picker). */
    async getAvailableParentIndexes(filePath: string, workspaceRoot: string): Promise<string[]> {
      const response = await backend.execute({
        type: 'GetAvailableParentIndexes',
        params: { file_path: filePath, workspace_root: workspaceRoot },
      });
      return expectResponse(response, 'Strings').data;
    },

    // =========================================================================
    // Export
    // =========================================================================

    /** Get available audiences. */
    async getAvailableAudiences(rootPath: string): Promise<string[]> {
      const response = await backend.execute({
        type: 'GetAvailableAudiences',
        params: { root_path: rootPath },
      });
      return expectResponse(response, 'Strings').data;
    },

    /** Plan an export operation. */
    async planExport(rootPath: string, audience: string): Promise<ExportPlan> {
      const response = await backend.execute({
        type: 'PlanExport',
        params: { root_path: rootPath, audience },
      });
      return expectResponse(response, 'ExportPlan').data;
    },

    /** Export to memory. */
    async exportToMemory(rootPath: string, audience: string): Promise<ExportedFile[]> {
      const response = await backend.execute({
        type: 'ExportToMemory',
        params: { root_path: rootPath, audience },
      });
      return expectResponse(response, 'ExportedFiles').data;
    },

    /** Export to HTML. */
    async exportToHtml(rootPath: string, audience: string): Promise<ExportedFile[]> {
      const response = await backend.execute({
        type: 'ExportToHtml',
        params: { root_path: rootPath, audience },
      });
      return expectResponse(response, 'ExportedFiles').data;
    },

    /** Export binary attachments (returns paths only, use readBinary to get data). */
    async exportBinaryAttachments(rootPath: string, audience: string): Promise<{ source_path: string; relative_path: string }[]> {
      const response = await backend.execute({
        type: 'ExportBinaryAttachments',
        params: { root_path: rootPath, audience },
      });
      return expectResponse(response, 'BinaryFilePaths').data;
    },

    // =========================================================================
    // Templates
    // =========================================================================

    /** List available templates. */
    async listTemplates(workspacePath?: string): Promise<TemplateInfo[]> {
      const response = await backend.execute({
        type: 'ListTemplates',
        params: { workspace_path: workspacePath ?? null },
      });
      return expectResponse(response, 'Templates').data;
    },

    /** Get a template's content. */
    async getTemplate(name: string, workspacePath?: string): Promise<string> {
      const response = await backend.execute({
        type: 'GetTemplate',
        params: { name, workspace_path: workspacePath ?? null },
      });
      return expectResponse(response, 'String').data;
    },

    /** Save a template. */
    async saveTemplate(name: string, content: string, workspacePath: string): Promise<void> {
      await backend.execute({
        type: 'SaveTemplate',
        params: { name, content, workspace_path: workspacePath },
      });
    },

    /** Delete a template. */
    async deleteTemplate(name: string, workspacePath: string): Promise<void> {
      await backend.execute({
        type: 'DeleteTemplate',
        params: { name, workspace_path: workspacePath },
      });
    },

    // =========================================================================
    // Attachments
    // =========================================================================

    /** Get attachments for an entry. */
    async getAttachments(path: string): Promise<string[]> {
      const response = await backend.execute({ type: 'GetAttachments', params: { path } });
      return expectResponse(response, 'Strings').data;
    },

    /** Upload an attachment. Returns the path to the uploaded file. */
    async uploadAttachment(entryPath: string, filename: string, dataBase64: string): Promise<string> {
      const response = await backend.execute({
        type: 'UploadAttachment',
        params: { entry_path: entryPath, filename, data_base64: dataBase64 },
      });
      return expectResponse(response, 'String').data;
    },

    /** Delete an attachment. */
    async deleteAttachment(entryPath: string, attachmentPath: string): Promise<void> {
      await backend.execute({
        type: 'DeleteAttachment',
        params: { entry_path: entryPath, attachment_path: attachmentPath },
      });
    },

    /** Get attachment data. */
    async getAttachmentData(entryPath: string, attachmentPath: string): Promise<number[]> {
      const response = await backend.execute({
        type: 'GetAttachmentData',
        params: { entry_path: entryPath, attachment_path: attachmentPath },
      });
      return expectResponse(response, 'Bytes').data;
    },

    /** Move an attachment from one entry to another. Returns the new attachment path. */
    async moveAttachment(
      sourceEntryPath: string,
      targetEntryPath: string,
      attachmentPath: string,
      newFilename?: string
    ): Promise<string> {
      const response = await backend.execute({
        type: 'MoveAttachment',
        params: {
          source_entry_path: sourceEntryPath,
          target_entry_path: targetEntryPath,
          attachment_path: attachmentPath,
          new_filename: newFilename ?? null,
        },
      });
      return expectResponse(response, 'String').data;
    },

    /** Get attachments from current entry and all ancestor indexes in the part_of chain. */
    async getAncestorAttachments(path: string): Promise<AncestorAttachmentsResult> {
      const response = await backend.execute({
        type: 'GetAncestorAttachments',
        params: { path },
      });
      return expectResponse(response, 'AncestorAttachments').data;
    },

    // =========================================================================
    // File System
    // =========================================================================

    /** Check if a file exists. */
    async fileExists(path: string): Promise<boolean> {
      const response = await backend.execute({ type: 'FileExists', params: { path } });
      return expectResponse(response, 'Bool').data;
    },

    /** Read a file's content. */
    async readFile(path: string): Promise<string> {
      const response = await backend.execute({ type: 'ReadFile', params: { path } });
      return expectResponse(response, 'String').data;
    },

    /** Write content to a file. */
    async writeFile(path: string, content: string): Promise<void> {
      await backend.execute({ type: 'WriteFile', params: { path, content } });
    },

    /** Delete a file. */
    async deleteFile(path: string): Promise<void> {
      await backend.execute({ type: 'DeleteFile', params: { path } });
    },

    /** Read a binary file's content. */
    async readBinary(path: string): Promise<Uint8Array> {
      return backend.readBinary(path);
    },

    /** Write binary content to a file. */
    async writeBinary(path: string, data: Uint8Array): Promise<void> {
      return backend.writeBinary(path, data);
    },

    // =========================================================================
    // Storage
    // =========================================================================

    /** Get storage usage information. */
    async getStorageUsage(): Promise<StorageInfo> {
      const response = await backend.execute({ type: 'GetStorageUsage' });
      return expectResponse(response, 'StorageInfo').data;
    },
  };
}

/** Type of the API wrapper returned by createApi(). */
export type Api = ReturnType<typeof createApi>;
