/**
 * Y.js collaboration utilities for Diaryx.
 *
 * Manages Y.Doc instances, Hocuspocus provider connections,
 * offline persistence, and markdown synchronization.
 */

import * as Y from "yjs";
import { HocuspocusProvider } from "@hocuspocus/provider";
import { IndexeddbPersistence } from "y-indexeddb";

// Store for active collaborations (keyed by document path)
interface CollaborationSession {
  ydoc: Y.Doc;
  provider: HocuspocusProvider;
  persistence: IndexeddbPersistence;
  saveTimeout: ReturnType<typeof setTimeout> | null;
  onMarkdownSave?: (markdown: string) => void;
}

const sessions = new Map<string, CollaborationSession>();

// Default configuration
let serverUrl = "ws://localhost:1234";
const SAVE_DEBOUNCE_MS = 5000; // 5 seconds debounce for markdown saves

/**
 * Configure the collaboration server URL.
 */
export function setCollaborationServer(url: string): void {
  serverUrl = url;
}

/**
 * Get current user info for collaborative cursors.
 * Can be customized per-session.
 */
export function getUserInfo(): { name: string; color: string } {
  // Try to get from localStorage or use defaults
  const stored =
    typeof window !== "undefined"
      ? localStorage.getItem("diaryx-user-info")
      : null;

  if (stored) {
    try {
      return JSON.parse(stored);
    } catch {
      // Fall through to default
    }
  }

  // Generate random color for this user
  const colors = [
    "#958DF1",
    "#F97583",
    "#79C0FF",
    "#A5D6FF",
    "#7EE787",
    "#FFA657",
    "#FF7B72",
    "#D2A8FF",
  ];
  const color = colors[Math.floor(Math.random() * colors.length)];
  const name = "User " + Math.floor(Math.random() * 1000);

  return { name, color };
}

/**
 * Set user info for collaborative cursors.
 */
export function setUserInfo(name: string, color: string): void {
  if (typeof window !== "undefined") {
    localStorage.setItem("diaryx-user-info", JSON.stringify({ name, color }));
  }

  // Update awareness in all active sessions
  for (const session of sessions.values()) {
    const awareness = session.provider.awareness;
    if (awareness) {
      awareness.setLocalStateField("user", { name, color });
    }
  }
}

/**
 * Get or create a collaborative document session.
 * Includes offline persistence and change tracking.
 */
export function getCollaborativeDocument(
  documentPath: string,
  options?: {
    onMarkdownSave?: (markdown: string) => void;
    initialContent?: string;
  },
): {
  ydoc: Y.Doc;
  provider: HocuspocusProvider;
} {
  // Check if we already have this session
  const existing = sessions.get(documentPath);
  if (existing) {
    // Update callback if provided
    if (options?.onMarkdownSave) {
      existing.onMarkdownSave = options.onMarkdownSave;
    }
    return { ydoc: existing.ydoc, provider: existing.provider };
  }

  // Create new Y.Doc
  const ydoc = new Y.Doc();

  // Create IndexedDB persistence for offline support
  // This persists the Y.Doc state locally so it survives page refreshes
  const persistence = new IndexeddbPersistence(`diaryx-${documentPath}`, ydoc);

  persistence.on("synced", () => {
    console.log(`[Y.js] IndexedDB synced for ${documentPath}`);
  });

  // Create Hocuspocus provider for real-time sync
  const userInfo = getUserInfo();
  const provider = new HocuspocusProvider({
    url: serverUrl,
    name: documentPath,
    document: ydoc,
    onConnect: () => {
      console.log(`[Y.js] Connected to ${documentPath}`);
    },
    onDisconnect: () => {
      console.log(`[Y.js] Disconnected from ${documentPath}`);
    },
    onSynced: ({ state }) => {
      console.log(
        `[Y.js] Synced ${documentPath}`,
        state ? "with server" : "from cache",
      );
    },
  });

  // Set user awareness for cursors
  provider.awareness?.setLocalStateField("user", userInfo);

  // Create session
  const session: CollaborationSession = {
    ydoc,
    provider,
    persistence,
    saveTimeout: null,
    onMarkdownSave: options?.onMarkdownSave,
  };
  sessions.set(documentPath, session);

  // Set up debounced markdown save on changes
  ydoc.on("update", () => {
    // Clear existing timeout
    if (session.saveTimeout) {
      clearTimeout(session.saveTimeout);
    }

    // Schedule debounced save
    session.saveTimeout = setTimeout(() => {
      if (session.onMarkdownSave) {
        // Get content from TipTap's fragment
        // Note: The actual markdown extraction happens in the Editor component
        console.log(`[Y.js] Triggering debounced save for ${documentPath}`);
        // The save is triggered via the onchange callback in Editor
      }
    }, SAVE_DEBOUNCE_MS);
  });

  return { ydoc, provider };
}

