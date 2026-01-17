import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'

// We need to mock dependencies before importing the module
vi.mock('../stores/shareSessionStore.svelte', () => ({
  shareSessionStore: {
    mode: 'idle',
    connected: false,
    isGuest: false,
    joinCode: null,
    setConnecting: vi.fn(),
    setError: vi.fn(),
    startHosting: vi.fn(),
    startGuest: vi.fn(),
    setSessionWs: vi.fn(),
    addPeer: vi.fn(),
    removePeer: vi.fn(),
    setConnected: vi.fn(),
    endSession: vi.fn(),
  },
}))

vi.mock('../stores/collaborationStore.svelte', () => ({
  collaborationStore: {
    collaborationServerUrl: 'https://sync.example.com',
  },
}))

vi.mock('$lib/crdt/workspaceCrdtBridge', () => ({
  startSessionSync: vi.fn(),
  stopSessionSync: vi.fn(),
}))

vi.mock('$lib/crdt/collaborationBridge', () => ({
  setActiveSessionCode: vi.fn(),
}))

import {
  getGuestStoragePath,
  isGuestMode,
  getCurrentJoinCode,
  getSessionSyncUrl,
  cleanupGuestStorage,
  cleanupAllGuestStorage,
} from './shareService'

import { shareSessionStore } from '../stores/shareSessionStore.svelte'

describe('shareService', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  describe('getGuestStoragePath', () => {
    it('should return correct OPFS path for guest storage', () => {
      const joinCode = 'ABC12345-DEF67890'
      const result = getGuestStoragePath(joinCode)
      expect(result).toBe('/guest/ABC12345-DEF67890')
    })

    it('should handle different join codes', () => {
      expect(getGuestStoragePath('XXXXXXXX-YYYYYYYY')).toBe('/guest/XXXXXXXX-YYYYYYYY')
      expect(getGuestStoragePath('12345678-ABCDEFGH')).toBe('/guest/12345678-ABCDEFGH')
    })
  })

  describe('isGuestMode', () => {
    it('should return isGuest from store', () => {
      // Default is false
      expect(isGuestMode()).toBe(false)

      // Mock the store to return true
      Object.defineProperty(shareSessionStore, 'isGuest', {
        get: () => true,
        configurable: true,
      })
      expect(isGuestMode()).toBe(true)

      // Reset
      Object.defineProperty(shareSessionStore, 'isGuest', {
        get: () => false,
        configurable: true,
      })
    })
  })

  describe('getCurrentJoinCode', () => {
    it('should return null when no active session', () => {
      expect(getCurrentJoinCode()).toBeNull()
    })

    it('should return join code when session is active', () => {
      Object.defineProperty(shareSessionStore, 'joinCode', {
        get: () => 'ABC12345-DEF67890',
        configurable: true,
      })
      expect(getCurrentJoinCode()).toBe('ABC12345-DEF67890')

      // Reset
      Object.defineProperty(shareSessionStore, 'joinCode', {
        get: () => null,
        configurable: true,
      })
    })
  })

  describe('getSessionSyncUrl', () => {
    it('should return null when no server URL is set', () => {
      // When currentServerUrl is null (not connected)
      expect(getSessionSyncUrl('test-doc')).toBeNull()
    })
  })

  describe('cleanupGuestStorage', () => {
    it('should handle missing guest folder gracefully', async () => {
      const mockRoot = {
        getDirectoryHandle: vi.fn().mockRejectedValue(new Error('Not found')),
      }

      vi.spyOn(navigator.storage, 'getDirectory').mockResolvedValue(mockRoot as any)

      // Should not throw
      await expect(cleanupGuestStorage('ABC12345-DEF67890')).resolves.toBeUndefined()
    })

    it('should remove session folder from guest storage', async () => {
      const mockRemoveEntry = vi.fn().mockResolvedValue(undefined)
      const mockKeys = vi.fn().mockReturnValue({
        [Symbol.asyncIterator]: async function* () {
          yield 'other-session'
        },
      })

      const mockGuestFolder = {
        removeEntry: mockRemoveEntry,
        keys: mockKeys,
      }

      const mockRoot = {
        getDirectoryHandle: vi.fn().mockResolvedValue(mockGuestFolder),
        removeEntry: vi.fn(),
      }

      vi.spyOn(navigator.storage, 'getDirectory').mockResolvedValue(mockRoot as any)

      await cleanupGuestStorage('ABC12345-DEF67890')

      expect(mockRemoveEntry).toHaveBeenCalledWith('ABC12345-DEF67890', { recursive: true })
    })

    it('should remove empty guest folder', async () => {
      const mockRemoveEntry = vi.fn().mockResolvedValue(undefined)
      const mockKeys = vi.fn().mockReturnValue({
        [Symbol.asyncIterator]: async function* () {
          // Empty - no entries
        },
      })

      const mockGuestFolder = {
        removeEntry: mockRemoveEntry,
        keys: mockKeys,
      }

      const mockRootRemoveEntry = vi.fn().mockResolvedValue(undefined)
      const mockRoot = {
        getDirectoryHandle: vi.fn().mockResolvedValue(mockGuestFolder),
        removeEntry: mockRootRemoveEntry,
      }

      vi.spyOn(navigator.storage, 'getDirectory').mockResolvedValue(mockRoot as any)

      await cleanupGuestStorage('ABC12345-DEF67890')

      expect(mockRootRemoveEntry).toHaveBeenCalledWith('guest')
    })
  })

  describe('cleanupAllGuestStorage', () => {
    it('should remove entire guest folder', async () => {
      const mockRemoveEntry = vi.fn().mockResolvedValue(undefined)
      const mockRoot = {
        removeEntry: mockRemoveEntry,
      }

      vi.spyOn(navigator.storage, 'getDirectory').mockResolvedValue(mockRoot as any)

      await cleanupAllGuestStorage()

      expect(mockRemoveEntry).toHaveBeenCalledWith('guest', { recursive: true })
    })

    it('should handle missing guest folder gracefully', async () => {
      const mockRemoveEntry = vi.fn().mockRejectedValue(new Error('Not found'))
      const mockRoot = {
        removeEntry: mockRemoveEntry,
      }

      vi.spyOn(navigator.storage, 'getDirectory').mockResolvedValue(mockRoot as any)

      // Should not throw
      await expect(cleanupAllGuestStorage()).resolves.toBeUndefined()
    })
  })

  describe('join code validation', () => {
    // These tests verify the join code format (8 chars - 8 chars, alphanumeric)
    it('should accept valid join codes', () => {
      const validCodes = [
        'ABC12345-DEF67890',
        'XXXXXXXX-YYYYYYYY',
        '12345678-ABCDEFGH',
        'abcd1234-efgh5678',
      ]

      const joinCodeRegex = /^[A-Z0-9]{8}-[A-Z0-9]{8}$/i
      for (const code of validCodes) {
        expect(joinCodeRegex.test(code)).toBe(true)
      }
    })

    it('should reject invalid join codes', () => {
      const invalidCodes = [
        'ABC1234-DEF67890', // First part too short
        'ABC12345-DEF6789', // Second part too short
        'ABC12345DEF67890', // Missing hyphen
        'ABC12345-DEF678901', // Second part too long
        'ABC-12345-DEF67890', // Extra hyphen
        '', // Empty
        'ABCDEFGH', // Only first part
      ]

      const joinCodeRegex = /^[A-Z0-9]{8}-[A-Z0-9]{8}$/i
      for (const code of invalidCodes) {
        expect(joinCodeRegex.test(code)).toBe(false)
      }
    })
  })
})
