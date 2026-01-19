<script lang="ts">
  import { onMount, onDestroy, tick } from "svelte";
  import { getBackend, isTauri } from "./lib/backend";
  import { createApi, type Api } from "./lib/backend/api";
  import type { JsonValue } from "./lib/backend/generated/serde_json/JsonValue";
  import type { FileMetadata } from "./lib/backend/generated";
  // New Rust CRDT module imports
  import { RustCrdtApi } from "./lib/crdt/rustCrdtApi";
  import {
    initCollaboration,
    getCollaborationServer,
    getCollaborativeDocument,
    releaseDocument,
    cleanup as cleanupCollaboration,
  } from "./lib/crdt/collaborationBridge";
  import {
    disconnectWorkspace,
    setWorkspaceId,
    setBackendApi,
    onSessionSync,
    onBodyChange,
    getTreeFromCrdt,
    populateCrdtFromFiles,
    updateFileBodyInYDoc,
  } from "./lib/crdt/workspaceCrdtBridge";
  // Note: YDoc and HocuspocusProvider types are now handled by collaborationStore
  import LeftSidebar from "./lib/LeftSidebar.svelte";
  import RightSidebar from "./lib/RightSidebar.svelte";
  import NewEntryModal from "./lib/NewEntryModal.svelte";
  import CommandPalette from "./lib/CommandPalette.svelte";
  import SettingsDialog from "./lib/SettingsDialog.svelte";
  import ExportDialog from "./lib/ExportDialog.svelte";
    import EditorHeader from "./views/editor/EditorHeader.svelte";
  import EditorEmptyState from "./views/editor/EditorEmptyState.svelte";
  import EditorContent from "./views/editor/EditorContent.svelte";
  import { Toaster } from "$lib/components/ui/sonner";
  import * as Tooltip from "$lib/components/ui/tooltip";
  import { toast } from "svelte-sonner";
  // Note: Button, icons, and LoadingSpinner are now only used in extracted view components

  // Import stores
  import {
    entryStore,
    uiStore,
    collaborationStore,
    workspaceStore,
    getThemeStore,
    shareSessionStore
  } from "./models/stores";

  // Import auth
  import { initAuth } from "./lib/auth";

  // Initialize theme store immediately
  getThemeStore();

  // Import services
  import {
    revokeBlobUrls,
    transformAttachmentPaths,
    reverseBlobUrlsToAttachmentPaths,
    trackBlobUrl,
    computeRelativeAttachmentPath,
    initializeWorkspaceCrdt,
    updateCrdtFileMetadata,
    addFileToCrdt,
    joinShareSession,
  } from "./models/services";

  // Import controllers
  import {
    refreshTree as refreshTreeController,
    loadNodeChildren as loadNodeChildrenController,
    runValidation as runValidationController,
    validatePath,
  } from "./controllers";

  // Dynamically import Editor to avoid SSR issues
  let Editor: typeof import("./lib/Editor.svelte").default | null =
    $state(null);

  // ========================================================================
  // Store-backed state (using getters for now, will migrate fully later)
  // This allows gradual migration without breaking the component
  // ========================================================================

  // Entry state - proxied from entryStore
  let currentEntry = $derived(entryStore.currentEntry);
  let isDirty = $derived(entryStore.isDirty);
  let isSaving = $derived(entryStore.isSaving);
  // Editor is read-only when guest is in a read-only session
  let editorReadonly = $derived(shareSessionStore.isGuest && shareSessionStore.readOnly);
  let isLoading = $derived(entryStore.isLoading);
  let titleError = $derived(entryStore.titleError);
  let displayContent = $derived(entryStore.displayContent);

  // UI state - proxied from uiStore
  let leftSidebarCollapsed = $derived(uiStore.leftSidebarCollapsed);
  let rightSidebarCollapsed = $derived(uiStore.rightSidebarCollapsed);
  let showSettingsDialog = $derived(uiStore.showSettingsDialog);
  let showExportDialog = $derived(uiStore.showExportDialog);
  let showNewEntryModal = $derived(uiStore.showNewEntryModal);
  let exportPath = $derived(uiStore.exportPath);
  let editorRef = $derived(uiStore.editorRef);

  // Right sidebar tab/session control
  let requestedSidebarTab: "properties" | "history" | "share" | null = $state(null);
  let triggerStartSession = $state(false);

  // Workspace state - proxied from workspaceStore
  let tree = $derived(workspaceStore.tree);
  let expandedNodes = $derived(workspaceStore.expandedNodes);
  let validationResult = $derived(workspaceStore.validationResult);
  let workspaceId = $derived(workspaceStore.workspaceId);
  let backend = $derived(workspaceStore.backend);
  let showUnlinkedFiles = $derived(workspaceStore.showUnlinkedFiles);
  let showHiddenFiles = $derived(workspaceStore.showHiddenFiles);
  let showEditorTitle = $derived(workspaceStore.showEditorTitle);
  let showEditorPath = $derived(workspaceStore.showEditorPath);
  let readableLineLength = $derived(workspaceStore.readableLineLength);
  let focusMode = $derived(workspaceStore.focusMode);

  // API wrapper - uses execute() internally for all operations
  let api: Api | null = $derived(backend ? createApi(backend) : null);

  // Rust CRDT API instance
  let rustApi: RustCrdtApi | null = $state(null);

  // Track whether initial guest sync has completed (to avoid re-opening root on every update)
  let guestInitialSyncDone = $state(false);

  // Track whether the current entry is a daily entry (for prev/next navigation)
  let isDailyEntry = $state(false);

  // Collaboration state - proxied from collaborationStore
  let currentCollaborationPath = $derived(collaborationStore.currentCollaborationPath);
  let collaborationEnabled = $derived(collaborationStore.collaborationEnabled);
  let collaborationServerUrl = $derived(collaborationStore.collaborationServerUrl);

  // Current YDocProxy for syncing local edits
  import type { YDocProxy } from './lib/crdt/yDocProxy';
  let currentYDocProxy: YDocProxy | null = $state(null);

  // ========================================================================
  // Non-store state (component-specific, not shared)
  // ========================================================================

  // Auto-save timer (component-local, not needed in global store)
  let autoSaveTimer: ReturnType<typeof setTimeout> | null = null;
  const AUTO_SAVE_DELAY_MS = 2500; // 2.5 seconds

  // Set VITE_DISABLE_WORKSPACE_CRDT=true to disable workspace CRDT for debugging
  // This keeps per-file collaboration working but disables the workspace-level sync
  const workspaceCrdtDisabled: boolean =
    typeof import.meta !== "undefined" &&
    (import.meta as any).env?.VITE_DISABLE_WORKSPACE_CRDT === "true";

  // Workspace ID from environment (for multi-device sync without chicken-and-egg problem)
  // Set VITE_WORKSPACE_ID to the same value on all devices to ensure they sync
  const envWorkspaceId: string | null =
    typeof import.meta !== "undefined" &&
    (import.meta as any).env?.VITE_WORKSPACE_ID
      ? (import.meta as any).env.VITE_WORKSPACE_ID
      : null;

  // Generate a UUID for workspace identification
  function generateUUID(): string {
    return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (c) => {
      const r = (Math.random() * 16) | 0;
      const v = c === "x" ? r : (r & 0x3) | 0x8;
      return v.toString(16);
    });
  }

  // Helper to handle mixed frontmatter types (Map from WASM vs Object from JSON/Tauri)
  function normalizeFrontmatter(frontmatter: any): Record<string, any> {
    if (!frontmatter) return {};
    if (frontmatter instanceof Map) {
      return Object.fromEntries(frontmatter.entries());
    }
    return frontmatter;
  }

  // Attachment state
  let pendingAttachmentPath = $state("");
  let attachmentError: string | null = $state(null);
  let attachmentFileInput: HTMLInputElement | null = $state(null);

  // Note: Blob URL management is now in attachmentService.ts

  // Persist display setting to localStorage when changed
  $effect(() => {
    if (typeof window !== "undefined") {
      localStorage.setItem(
        "diaryx-show-unlinked-files",
        String(showUnlinkedFiles),
      );
      localStorage.setItem("diaryx-show-hidden-files", String(showHiddenFiles));
    }
  });

  // Reset guest initial sync flag when leaving guest mode
  $effect(() => {
    if (!shareSessionStore.isGuest) {
      guestInitialSyncDone = false;
    }
  });

  // Check if we're on desktop and expand sidebars by default
  onMount(async () => {
    // Expand sidebars on desktop
    if (window.innerWidth >= 768) {
      uiStore.setLeftSidebarCollapsed(false);
      uiStore.setRightSidebarCollapsed(false);
    }

    // Load saved collaboration settings
    // Note: We only load the URL into the store, but do NOT call setWorkspaceServer()
    // or setCollaborationServer() here. Those are called by initializeWorkspaceCrdt()
    // only when collaborationEnabled is true. This prevents the sync bridge from
    // trying to connect when there's no active sync session.
    if (typeof window !== "undefined") {
      const savedServerUrl = localStorage.getItem("diaryx-sync-server");
      if (savedServerUrl) {
        collaborationStore.setServerUrl(savedServerUrl);
      }
    }

    // Initialize auth state - if user was previously logged in,
    // this will validate their token and enable collaboration automatically
    await initAuth();

    try {
      // Dynamically import the Editor component
      const module = await import("./lib/Editor.svelte");
      Editor = module.default;

      // Initialize the backend (auto-detects Tauri vs WASM)
      const backendInstance = await getBackend();
      workspaceStore.setBackend(backendInstance);

      // Set the backend API for CRDT bridge (used for writing synced files to disk)
      const apiInstance = createApi(backendInstance);
      setBackendApi(apiInstance);

      // Initialize Rust CRDT API
      rustApi = new RustCrdtApi(backendInstance);

      // Initialize collaboration system with the Rust API
      initCollaboration(rustApi);

      // Initialize workspace CRDT (unless disabled for debugging)
      if (!workspaceCrdtDisabled) {
        await setupWorkspaceCrdt();
      } else {
        console.log(
          "[App] Workspace CRDT disabled via VITE_DISABLE_WORKSPACE_CRDT",
        );
      }

      await refreshTree();

      // Register callback to refresh tree when session data is received
      onSessionSync(async () => {
        // Only build tree from CRDT when in guest mode
        // Guests don't have files on disk, so they need the CRDT tree
        // Hosts and local users should use the filesystem tree (already loaded)
        if (!shareSessionStore.isGuest) {
          console.log('[App] Session sync received, but not in guest mode - skipping CRDT tree');
          return;
        }

        console.log('[App] Session sync received (guest mode), building tree from CRDT');
        const crdtTree = await getTreeFromCrdt();
        if (crdtTree) {
          console.log('[App] Setting tree from CRDT:', crdtTree);
          workspaceStore.setTree(crdtTree);

          // Only open root entry on the first sync, not on every update
          if (!guestInitialSyncDone) {
            console.log('[App] Guest session - initial sync, opening root entry:', crdtTree.path);
            workspaceStore.expandNode(crdtTree.path);
            await openEntry(crdtTree.path);
            guestInitialSyncDone = true;
          } else {
            console.log('[App] Guest session - incremental sync, tree updated');
          }
        } else {
          console.log('[App] No CRDT tree available, falling back to filesystem refresh');
          await refreshTree();
        }
      });

      // Register callback to reload editor when remote body changes arrive
      onBodyChange(async (path, body) => {
        console.log('[App] Body change received for:', path, 'current entry:', currentEntry?.path);
        // Only update if this is the currently open file
        if (currentEntry && path === currentEntry.path) {
          console.log('[App] Updating display content with remote body, length:', body.length);
          // Transform attachment paths to blob URLs for display
          const transformed = await transformAttachmentPaths(body, path, api);
          entryStore.setDisplayContent(transformed);
        }
      });

      // Expand root and open it by default
      if (tree && !currentEntry) {
        workspaceStore.expandNode(tree.path);
        await openEntry(tree.path);
      }

      // Run initial validation
      await runValidation();

      // Add swipe-down gesture for command palette on mobile
      let touchStartY = 0;
      let touchStartX = 0;
      const handleTouchStart = (e: TouchEvent) => {
        touchStartY = e.touches[0].clientY;
        touchStartX = e.touches[0].clientX;
      };
      const handleTouchEnd = (e: TouchEvent) => {
        const touchEndY = e.changedTouches[0].clientY;
        const touchEndX = e.changedTouches[0].clientX;
        const deltaY = touchEndY - touchStartY;
        const deltaX = Math.abs(touchEndX - touchStartX);
        // Swipe down from top 100px of screen, mostly vertical
        if (touchStartY < 100 && deltaY > 80 && deltaX < 50) {
          uiStore.openCommandPalette();
        }
      };
      document.addEventListener("touchstart", handleTouchStart);
      document.addEventListener("touchend", handleTouchEnd);
    } catch (e) {
      console.error("[App] Initialization error:", e);
      uiStore.setError(e instanceof Error ? e.message : String(e));
    } finally {
      entryStore.setLoading(false);
    }
  });

  onDestroy(() => {
    // Cleanup blob URLs
    revokeBlobUrls();
    // Cleanup collaboration sessions
    cleanupCollaboration();
    // Disconnect workspace CRDT (keeps local state for quick reconnect)
    disconnectWorkspace();
  });

  // Initialize the workspace CRDT
  async function setupWorkspaceCrdt(retryCount = 0) {
    if (!api || !backend || !rustApi) return;

    try {
      // Workspace ID priority:
      // 1. Environment variable VITE_WORKSPACE_ID (best for multi-device, avoids bootstrap issue)
      // 2. workspace_id from root index frontmatter (should persist in the workspace index)
      // 3. null (no prefix - uses simple room names like "doc:path/to/file.md")
      let sharedWorkspaceId: string | null = envWorkspaceId;

      // Get the workspace directory from the backend, then find the actual root index
      const workspaceDir = backend.getWorkspacePath().replace(/\/index\.md$/, '').replace(/\/README\.md$/, '');
      console.log("[App] Workspace directory:", workspaceDir);

      let workspacePath: string;
      try {
        workspacePath = await api.findRootIndex(workspaceDir);
        console.log("[App] Found root index at:", workspacePath);
      } catch (e) {
        console.warn("[App] Could not find root index:", e);
        // Fall back to default - will trigger workspace creation
        workspacePath = `${workspaceDir}/index.md`;
      }

      if (sharedWorkspaceId) {
        console.log(
          "[App] Using workspace_id from environment:",
          sharedWorkspaceId,
        );
      } else {
        // Try to get/create workspace_id from root index frontmatter
        try {
          const rootTree = await api.getWorkspaceTree(workspacePath);
          console.log("[App] Workspace tree root path:", rootTree?.path);

          if (rootTree?.path) {
            const rootFrontmatter = await api.getFrontmatter(rootTree.path);

            sharedWorkspaceId =
              (rootFrontmatter.workspace_id as string) ?? null;

            // If no workspace_id exists, generate one and save it
            if (!sharedWorkspaceId) {
              sharedWorkspaceId = generateUUID();
              console.log(
                "[App] workspace_id missing in index; generating:",
                sharedWorkspaceId,
              );

              await api.setFrontmatterProperty(
                rootTree.path,
                "workspace_id",
                sharedWorkspaceId,
              );

              console.log(
                "[App] Wrote workspace_id to index, persisting...",
                sharedWorkspaceId,
              );

              // Re-read to confirm it actually persisted (especially important in WASM mode)
              const verifyFrontmatter = await api.getFrontmatter(
                rootTree.path,
              );
              console.log(
                "[App] Verified workspace_id after write:",
                normalizeFrontmatter(verifyFrontmatter)?.workspace_id,
              );

              // Also re-read raw content to confirm it actually wrote frontmatter into the file.
              try {
                const rootEntryAfter = await api.getEntry(rootTree.path);
                const afterHead = (rootEntryAfter?.content ?? "").slice(0, 600);
                console.log(
                  "[App] Root entry content head after write (first 600 chars):",
                  afterHead,
                );
              } catch (e) {
                console.warn(
                  "[App] Failed to read root entry content after write for debugging:",
                  e,
                );
              }
            } else {
              console.log(
                "[App] Using workspace_id from index:",
                sharedWorkspaceId,
              );
            }
          }
        } catch (e) {
          const errStr = e instanceof Error ? e.message : String(e);
          if (
            errStr.includes("No workspace found") ||
            errStr.includes("NotFoundError") ||
            errStr.includes("The object can not be found here")
          ) {
            console.log("[App] Default workspace missing, creating...");
            try {
              await api.createWorkspace("workspace", "My Journal");
              // Recursively try again to setup workspace_id and CRDT
              return await setupWorkspaceCrdt();
            } catch (createErr) {
              const createErrStr = String(createErr);
              if (createErrStr.includes("Workspace already exists")) {
                 if (retryCount >= 3) {
                    console.error("[App] Max retries reached for workspace setup. Stopping to prevent infinite loop.");
                    uiStore.setError("Failed to initialize workspace: Unrecoverable error.");
                    return;
                 }
                 console.log(`[App] Workspace existed but wasn't found initially. Retrying setup (attempt ${retryCount + 1})...`);
                 await new Promise((resolve) => setTimeout(resolve, 500));
                 return await setupWorkspaceCrdt(retryCount + 1);
              }
              console.error(
                "[App] Failed to create default workspace:",
                createErr,
              );
            }
          }

          console.warn("[App] Could not get/set workspace_id from index:", e);
          // Fall back to null - will use simple room names without workspace prefix
          console.log("[App] Using no workspace_id prefix (simple room names)");
        }
      }

      workspaceId = sharedWorkspaceId;

      // Set workspace ID for per-file document room naming
      // If null, rooms will be "doc:{path}" instead of "{id}:doc:{path}"
      setWorkspaceId(workspaceId);

      // Initialize workspace CRDT using service with Rust API
      const initialized = await initializeWorkspaceCrdt(
        workspaceId,
        workspacePath,
        collaborationServerUrl,
        collaborationEnabled,
        rustApi,
        {
          onConnectionChange: (connected: boolean) => {
            console.log("[App] Workspace CRDT connection:", connected ? "online" : "offline");
            collaborationStore.setConnected(connected);
          },
        },
      );
      workspaceStore.setWorkspaceCrdtInitialized(initialized);
    } catch (e) {
      console.error("[App] Failed to initialize workspace CRDT:", e);
      workspaceStore.setWorkspaceCrdtInitialized(false);
    }
  }

  // Open an entry
  async function openEntry(path: string) {
    if (!api || !backend) return;

    // Auto-save before switching documents
    if (isDirty) {
      cancelAutoSave();
      await save();
    }

    try {
      entryStore.setLoading(true);

      // Cleanup previous blob URLs
      revokeBlobUrls();

      const entry = await api.getEntry(path);
      // Normalize frontmatter to Object
      entry.frontmatter = normalizeFrontmatter(entry.frontmatter);
      currentEntry = entry;

      titleError = null; // Clear any title error when switching files
      console.log("[App] Loaded entry:", currentEntry);
      console.log("[App] Frontmatter:", currentEntry?.frontmatter);
      console.log(
        "[App] Frontmatter keys:",
        Object.keys(currentEntry?.frontmatter ?? {}),
      );

      // Transform attachment paths to blob URLs for display
      if (currentEntry) {
        displayContent = await transformAttachmentPaths(
          currentEntry.content,
          currentEntry.path,
          api,
        );

        // Setup Y.js collaboration for this document
        // Destroy the previous collaboration session to prevent stale data from corrupting other clients.
        // We use destroyDocument with a delay to let TipTap plugins finish tearing down.
        // IMPORTANT: Skip destruction if we're re-opening the same file (e.g., from remote sync callback)

        // Calculate what the new collaboration path will be
        let workspaceDir = tree?.path || "";
        if (workspaceDir.endsWith("/"))
          workspaceDir = workspaceDir.slice(0, -1);
        if (
          workspaceDir.endsWith("README.md") ||
          workspaceDir.endsWith("index.md")
        ) {
          workspaceDir = workspaceDir.substring(
            0,
            workspaceDir.lastIndexOf("/"),
          );
        }
        let newRelativePath = currentEntry.path;
        if (workspaceDir && currentEntry.path.startsWith(workspaceDir)) {
          newRelativePath = currentEntry.path.substring(
            workspaceDir.length + 1,
          );
        }

        // Only destroy previous session if switching to a different file
        if (currentCollaborationPath && currentCollaborationPath !== newRelativePath) {
          const pathToDestroy = currentCollaborationPath;
          collaborationStore.clearCollaborationSession();
          currentYDocProxy = null;
          await tick();
          // Delay destruction to avoid `ystate.doc` errors from TipTap
          setTimeout(() => {
            try {
              releaseDocument(pathToDestroy);
            } catch (e) {
              console.warn(
                "[App] Failed to release collaboration session:",
                e,
              );
            }
          }, 100);
        }

        // Connect to collaboration for this entry only if enabled.
        // If not enabled, Editor will render the plain markdown `content` immediately.
        if (collaborationEnabled) {
          try {
            console.log(
              "[App] Collaboration room:",
              newRelativePath,
              "(from",
              currentEntry.path,
              ")",
            );

            // Use new CRDT module
            // Only use initialContent if no server is configured (offline mode).
            // When syncing with a server, the server's state is authoritative to avoid duplication.
            const serverConfigured = !!getCollaborationServer();
            const { yDocProxy, bridge } = await getCollaborativeDocument(newRelativePath, {
              initialContent: serverConfigured ? undefined : currentEntry.content,
              onRemoteContentChange: async (content: string) => {
                // Remote content changed - update the editor
                console.log('[App] Remote content changed, length:', content.length);
                if (currentEntry && api) {
                  // Transform attachment paths to blob URLs
                  const transformed = await transformAttachmentPaths(
                    content,
                    currentEntry.path,
                    api,
                  );
                  // Update display content - this will trigger editor re-render via editorKey
                  entryStore.setDisplayContent(transformed);
                }
              },
            });
            // Store YDocProxy for syncing local edits
            currentYDocProxy = yDocProxy;
            // Get the underlying Y.Doc for TipTap, bridge implements HocuspocusProvider interface
            collaborationStore.setCollaborationSession(
              yDocProxy.getYDoc(),
              bridge as any, // HocuspocusBridge is compatible with HocuspocusProvider
              newRelativePath
            );
          } catch (e) {
            console.warn("[App] Failed to setup collaboration:", e);
            collaborationStore.clearCollaborationSession();
            currentYDocProxy = null;
          }
        }
      } else {
        entryStore.setDisplayContent("");
        collaborationStore.clearCollaborationSession();
        currentYDocProxy = null;
      }

      entryStore.markClean();
      uiStore.clearError();

      // Check if this is a daily entry for prev/next navigation
      if (api) {
        isDailyEntry = await api.isDailyEntry(path);
      }
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    } finally {
      entryStore.setLoading(false);
    }
  }

  // Save current entry
  async function save() {
    if (!api || !currentEntry || !editorRef) return;
    if (isSaving) return; // Prevent concurrent saves

    try {
      entryStore.setSaving(true);
      const markdownWithBlobUrls = editorRef.getMarkdown();
      // Reverse-transform blob URLs back to attachment paths
      const markdown = reverseBlobUrlsToAttachmentPaths(
        markdownWithBlobUrls || "",
      );

      // Note: saveEntry expects only the body content, not frontmatter.
      // Frontmatter is preserved by the backend's save_content() method.
      // Frontmatter changes are saved separately via setFrontmatterProperty().
      await api.saveEntry(currentEntry.path, markdown);
      entryStore.markClean();

      // Sync body content to workspace Y.Doc for live share sessions
      // This allows other session participants to see the updated content
      if (shareSessionStore.mode !== 'idle' && shareSessionStore.joinCode) {
        updateFileBodyInYDoc(currentEntry.path, markdown);
      }
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    } finally {
      entryStore.setSaving(false);
    }
  }

  // Cancel pending auto-save
  function cancelAutoSave() {
    if (autoSaveTimer) {
      clearTimeout(autoSaveTimer);
      autoSaveTimer = null;
    }
  }

  // Schedule auto-save with debounce
  function scheduleAutoSave() {
    cancelAutoSave();
    autoSaveTimer = setTimeout(() => {
      autoSaveTimer = null;
      if (isDirty) {
        save();
      }
    }, AUTO_SAVE_DELAY_MS);
  }

  // Handle content changes - triggers debounced auto-save and CRDT sync
  function handleContentChange(markdown: string) {
    entryStore.markDirty();
    scheduleAutoSave();

    // Sync to CRDT for real-time collaboration (if enabled)
    if (collaborationEnabled && currentYDocProxy) {
      // Reverse-transform blob URLs back to attachment paths before syncing
      const markdownForSync = reverseBlobUrlsToAttachmentPaths(markdown);
      currentYDocProxy.setContent(markdownForSync);
    }
  }

  // Handle editor blur - save immediately if dirty
  function handleEditorBlur() {
    cancelAutoSave();
    if (isDirty) {
      save();
    }
  }

  // Toggle node expansion
  function toggleNode(path: string) {
    // Use store method to ensure expanded state persists across tree refreshes
    workspaceStore.toggleNode(path);
  }

  // Sidebar toggles
  function toggleLeftSidebar() {
    uiStore.toggleLeftSidebar();
  }

  function toggleRightSidebar() {
    uiStore.toggleRightSidebar();
  }

  // Keyboard shortcuts
  function handleKeydown(event: KeyboardEvent) {
    if ((event.metaKey || event.ctrlKey) && event.key === "s") {
      event.preventDefault();
      save();
    }
    // Command palette with Cmd/Ctrl + K
    if ((event.metaKey || event.ctrlKey) && event.key === "k") {
      event.preventDefault();
      uiStore.openCommandPalette();
    }
    // Toggle left sidebar with Cmd/Ctrl + [ (bracket)
    if ((event.metaKey || event.ctrlKey) && event.key === "[") {
      event.preventDefault();
      toggleLeftSidebar();
    }
    // Toggle right sidebar with Cmd/Ctrl + ]
    if ((event.metaKey || event.ctrlKey) && event.key === "]") {
      event.preventDefault();
      toggleRightSidebar();
    }
    // Open settings:
    // - Tauri: Cmd/Ctrl + , (standard desktop convention)
    // - Web: Cmd/Ctrl + Shift + , (to avoid browser settings conflict)
    if ((event.metaKey || event.ctrlKey) && event.key === ",") {
      if (isTauri() || event.shiftKey) {
        event.preventDefault();
        showSettingsDialog = true;
      }
    }
    // Navigate daily entries with Alt+Left/Right
    if (event.altKey && isDailyEntry) {
      if (event.key === "ArrowLeft") {
        event.preventDefault();
        handlePrevDay();
      } else if (event.key === "ArrowRight") {
        event.preventDefault();
        handleNextDay();
      }
    }
  }

  async function handleCreateChildEntry(parentPath: string) {
    if (!api) return;
    try {
      const newPath = await api.createChildEntry(parentPath);

      // Update CRDT with new file
      const entry = await api.getEntry(newPath);
      addFileToCrdt(newPath, entry.frontmatter, parentPath);

      await refreshTree();
      // Also refresh the parent node directly to ensure deep nodes update correctly
      // (refreshTree only fetches depth 2, so deeper nodes may still have stale data)
      await loadNodeChildren(parentPath);
      await openEntry(newPath);
      await runValidation();
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function createNewEntry(path: string, title: string) {
    if (!api) return;
    try {
      // Get default template from settings
      const defaultTemplate = typeof window !== "undefined"
        ? localStorage.getItem("diaryx-default-template") || "note"
        : "note";
      const newPath = await api.createEntry(path, { title, template: defaultTemplate });

      // Persist to IndexedDB immediately so file survives refresh

      // Update CRDT with new file
      const entry = await api.getEntry(newPath);
      addFileToCrdt(newPath, entry.frontmatter, null);

      await refreshTree();
      await openEntry(newPath);
      await runValidation();
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    } finally {
      uiStore.closeNewEntryModal();
    }
  }

  async function handleDailyEntry() {
    if (!api || !tree) return;
    try {
      // Get daily_entry_folder from localStorage settings
      const dailyEntryFolder = typeof window !== "undefined"
        ? localStorage.getItem("diaryx-daily-entry-folder") || undefined
        : undefined;

      // Get daily template from settings
      const dailyTemplate = typeof window !== "undefined"
        ? localStorage.getItem("diaryx-daily-template") || "daily"
        : "daily";

      // Pass the workspace path, daily_entry_folder, and template to EnsureDailyEntry
      // The workspace path is the root index file path (e.g., "workspace/README.md")
      const path = await api.ensureDailyEntry(tree.path, dailyEntryFolder, dailyTemplate);
      await refreshTree();
      await openEntry(path);
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  // Navigate to the previous day's entry
  async function handlePrevDay() {
    if (!api || !currentEntry) return;
    try {
      const prevPath = await api.getAdjacentDailyEntry(currentEntry.path, 'prev');
      if (prevPath) {
        // Check if entry exists before navigating
        const exists = await api.fileExists(prevPath);
        if (exists) {
          await openEntry(prevPath);
        } else {
          // Entry doesn't exist - show a subtle notification
          uiStore.setError("No entry for previous day");
        }
      }
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  // Navigate to the next day's entry
  async function handleNextDay() {
    if (!api || !currentEntry) return;
    try {
      const nextPath = await api.getAdjacentDailyEntry(currentEntry.path, 'next');
      if (nextPath) {
        // Check if entry exists before navigating
        const exists = await api.fileExists(nextPath);
        if (exists) {
          await openEntry(nextPath);
        } else {
          // Entry doesn't exist - show a subtle notification
          uiStore.setError("No entry for next day");
        }
      }
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function handleRenameEntry(path: string, newFilename: string): Promise<string> {
    if (!api) throw new Error("API not initialized");
    // Find parent before rename (in case tree structure helps locate it)
    const parentPath = workspaceStore.getParentNodePath(path);
    const newPath = await api.renameEntry(path, newFilename);
    await refreshTree();
    // Refresh parent to ensure deep nodes update correctly
    if (parentPath) {
      await loadNodeChildren(parentPath);
    }
    await runValidation();
    return newPath;
  }

  async function handleDuplicateEntry(path: string): Promise<string> {
    if (!api) throw new Error("API not initialized");
    // Find parent before duplicate
    const parentPath = workspaceStore.getParentNodePath(path);
    const newPath = await api.duplicateEntry(path);

    // Update CRDT with new file
    const entry = await api.getEntry(newPath);
    addFileToCrdt(newPath, entry.frontmatter, parentPath || null);

    await refreshTree();
    // Refresh parent to ensure deep nodes update correctly
    if (parentPath) {
      await loadNodeChildren(parentPath);
    }
    await runValidation();
    return newPath;
  }

  async function handleDeleteEntry(path: string) {
    if (!api) return;
    const confirm = window.confirm(
      `Are you sure you want to delete "${path.split("/").pop()?.replace(".md", "")}"?`,
    );
    if (!confirm) return;

    // Find the parent node BEFORE deleting (while the tree still has the entry)
    const parentPath = workspaceStore.getParentNodePath(path);

    try {
      await api.deleteEntry(path);

      // CRDT is now automatically updated via backend event subscription
      // (file:deleted event triggers crdtDeleteFile and removeFromContents)

      // If we deleted the currently open entry, clear it
      if (currentEntry?.path === path) {
        currentEntry = null;
        isDirty = false;
      }

      // Try to refresh the tree - this might fail if workspace state is temporarily inconsistent
      try {
        await refreshTree();
        // Also refresh the parent node directly to ensure deep nodes update correctly
        // (refreshTree only fetches depth 2, so deeper nodes may still have stale data)
        if (parentPath) {
          await loadNodeChildren(parentPath);
        }
        await runValidation();
      } catch (refreshError) {
        console.warn("[App] Error refreshing tree after delete:", refreshError);
        // Try again after a short delay
        setTimeout(async () => {
          try {
            if (backend) {
              await refreshTree();
              if (parentPath) {
                await loadNodeChildren(parentPath);
              }
              await runValidation();
            }
          } catch (e) {
            console.error("[App] Retry tree refresh failed:", e);
          }
        }, 500);
      }
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  // Run workspace validation (delegates to controller)
  async function runValidation() {
    if (!api || !backend) return;
    await runValidationController(api, backend, tree);
  }

  // Validate a specific path (delegates to controller)
  async function handleValidate(path: string) {
    if (!api) return;
    await validatePath(api, path);
  }

  // Quick fix: Remove broken part_of reference from a file
  async function handleRemoveBrokenPartOf(filePath: string) {
    if (!api) return;
    try {
       await api.removeFrontmatterProperty(filePath, "part_of");
      await runValidation();
      // Refresh current entry if it's the fixed file
      if (currentEntry?.path === filePath) {
        currentEntry = await api.getEntry(filePath);
      }
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  // Quick fix: Remove broken entry from an index's contents
  async function handleRemoveBrokenContentsRef(indexPath: string, target: string) {
    if (!api) return;
    try {
      // Get current contents
      const entry = await api.getEntry(indexPath);
      const contents = entry.frontmatter?.contents;
      if (Array.isArray(contents)) {
        // Filter out the broken target
        const newContents = contents.filter((item) => item !== target);
         await api.setFrontmatterProperty(indexPath, "contents", newContents);
        await refreshTree();
        await runValidation();
        // Refresh current entry if it's the fixed file
        if (currentEntry?.path === indexPath) {
          currentEntry = await api.getEntry(indexPath);
        }
      }
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  // Quick fix: Attach an unlinked entry to the workspace root
  async function handleAttachUnlinkedEntry(entryPath: string) {
    if (!api || !tree) return;
    try {
       // Attach to the workspace root (tree.path is the root index)
      await api.attachEntryToParent(entryPath, tree.path);
      await refreshTree();
      await runValidation();
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  // Wrapper functions that delegate to controllers
  async function refreshTree() {
    if (!api || !backend) return;
    await refreshTreeController(api, backend, showUnlinkedFiles, showHiddenFiles);
  }

  async function loadNodeChildren(nodePath: string) {
    if (!api) return;
    await loadNodeChildrenController(api, nodePath, showUnlinkedFiles, showHiddenFiles);
  }

  // ========================================================================
  // Command Palette Handlers
  // ========================================================================

  // Validation with toast feedback
  async function handleValidateWorkspace() {
    await runValidation();
    const result = workspaceStore.validationResult;
    if (result) {
      const errorCount = result.errors?.length ?? 0;
      const warningCount = result.warnings?.length ?? 0;
      if (errorCount === 0 && warningCount === 0) {
        toast.success("Workspace is valid", { description: "No issues found" });
      } else {
        toast.warning("Validation complete", {
          description: `${errorCount} error(s), ${warningCount} warning(s) found`,
        });
      }
    }
  }

  // Refresh tree with toast
  async function handleRefreshTree() {
    await refreshTree();
    toast.success("Tree refreshed");
  }

  // Duplicate current entry
  async function handleDuplicateCurrentEntry() {
    if (!api || !currentEntry) {
      toast.error("No entry selected");
      return;
    }
    try {
      const newPath = await handleDuplicateEntry(currentEntry.path);
      await openEntry(newPath);
      toast.success("Entry duplicated", { description: newPath.split("/").pop() });
    } catch (e) {
      toast.error("Failed to duplicate", { description: e instanceof Error ? e.message : String(e) });
    }
  }

  // Rename current entry (prompt for new name)
  async function handleRenameCurrentEntry() {
    if (!api || !currentEntry) {
      toast.error("No entry selected");
      return;
    }
    const currentName = currentEntry.path.split("/").pop()?.replace(".md", "") || "";
    const newName = window.prompt("Enter new name:", currentName);
    if (!newName || newName === currentName) return;

    try {
      const newPath = await handleRenameEntry(currentEntry.path, newName + ".md");
      await openEntry(newPath);
      toast.success("Entry renamed", { description: newName });
    } catch (e) {
      toast.error("Failed to rename", { description: e instanceof Error ? e.message : String(e) });
    }
  }

  // Delete current entry
  async function handleDeleteCurrentEntry() {
    if (!currentEntry) {
      toast.error("No entry selected");
      return;
    }
    await handleDeleteEntry(currentEntry.path);
  }

  // Move current entry (prompt for new parent)
  async function handleMoveCurrentEntry() {
    const entry = currentEntry;
    const currentTree = tree;
    if (!api || !entry || !currentTree) {
      toast.error("No entry selected");
      return;
    }
    // Collect all potential parent paths
    const allEntries: string[] = [];
    function collectPaths(node: typeof currentTree) {
      if (!node) return;
      // Only index files (with children) can be parents
      if (node.children.length > 0 || node.path.endsWith("index.md") || node.path.endsWith("README.md")) {
        allEntries.push(node.path);
      }
      node.children.forEach(collectPaths);
    }
    collectPaths(currentTree);

    const parentOptions = allEntries
      .filter(p => p !== entry.path)
      .map(p => p.split("/").pop()?.replace(".md", "") || p)
      .join(", ");

    const newParentName = window.prompt(
      `Move "${entry.path.split("/").pop()?.replace(".md", "")}" to which parent?\n\nAvailable: ${parentOptions}`
    );
    if (!newParentName) return;

    // Find the matching parent path
    const newParentPath = allEntries.find(p =>
      p.split("/").pop()?.replace(".md", "").toLowerCase() === newParentName.toLowerCase()
    );
    if (!newParentPath) {
      toast.error("Parent not found", { description: `"${newParentName}" is not a valid parent` });
      return;
    }

    try {
      await handleMoveEntry(entry.path, newParentPath);
      toast.success("Entry moved", { description: `Moved to ${newParentName}` });
    } catch (e) {
      toast.error("Failed to move", { description: e instanceof Error ? e.message : String(e) });
    }
  }

  // Create child entry under current
  async function handleCreateChildUnderCurrent() {
    if (!currentEntry) {
      toast.error("No entry selected");
      return;
    }
    await handleCreateChildEntry(currentEntry.path);
    toast.success("Child entry created");
  }

  // Start share session
  async function handleStartShareSession() {
    if (shareSessionStore.mode !== 'idle') {
      toast.info("Session already active", { description: "End current session first" });
      return;
    }
    // Open right sidebar, navigate to share tab, and trigger session start
    uiStore.setRightSidebarCollapsed(false);
    requestedSidebarTab = "share";
    // Wait for sidebar to render before triggering session start
    await tick();
    triggerStartSession = true;
  }

  // Join share session
  async function handleJoinShareSession() {
    if (shareSessionStore.mode !== 'idle') {
      toast.info("Session already active", { description: "End current session first" });
      return;
    }
    const joinCode = window.prompt("Enter join code:");
    if (!joinCode?.trim()) return;

    try {
      workspaceStore.saveTreeState();
      await joinShareSession(joinCode.trim());
      toast.success("Joined session", { description: `Code: ${joinCode.trim()}` });
    } catch (e) {
      workspaceStore.clearSavedTreeState();
      toast.error("Failed to join", { description: e instanceof Error ? e.message : String(e) });
    }
  }

  // Find in file (trigger browser's find or show info)
  function handleFindInFile() {
    // Use browser's native find functionality
    if (typeof window !== "undefined") {
      // Try to trigger the browser's find dialog
      // This works in most browsers
      try {
        // @ts-ignore - execCommand is deprecated but still works
        document.execCommand('find');
      } catch {
        // Fallback: show keyboard shortcut hint
        toast.info("Find in File", { description: "Use Cmd/Ctrl+F to search" });
      }
    }
  }

  // Word count for current entry
  function handleWordCount() {
    if (!editorRef || !currentEntry) {
      toast.error("No entry open");
      return;
    }
    const markdown = editorRef.getMarkdown() || "";
    const text = markdown.replace(/[#*_`~\[\]()>-]/g, " "); // Remove markdown syntax
    const words = text.trim().split(/\s+/).filter((w: string) => w.length > 0).length;
    const characters = text.length;
    const lines = markdown.split("\n").length;

    toast.info("Word Count", {
      description: `${words.toLocaleString()} words · ${characters.toLocaleString()} characters · ${lines} lines`,
      duration: 5000,
    });
  }

  // Import from clipboard
  async function handleImportFromClipboard() {
    if (!api || !tree) {
      toast.error("Workspace not ready");
      return;
    }
    try {
      const clipboardText = await navigator.clipboard.readText();
      if (!clipboardText.trim()) {
        toast.error("Clipboard is empty");
        return;
      }

      // Create a new entry with clipboard content
      const timestamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
      const newPath = `${tree.path.replace(/[^/]+\.md$/, "")}imported-${timestamp}.md`;

      // Check if it has frontmatter, if not add a basic title
      let content = clipboardText;
      if (!clipboardText.trim().startsWith("---")) {
        const title = `Imported ${new Date().toLocaleDateString()}`;
        content = `---\ntitle: "${title}"\n---\n\n${clipboardText}`;
      }

      await api.createEntry(newPath, { title: `Imported ${timestamp}` });
      await api.saveEntry(newPath, content);
      await refreshTree();
      await openEntry(newPath);
      toast.success("Imported from clipboard", { description: `${clipboardText.length} characters` });
    } catch (e) {
      toast.error("Failed to import", { description: e instanceof Error ? e.message : String(e) });
    }
  }

  // Copy current entry as markdown
  async function handleCopyAsMarkdown() {
    if (!editorRef || !currentEntry) {
      toast.error("No entry open");
      return;
    }
    try {
      const markdown = editorRef.getMarkdown() || "";
      // Reverse blob URLs to attachment paths for clean export
      const cleanMarkdown = reverseBlobUrlsToAttachmentPaths(markdown);
      await navigator.clipboard.writeText(cleanMarkdown);
      toast.success("Copied to clipboard", { description: `${cleanMarkdown.length} characters` });
    } catch (e) {
      toast.error("Failed to copy", { description: e instanceof Error ? e.message : String(e) });
    }
  }

  /**
   * Populate the CRDT with files from the filesystem.
   * Called before hosting a share session to ensure all files are available.
   * @param audience - If provided, only include files accessible to this audience
   */
  async function populateCrdtBeforeHost(audience: string | null = null) {
    if (!api || !tree) {
      console.warn('[App] Cannot populate CRDT: api or tree not available');
      return;
    }

    console.log('[App] Populating CRDT from filesystem before hosting, audience:', audience);

    // Get filtered file set if audience is specified
    let allowedPaths: Set<string> | null = null;
    if (audience) {
      try {
        const exportPlan = await api.planExport(tree.path, audience);
        allowedPaths = new Set(exportPlan.included.map(f => f.source_path));
        console.log('[App] Filtered to', allowedPaths.size, 'files for audience:', audience);
      } catch (e) {
        console.warn('[App] Failed to get export plan, sharing all files:', e);
      }
    }

    // Collect all files from the tree recursively
    const files: Array<{ path: string; metadata: Partial<FileMetadata> }> = [];

    async function collectFiles(node: typeof tree, parentPath: string | null) {
      if (!node || !api) return;

      // Skip files not in allowed paths (if filtering)
      if (allowedPaths && !allowedPaths.has(node.path)) {
        console.log('[App] Skipping file not in audience:', node.path);
        return;
      }

      // Filter children to only include allowed paths
      const filteredChildren = allowedPaths
        ? node.children.filter(c => allowedPaths!.has(c.path))
        : node.children;

      // Get entry data including content
      try {
        const entry = await api.getEntry(node.path);
        const frontmatter = entry.frontmatter || {};
        files.push({
          path: node.path,
          metadata: {
            title: (frontmatter.title as string) || entry.title || node.name,
            part_of: parentPath,
            contents: filteredChildren.length > 0 ? filteredChildren.map(c => c.path) : null,
            // Store file content in extra field for syncing
            extra: {
              _body: entry.content,
              ...Object.fromEntries(
                Object.entries(frontmatter).filter(([k]) =>
                  !['title', 'part_of', 'contents', 'attachments', 'audience', 'description'].includes(k)
                )
              ),
            } as FileMetadata['extra'],
          },
        });
      } catch (e) {
        console.warn('[App] Could not get entry for', node.path, e);
        // Still add the file with basic metadata
        files.push({
          path: node.path,
          metadata: {
            title: node.name,
            part_of: parentPath,
            contents: filteredChildren.length > 0 ? filteredChildren.map(c => c.path) : null,
          },
        });
      }

      // Recurse into children (only filtered ones if audience specified)
      for (const child of filteredChildren) {
        await collectFiles(child, node.path);
      }
    }

    await collectFiles(tree, null);
    console.log('[App] Collected', files.length, 'files with content from filesystem');

    // Populate CRDT
    await populateCrdtFromFiles(files);
    console.log('[App] CRDT populated successfully');
  }

  // Handle add attachment from context menu
  function handleAddAttachment(entryPath: string) {
    pendingAttachmentPath = entryPath;
    attachmentError = null;
    attachmentFileInput?.click();
  }

  // Handle file selection for attachment
  async function handleAttachmentFileSelect(event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file || !api || !pendingAttachmentPath) return;

    // Check size limit (10MB for all files)
    const MAX_SIZE = 10 * 1024 * 1024;
    if (file.size > MAX_SIZE) {
      attachmentError = `File too large (${(file.size / 1024 / 1024).toFixed(1)}MB). Maximum is 10MB.`;
      input.value = "";
      return;
    }

    try {
      // Convert file to base64
      const dataBase64 = await fileToBase64(file);

      // Upload attachment
      const attachmentPath = await api.uploadAttachment(
        pendingAttachmentPath,
        file.name,
        dataBase64,
      );

      // Attachments are synced as part of file metadata via CRDT events

      // Refresh the entry if it's currently open
      if (currentEntry?.path === pendingAttachmentPath) {
        const entry = await api.getEntry(pendingAttachmentPath);
        entry.frontmatter = normalizeFrontmatter(entry.frontmatter);
        currentEntry = entry;

        // If it's an image, also insert it into the editor at cursor
        if (file.type.startsWith("image/") && editorRef) {
          // Get the binary data and create blob URL
          const data = await api.getAttachmentData(
            currentEntry.path,
            attachmentPath,
          );
          const blob = new Blob([new Uint8Array(data)], { type: file.type });
          const blobUrl = URL.createObjectURL(blob);

          // Track for cleanup
          trackBlobUrl(attachmentPath, blobUrl);

          // Insert image at cursor using Editor's insertImage method
          editorRef.insertImage(blobUrl, file.name);
        }
      }

      attachmentError = null;
    } catch (e) {
      attachmentError = e instanceof Error ? e.message : String(e);
    }

    input.value = "";
    pendingAttachmentPath = "";
  }

  // Convert file to base64
  function fileToBase64(file: File): Promise<string> {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => {
        const result = reader.result as string;
        // Extract base64 part from data URL
        const base64 = result.split(",")[1];
        resolve(base64);
      };
      reader.onerror = reject;
      reader.readAsDataURL(file);
    });
  }

  // Handle file drop in Editor - upload and return blob URL for images
  async function handleEditorFileDrop(
    file: File,
  ): Promise<{ blobUrl: string; attachmentPath: string } | null> {
    if (!api || !currentEntry) return null;

    // Check size limit (10MB for all files)
    const MAX_SIZE = 10 * 1024 * 1024;
    if (file.size > MAX_SIZE) {
      attachmentError = `File too large (${(file.size / 1024 / 1024).toFixed(1)}MB). Maximum is 10MB.`;
      return null;
    }

    try {
      const dataBase64 = await fileToBase64(file);
      const attachmentPath = await api.uploadAttachment(
        currentEntry.path,
        file.name,
        dataBase64,
      );

      // Attachments are synced as part of file metadata via CRDT events

      // Refresh the entry to update attachments list
      const entry = await api.getEntry(currentEntry.path);
      entry.frontmatter = normalizeFrontmatter(entry.frontmatter);
      currentEntry = entry;

      // For images, create blob URL for display in editor
      if (file.type.startsWith("image/")) {
        const data = await api.getAttachmentData(
          currentEntry.path,
          attachmentPath,
        );
        // Use the file's actual MIME type when available, fall back to extension-based lookup
        const ext = file.name.split(".").pop()?.toLowerCase() || "";
        const mimeTypes: Record<string, string> = {
          png: "image/png",
          jpg: "image/jpeg",
          jpeg: "image/jpeg",
          gif: "image/gif",
          webp: "image/webp",
          svg: "image/svg+xml",
          bmp: "image/bmp",
          ico: "image/x-icon",
        };
        const mimeType = file.type || mimeTypes[ext] || "image/png";
        const blob = new Blob([new Uint8Array(data)], { type: mimeType });
        const blobUrl = URL.createObjectURL(blob);

        // Track for cleanup
        trackBlobUrl(attachmentPath, blobUrl);

        return { blobUrl, attachmentPath };
      }

      // For non-image files, just return the path (no blob URL for editor display)
      return { blobUrl: "", attachmentPath };
    } catch (e) {
      attachmentError = e instanceof Error ? e.message : String(e);
      return null;
    }
  }

  // Handle delete attachment from RightSidebar
  async function handleDeleteAttachment(attachmentPath: string) {
    if (!api || !currentEntry) return;

    try {
      await api.deleteAttachment(currentEntry.path, attachmentPath);

      // Attachments are synced as part of file metadata via CRDT events

      // Refresh current entry to update attachments list
      const entry = await api.getEntry(currentEntry.path);
      entry.frontmatter = normalizeFrontmatter(entry.frontmatter);
      currentEntry = entry;
      attachmentError = null;
    } catch (e) {
      attachmentError = e instanceof Error ? e.message : String(e);
    }
  }

  // Handle attachment selection from inline picker node
  function handleAttachmentInsert(selection: {
    path: string;
    isImage: boolean;
    blobUrl?: string;
    sourceEntryPath: string;
  }) {
    if (!selection || !editorRef || !currentEntry) return;

    const filename = selection.path.split("/").pop() || selection.path;

    // Calculate relative path from current entry to the attachment
    // This handles ancestor attachments correctly
    const relativePath = computeRelativeAttachmentPath(
      currentEntry.path,
      selection.sourceEntryPath,
      selection.path
    );

    // Always embed mode (link mode removed)
    if (selection.isImage && selection.blobUrl) {
      // Track the blob URL for reverse transformation on save
      trackBlobUrl(relativePath, selection.blobUrl);
      // Insert image with blob URL
      editorRef.insertImage(selection.blobUrl, filename);
    } else {
      // For non-images or images without blob URL, insert markdown syntax
      // This will be converted to blob URL by the attachment service when content is displayed
      const markdown = `![${filename}](${relativePath})`;
      editorRef.setContent(editorRef.getMarkdown() + `\n${markdown}\n`);
    }
  }

  // Handle drag-drop: attach entry to new parent
  async function handleMoveEntry(entryPath: string, newParentPath: string) {
    if (!api) return;
    if (entryPath === newParentPath) return; // Can't attach to self

    console.log(
      `[Drag-Drop] entryPath="${entryPath}" -> newParentPath="${newParentPath}"`,
    );

    try {
      // Attach the entry to the new parent
      // This will:
      // - Add entry to newParent's `contents`
      // - Set entry's `part_of` to point to newParent
      await api.attachEntryToParent(entryPath, newParentPath);

      // CRDT is now automatically updated via backend event subscription
      // (file:moved event triggers CRDT updates)

      await refreshTree();
      await runValidation();
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  // Handle moving an attachment from one entry to another
  async function handleMoveAttachment(
    sourceEntryPath: string,
    targetEntryPath: string,
    attachmentPath: string
  ) {
    if (!api) return;
    if (sourceEntryPath === targetEntryPath) return;

    try {
      await api.moveAttachment(sourceEntryPath, targetEntryPath, attachmentPath);

      // Refresh current entry if it was affected
      if (currentEntry?.path === sourceEntryPath || currentEntry?.path === targetEntryPath) {
        const entry = await api.getEntry(currentEntry.path);
        entry.frontmatter = normalizeFrontmatter(entry.frontmatter);
        currentEntry = entry;
      }

      toast.success("Attachment moved successfully");
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      toast.error(`Failed to move attachment: ${message}`);
    }
  }

  // Handle frontmatter property changes
  async function handlePropertyChange(key: string, value: unknown) {
    if (!api || !currentEntry) return;
    try {
      const path = currentEntry.path;
      // Special handling for title: need to check rename first
      if (key === "title" && typeof value === "string" && value.trim()) {
        // Use a simple slugify for title -> filename conversion
        const newFilename = slugifyTitle(value);
        const currentFilename = currentEntry.path.split("/").pop() || "";

        // Only rename if the filename would actually change
        // For index files (have contents property), compare the directory name
        const isIndex = Array.isArray(currentEntry.frontmatter?.contents);
        const pathParts = currentEntry.path.split("/");
        const currentDir = isIndex
          ? pathParts.slice(-2, -1)[0] || ""
          : currentFilename.replace(/\.md$/, "");
        const newDir = newFilename.replace(/\.md$/, "");

        if (currentDir !== newDir) {
          // Try rename FIRST, before updating frontmatter
          try {
            const oldPath = currentEntry.path;
            const newPath = await api.renameEntry(oldPath, newFilename);
            // Rename succeeded, now update title in frontmatter (at new path)
            await api.setFrontmatterProperty(newPath, key, value);

            // Transfer expanded state from old path to new path
            if (expandedNodes.has(oldPath)) {
              workspaceStore.collapseNode(oldPath);
              workspaceStore.expandNode(newPath);
            }

            // CRDT is now automatically updated via backend event subscription
            // (file:renamed event triggers CRDT updates)

            // Update current entry path and refresh tree
            currentEntry = {
              ...currentEntry,
              path: newPath,
              frontmatter: { ...currentEntry.frontmatter, [key]: value },
            };
            await refreshTree();
            titleError = null; // Clear any previous error
          } catch (renameError) {
            // Rename failed (e.g., target exists), show user-friendly error near title input
            // DON'T update the title - leave frontmatter unchanged
            const errorMsg =
              renameError instanceof Error
                ? renameError.message
                : String(renameError);
            if (
              errorMsg.includes("already exists") ||
              errorMsg.includes("Destination")
            ) {
              titleError = `A file named "${newFilename.replace(".md", "")}" already exists. Choose a different title.`;
            } else {
              titleError = `Could not rename: ${errorMsg}`;
            }
            // Don't update anything - input will show original value
          }
        } else {
          // No rename needed, just update title
          await api.setFrontmatterProperty(currentEntry.path, key, value);
          currentEntry = {
            ...currentEntry,
            frontmatter: { ...currentEntry.frontmatter, [key]: value },
          };

          // Update CRDT
          updateCrdtFileMetadata(path, currentEntry.frontmatter);
          titleError = null;

          // Refresh tree to update title display
          await refreshTree();
        }
      } else {
        // Non-title properties: update normally
        await api.setFrontmatterProperty(currentEntry.path, key, value as JsonValue);
        currentEntry = {
          ...currentEntry,
          frontmatter: { ...currentEntry.frontmatter, [key]: value },
        };

        // Update CRDT
        updateCrdtFileMetadata(path, currentEntry.frontmatter);

        // Refresh tree if contents or part_of changed (affects hierarchy)
        if (key === 'contents' || key === 'part_of') {
          await refreshTree();
        }
      }
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  // Helper function to convert title to kebab-case filename
  function slugifyTitle(title: string): string {
    return title
      .toLowerCase()
      .replace(/[^a-z0-9\s-]/g, '')
      .replace(/\s+/g, '-')
      .replace(/-+/g, '-')
      .replace(/^-|-$/g, '') + '.md';
  }

  async function handlePropertyRemove(key: string) {
    if (!api || !currentEntry) return;
    try {
      const path = currentEntry.path;
      await api.removeFrontmatterProperty(currentEntry.path, key);
      // Update local state
      const newFrontmatter = { ...currentEntry.frontmatter };
      delete newFrontmatter[key];
      currentEntry = { ...currentEntry, frontmatter: newFrontmatter };

      // Update CRDT
      updateCrdtFileMetadata(path, newFrontmatter);
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function handlePropertyAdd(key: string, value: unknown) {
    if (!api || !currentEntry) return;
    try {
      const path = currentEntry.path;
      await api.setFrontmatterProperty(currentEntry.path, key, value as JsonValue);
      // Update local state
      currentEntry = {
        ...currentEntry,
        frontmatter: { ...currentEntry.frontmatter, [key]: value },
      };

      // Update CRDT
      updateCrdtFileMetadata(path, currentEntry.frontmatter);
    } catch (e) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  }

  function getEntryTitle(entry: { path: string; title?: string | null; frontmatter?: Record<string, unknown> }): string {
    // Prioritize frontmatter.title for live updates, fall back to cached title
    const fm = normalizeFrontmatter(entry.frontmatter);
    const frontmatterTitle = fm?.title as string | undefined;
    return (
      frontmatterTitle ??
      entry.title ??
      entry.path.split("/").pop()?.replace(".md", "") ??
      "Untitled"
    );
  }

  /**
   * Handle link clicks in the editor.
   * - Relative links (./file.md, ../folder/file.md) navigate to other notes
   * - External links (http://, https://) open in a new tab
   * - Broken relative links offer to create the file
   */
  async function handleLinkClick(href: string) {
    if (!href) return;

    // External links - open in new tab
    if (href.startsWith("http://") || href.startsWith("https://")) {
      window.open(href, "_blank", "noopener,noreferrer");
      return;
    }

    // Relative link - resolve against current file's directory
    if (!currentEntry) return;

    // Get the directory of the current file
    const currentDir = currentEntry.path.substring(
      0,
      currentEntry.path.lastIndexOf("/"),
    );

    // Resolve the relative path
    let targetPath: string;
    if (href.startsWith("/")) {
      // Absolute path from workspace root
      const workspaceRoot =
        tree?.path?.substring(0, tree.path.lastIndexOf("/")) || "";
      targetPath = workspaceRoot + href;
    } else {
      // Relative path - resolve against current directory
      const parts = [...currentDir.split("/"), ...href.split("/")];
      const resolved: string[] = [];
      for (const part of parts) {
        if (part === "..") {
          resolved.pop();
        } else if (part !== "." && part !== "") {
          resolved.push(part);
        }
      }
      targetPath = resolved.join("/");
    }

    // Ensure .md extension
    if (!targetPath.endsWith(".md")) {
      targetPath += ".md";
    }

    // Try to open the entry
    try {
      if (api) {
        // Check if file exists by trying to get it
        const entry = await api.getEntry(targetPath);
        if (entry) {
          await openEntry(targetPath);
          return;
        }
      }
    } catch {
      // File doesn't exist - offer to create it
      const fileName = targetPath.split("/").pop() || "note.md";
      const create = window.confirm(
        `"${fileName}" doesn't exist.\n\nWould you like to create it?`,
      );
      if (create && api) {
        try {
          // Create the file with basic frontmatter
          const title = fileName.replace(".md", "").replace(/-/g, " ");
          await api.createEntry(targetPath, { title });
          await refreshTree();
          await openEntry(targetPath);
        } catch (e) {
          console.error("Failed to create entry:", e);
          uiStore.setError(e instanceof Error ? e.message : String(e));
        }
      }
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if showNewEntryModal}
  <NewEntryModal
    onSave={createNewEntry}
    onCancel={() => (showNewEntryModal = false)}
  />
{/if}

<!-- Command Palette -->
<CommandPalette
  bind:open={uiStore.showCommandPalette}
  {tree}
  {api}
  currentEntryPath={currentEntry?.path ?? null}
  onOpenEntry={openEntry}
  onNewEntry={() => (showNewEntryModal = true)}
  onDailyEntry={handleDailyEntry}
  onSettings={() => (showSettingsDialog = true)}
  onExport={() => {
    exportPath = currentEntry?.path ?? tree?.path ?? "";
    if (exportPath) showExportDialog = true;
  }}
  onValidate={handleValidateWorkspace}
  onRefreshTree={handleRefreshTree}
  onDuplicateEntry={handleDuplicateCurrentEntry}
  onRenameEntry={handleRenameCurrentEntry}
  onDeleteEntry={handleDeleteCurrentEntry}
  onMoveEntry={handleMoveCurrentEntry}
  onCreateChildEntry={handleCreateChildUnderCurrent}
  onStartShare={handleStartShareSession}
  onJoinSession={handleJoinShareSession}
  onFindInFile={handleFindInFile}
  onWordCount={handleWordCount}
  onImportFromClipboard={handleImportFromClipboard}
  onCopyAsMarkdown={handleCopyAsMarkdown}
/>

<!-- Settings Dialog -->
<SettingsDialog
  bind:open={showSettingsDialog}
  bind:showUnlinkedFiles
  bind:showHiddenFiles
  bind:showEditorTitle
  bind:showEditorPath
  bind:readableLineLength
  bind:focusMode
  workspacePath={tree?.path}
/>

<!-- Export Dialog -->
<ExportDialog
  bind:open={showExportDialog}
  rootPath={exportPath}
  {api}
  onOpenChange={(open) => (showExportDialog = open)}
/>

<!-- Toast Notifications -->
<Toaster />

<!-- Tooltip Provider for keyboard shortcut hints -->
<Tooltip.Provider>

<div class="flex h-dvh bg-background overflow-hidden pt-[env(safe-area-inset-top)]">
  <!-- Left Sidebar -->
  <LeftSidebar
    {tree}
    {currentEntry}
    {isLoading}
    {expandedNodes}
    {validationResult}
    {showUnlinkedFiles}
    {api}
    collapsed={leftSidebarCollapsed}
    onOpenEntry={openEntry}
    onToggleNode={toggleNode}
    onToggleCollapse={toggleLeftSidebar}
    onOpenSettings={() => (showSettingsDialog = true)}
    onMoveEntry={handleMoveEntry}
    onCreateChildEntry={handleCreateChildEntry}
    onDeleteEntry={handleDeleteEntry}
    onExport={(path) => {
      exportPath = path;
      showExportDialog = true;
    }}
    onAddAttachment={handleAddAttachment}
    onMoveAttachment={handleMoveAttachment}
    onRemoveBrokenPartOf={handleRemoveBrokenPartOf}
    onRemoveBrokenContentsRef={handleRemoveBrokenContentsRef}
    onAttachUnlinkedEntry={handleAttachUnlinkedEntry}
    onValidationFix={async () => {
      await refreshTree();
      await runValidation();
      // Refresh current entry to reflect frontmatter changes
      if (currentEntry && api) {
        const entry = await api.getEntry(currentEntry.path);
        entry.frontmatter = normalizeFrontmatter(entry.frontmatter);
        currentEntry = entry;
      }
    }}
    onLoadChildren={loadNodeChildren}
    onValidate={handleValidate}
    onRenameEntry={handleRenameEntry}
    onDuplicateEntry={handleDuplicateEntry}
  />

  <!-- Hidden file input for attachments (accepts all file types) -->
  <input
    type="file"
    bind:this={attachmentFileInput}
    onchange={handleAttachmentFileSelect}
    class="hidden"
  />

  <!-- Main Content Area -->
  <main class="flex-1 flex flex-col overflow-hidden min-w-0">
    {#if currentEntry}
      <EditorHeader
        title={getEntryTitle(currentEntry)}
        path={currentEntry.path}
        {isDirty}
        {isSaving}
        showTitle={showEditorTitle}
        showPath={showEditorPath}
        leftSidebarOpen={!leftSidebarCollapsed}
        rightSidebarOpen={!rightSidebarCollapsed}
        {focusMode}
        readonly={editorReadonly}
        {isDailyEntry}
        onSave={save}
        onToggleLeftSidebar={toggleLeftSidebar}
        onToggleRightSidebar={toggleRightSidebar}
        onOpenCommandPalette={uiStore.openCommandPalette}
        onPrevDay={handlePrevDay}
        onNextDay={handleNextDay}
      />

      <EditorContent
        {Editor}
        bind:editorRef
        content={displayContent}
        editorKey={currentEntry.path}
        {readableLineLength}
        readonly={editorReadonly}
        onchange={handleContentChange}
        onblur={handleEditorBlur}
        entryPath={currentEntry.path}
        {api}
        onAttachmentInsert={handleAttachmentInsert}
        onFileDrop={handleEditorFileDrop}
        onLinkClick={handleLinkClick}
      />
    {:else}
      <EditorEmptyState
        {leftSidebarCollapsed}
        onToggleLeftSidebar={toggleLeftSidebar}
        onOpenCommandPalette={uiStore.openCommandPalette}
      />
    {/if}
  </main>

  <!-- Right Sidebar (Properties & History) -->
  <RightSidebar
    entry={currentEntry}
    collapsed={rightSidebarCollapsed}
    onToggleCollapse={toggleRightSidebar}
    onPropertyChange={handlePropertyChange}
    onPropertyRemove={handlePropertyRemove}
    onPropertyAdd={handlePropertyAdd}
    {titleError}
    onTitleErrorClear={() => (titleError = null)}
    onDeleteAttachment={handleDeleteAttachment}
    {attachmentError}
    onAttachmentErrorClear={() => (attachmentError = null)}
    {rustApi}
    onHistoryRestore={async () => {
      // Refresh current entry after restore
      if (currentEntry) {
        await openEntry(currentEntry.path);
      }
    }}
    onBeforeHost={async (audience) => await populateCrdtBeforeHost(audience)}
    onOpenEntry={async (path) => await openEntry(path)}
    {api}
    requestedTab={requestedSidebarTab}
    onRequestedTabConsumed={() => (requestedSidebarTab = null)}
    {triggerStartSession}
    onTriggerStartSessionConsumed={() => (triggerStartSession = false)}
  />
</div>

</Tooltip.Provider>
