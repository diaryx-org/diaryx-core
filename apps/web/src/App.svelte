<script lang="ts">
  import { onMount, onDestroy, tick } from "svelte";
  import { getBackend, type TreeNode } from "./lib/backend";
  import { createApi, type Api } from "./lib/backend/api";
  import type { JsonValue } from "./lib/backend/generated/serde_json/JsonValue";
  import {
    getCollaborativeDocument,
    disconnectDocument,
    destroyDocument,
    setWorkspaceId,
    setCollaborationServer,
  } from "./lib/collaborationUtils";
  import {
    disconnectWorkspace,
    reconnectWorkspace,
    destroyWorkspace,
    setWorkspaceServer,
  } from "./lib/workspaceCrdt";
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
  import { toast } from "svelte-sonner";
  // Note: Button, icons, and LoadingSpinner are now only used in extracted view components
  
  // Import stores
  import { 
    entryStore, 
    uiStore, 
    collaborationStore, 
    workspaceStore,
    getThemeStore
  } from "./models/stores";

  // Initialize theme store immediately
  getThemeStore();
  
  // Import services
  import {
    revokeBlobUrls,
    transformAttachmentPaths,
    reverseBlobUrlsToAttachmentPaths,
    trackBlobUrl,
    initializeWorkspaceCrdt,
    updateCrdtFileMetadata,
    addFileToCrdt,
  } from "./models/services";

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
  let error = $derived(uiStore.error);
  let editorRef = $derived(uiStore.editorRef);
  
  // Workspace state - proxied from workspaceStore
  let tree = $derived(workspaceStore.tree);
  let expandedNodes = $derived(workspaceStore.expandedNodes);
  let validationResult = $derived(workspaceStore.validationResult);
  let workspaceCrdtInitialized = $derived(workspaceStore.workspaceCrdtInitialized);
  let workspaceId = $derived(workspaceStore.workspaceId);
  let backend = $derived(workspaceStore.backend);
  let showUnlinkedFiles = $derived(workspaceStore.showUnlinkedFiles);
  let showHiddenFiles = $derived(workspaceStore.showHiddenFiles);

  // API wrapper - uses execute() internally for all operations
  let api: Api | null = $derived(backend ? createApi(backend) : null);
  
  // Collaboration state - proxied from collaborationStore  
  let currentYDoc = $derived(collaborationStore.currentYDoc);
  let currentProvider = $derived(collaborationStore.currentProvider);
  let currentCollaborationPath = $derived(collaborationStore.currentCollaborationPath);
  let collaborationEnabled = $derived(collaborationStore.collaborationEnabled);
  let collaborationConnected = $derived(collaborationStore.collaborationConnected);
  let collaborationServerUrl = $derived(collaborationStore.collaborationServerUrl);

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

  // Check if we're on desktop and expand sidebars by default
  onMount(async () => {
    // Expand sidebars on desktop
    if (window.innerWidth >= 768) {
      leftSidebarCollapsed = false;
      rightSidebarCollapsed = false;
    }

    // Load saved collaboration settings
    if (typeof window !== "undefined") {
      const savedServerUrl = localStorage.getItem("diaryx-sync-server");
      if (savedServerUrl) {
        collaborationStore.setServerUrl(savedServerUrl);
        setCollaborationServer(savedServerUrl);
        setWorkspaceServer(savedServerUrl);
        // Auto-enable collaboration if we have a saved server
        // collaborationStore.setEnabled(true); // DISABLED BY DEFAULT per user request
      }
    }

    try {
      // Dynamically import the Editor component
      const module = await import("./lib/Editor.svelte");
      Editor = module.default;

      // Initialize the backend (auto-detects Tauri vs WASM)
      workspaceStore.setBackend(await getBackend());

      // Start auto-persist for WASM backend (no-op for Tauri)
      //startAutoPersist(5000);

      // Initialize workspace CRDT (unless disabled for debugging)
      if (!workspaceCrdtDisabled) {
        await setupWorkspaceCrdt();
      } else {
        console.log(
          "[App] Workspace CRDT disabled via VITE_DISABLE_WORKSPACE_CRDT",
        );
      }

      await refreshTree();

      // Expand root and open it by default
      if (tree && !currentEntry) {
        expandedNodes.add(tree.path);
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
    // Disconnect workspace CRDT (keeps local state for quick reconnect)
    disconnectWorkspace();
  });

  // Initialize the workspace CRDT
  // Initialize the workspace CRDT
  async function setupWorkspaceCrdt(retryCount = 0) {
    if (!api || !backend) return;

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

      // Initialize workspace CRDT using service
      // Note: initializeWorkspaceCrdt needs both backend (for events) and api (for data operations)
      workspaceCrdtInitialized = await initializeWorkspaceCrdt(
        workspaceId,
        workspacePath,
        collaborationServerUrl,
        collaborationEnabled,
        backend,
        api,
        {
          onFilesChange: async (files: Map<string, any>) => {
            console.log("[App] Workspace CRDT files changed:", files.size);
            // Refresh tree to show updated metadata (titles, etc.)
            await refreshTree();
            // Reload current entry if it was updated to show new metadata
            if (currentEntry && files.has(currentEntry.path) && api) {
              try {
                const entry = await api.getEntry(currentEntry.path);
                entry.frontmatter = normalizeFrontmatter(entry.frontmatter);
                currentEntry = entry;
              } catch {
                // File might have been deleted
              }
            }
          },
          onConnectionChange: (connected: boolean) => {
            console.log("[App] Workspace CRDT connection:", connected ? "online" : "offline");
            collaborationStore.setConnected(connected);
          },
          onRemoteFileSync: async (created: string[], deleted: string[]) => {
            console.log(`[App] Remote file sync: created ${created.length}, deleted ${deleted.length}`);
            if (created.length > 0 || deleted.length > 0) {
              await refreshTree();
              await runValidation();
            }
          },
        },
      );
    } catch (e) {
      console.error("[App] Failed to initialize workspace CRDT:", e);
      workspaceCrdtInitialized = false;
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
          await tick();
          // Delay destruction to avoid `ystate.doc` errors from TipTap
          setTimeout(() => {
            try {
              destroyDocument(pathToDestroy);
            } catch (e) {
              console.warn(
                "[App] Failed to destroy collaboration session:",
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

            const { ydoc, provider } = getCollaborativeDocument(newRelativePath, {
              initialContent: currentEntry.content, // Seed Y.Doc with content on first create
            });
            collaborationStore.setCollaborationSession(ydoc, provider, newRelativePath);
          } catch (e) {
            console.warn("[App] Failed to setup collaboration:", e);
            collaborationStore.clearCollaborationSession();
          }
        }
      } else {
        entryStore.setDisplayContent("");
        collaborationStore.clearCollaborationSession();
      }

      entryStore.markClean();
      uiStore.clearError();
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

  // Handle content changes - triggers debounced auto-save
  function handleContentChange(_markdown: string) {
    entryStore.markDirty();
    scheduleAutoSave();
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
    if (expandedNodes.has(path)) {
      expandedNodes.delete(path);
    } else {
      expandedNodes.add(path);
    }
    expandedNodes = new Set(expandedNodes); // Trigger reactivity
  }

  // Sidebar toggles
  function toggleLeftSidebar() {
    leftSidebarCollapsed = !leftSidebarCollapsed;
  }

  function toggleRightSidebar() {
    rightSidebarCollapsed = !rightSidebarCollapsed;
  }

  // Keyboard shortcuts
  // Collaboration handlers
  async function handleCollaborationToggle(enabled: boolean) {
    collaborationStore.setEnabled(enabled);

    if (enabled) {
      // Reconnect with the current server URL
      if (collaborationServerUrl) {
        setCollaborationServer(collaborationServerUrl);
        setWorkspaceServer(collaborationServerUrl);
      }
      // Re-initialize workspace CRDT if needed
      if (!workspaceCrdtInitialized && !workspaceCrdtDisabled) {
        await setupWorkspaceCrdt();
      } else {
        reconnectWorkspace();
      }
      // Refresh current entry to establish collaboration
      if (currentEntry) {
        await openEntry(currentEntry.path);
      }
    } else {
      // Disconnect collaboration
      collaborationStore.setConnected(false);
      disconnectWorkspace();
      if (currentCollaborationPath) {
        disconnectDocument(currentCollaborationPath);
        collaborationStore.clearCollaborationSession();
      }
      // Refresh current entry without collaboration
      if (currentEntry) {
        await openEntry(currentEntry.path);
      }
    }
  }

  async function handleCollaborationReconnect() {
    if (!collaborationEnabled) return;

    // Reload server URL from localStorage in case it was updated
    const savedUrl = localStorage.getItem("diaryx-sync-server");
    if (savedUrl) {
      collaborationStore.setServerUrl(savedUrl);
      setCollaborationServer(savedUrl);
      setWorkspaceServer(savedUrl);
    }

    // Disconnect and reconnect
    collaborationStore.setConnected(false);
    disconnectWorkspace();
    if (currentCollaborationPath) {
      disconnectDocument(currentCollaborationPath);
      collaborationStore.clearCollaborationSession();
    }

    // Re-initialize
    if (!workspaceCrdtDisabled) {
      await destroyWorkspace();
      workspaceStore.setWorkspaceCrdtInitialized(false);
      await setupWorkspaceCrdt();
    }

    // Refresh current entry
    if (currentEntry) {
      await openEntry(currentEntry.path);
    }
  }

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
    // Toggle left sidebar with Cmd/Ctrl + B
    if ((event.metaKey || event.ctrlKey) && event.key === "b") {
      event.preventDefault();
      toggleLeftSidebar();
    }
    // Toggle right sidebar with Cmd/Ctrl + Shift + I (for Info)
    if (
      (event.metaKey || event.ctrlKey) &&
      event.shiftKey &&
      event.key === "I"
    ) {
      event.preventDefault();
      toggleRightSidebar();
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
      await openEntry(newPath);
      await runValidation();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function createNewEntry(path: string, title: string) {
    if (!api) return;
    try {
      const newPath = await api.createEntry(path, { title });

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
    if (!api) return;
    try {
      const path = await api.ensureDailyEntry();
      await refreshTree();
      await openEntry(path);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function handleDeleteEntry(path: string) {
    if (!api) return;
    const confirm = window.confirm(
      `Are you sure you want to delete "${path.split("/").pop()?.replace(".md", "")}"?`,
    );
    if (!confirm) return;

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
        await runValidation();
      } catch (refreshError) {
        console.warn("[App] Error refreshing tree after delete:", refreshError);
        // Try again after a short delay
        setTimeout(async () => {
          try {
            if (backend) {
              await refreshTree();
              await runValidation();
            }
          } catch (e) {
            console.error("[App] Retry tree refresh failed:", e);
          }
        }, 500);
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  // Run workspace validation
  async function runValidation() {
    if (!api || !backend) return;
    try {
      // Pass the actual workspace root path for validation
      // tree?.path is the root index file path (e.g., "/Users/.../workspace/index.md")
      // This is required for Tauri which uses absolute filesystem paths
      // Fall back to backend.getWorkspacePath() if tree is not yet loaded
      const rootPath = tree?.path ?? backend.getWorkspacePath();
      workspaceStore.setValidationResult(await api.validateWorkspace(rootPath));
      console.log("[App] Validation result:", validationResult);
      console.log("[App] Warnings:", validationResult?.warnings);
    } catch (e) {
      console.error("[App] Validation error:", e);
    }
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
      error = e instanceof Error ? e.message : String(e);
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
      error = e instanceof Error ? e.message : String(e);
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
      error = e instanceof Error ? e.message : String(e);
    }
  }

  // Depth limit for initial tree loading (lazy loading)
  const TREE_INITIAL_DEPTH = 2;

  // Refresh the tree using the appropriate method based on showUnlinkedFiles setting
  async function refreshTree() {
    if (!api || !backend) return;
    try {
      // Get the workspace directory from the backend
      const workspaceDir = backend.getWorkspacePath().replace(/\/index\.md$/, '').replace(/\/README\.md$/, '');

      if (showUnlinkedFiles) {
        // "Show All Files" mode - use filesystem tree with depth limit
        workspaceStore.setTree(await api.getFilesystemTree(workspaceDir, showHiddenFiles, TREE_INITIAL_DEPTH));
      } else {
        // Normal mode - find the actual root index and use hierarchy tree with depth limit
        try {
          const rootIndexPath = await api.findRootIndex(workspaceDir);
          workspaceStore.setTree(await api.getWorkspaceTree(rootIndexPath, TREE_INITIAL_DEPTH));
        } catch (e) {
          console.warn("[App] Could not find root index for tree:", e);
          // Fall back to filesystem tree if no root index found
          workspaceStore.setTree(await api.getFilesystemTree(workspaceDir, showHiddenFiles, TREE_INITIAL_DEPTH));
        }
      }
    } catch (e) {
      console.error("[App] Error refreshing tree:", e);
    }
  }

  // Load children for a node (lazy loading when user expands)
  async function loadNodeChildren(nodePath: string) {
    if (!api) return;
    try {
      let subtree: TreeNode;

      if (showUnlinkedFiles) {
        // Filesystem tree mode - need directory path
        // If nodePath ends with .md, it's an index file - use parent directory
        const dirPath = nodePath.endsWith('.md')
          ? nodePath.substring(0, nodePath.lastIndexOf('/'))
          : nodePath;
        subtree = await api.getFilesystemTree(dirPath, showHiddenFiles, TREE_INITIAL_DEPTH);
      } else {
        // Workspace tree mode - use index file path directly
        subtree = await api.getWorkspaceTree(nodePath, TREE_INITIAL_DEPTH);
      }

      // Merge into existing tree
      workspaceStore.updateSubtree(nodePath, subtree);
    } catch (e) {
      console.error("[App] Error loading children for", nodePath, e);
    }
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

  // Handle image insert from Editor toolbar
  function handleEditorImageInsert() {
    if (!currentEntry) return;
    pendingAttachmentPath = currentEntry.path;
    attachmentFileInput?.click();
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
      error = e instanceof Error ? e.message : String(e);
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
              expandedNodes.delete(oldPath);
              expandedNodes.add(newPath);
              expandedNodes = expandedNodes; // trigger reactivity
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
      error = e instanceof Error ? e.message : String(e);
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
      error = e instanceof Error ? e.message : String(e);
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
      error = e instanceof Error ? e.message : String(e);
    }
  }

  function exportEntry() {
    if (!currentEntry) return;

    // Reconstruct full markdown with frontmatter
    let fullContent = "";
    if (
      currentEntry.frontmatter &&
      Object.keys(currentEntry.frontmatter).length > 0
    ) {
      const yamlLines = ["---"];
      for (const [key, value] of Object.entries(currentEntry.frontmatter)) {
        if (Array.isArray(value)) {
          yamlLines.push(`${key}:`);
          for (const item of value) {
            yamlLines.push(`  - ${item}`);
          }
        } else if (typeof value === "string" && value.includes("\n")) {
          // Multi-line string
          yamlLines.push(`${key}: |`);
          for (const line of value.split("\n")) {
            yamlLines.push(`  ${line}`);
          }
        } else if (typeof value === "string") {
          // Quote strings that might need it
          const needsQuotes = /[:#{}[\],&*?|<>=!%@`]/.test(value);
          yamlLines.push(
            `${key}: ${needsQuotes ? `"${value.replace(/"/g, '\\"')}"` : value}`,
          );
        } else {
          yamlLines.push(`${key}: ${JSON.stringify(value)}`);
        }
      }
      yamlLines.push("---");
      fullContent = yamlLines.join("\n") + "\n" + currentEntry.content;
    } else {
      fullContent = currentEntry.content;
    }

    const blob = new Blob([fullContent], {
      type: "text/markdown;charset=utf-8",
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = currentEntry.path.split("/").pop() || "entry.md";
    a.click();
    URL.revokeObjectURL(url);
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
          error = e instanceof Error ? e.message : String(e);
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
  onOpenEntry={openEntry}
  onNewEntry={() => (showNewEntryModal = true)}
  onDailyEntry={handleDailyEntry}
  onSettings={() => (showSettingsDialog = true)}
  onExport={() => {
    exportPath = currentEntry?.path ?? tree?.path ?? "";
    if (exportPath) showExportDialog = true;
  }}
  onAddAttachment={() => currentEntry && handleAddAttachment(currentEntry.path)}
/>

<!-- Settings Dialog -->
<SettingsDialog
  bind:open={showSettingsDialog}
  bind:showUnlinkedFiles
  bind:showHiddenFiles
  workspacePath={tree?.path}
  bind:collaborationEnabled
  {collaborationConnected}
  onCollaborationToggle={handleCollaborationToggle}
  onCollaborationReconnect={handleCollaborationReconnect}
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

<div class="flex h-screen bg-background overflow-hidden">
  <!-- Left Sidebar -->
  <LeftSidebar
    {tree}
    {currentEntry}
    {isLoading}
    {error}
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
    }}
    onLoadChildren={loadNodeChildren}
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
        onSave={save}
        onExport={exportEntry}
        onToggleLeftSidebar={toggleLeftSidebar}
        onToggleRightSidebar={toggleRightSidebar}
        onOpenCommandPalette={uiStore.openCommandPalette}
      />

      <EditorContent
        {Editor}
        bind:editorRef
        content={displayContent}
        editorKey={`${currentCollaborationPath ?? currentEntry.path}:${collaborationEnabled ? "collab" : "local"}`}
        {collaborationEnabled}
        {currentYDoc}
        {currentProvider}
        onchange={handleContentChange}
        onblur={handleEditorBlur}
        onInsertImage={handleEditorImageInsert}
        onFileDrop={handleEditorFileDrop}
        onLinkClick={handleLinkClick}
      />
    {:else}
      <EditorEmptyState
        {leftSidebarCollapsed}
        onToggleLeftSidebar={toggleLeftSidebar}
      />
    {/if}
  </main>

  <!-- Right Sidebar (Properties) -->
  <RightSidebar
    entry={currentEntry}
    collapsed={rightSidebarCollapsed}
    onToggleCollapse={toggleRightSidebar}
    onPropertyChange={handlePropertyChange}
    onPropertyRemove={handlePropertyRemove}
    onPropertyAdd={handlePropertyAdd}
    {titleError}
    onTitleErrorClear={() => (titleError = null)}
    onAddAttachment={() =>
      currentEntry && handleAddAttachment(currentEntry.path)}
    onDeleteAttachment={handleDeleteAttachment}
    {attachmentError}
    onAttachmentErrorClear={() => (attachmentError = null)}
  />
</div>
