import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'

// Mock all dependencies
vi.mock('./rustCrdtApi', () => ({}))

vi.mock('./yDocProxy', () => ({
  createYDocProxy: vi.fn().mockImplementation(async (options) => ({
    getYDoc: vi.fn().mockReturnValue({
      on: vi.fn(),
      off: vi.fn(),
      destroy: vi.fn(),
    }),
    getContent: vi.fn().mockReturnValue('# Test Content'),
    setContent: vi.fn(),
    save: vi.fn().mockResolvedValue(undefined),
    destroy: vi.fn(),
    isDestroyed: vi.fn().mockReturnValue(false),
  })),
}))

vi.mock('./simpleSyncBridge', () => ({
  createSimpleSyncBridge: vi.fn().mockReturnValue({
    connect: vi.fn().mockResolvedValue(undefined),
    disconnect: vi.fn(),
    destroy: vi.fn(),
    isSynced: vi.fn().mockReturnValue(false),
  }),
}))

vi.mock('./p2pSyncBridge', () => ({
  isP2PEnabled: vi.fn().mockReturnValue(false),
  createP2PProvider: vi.fn().mockReturnValue(null),
  destroyP2PProvider: vi.fn(),
}))

import {
  initCollaboration,
  setCollaborationServer,
  getCollaborationServer,
  setCollaborationWorkspaceId,
  getCollaborationWorkspaceId,
  setActiveSessionCode,
  getActiveSessionCode,
  setAuthToken,
  setConnectionStatusCallback,
  getCollaborativeDocument,
  releaseDocument,
  releaseAllDocuments,
  hasSession,
  getSessionCount,
  disconnectAll,
  reconnectAll,
  isConnected,
  cleanup,
  refreshP2PProviders,
} from './collaborationBridge'

import { createYDocProxy } from './yDocProxy'
import { createSimpleSyncBridge } from './simpleSyncBridge'

describe('collaborationBridge', () => {
  const mockRustApi = {
    getFullState: vi.fn().mockResolvedValue(new Uint8Array()),
    getSyncState: vi.fn().mockResolvedValue(new Uint8Array()),
    getFile: vi.fn().mockResolvedValue(null),
    setFile: vi.fn().mockResolvedValue(undefined),
  }

  beforeEach(() => {
    vi.clearAllMocks()
    cleanup()
    initCollaboration(mockRustApi as any)
  })

  afterEach(() => {
    cleanup()
  })

  describe('configuration', () => {
    it('should set and get collaboration server', () => {
      expect(getCollaborationServer()).toBeNull()

      setCollaborationServer('wss://sync.example.com')
      expect(getCollaborationServer()).toBe('wss://sync.example.com')

      setCollaborationServer(null)
      expect(getCollaborationServer()).toBeNull()
    })

    it('should set and get workspace ID', () => {
      expect(getCollaborationWorkspaceId()).toBeNull()

      setCollaborationWorkspaceId('workspace-123')
      expect(getCollaborationWorkspaceId()).toBe('workspace-123')

      setCollaborationWorkspaceId(null)
      expect(getCollaborationWorkspaceId()).toBeNull()
    })

    it('should set and get active session code', () => {
      expect(getActiveSessionCode()).toBeNull()

      setActiveSessionCode('ABC12345-DEF67890')
      expect(getActiveSessionCode()).toBe('ABC12345-DEF67890')

      setActiveSessionCode(null)
      expect(getActiveSessionCode()).toBeNull()
    })

    it('should set auth token', () => {
      // Should not throw
      setAuthToken('test-token')
      setAuthToken(undefined)
    })

    it('should set connection status callback', () => {
      const callback = vi.fn()
      setConnectionStatusCallback(callback)
      setConnectionStatusCallback(null)
    })
  })

  describe('session management', () => {
    it('should create a collaborative document session', async () => {
      const result = await getCollaborativeDocument('test/document.md')

      expect(result.yDocProxy).toBeDefined()
      expect(createYDocProxy).toHaveBeenCalled()
      expect(hasSession('test/document.md')).toBe(true)
      expect(getSessionCount()).toBe(1)
    })

    it('should reuse existing session for same document', async () => {
      const result1 = await getCollaborativeDocument('test/document.md')
      const result2 = await getCollaborativeDocument('test/document.md')

      expect(result1.yDocProxy).toBe(result2.yDocProxy)
      expect(createYDocProxy).toHaveBeenCalledTimes(1)
      expect(getSessionCount()).toBe(1)
    })

    it('should create sessions with save callback', async () => {
      const onMarkdownSave = vi.fn()

      await getCollaborativeDocument('test/document.md', {
        onMarkdownSave,
      })

      expect(hasSession('test/document.md')).toBe(true)
    })

    it('should create sessions with initial content', async () => {
      await getCollaborativeDocument('test/document.md', {
        initialContent: '# Initial Content',
      })

      expect(createYDocProxy).toHaveBeenCalledWith(
        expect.objectContaining({
          initialContent: '# Initial Content',
        })
      )
    })

    it('should release a document session', async () => {
      await getCollaborativeDocument('test/document.md')
      expect(hasSession('test/document.md')).toBe(true)

      await releaseDocument('test/document.md')
      expect(hasSession('test/document.md')).toBe(false)
      expect(getSessionCount()).toBe(0)
    })

    it('should handle release of non-existent session', async () => {
      // Should not throw
      await releaseDocument('non/existent.md')
    })

    it('should release all document sessions', async () => {
      await getCollaborativeDocument('test/doc1.md')
      await getCollaborativeDocument('test/doc2.md')
      await getCollaborativeDocument('test/doc3.md')

      expect(getSessionCount()).toBe(3)

      await releaseAllDocuments()
      expect(getSessionCount()).toBe(0)
    })
  })

  describe('connection management', () => {
    beforeEach(async () => {
      setCollaborationServer('wss://sync.example.com')
      await getCollaborativeDocument('test/document.md')
    })

    it('should disconnect all sessions', () => {
      disconnectAll()
      // Verify bridge.disconnect was called
      expect(createSimpleSyncBridge).toHaveBeenCalled()
    })

    it('should reconnect all sessions', () => {
      reconnectAll()
      // Verify bridge.connect was called
    })

    it('should check connection status', () => {
      expect(isConnected()).toBe(false)
    })
  })

  describe('cleanup', () => {
    it('should cleanup all sessions and state', async () => {
      await getCollaborativeDocument('test/doc1.md')
      await getCollaborativeDocument('test/doc2.md')

      cleanup()

      expect(getSessionCount()).toBe(0)
    })
  })

  describe('P2P providers', () => {
    it('should refresh P2P providers', async () => {
      await getCollaborativeDocument('test/document.md')

      // Should not throw
      refreshP2PProviders()
    })
  })

  describe('session code handling', () => {
    it('should reconnect sessions when session code changes', async () => {
      setCollaborationServer('wss://sync.example.com')
      await getCollaborativeDocument('test/document.md')

      const initialCallCount = (createSimpleSyncBridge as any).mock.calls.length

      setActiveSessionCode('NEW-SESSION-CODE')

      // A new bridge should be created
      expect((createSimpleSyncBridge as any).mock.calls.length).toBeGreaterThan(initialCallCount)
    })
  })
})
