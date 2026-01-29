import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// Mock WebSocket
class MockWebSocket {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;

  readyState = MockWebSocket.CONNECTING;
  binaryType: string = 'blob';
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;

  constructor(public url: string) {
    // Simulate async connection
    setTimeout(() => {
      this.readyState = MockWebSocket.OPEN;
      this.onopen?.(new Event('open'));
    }, 0);
  }

  send = vi.fn();
  close = vi.fn(() => {
    this.readyState = MockWebSocket.CLOSED;
  });

  // Test helper to simulate receiving a message
  simulateMessage(data: string | ArrayBuffer) {
    this.onmessage?.(new MessageEvent('message', { data }));
  }
}

// Replace global WebSocket
const originalWebSocket = global.WebSocket;
beforeEach(() => {
  (global as any).WebSocket = MockWebSocket;
});
afterEach(() => {
  (global as any).WebSocket = originalWebSocket;
});

// Import after mocking
import { SyncTransport, type SyncTransportOptions } from './syncTransport';

describe('SyncTransport', () => {
  const mockBackend = {
    execute: vi.fn().mockResolvedValue({ type: 'Binary', data: [] }),
  };

  const defaultOptions: SyncTransportOptions = {
    serverUrl: 'wss://sync.example.com/sync',
    docType: 'workspace',
    docName: 'test-workspace',
    backend: mockBackend as any,
    writeToDisk: true,
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('control message handling', () => {
    it('should call onProgress when receiving sync_progress message', async () => {
      const onProgress = vi.fn();
      const transport = new SyncTransport({
        ...defaultOptions,
        onProgress,
      });

      await transport.connect();

      // Wait for connection
      await new Promise(resolve => setTimeout(resolve, 10));

      // Get the mock WebSocket instance
      const ws = (transport as any).ws as MockWebSocket;
      expect(ws).toBeDefined();

      // Simulate receiving sync_progress control message
      ws.simulateMessage(JSON.stringify({
        type: 'sync_progress',
        completed: 5,
        total: 42,
      }));

      expect(onProgress).toHaveBeenCalledWith(5, 42);

      transport.destroy();
    });

    it('should call onSynced when receiving sync_complete message', async () => {
      const onSynced = vi.fn();
      const transport = new SyncTransport({
        ...defaultOptions,
        onSynced,
      });

      await transport.connect();

      // Wait for connection
      await new Promise(resolve => setTimeout(resolve, 10));

      const ws = (transport as any).ws as MockWebSocket;

      // Simulate receiving sync_complete control message
      ws.simulateMessage(JSON.stringify({
        type: 'sync_complete',
        files_synced: 100,
      }));

      expect(onSynced).toHaveBeenCalled();
      expect(transport.isSynced).toBe(true);

      transport.destroy();
    });

    it('should only call onSynced once for multiple sync_complete messages', async () => {
      const onSynced = vi.fn();
      const transport = new SyncTransport({
        ...defaultOptions,
        onSynced,
      });

      await transport.connect();
      await new Promise(resolve => setTimeout(resolve, 10));

      const ws = (transport as any).ws as MockWebSocket;

      // Simulate receiving multiple sync_complete messages
      ws.simulateMessage(JSON.stringify({ type: 'sync_complete', files_synced: 100 }));
      ws.simulateMessage(JSON.stringify({ type: 'sync_complete', files_synced: 100 }));

      expect(onSynced).toHaveBeenCalledTimes(1);

      transport.destroy();
    });

    it('should handle malformed JSON gracefully', async () => {
      const onProgress = vi.fn();
      const consoleWarn = vi.spyOn(console, 'warn').mockImplementation(() => {});

      const transport = new SyncTransport({
        ...defaultOptions,
        onProgress,
      });

      await transport.connect();
      await new Promise(resolve => setTimeout(resolve, 10));

      const ws = (transport as any).ws as MockWebSocket;

      // Simulate receiving malformed JSON
      ws.simulateMessage('not valid json {{{');

      expect(onProgress).not.toHaveBeenCalled();
      expect(consoleWarn).toHaveBeenCalledWith(
        '[SyncTransport] Failed to parse control message:',
        expect.any(Error)
      );

      consoleWarn.mockRestore();
      transport.destroy();
    });

    it('should ignore unknown control message types', async () => {
      const onProgress = vi.fn();
      const onSynced = vi.fn();

      const transport = new SyncTransport({
        ...defaultOptions,
        onProgress,
        onSynced,
      });

      await transport.connect();
      await new Promise(resolve => setTimeout(resolve, 10));

      const ws = (transport as any).ws as MockWebSocket;

      // Simulate receiving unknown control message
      ws.simulateMessage(JSON.stringify({
        type: 'peer_joined',
        guest_id: 'abc123',
        peer_count: 3,
      }));

      // Should not trigger progress or synced callbacks
      expect(onProgress).not.toHaveBeenCalled();
      expect(onSynced).not.toHaveBeenCalled();

      transport.destroy();
    });
  });
});
