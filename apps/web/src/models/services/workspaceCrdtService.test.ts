import { describe, it, expect, vi, beforeEach } from 'vitest'

// Mock all CRDT bridge dependencies
vi.mock('$lib/crdt/workspaceCrdtBridge', () => ({
  initWorkspace: vi.fn().mockResolvedValue(undefined),
  setWorkspaceId: vi.fn(),
  setInitializing: vi.fn(),
  updateFileMetadata: vi.fn().mockResolvedValue(undefined),
  addToContents: vi.fn().mockResolvedValue(undefined),
  getWorkspaceStats: vi.fn().mockResolvedValue({ activeFiles: 10, deletedFiles: 2 }),
}))

vi.mock('$lib/crdt/collaborationBridge', () => ({
  setCollaborationWorkspaceId: vi.fn(),
}))

import {
  initializeWorkspaceCrdt,
  isCrdtInitialized,
  resetCrdtState,
  updateCrdtFileMetadata,
  addFileToCrdt,
  createAttachmentRef,
  getCrdtStats,
} from './workspaceCrdtService'

import {
  initWorkspace,
  setWorkspaceId,
  setInitializing,
  updateFileMetadata,
  addToContents,
  getWorkspaceStats,
} from '$lib/crdt/workspaceCrdtBridge'

import { setCollaborationWorkspaceId } from '$lib/crdt/collaborationBridge'

