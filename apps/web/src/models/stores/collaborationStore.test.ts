import { describe, it, expect, beforeEach } from 'vitest'
import { getCollaborationStore } from './collaborationStore.svelte'

describe('collaborationStore', () => {
  let store: ReturnType<typeof getCollaborationStore>

  beforeEach(() => {
    store = getCollaborationStore()
    // Reset state
    store.clearCollaborationSession()
    store.setEnabled(false)
    store.setConnected(false)
    store.setServerUrl(null)
  })

  describe('Y.js document management', () => {
    it('should initialize with null Y.Doc', () => {
      expect(store.currentYDoc).toBeNull()
    })

    it('should set Y.Doc', () => {
      const mockYDoc = { guid: 'test-doc' }
      store.setYDoc(mockYDoc as any)
      expect(store.currentYDoc).toStrictEqual(mockYDoc)
    })

    it('should clear Y.Doc', () => {
      const mockYDoc = { guid: 'test-doc' }
      store.setYDoc(mockYDoc as any)
      store.setYDoc(null)
      expect(store.currentYDoc).toBeNull()
    })
  })

  describe('provider management', () => {
    it('should initialize with null provider', () => {
      expect(store.currentProvider).toBeNull()
    })

    it('should set provider', () => {
      const mockProvider = { on: () => {} }
      store.setProvider(mockProvider as any)
      expect(store.currentProvider).toStrictEqual(mockProvider)
    })

    it('should clear provider', () => {
      const mockProvider = { on: () => {} }
      store.setProvider(mockProvider as any)
      store.setProvider(null)
      expect(store.currentProvider).toBeNull()
    })
  })

  describe('collaboration path', () => {
    it('should initialize with null path', () => {
      expect(store.currentCollaborationPath).toBeNull()
    })

    it('should set collaboration path', () => {
      store.setCollaborationPath('workspace/entry.md')
      expect(store.currentCollaborationPath).toBe('workspace/entry.md')
    })

    it('should clear collaboration path', () => {
      store.setCollaborationPath('workspace/entry.md')
      store.setCollaborationPath(null)
      expect(store.currentCollaborationPath).toBeNull()
    })
  })

  describe('collaboration session', () => {
    it('should set collaboration session with all parameters', () => {
      const mockYDoc = { guid: 'test-doc' }
      const mockProvider = { on: () => {} }
      const path = 'workspace/entry.md'

      store.setCollaborationSession(mockYDoc as any, mockProvider as any, path)

      expect(store.currentYDoc).toStrictEqual(mockYDoc)
      expect(store.currentProvider).toStrictEqual(mockProvider)
      expect(store.currentCollaborationPath).toBe(path)
    })

    it('should clear collaboration session', () => {
      const mockYDoc = { guid: 'test-doc' }
      const mockProvider = { on: () => {} }

      store.setCollaborationSession(mockYDoc as any, mockProvider as any, 'path')
      store.clearCollaborationSession()

      expect(store.currentYDoc).toBeNull()
      expect(store.currentProvider).toBeNull()
      expect(store.currentCollaborationPath).toBeNull()
    })
  })

  describe('connection status', () => {
    it('should initialize with disabled collaboration', () => {
      expect(store.collaborationEnabled).toBe(false)
    })

    it('should set enabled state', () => {
      store.setEnabled(true)
      expect(store.collaborationEnabled).toBe(true)

      store.setEnabled(false)
      expect(store.collaborationEnabled).toBe(false)
    })

    it('should initialize as disconnected', () => {
      expect(store.collaborationConnected).toBe(false)
    })

    it('should set connected state', () => {
      store.setConnected(true)
      expect(store.collaborationConnected).toBe(true)

      store.setConnected(false)
      expect(store.collaborationConnected).toBe(false)
    })
  })

  describe('server URL', () => {
    it('should initialize with null server URL', () => {
      expect(store.collaborationServerUrl).toBeNull()
    })

    it('should set server URL', () => {
      store.setServerUrl('wss://sync.example.com')
      expect(store.collaborationServerUrl).toBe('wss://sync.example.com')
    })

    it('should persist server URL to localStorage', () => {
      store.setServerUrl('wss://sync.example.com')
      expect(localStorage.setItem).toHaveBeenCalledWith(
        'diaryx-sync-server',
        'wss://sync.example.com'
      )
    })

    it('should remove from localStorage when setting to null', () => {
      store.setServerUrl('wss://sync.example.com')
      store.setServerUrl(null)
      expect(localStorage.removeItem).toHaveBeenCalledWith('diaryx-sync-server')
    })
  })
})
