/**
 * Simple Y.js Sync Server with Session Management
 *
 * Protocol:
 * 1. Create session: ?action=create&workspaceId=X
 *    - Returns JSON: { type: 'session_created', joinCode: 'XXXX-XXXX' }
 *
 * 2. Join session: ?action=join&code=XXXX-XXXX
 *    - Returns JSON: { type: 'session_joined', workspaceId: 'X' }
 *
 * 3. Sync document: ?doc=documentName[&session=XXXX-XXXX]
 *    - Send raw Y.js updates
 *    - Server broadcasts to all other clients on same document
 *    - If session provided, document is scoped to that session
 */

import { WebSocketServer, WebSocket, type RawData } from 'ws';
import * as Y from 'yjs';
import { createServer, type IncomingMessage } from 'http';
import { randomBytes } from 'crypto';

const PORT = parseInt(process.env.PORT || '1234');

// ============================================================================
// Types
// ============================================================================

interface Session {
  joinCode: string;
  workspaceId: string;
  ownerId: string;
  ownerWs: WebSocket | null;
  guests: Map<string, WebSocket>; // guestId -> ws
  documents: Map<string, Y.Doc>;
  connections: Map<string, Set<WebSocket>>; // docName -> connections
  createdAt: Date;
}

interface ClientInfo {
  ws: WebSocket;
  docName: string;
  sessionCode: string | null;
}

// ============================================================================
// State
// ============================================================================

// Session storage: joinCode -> Session
const sessions = new Map<string, Session>();

// Global document storage (for non-session documents): docName -> Y.Doc
const globalDocuments = new Map<string, Y.Doc>();

// Global connections (for non-session documents): docName -> Set<WebSocket>
const globalConnections = new Map<string, Set<WebSocket>>();

// Track client info for cleanup
const clientInfoMap = new WeakMap<WebSocket, ClientInfo>();

// ============================================================================
// Join Code Generation
// ============================================================================

function generateJoinCode(): string {
  // Format: XXXXXXXX-XXXXXXXX (uppercase alphanumeric)
  const chars = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789'; // Exclude confusing chars: I, O, 0, 1
  let code = '';
  const bytes = randomBytes(16);
  for (let i = 0; i < 16; i++) {
    code += chars[bytes[i] % chars.length];
    if (i === 7) code += '-';
  }
  return code;
}

function validateJoinCode(code: string): boolean {
  return /^[A-Z0-9]{8}-[A-Z0-9]{8}$/.test(code);
}

// ============================================================================
// Document Management
// ============================================================================

function getOrCreateSessionDoc(session: Session, docName: string): Y.Doc {
  let doc = session.documents.get(docName);
  if (!doc) {
    doc = new Y.Doc();
    session.documents.set(docName, doc);
    console.log(`[Server] Created session doc: ${session.joinCode}/${docName}`);
  }
  return doc;
}

function getOrCreateGlobalDoc(docName: string): Y.Doc {
  let doc = globalDocuments.get(docName);
  if (!doc) {
    doc = new Y.Doc();
    globalDocuments.set(docName, doc);
    console.log(`[Server] Created global document: ${docName}`);
  }
  return doc;
}

function addSessionConnection(session: Session, docName: string, ws: WebSocket): void {
  let conns = session.connections.get(docName);
  if (!conns) {
    conns = new Set();
    session.connections.set(docName, conns);
  }
  conns.add(ws);
  console.log(`[Server] Client joined session doc ${session.joinCode}/${docName} (${conns.size} clients)`);
}

function addGlobalConnection(docName: string, ws: WebSocket): void {
  let conns = globalConnections.get(docName);
  if (!conns) {
    conns = new Set();
    globalConnections.set(docName, conns);
  }
  conns.add(ws);
  console.log(`[Server] Client joined global doc ${docName} (${conns.size} clients)`);
}

function removeConnection(ws: WebSocket): void {
  const info = clientInfoMap.get(ws);
  if (!info) return;

  if (info.sessionCode) {
    const session = sessions.get(info.sessionCode);
    if (session) {
      const conns = session.connections.get(info.docName);
      if (conns) {
        conns.delete(ws);
        console.log(`[Server] Client left session doc ${info.sessionCode}/${info.docName} (${conns.size} clients)`);
        if (conns.size === 0) {
          session.connections.delete(info.docName);
        }
      }
      // Check if this was the owner
      if (session.ownerWs === ws) {
        session.ownerWs = null;
      }
      // Remove from guests
      for (const [guestId, guestWs] of session.guests) {
        if (guestWs === ws) {
          session.guests.delete(guestId);
          break;
        }
      }
    }
  } else {
    const conns = globalConnections.get(info.docName);
    if (conns) {
      conns.delete(ws);
      console.log(`[Server] Client left global doc ${info.docName} (${conns.size} clients)`);
      if (conns.size === 0) {
        globalConnections.delete(info.docName);
      }
    }
  }
}