describe('workspaceCrdtService', () => {
  const mockRustApi = {
    applyUpdate: vi.fn(),
    getState: vi.fn(),
  }

  beforeEach(() => {
    vi.clearAllMocks()
    resetCrdtState()
  })

  describe('initializeWorkspaceCrdt', () => {
    it('should initialize workspace CRDT with collaboration enabled', async () => {
      const result = await initializeWorkspaceCrdt(
        'workspace-123',
        '/path/to/workspace',
        'https://sync.example.com',
        true,
        mockRustApi as any,
        {}
      )

      expect(result).toBe(true)
      expect(isCrdtInitialized()).toBe(true)
      expect(setWorkspaceId).toHaveBeenCalledWith('workspace-123')
      expect(setCollaborationWorkspaceId).toHaveBeenCalledWith('workspace-123')
      expect(setInitializing).toHaveBeenCalledWith(true)
      expect(initWorkspace).toHaveBeenCalledWith({
        rustApi: mockRustApi,
        serverUrl: 'https://sync.example.com',
        workspaceId: 'workspace-123',
        onReady: expect.any(Function),
      })
      expect(setInitializing).toHaveBeenCalledWith(false)
    })

    it('should initialize workspace CRDT with collaboration disabled', async () => {
      const result = await initializeWorkspaceCrdt(
        'workspace-123',
        '/path/to/workspace',
        null,
        false,
        mockRustApi as any,
        {}
      )

      expect(result).toBe(true)
      expect(initWorkspace).toHaveBeenCalledWith({
        rustApi: mockRustApi,
        serverUrl: undefined,
        workspaceId: 'workspace-123',
        onReady: expect.any(Function),
      })
    })

    it('should handle null workspace ID', async () => {
      const result = await initializeWorkspaceCrdt(
        null,
        null,
        null,
        false,
        mockRustApi as any,
        {}
      )

      expect(result).toBe(true)
      expect(setWorkspaceId).toHaveBeenCalledWith(null)
      expect(setCollaborationWorkspaceId).toHaveBeenCalledWith(null)
      expect(initWorkspace).toHaveBeenCalledWith({
        rustApi: mockRustApi,
        serverUrl: undefined,
        workspaceId: undefined,
        onReady: expect.any(Function),
      })
    })

    it('should return false on initialization error', async () => {
      vi.mocked(initWorkspace).mockRejectedValueOnce(new Error('Init failed'))

      const result = await initializeWorkspaceCrdt(
        'workspace-123',
        '/path',
        null,
        false,
        mockRustApi as any,
        {}
      )

      expect(result).toBe(false)
      expect(isCrdtInitialized()).toBe(false)
    })

    it('should set initializing to false even on error', async () => {
      vi.mocked(initWorkspace).mockRejectedValueOnce(new Error('Init failed'))

      await initializeWorkspaceCrdt(
        'workspace-123',
        '/path',
        null,
        false,
        mockRustApi as any,
        {}
      )

      expect(setInitializing).toHaveBeenCalledWith(true)
      expect(setInitializing).toHaveBeenCalledWith(false)
    })
  })

  describe('isCrdtInitialized', () => {
    it('should return false initially', () => {
      expect(isCrdtInitialized()).toBe(false)
    })

    it('should return true after successful initialization', async () => {
      await initializeWorkspaceCrdt(
        'workspace-123',
        '/path',
        null,
        false,
        mockRustApi as any,
        {}
      )

      expect(isCrdtInitialized()).toBe(true)
    })
  })

  describe('resetCrdtState', () => {
    it('should reset initialization state', async () => {
      await initializeWorkspaceCrdt(
        'workspace-123',
        '/path',
        null,
        false,
        mockRustApi as any,
        {}
      )

      expect(isCrdtInitialized()).toBe(true)

      resetCrdtState()

      expect(isCrdtInitialized()).toBe(false)
    })
  })

  describe('updateCrdtFileMetadata', () => {
    it('should not update if CRDT is not initialized', async () => {
      await updateCrdtFileMetadata('test.md', { title: 'Test' })

      expect(updateFileMetadata).not.toHaveBeenCalled()
    })

    it('should update file metadata when initialized', async () => {
      await initializeWorkspaceCrdt(
        'workspace-123',
        '/path',
        null,
        false,
        mockRustApi as any,
        {}
      )

      await updateCrdtFileMetadata('test.md', {
        title: 'Test Title',
        part_of: 'parent.md',
        contents: ['child1.md', 'child2.md'],
        audience: ['developers'],
        description: 'A test file',
        custom_field: 'custom value',
      })

      expect(updateFileMetadata).toHaveBeenCalledWith('test.md', {
        title: 'Test Title',
        part_of: 'parent.md',
        contents: ['child1.md', 'child2.md'],
        audience: ['developers'],
        description: 'A test file',
        extra: {
          custom_field: 'custom value',
        },
      })
    })

    it('should handle missing optional fields', async () => {
      await initializeWorkspaceCrdt(
        'workspace-123',
        '/path',
        null,
        false,
        mockRustApi as any,
        {}
      )

      await updateCrdtFileMetadata('test.md', {})

      expect(updateFileMetadata).toHaveBeenCalledWith('test.md', {
        title: null,
        part_of: null,
        contents: null,
        audience: null,
        description: null,
        extra: {},
      })
    })
  })

  describe('addFileToCrdt', () => {
    it('should not add file if CRDT is not initialized', async () => {
      await addFileToCrdt('test.md', { title: 'Test' }, 'parent.md')

      expect(updateFileMetadata).not.toHaveBeenCalled()
      expect(addToContents).not.toHaveBeenCalled()
    })

    it('should add file with parent', async () => {
      await initializeWorkspaceCrdt(
        'workspace-123',
        '/path',
        null,
        false,
        mockRustApi as any,
        {}
      )

      await addFileToCrdt('folder/test.md', {
        title: 'Test',
        attachments: ['image.png'],
      }, 'folder/index.md')

      expect(updateFileMetadata).toHaveBeenCalledWith('folder/test.md', expect.objectContaining({
        title: 'Test',
        part_of: 'folder/index.md',
      }))
      expect(addToContents).toHaveBeenCalledWith('folder/index.md', 'test.md')
    })

    it('should handle file in parent directory', async () => {
      await initializeWorkspaceCrdt(
        'workspace-123',
        '/path',
        null,
        false,
        mockRustApi as any,
        {}
      )

      await addFileToCrdt('folder/index.md/child.md', {
        title: 'Child',
      }, 'folder/index.md')

      expect(addToContents).toHaveBeenCalledWith('folder/index.md', 'child.md')
    })

    it('should add file without parent', async () => {
      await initializeWorkspaceCrdt(
        'workspace-123',
        '/path',
        null,
        false,
        mockRustApi as any,
        {}
      )

      await addFileToCrdt('orphan.md', {
        title: 'Orphan',
      }, null)

      expect(updateFileMetadata).toHaveBeenCalled()
      expect(addToContents).not.toHaveBeenCalled()
    })
  })

  describe('createAttachmentRef', () => {
    it('should create attachment reference from file', () => {
      const mockFile = new File(['content'], 'image.png', { type: 'image/png' })
      Object.defineProperty(mockFile, 'size', { value: 1024 })

      const ref = createAttachmentRef('attachments/image.png', mockFile)

      expect(ref).toEqual({
        path: 'attachments/image.png',
        source: 'local',
        hash: '',
        mime_type: 'image/png',
        size: BigInt(1024),
        uploaded_at: expect.any(BigInt),
        deleted: false,
      })
    })

    it('should handle different file types', () => {
      const pdfFile = new File(['content'], 'doc.pdf', { type: 'application/pdf' })
      Object.defineProperty(pdfFile, 'size', { value: 2048 })

      const ref = createAttachmentRef('docs/doc.pdf', pdfFile)

      expect(ref.mime_type).toBe('application/pdf')
      expect(ref.size).toBe(BigInt(2048))
    })
  })

  describe('getCrdtStats', () => {
    it('should return workspace statistics', async () => {
      const stats = await getCrdtStats()

      expect(stats).toEqual({
        activeFiles: 10,
        totalAttachments: 0,
      })
      expect(getWorkspaceStats).toHaveBeenCalled()
    })
  })
})
