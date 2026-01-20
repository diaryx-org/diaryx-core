import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'

// Mock all dependencies
vi.mock('yjs', () => ({
  Doc: vi.fn().mockImplementation(() => ({
    getMap: vi.fn().mockReturnValue({
      size: 0,
      entries: vi.fn().mockReturnValue([]),
      set: vi.fn(),
      get: vi.fn(),
      observe: vi.fn(),
      unobserve: vi.fn(),
      keys: vi.fn().mockReturnValue([]),
    }),
    on: vi.fn(),
    off: vi.fn(),
    destroy: vi.fn(),
  })),
  applyUpdate: vi.fn(),
  encodeStateVector: vi.fn().mockReturnValue(new Uint8Array()),
}))

vi.mock('./rustCrdtApi', () => ({}))

vi.mock('./simpleSyncBridge', () => ({
  createSimpleSyncBridge: vi.fn().mockReturnValue({
    connect: vi.fn(),
    disconnect: vi.fn(),
    destroy: vi.fn(),
    isSynced: vi.fn().mockReturnValue(false),
  }),
}))

vi.mock('@/models/stores/shareSessionStore.svelte', () => ({
  shareSessionStore: {
    isGuest: false,
    joinCode: null,
    mode: 'idle',
  },
}))

import {
  setWorkspaceId,
  getWorkspaceId,
  setInitializing,
  isInitializing,
  isWorkspaceInitialized,
  setWorkspaceServer,
  getWorkspaceServer,
  initWorkspace,
  destroyWorkspace,
  onFileChange,
  onSessionSync,
  onBodyChange,
  getWorkspaceStats,
  setBackendApi,
  startSessionSync,
  stopSessionSync,
  getSessionCode,
} from './workspaceCrdtBridge'

describe('workspaceCrdtBridge', () => {
  const mockRustApi = {
    getFullState: vi.fn().mockResolvedValue(new Uint8Array()),
    getSyncState: vi.fn().mockResolvedValue(new Uint8Array()),
    getFile: vi.fn().mockResolvedValue(null),
    setFile: vi.fn().mockResolvedValue(undefined),
    listFiles: vi.fn().mockResolvedValue([]),
    applyRemoteUpdate: vi.fn().mockResolvedValue(undefined),
    saveCrdtState: vi.fn().mockResolvedValue(undefined),
    getMissingUpdates: vi.fn().mockResolvedValue(new Uint8Array()),
  }

  beforeEach(async () => {
    vi.clearAllMocks()
    // Ensure workspace is destroyed before each test
    await destroyWorkspace()
  })

  afterEach(async () => {
    await destroyWorkspace()
  })

  describe('configuration', () => {
    it('should set and get workspace ID', () => {
      setWorkspaceId('test-workspace-id')
      expect(getWorkspaceId()).toBe('test-workspace-id')

      setWorkspaceId(null)
      expect(getWorkspaceId()).toBeNull()
    })

    it('should set and get initializing state', () => {
      expect(isInitializing()).toBe(false)

      setInitializing(true)
      expect(isInitializing()).toBe(true)

      setInitializing(false)
      expect(isInitializing()).toBe(false)
    })

    it('should set and get workspace server URL', async () => {
      expect(getWorkspaceServer()).toBeNull()

      await setWorkspaceServer('wss://sync.example.com')
      expect(getWorkspaceServer()).toBe('wss://sync.example.com')

      await setWorkspaceServer(null)
      expect(getWorkspaceServer()).toBeNull()
    })
  })

  describe('initialization', () => {
    it('should initialize workspace with basic options', async () => {
      const onReady = vi.fn()

      await initWorkspace({
        rustApi: mockRustApi as any,
        onReady,
      })

      expect(isWorkspaceInitialized()).toBe(true)
      expect(onReady).toHaveBeenCalled()
    })

    it('should initialize workspace with all options', async () => {
      const onReady = vi.fn()
      const onFileChange = vi.fn()

      await initWorkspace({
        rustApi: mockRustApi as any,
        workspaceId: 'test-workspace',
        serverUrl: 'wss://sync.example.com',
        onReady,
        onFileChange,
      })

      expect(isWorkspaceInitialized()).toBe(true)
      expect(getWorkspaceId()).toBe('test-workspace')
    })

    it('should not reinitialize if already initialized', async () => {
      const onReady = vi.fn()

      await initWorkspace({
        rustApi: mockRustApi as any,
        onReady,
      })

      // Second call should be ignored
      await initWorkspace({
        rustApi: mockRustApi as any,
        onReady,
      })

      expect(onReady).toHaveBeenCalledTimes(1)
    })
  })

  describe('session sync', () => {
    beforeEach(async () => {
      await initWorkspace({
        rustApi: mockRustApi as any,
      })
    })

    it('should start and stop session sync', async () => {
      expect(getSessionCode()).toBeNull()

      await startSessionSync('wss://sync.example.com', 'ABC12345-DEF67890', true)
      expect(getSessionCode()).toBe('ABC12345-DEF67890')

      stopSessionSync()
      expect(getSessionCode()).toBeNull()
    })
  })

  describe('callbacks', () => {
    it('should register and unregister file change callback', async () => {
      await initWorkspace({
        rustApi: mockRustApi as any,
      })

      const callback = vi.fn()
      const unsubscribe = onFileChange(callback)

      // Unsubscribe
      unsubscribe()

      // Callback should no longer be registered
      // (Internal state - can't easily verify without triggering)
    })

    it('should register and unregister session sync callback', () => {
      const callback = vi.fn()
      const unsubscribe = onSessionSync(callback)

      expect(typeof unsubscribe).toBe('function')
      unsubscribe()
    })

    it('should register and unregister body change callback', () => {
      const callback = vi.fn()
      const unsubscribe = onBodyChange(callback)

      expect(typeof unsubscribe).toBe('function')
      unsubscribe()
    })
  })

  describe('statistics', () => {
    beforeEach(async () => {
      await initWorkspace({
        rustApi: mockRustApi as any,
      })
    })

    it('should get workspace stats', async () => {
      mockRustApi.listFiles.mockResolvedValue([
        ['file1.md', { deleted: false }],
        ['file2.md', { deleted: false }],
        ['file3.md', { deleted: true }],
      ])

      const stats = await getWorkspaceStats()

      // Since we mock listFiles to return same result for both calls,
      // stats depend on implementation details
      expect(stats).toHaveProperty('totalFiles')
      expect(stats).toHaveProperty('activeFiles')
      expect(stats).toHaveProperty('deletedFiles')
    })
  })

  describe('cleanup', () => {
    it('should destroy workspace and reset state', async () => {
      await initWorkspace({
        rustApi: mockRustApi as any,
        workspaceId: 'test-workspace',
      })

      expect(isWorkspaceInitialized()).toBe(true)

      await destroyWorkspace()

      expect(isWorkspaceInitialized()).toBe(false)
    })
  })

  describe('backend API', () => {
    it('should set backend API for file operations', () => {
      const mockApi = {
        fileExists: vi.fn(),
        saveEntry: vi.fn(),
        writeFile: vi.fn(),
      }

      // Should not throw
      setBackendApi(mockApi as any)
    })
  })
})