function broadcast(docName: string, sessionCode: string | null, update: Uint8Array, sender: WebSocket): void {
  let conns: Set<WebSocket> | undefined;

  if (sessionCode) {
    const session = sessions.get(sessionCode);
    conns = session?.connections.get(docName);
  } else {
    conns = globalConnections.get(docName);
  }

  if (!conns) return;

  let sent = 0;
  for (const ws of conns) {
    if (ws !== sender && ws.readyState === WebSocket.OPEN) {
      ws.send(update);
      sent++;
    }
  }
  const scope = sessionCode ? `session ${sessionCode}/` : 'global/';
  console.log(`[Server] Broadcast ${update.length} bytes to ${sent} clients on ${scope}${docName}`);
}

// ============================================================================
// Session Management
// ============================================================================

function createSession(workspaceId: string, ownerId: string, ownerWs: WebSocket): Session {
  const joinCode = generateJoinCode();
  const session: Session = {
    joinCode,
    workspaceId,
    ownerId,
    ownerWs,
    guests: new Map(),
    documents: new Map(),
    connections: new Map(),
    createdAt: new Date(),
  };
  sessions.set(joinCode, session);
  console.log(`[Server] Created session ${joinCode} for workspace ${workspaceId}`);
  return session;
}

function joinSession(joinCode: string, guestId: string, guestWs: WebSocket): Session | null {
  const session = sessions.get(joinCode);
  if (!session) {
    console.log(`[Server] Session not found: ${joinCode}`);
    return null;
  }
  session.guests.set(guestId, guestWs);
  console.log(`[Server] Guest ${guestId} joined session ${joinCode}`);
  return session;
}

function getSessionPeerCount(session: Session): number {
  let count = session.ownerWs ? 1 : 0;
  count += session.guests.size;
  return count;
}

// ============================================================================
// HTTP Server
// ============================================================================

const server = createServer((req, res) => {
  // CORS headers for browser access
  res.setHeader('Access-Control-Allow-Origin', '*');
  res.setHeader('Access-Control-Allow-Methods', 'GET, OPTIONS');

  if (req.method === 'OPTIONS') {
    res.writeHead(204);
    res.end();
    return;
  }

  // Status endpoint
  if (req.url === '/status') {
    const status = {
      sessions: sessions.size,
      globalDocs: globalDocuments.size,
      uptime: process.uptime(),
    };
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify(status));
    return;
  }

  res.writeHead(200, { 'Content-Type': 'text/plain' });
  res.end('Diaryx Sync Server\n');
});

// ============================================================================
// WebSocket Server
// ============================================================================

const wss = new WebSocketServer({ server });