/**
 * Update the markdown save callback for a document.
 */
export function setMarkdownSaveCallback(
  documentPath: string,
  callback: (markdown: string) => void,
): void {
  const session = sessions.get(documentPath);
  if (session) {
    session.onMarkdownSave = callback;
  }
}

/**
 * Disconnect a collaborative document session.
 *
 * IMPORTANT:
 * We intentionally do NOT destroy the Y.Doc here.
 *
 * TipTap's Collaboration extension maintains internal plugin state (ystate) that
 * can briefly outlive component teardown during doc switches. Destroying the Y.Doc
 * during that window can cause `ystate.doc` to become undefined and crash.
 *
 * We disconnect the provider to stop network activity. The Y.Doc+IndexedDB persistence
 * are kept so reopening the same document is fast and stable.
 */
export function disconnectDocument(documentPath: string): void {
  const session = sessions.get(documentPath);
  if (!session) return;

  // Clear any pending save
  if (session.saveTimeout) {
    clearTimeout(session.saveTimeout);
    session.saveTimeout = null;
  }

  // Disconnect provider
  session.provider.disconnect();

  // Keep Y.Doc + persistence alive (do NOT destroy)
  console.log(`[Y.js] Disconnected (kept Y.Doc) ${documentPath}`);
}

/**
 * Fully destroy a collaborative document session (use rarely).
 * Safe to call on app shutdown, not during rapid TipTap doc switches.
 */
export function destroyDocument(documentPath: string): void {
  const session = sessions.get(documentPath);
  if (!session) return;

  if (session.saveTimeout) {
    clearTimeout(session.saveTimeout);
    session.saveTimeout = null;
  }

  session.provider.disconnect();
  session.ydoc.destroy();
  sessions.delete(documentPath);

  console.log(`[Y.js] Destroyed session ${documentPath}`);
}

/**
 * Disconnect all collaborative document sessions.
 */
export function disconnectAll(): void {
  for (const [path] of sessions) {
    disconnectDocument(path);
  }
}

/**
 * Destroy all collaborative document sessions.
 */
export function destroyAll(): void {
  for (const [path] of sessions) {
    destroyDocument(path);
  }
}

/**
 * Check if a document has an active collaboration session.
 */
export function hasSession(documentPath: string): boolean {
  return sessions.has(documentPath);
}

/**
 * Check if collaboration server is connected for a document.
 */
export function isConnected(documentPath: string): boolean {
  const session = sessions.get(documentPath);
  // Check if provider exists and has synced at least once
  return session?.provider?.synced ?? false;
}

/**
 * Get the awareness (user presence) for a document.
 * Used for collaborative cursors.
 */
export function getAwareness(documentPath: string) {
  const session = sessions.get(documentPath);
  return session?.provider?.awareness;
}

/**
 * Force an immediate markdown save (bypasses debounce).
 */
export function forceSave(documentPath: string): void {
  const session = sessions.get(documentPath);
  if (session?.saveTimeout) {
    clearTimeout(session.saveTimeout);
    session.saveTimeout = null;
  }
  // The actual save is handled by the Editor's onchange callback
  console.log(`[Y.js] Force save triggered for ${documentPath}`);
}
