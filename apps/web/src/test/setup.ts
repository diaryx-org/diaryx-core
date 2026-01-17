import '@testing-library/jest-dom/vitest'
import { vi, beforeEach, afterEach } from 'vitest'

// ============================================================================
// Mock Browser APIs
// ============================================================================

// Mock URL.createObjectURL and URL.revokeObjectURL
global.URL.createObjectURL = vi.fn(() => 'blob:mock-url')
global.URL.revokeObjectURL = vi.fn()

// Mock localStorage
const localStorageMock = (() => {
  let store: Record<string, string> = {}
  return {
    getItem: vi.fn((key: string) => store[key] ?? null),
    setItem: vi.fn((key: string, value: string) => {
      store[key] = value
    }),
    removeItem: vi.fn((key: string) => {
      delete store[key]
    }),
    clear: vi.fn(() => {
      store = {}
    }),
    get length() {
      return Object.keys(store).length
    },
    key: vi.fn((index: number) => Object.keys(store)[index] ?? null),
  }
})()

Object.defineProperty(global, 'localStorage', {
  value: localStorageMock,
})

// Mock navigator.storage for OPFS
const storageMock = {
  getDirectory: vi.fn().mockResolvedValue({
    getDirectoryHandle: vi.fn().mockRejectedValue(new Error('Not found')),
    getFileHandle: vi.fn().mockRejectedValue(new Error('Not found')),
    removeEntry: vi.fn().mockResolvedValue(undefined),
  }),
  estimate: vi.fn().mockResolvedValue({ usage: 0, quota: 1000000000 }),
  persist: vi.fn().mockResolvedValue(true),
  persisted: vi.fn().mockResolvedValue(false),
}

Object.defineProperty(global.navigator, 'storage', {
  value: storageMock,
  writable: true,
})

// Mock WebSocket
class MockWebSocket {
  static CONNECTING = 0
  static OPEN = 1
  static CLOSING = 2
  static CLOSED = 3

  url: string
  readyState: number = MockWebSocket.CONNECTING
  onopen: ((event: Event) => void) | null = null
  onclose: ((event: CloseEvent) => void) | null = null
  onmessage: ((event: MessageEvent) => void) | null = null
  onerror: ((event: Event) => void) | null = null

  constructor(url: string) {
    this.url = url
    // Simulate async connection
    setTimeout(() => {
      this.readyState = MockWebSocket.OPEN
      if (this.onopen) {
        this.onopen(new Event('open'))
      }
    }, 0)
  }

  send = vi.fn()
  close = vi.fn(() => {
    this.readyState = MockWebSocket.CLOSED
    if (this.onclose) {
      this.onclose(new CloseEvent('close'))
    }
  })
}

global.WebSocket = MockWebSocket as unknown as typeof WebSocket

// ============================================================================
// Mock Backend
// ============================================================================

export const mockBackend = {
  init: vi.fn().mockResolvedValue(undefined),
  execute: vi.fn().mockResolvedValue({ type: 'Ok' }),
  readBinary: vi.fn().mockResolvedValue(new Uint8Array()),
  writeBinary: vi.fn().mockResolvedValue(undefined),
  persist: vi.fn().mockResolvedValue(undefined),
}

vi.mock('$lib/backend', () => ({
  backend: mockBackend,
  isTauri: () => false,
}))

// ============================================================================
// Mock svelte-sonner (toast library)
// ============================================================================

vi.mock('svelte-sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    warning: vi.fn(),
    info: vi.fn(),
    loading: vi.fn(() => 'mock-toast-id'),
    dismiss: vi.fn(),
  },
}))

// ============================================================================
// Mock heic2any
// ============================================================================

vi.mock('heic2any', () => ({
  default: vi.fn().mockResolvedValue(new Blob(['mock-jpeg'], { type: 'image/jpeg' })),
}))

// ============================================================================
// Test Lifecycle Hooks
// ============================================================================

beforeEach(() => {
  // Clear all mocks before each test
  vi.clearAllMocks()
  localStorageMock.clear()
})

afterEach(() => {
  // Clean up after each test
})

// ============================================================================
// Test Utilities
// ============================================================================

/**
 * Create a mock API response for testing
 */
export function createMockResponse<T extends { type: string }>(response: T): T {
  return response
}

/**
 * Create a mock entry for testing
 */
export function createMockEntry(overrides: Partial<{
  path: string
  title: string
  content: string
  frontmatter: Record<string, unknown>
}> = {}) {
  return {
    path: overrides.path ?? 'test/entry.md',
    title: overrides.title ?? 'Test Entry',
    content: overrides.content ?? '# Test Content\n\nSome test content.',
    frontmatter: {
      title: overrides.title ?? 'Test Entry',
      ...overrides.frontmatter,
    },
  }
}

/**
 * Create a mock tree node for testing
 */
export function createMockTreeNode(overrides: Partial<{
  path: string
  name: string
  title: string
  is_index: boolean
  children: any[]
}> = {}) {
  return {
    path: overrides.path ?? 'test',
    name: overrides.name ?? 'test',
    title: overrides.title ?? 'Test',
    is_index: overrides.is_index ?? false,
    children: overrides.children ?? [],
  }
}