wss.on('connection', (ws: WebSocket, req: IncomingMessage) => {
  const url = new URL(req.url || '', `http://${req.headers.host}`);
  const action = url.searchParams.get('action');
  const docName = url.searchParams.get('doc');
  const sessionCode = url.searchParams.get('session');

  // Handle session creation
  if (action === 'create') {
    const workspaceId = url.searchParams.get('workspaceId');
    const ownerId = url.searchParams.get('ownerId') || 'unknown';

    if (!workspaceId) {
      ws.send(JSON.stringify({ type: 'error', message: 'Missing workspaceId parameter' }));
      ws.close(4000, 'Missing workspaceId');
      return;
    }

    const session = createSession(workspaceId, ownerId, ws);
    ws.send(JSON.stringify({
      type: 'session_created',
      joinCode: session.joinCode,
      workspaceId: session.workspaceId,
    }));

    console.log(`[Server] Owner connected to session ${session.joinCode}`);

    // Handle session control messages
    ws.on('message', (data: RawData) => {
      try {
        const msg = JSON.parse(data.toString());
        if (msg.type === 'get_peers') {
          ws.send(JSON.stringify({
            type: 'peers',
            count: getSessionPeerCount(session),
          }));
        }
      } catch {
        // Not JSON, ignore
      }
    });

    ws.on('close', () => {
      console.log(`[Server] Owner disconnected from session ${session.joinCode}`);
      session.ownerWs = null;
      // Don't delete session - guests might still be connected
    });

    return;
  }

  // Handle session join
  if (action === 'join') {
    const code = url.searchParams.get('code');
    const guestId = url.searchParams.get('guestId') || 'guest-' + Date.now();

    if (!code || !validateJoinCode(code)) {
      ws.send(JSON.stringify({ type: 'error', message: 'Invalid join code' }));
      ws.close(4001, 'Invalid join code');
      return;
    }

    const session = joinSession(code, guestId, ws);
    if (!session) {
      ws.send(JSON.stringify({ type: 'error', message: 'Session not found' }));
      ws.close(4002, 'Session not found');
      return;
    }

    ws.send(JSON.stringify({
      type: 'session_joined',
      joinCode: session.joinCode,
      workspaceId: session.workspaceId,
    }));

    // Notify owner if connected
    if (session.ownerWs && session.ownerWs.readyState === WebSocket.OPEN) {
      session.ownerWs.send(JSON.stringify({
        type: 'peer_joined',
        guestId,
        peerCount: getSessionPeerCount(session),
      }));
    }

    ws.on('close', () => {
      console.log(`[Server] Guest ${guestId} disconnected from session ${code}`);
      session.guests.delete(guestId);
      // Notify owner
      if (session.ownerWs && session.ownerWs.readyState === WebSocket.OPEN) {
        session.ownerWs.send(JSON.stringify({
          type: 'peer_left',
          guestId,
          peerCount: getSessionPeerCount(session),
        }));
      }
    });

    return;
  }

  // Handle document sync
  if (docName) {
    let doc: Y.Doc;
    let effectiveSessionCode: string | null = null;

    if (sessionCode) {
      // Session-scoped document
      if (!validateJoinCode(sessionCode)) {
        ws.close(4003, 'Invalid session code');
        return;
      }

      const session = sessions.get(sessionCode);
      if (!session) {
        ws.close(4004, 'Session not found');
        return;
      }

      doc = getOrCreateSessionDoc(session, docName);
      addSessionConnection(session, docName, ws);
      effectiveSessionCode = sessionCode;
    } else {
      // Global document
      doc = getOrCreateGlobalDoc(docName);
      addGlobalConnection(docName, ws);
    }

    // Track client info for cleanup
    clientInfoMap.set(ws, { ws, docName, sessionCode: effectiveSessionCode });

    // Send current state to new client
    const state = Y.encodeStateAsUpdate(doc);
    if (state.length > 0) {
      console.log(`[Server] Sending initial state: ${state.length} bytes`);
      ws.send(state);
    }

    // Handle incoming messages
    ws.on('message', (data: RawData) => {
      try {
        const update = new Uint8Array(data as ArrayBuffer);
        console.log(`[Server] Received update: ${update.length} bytes on ${docName}`);

        // Apply to server's Y.Doc
        Y.applyUpdate(doc, update);

        // Broadcast to other clients
        broadcast(docName, effectiveSessionCode, update, ws);
      } catch (err) {
        console.error('[Server] Error processing update:', err);
      }
    });

    ws.on('close', () => {
      removeConnection(ws);
    });

    ws.on('error', (err: Error) => {
      console.error('[Server] WebSocket error:', err);
      removeConnection(ws);
    });

    return;
  }

  // Invalid request
  console.log('[Server] Client connected without valid parameters, closing');
  ws.close(4000, 'Missing required parameters (doc, action)');
});

// ============================================================================
// Server Lifecycle
// ============================================================================

server.listen(PORT, () => {
  console.log(`[Server] Diaryx Sync Server running on port ${PORT}`);
  console.log(`[Server] Endpoints:`);
  console.log(`  - Create session: ws://localhost:${PORT}?action=create&workspaceId=X`);
  console.log(`  - Join session:   ws://localhost:${PORT}?action=join&code=XXXX-XXXX`);
  console.log(`  - Sync document:  ws://localhost:${PORT}?doc=name[&session=code]`);
  console.log(`  - Status:         http://localhost:${PORT}/status`);
});

// Graceful shutdown
function shutdown() {
  console.log('[Server] Shutting down...');

  // Close all WebSocket connections
  wss.clients.forEach((ws) => {
    ws.close(1001, 'Server shutting down');
  });

  wss.close();
  server.close();
  process.exit(0);
}

process.on('SIGTERM', shutdown);
process.on('SIGINT', shutdown);

// Periodic cleanup of stale sessions (no connections for 1 hour)
setInterval(() => {
  const now = Date.now();
  const staleThreshold = 60 * 60 * 1000; // 1 hour

  for (const [code, session] of sessions) {
    const hasConnections = session.ownerWs || session.guests.size > 0;
    const age = now - session.createdAt.getTime();

    if (!hasConnections && age > staleThreshold) {
      console.log(`[Server] Cleaning up stale session: ${code}`);
      sessions.delete(code);
    }
  }
}, 10 * 60 * 1000); // Check every 10 minutes
