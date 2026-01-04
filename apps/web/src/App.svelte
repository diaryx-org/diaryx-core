<script lang="ts">
  import { onMount, onDestroy, tick } from "svelte";
  import {
    getBackend,
    startAutoPersist,
    stopAutoPersist,
    persistNow,
    type Backend,
    type TreeNode,
    type EntryData,
    type ValidationResult,
  } from "./lib/backend";
  import {
    getCollaborativeDocument,
    disconnectDocument,
    setWorkspaceId,
  } from "./lib/collaborationUtils";
  import {
    initWorkspace,
    disconnectWorkspace,
    getFileMetadata,
    updateFileMetadata,
    deleteFile as crdtDeleteFile,
    addToContents,
    removeFromContents,
    moveFile as crdtMoveFile,
    renameFile as crdtRenameFile,
    addAttachment as crdtAddAttachment,
    removeAttachment as crdtRemoveAttachment,
    syncFromBackend,
    garbageCollect,
    getWorkspaceStats,
    type FileMetadata,
    type BinaryRef,
  } from "./lib/workspaceCrdt";
  import type { Doc as YDoc } from "yjs";
  import type { HocuspocusProvider } from "@hocuspocus/provider";
  import LeftSidebar from "./lib/LeftSidebar.svelte";
  import RightSidebar from "./lib/RightSidebar.svelte";
  import NewEntryModal from "./lib/NewEntryModal.svelte";
  import CommandPalette from "./lib/CommandPalette.svelte";
  import SettingsDialog from "./lib/SettingsDialog.svelte";
  import ExportDialog from "./lib/ExportDialog.svelte";
  import { Toaster } from "$lib/components/ui/sonner";
  import { Button } from "$lib/components/ui/button";
  import { Save, Download, PanelLeft, PanelRight, Menu } from "@lucide/svelte";

  // Dynamically import Editor to avoid SSR issues
  let Editor: typeof import("./lib/Editor.svelte").default | null =
    $state(null);

  // Backend instance
  let backend: Backend | null = $state(null);

  // State
  let tree: TreeNode | null = $state(null);
  let currentEntry: EntryData | null = $state(null);
  let isDirty = $state(false);
  let isLoading = $state(true);
  let error: string | null = $state(null);
  let expandedNodes = $state(new Set<string>());
  let editorRef: any = $state(null);
  let showNewEntryModal = $state(false);
  let validationResult: ValidationResult | null = $state(null);
  let titleError: string | null = $state(null);

  // Sidebar states - collapsed by default on mobile
  let leftSidebarCollapsed = $state(true);
  let rightSidebarCollapsed = $state(true);

  // Modal states
  let showCommandPalette = $state(false);
  let showSettingsDialog = $state(false);
  let showExportDialog = $state(false);
  let exportPath = $state("");

  // Y.js collaboration state
  let currentYDoc: YDoc | null = $state(null);
  let currentProvider: HocuspocusProvider | null = $state(null);
  let currentCollaborationPath: string | null = $state(null); // Track absolute path for cleanup
  let collaborationEnabled = $state(true); // Toggle for enabling/disabling collaboration

  // Workspace CRDT state
  let workspaceCrdtInitialized = $state(false);
  let workspaceId: string | null = $state(null);

  // Collaboration server URL (null for local-only mode)
  const collaborationServerUrl: string | null =
    typeof import.meta !== "undefined" &&
    (import.meta as any).env?.VITE_COLLAB_SERVER
      ? (import.meta as any).env.VITE_COLLAB_SERVER
      : "ws://localhost:1234";

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

  /**
   * WASM backend may create a workspace root file without YAML frontmatter (e.g. just "# My Workspace").
   * Our frontmatter getter returns `{}` in that case, and writing a frontmatter property can succeed
   * but appears "missing" until the file is reconstructed with actual frontmatter delimiters.
   *
   * This helper ensures the workspace root index always has a frontmatter header so `workspace_id`
   * (and other metadata) can be stored and read consistently.
   */
  async function ensureWorkspaceRootHasFrontmatter(
    indexPath: string,
  ): Promise<void> {
    if (!backend) return;

    // Check if frontmatter already exists by looking for any existing properties
    // If frontmatter exists (even empty), the file already has frontmatter delimiters
    const frontmatter = await backend.getFrontmatter(indexPath);

    // If frontmatter has properties, it definitely exists - nothing to do
    if (Object.keys(frontmatter).length > 0) return;

    // Check if the file physically has frontmatter by looking at raw content
    // We need to use getEntry and check if frontmatter is not empty, or use a raw read
    const entry = await backend.getEntry(indexPath);
    const body = entry?.content ?? "";

    // If getFrontmatter() returned {} but the file was created by our backend,
    // it should have frontmatter. The issue is when a file has no frontmatter at all.
    // We can detect this by checking if title exists (our backend always sets title).
    // If no title and no other frontmatter keys, the file likely has no frontmatter block.

    // For safety, if frontmatter is empty, re-save to ensure frontmatter is created
    // The save_content function in core preserves frontmatter and creates it if missing
    await backend.saveEntry(indexPath, body);
    await persistNow();
  }

  // Display settings - initialized from localStorage if available
  let showUnlinkedFiles = $state(
    typeof window !== "undefined"
      ? localStorage.getItem("diaryx-show-unlinked-files") !== "false"
      : true,
  );
  let showHiddenFiles = $state(
    typeof window !== "undefined"
      ? localStorage.getItem("diaryx-show-hidden-files") === "true"
      : false,
  );

  // Attachment state
  let pendingAttachmentPath = $state("");
  let attachmentError: string | null = $state(null);
  let attachmentFileInput: HTMLInputElement | null = $state(null);

  // Blob URL tracking for attachments
  let blobUrlMap = $state(new Map<string, string>()); // originalPath -> blobUrl
  let displayContent = $state(""); // Content with blob URLs for editor

  // Revoke all blob URLs (cleanup)
  function revokeBlobUrls() {
    for (const url of blobUrlMap.values()) {
      URL.revokeObjectURL(url);
    }
    blobUrlMap.clear();
  }

  // Transform attachment paths in content to blob URLs
  async function transformAttachmentPaths(
    content: string,
    entryPath: string,
  ): Promise<string> {
    if (!backend) return content;

    // Find all image references: ![alt](...) or ![alt](<...>) for paths with spaces
    const imageRegex = /!\[([^\]]*)\]\((?:<([^>]+)>|([^)]+))\)/g;
    let match;
    const replacements: { original: string; replacement: string }[] = [];

    while ((match = imageRegex.exec(content)) !== null) {
      const [fullMatch, alt] = match;
      // Angle bracket path is in group 2, regular path is in group 3
      const imagePath = match[2] || match[3];

      // Skip external URLs
      if (imagePath.startsWith("http://") || imagePath.startsWith("https://")) {
        continue;
      }

      // Skip already-transformed blob URLs
      if (imagePath.startsWith("blob:")) {
        continue;
      }

      try {
        // Try to read the attachment data
        const data = await backend.getAttachmentData(entryPath, imagePath);

        // Determine MIME type from extension
        const ext = imagePath.split(".").pop()?.toLowerCase() || "";
        const mimeTypes: Record<string, string> = {
          png: "image/png",
          jpg: "image/jpeg",
          jpeg: "image/jpeg",
          gif: "image/gif",
          webp: "image/webp",
          svg: "image/svg+xml",
          pdf: "application/pdf",
        };
        const mimeType = mimeTypes[ext] || "application/octet-stream";

        // Create blob and URL
        const blob = new Blob([new Uint8Array(data)], { type: mimeType });
        const blobUrl = URL.createObjectURL(blob);

        // Track for cleanup
        blobUrlMap.set(imagePath, blobUrl);

        // Queue replacement
        replacements.push({
          original: fullMatch,
          replacement: `![${alt}](${blobUrl})`,
        });
      } catch (e) {
        // Attachment not found or error - leave original path
        console.warn(`[App] Could not load attachment: ${imagePath}`, e);
      }
    }

    // Apply replacements
    let result = content;
    for (const { original, replacement } of replacements) {
      result = result.replace(original, replacement);
    }

    return result;
  }

  // Reverse-transform blob URLs back to attachment paths (for saving)
  // Wraps paths with spaces in angle brackets for CommonMark compatibility
  function reverseBlobUrlsToAttachmentPaths(content: string): string {
    let result = content;

    // Iterate through blobUrlMap (originalPath -> blobUrl) and replace blob URLs with original paths
    for (const [originalPath, blobUrl] of blobUrlMap.entries()) {
      // Wrap path in angle brackets if it contains spaces (CommonMark spec)
      const pathToUse = originalPath.includes(" ")
        ? `<${originalPath}>`
        : originalPath;
      // Replace all occurrences of the blob URL with the original path
      result = result.replaceAll(blobUrl, pathToUse);
    }

    return result;
  }

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

    try {
      // Dynamically import the Editor component
      const module = await import("./lib/Editor.svelte");
      Editor = module.default;

      // Initialize the backend (auto-detects Tauri vs WASM)
      backend = await getBackend();

      // Start auto-persist for WASM backend (no-op for Tauri)
      startAutoPersist(5000);

      // Initialize workspace CRDT (unless disabled for debugging)
      if (!workspaceCrdtDisabled) {
        await initializeWorkspaceCrdt();
      } else {
        console.log(
          "[App] Workspace CRDT disabled via VITE_DISABLE_WORKSPACE_CRDT",
        );
      }

      await refreshTree();

      // Expand root by default
      if (tree) {
        expandedNodes.add(tree.path);
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
          showCommandPalette = true;
        }
      };
      document.addEventListener("touchstart", handleTouchStart);
      document.addEventListener("touchend", handleTouchEnd);
    } catch (e) {
      console.error("[App] Initialization error:", e);
      error = e instanceof Error ? e.message : String(e);
    } finally {
      isLoading = false;
    }
  });

  onDestroy(() => {
    // Stop auto-persist and do a final persist
    stopAutoPersist();
    persistNow();
    // Cleanup blob URLs
    revokeBlobUrls();
    // Disconnect workspace CRDT (keeps local state for quick reconnect)
    disconnectWorkspace();
  });

  // Initialize the workspace CRDT
  async function initializeWorkspaceCrdt() {
    if (!backend) return;

    try {
      // Workspace ID priority:
      // 1. Environment variable VITE_WORKSPACE_ID (best for multi-device, avoids bootstrap issue)
      // 2. workspace_id from root index frontmatter (should persist in the workspace index)
      // 3. null (no prefix - uses simple room names like "doc:path/to/file.md")
      let sharedWorkspaceId: string | null = envWorkspaceId;

      if (sharedWorkspaceId) {
        console.log(
          "[App] Using workspace_id from environment:",
          sharedWorkspaceId,
        );
      } else {
        // Try to get/create workspace_id from root index frontmatter
        try {
          const rootTree = await backend.getWorkspaceTree();
          console.log("[App] Workspace tree root path:", rootTree?.path);

          if (rootTree?.path) {
            // =========================================================================
            // Startup debug probe:
            // Compare the raw entry content + entry.frontmatter vs getFrontmatter() for
            // the exact same path to diagnose mismatches in WASM mode.
            // =========================================================================
            try {
              const rootEntryProbe = await backend.getEntry(rootTree.path);
              const probeContent = rootEntryProbe?.content ?? "";
              const probeHead = probeContent.slice(0, 600);

              console.log("[App] Root entry probe path:", rootEntryProbe?.path);
              console.log(
                "[App] Root entry probe frontmatter keys:",
                Object.keys(rootEntryProbe?.frontmatter ?? {}),
              );
              console.log(
                "[App] Root entry probe workspace_id (from getEntry.frontmatter):",
                (rootEntryProbe?.frontmatter as any)?.workspace_id,
              );
              console.log(
                "[App] Root entry probe content head (first 600 chars):",
                probeHead,
              );
              console.log(
                "[App] Root entry probe starts with frontmatter delimiter:",
                probeHead.startsWith("---"),
              );
              console.log(
                "[App] Root entry probe contains closing frontmatter delimiter:",
                probeHead.includes("\n---"),
              );

              const rootFrontmatterProbe = await backend.getFrontmatter(
                rootTree.path,
              );
              console.log(
                "[App] Root getFrontmatter() keys:",
                Object.keys(rootFrontmatterProbe ?? {}),
              );
              console.log(
                "[App] Root getFrontmatter() workspace_id:",
                (rootFrontmatterProbe as any)?.workspace_id,
              );
            } catch (e) {
              console.warn("[App] Root entry/frontmatter probe failed:", e);
            }

            // Ensure the root index has an actual frontmatter block so workspace_id can persist.
            // Without this, getFrontmatter() returns {} and workspace_id reads as undefined.
            try {
              await ensureWorkspaceRootHasFrontmatter(rootTree.path);
            } catch (e) {
              console.warn(
                "[App] Failed to ensure root frontmatter exists (continuing):",
                e,
              );
            }

            const rootFrontmatter = await backend.getFrontmatter(rootTree.path);
            console.log(
              "[App] Root frontmatter keys:",
              Object.keys(rootFrontmatter ?? {}),
            );
            console.log(
              "[App] Root workspace_id (raw):",
              (rootFrontmatter as any)?.workspace_id,
            );

            sharedWorkspaceId =
              (rootFrontmatter.workspace_id as string) ?? null;

            // If no workspace_id exists, generate one and save it
            if (!sharedWorkspaceId) {
              sharedWorkspaceId = generateUUID();
              console.log(
                "[App] workspace_id missing in index; generating:",
                sharedWorkspaceId,
              );

              await backend.setFrontmatterProperty(
                rootTree.path,
                "workspace_id",
                sharedWorkspaceId,
              );

              console.log(
                "[App] Wrote workspace_id to index, persisting...",
                sharedWorkspaceId,
              );

              await persistNow();

              // Re-read to confirm it actually persisted (especially important in WASM mode)
              const verifyFrontmatter = await backend.getFrontmatter(
                rootTree.path,
              );
              console.log(
                "[App] Verified workspace_id after write:",
                (verifyFrontmatter as any)?.workspace_id,
              );

              // Also re-read raw content to confirm it actually wrote frontmatter into the file.
              try {
                const rootEntryAfter = await backend.getEntry(rootTree.path);
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
          console.warn("[App] Could not get/set workspace_id from index:", e);
          // Fall back to null - will use simple room names without workspace prefix
          console.log("[App] Using no workspace_id prefix (simple room names)");
        }
      }

      workspaceId = sharedWorkspaceId;

      // Set workspace ID for per-file document room naming
      // If null, rooms will be "doc:{path}" instead of "{id}:doc:{path}"
      setWorkspaceId(workspaceId);

      // Initialize workspace CRDT
      await initWorkspace({
        workspaceId: workspaceId ?? undefined,
        serverUrl: collaborationEnabled ? collaborationServerUrl : null,
        onFilesChange: (files) => {
          console.log("[App] Workspace CRDT files changed:", files.size);
          // NOTE: We intentionally do NOT rebuild the tree from CRDT here.
          // The tree continues to come from the backend to avoid sync issues.
          // The CRDT is used only for syncing metadata between clients.
          // In the future, we can enable CRDT-driven tree once the sync is more robust.
        },
        onConnectionChange: (connected) => {
          console.log(
            "[App] Workspace CRDT connection:",
            connected ? "online" : "offline",
          );
        },
      });

      workspaceCrdtInitialized = true;

      // Sync existing files from backend into CRDT
      await syncFromBackend(backend);

      // Garbage collect old deleted files (older than 7 days)
      const purged = garbageCollect(7 * 24 * 60 * 60 * 1000);
      if (purged > 0) {
        console.log(
          `[App] Garbage collected ${purged} old deleted files from CRDT`,
        );
      }

      const stats = getWorkspaceStats();
      console.log(
        `[App] Workspace CRDT initialized: ${stats.activeFiles} files, ${stats.totalAttachments} attachments`,
      );
    } catch (e) {
      console.error("[App] Failed to initialize workspace CRDT:", e);
      // Continue without CRDT - fall back to backend-only mode
      workspaceCrdtInitialized = false;
    }
  }

  // Update CRDT when a file's metadata changes
  function updateCrdtFileMetadata(
    path: string,
    frontmatter: Record<string, unknown>,
  ) {
    if (!workspaceCrdtInitialized) return;

    try {
      updateFileMetadata(path, {
        title: (frontmatter.title as string) ?? null,
        partOf: (frontmatter.part_of as string) ?? null,
        contents: frontmatter.contents
          ? (frontmatter.contents as string[])
          : null,
        audience: (frontmatter.audience as string[]) ?? null,
        description: (frontmatter.description as string) ?? null,
        extra: Object.fromEntries(
          Object.entries(frontmatter).filter(
            ([key]) =>
              ![
                "title",
                "part_of",
                "contents",
                "attachments",
                "audience",
                "description",
              ].includes(key),
          ),
        ),
      });
    } catch (e) {
      console.error("[App] Failed to update CRDT metadata:", e);
      // Don't throw - CRDT errors should not break the app
    }
  }

  // Add a new file to CRDT
  function addFileToCrdt(
    path: string,
    frontmatter: Record<string, unknown>,
    parentPath: string | null,
  ) {
    if (!workspaceCrdtInitialized) return;

    try {
      const metadata: FileMetadata = {
        title: (frontmatter.title as string) ?? null,
        partOf: parentPath ?? (frontmatter.part_of as string) ?? null,
        contents: frontmatter.contents
          ? (frontmatter.contents as string[])
          : null,
        attachments: ((frontmatter.attachments as string[]) ?? []).map((p) => ({
          path: p,
          source: "local",
          hash: "",
          mimeType: "",
          size: 0,
          deleted: false,
        })),
        deleted: false,
        audience: (frontmatter.audience as string[]) ?? null,
        description: (frontmatter.description as string) ?? null,
        extra: Object.fromEntries(
          Object.entries(frontmatter).filter(
            ([key]) =>
              ![
                "title",
                "part_of",
                "contents",
                "attachments",
                "audience",
                "description",
              ].includes(key),
          ),
        ),
        modifiedAt: Date.now(),
      };

      updateFileMetadata(path, metadata);

      // Add to parent's contents if parent exists
      if (parentPath) {
        addToContents(parentPath, path);
      }
    } catch (e) {
      console.error("[App] Failed to add file to CRDT:", e);
      // Don't throw - CRDT errors should not break the app
    }
  }

  // Open an entry
  async function openEntry(path: string) {
    if (!backend) return;

    if (isDirty) {
      const confirm = window.confirm(
        "You have unsaved changes. Do you want to discard them?",
      );
      if (!confirm) return;
    }

    try {
      isLoading = true;

      // Cleanup previous blob URLs
      revokeBlobUrls();

      currentEntry = await backend.getEntry(path);
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
        );

        // Setup Y.js collaboration for this document
        // Always disconnect previous collaboration session (if any). We keep the session cached
        // to avoid `ystate.doc` errors from destroying Y.Doc while TipTap plugins are still tearing down.
        if (currentCollaborationPath) {
          currentYDoc = null;
          currentProvider = null;
          await tick();
          try {
            disconnectDocument(currentCollaborationPath);
          } catch (e) {
            console.warn(
              "[App] Failed to disconnect collaboration session:",
              e,
            );
          }
          currentCollaborationPath = null;
        }

        // Connect to collaboration for this entry only if enabled.
        // If not enabled, Editor will render the plain markdown `content` immediately.
        if (collaborationEnabled) {
          // Use RELATIVE path within workspace so desktop and iOS match
          try {
            // Get workspace directory from tree path
            // We want the root folder so we can create relative paths like "Utility/doc.md"
            let workspaceDir = tree?.path || "";
            // Handle case where tree.path might be the index file or have trailing slash
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

            // Calculate relative path within workspace
            let relativePath = currentEntry.path;
            if (workspaceDir && currentEntry.path.startsWith(workspaceDir)) {
              relativePath = currentEntry.path.substring(
                workspaceDir.length + 1,
              ); // +1 for the /
            }

            console.log(
              "[App] Collaboration room:",
              relativePath,
              "(from",
              currentEntry.path,
              ")",
            );

            const { ydoc, provider } = getCollaborativeDocument(relativePath);
            currentYDoc = ydoc;
            currentProvider = provider;
            currentCollaborationPath = relativePath;
          } catch (e) {
            console.warn("[App] Failed to setup collaboration:", e);
            currentYDoc = null;
            currentProvider = null;
            currentCollaborationPath = null;
          }
        }
      } else {
        displayContent = "";
        currentYDoc = null;
        currentProvider = null;
      }

      isDirty = false;
      error = null;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      isLoading = false;
    }
  }

  // Save current entry
  async function save() {
    if (!backend || !currentEntry || !editorRef) return;

    try {
      const markdownWithBlobUrls = editorRef.getMarkdown();
      // Reverse-transform blob URLs back to attachment paths
      const markdown = reverseBlobUrlsToAttachmentPaths(
        markdownWithBlobUrls || "",
      );
      await backend.saveEntry(currentEntry.path, markdown);
      isDirty = false;
      // Trigger persist for WASM backend
      await persistNow();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  // Handle content changes
  function handleContentChange(_markdown: string) {
    isDirty = true;
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
  function handleKeydown(event: KeyboardEvent) {
    if ((event.metaKey || event.ctrlKey) && event.key === "s") {
      event.preventDefault();
      save();
    }
    // Command palette with Cmd/Ctrl + K
    if ((event.metaKey || event.ctrlKey) && event.key === "k") {
      event.preventDefault();
      showCommandPalette = true;
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
    if (!backend) return;
    try {
      const newPath = await backend.createChildEntry(parentPath);
      await persistNow();

      // Update CRDT with new file
      const entry = await backend.getEntry(newPath);
      addFileToCrdt(newPath, entry.frontmatter, parentPath);

      await refreshTree();
      await openEntry(newPath);
      await runValidation();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function createNewEntry(path: string, title: string) {
    if (!backend) return;
    try {
      const newPath = await backend.createEntry(path, { title });

      // Update CRDT with new file
      const entry = await backend.getEntry(newPath);
      addFileToCrdt(newPath, entry.frontmatter, null);

      await refreshTree();
      await openEntry(newPath);
      await runValidation();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      showNewEntryModal = false;
    }
  }

  async function handleDailyEntry() {
    if (!backend) return;
    try {
      const path = await backend.ensureDailyEntry();
      await refreshTree();
      await openEntry(path);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function handleDeleteEntry(path: string) {
    if (!backend) return;
    const confirm = window.confirm(
      `Are you sure you want to delete "${path.split("/").pop()?.replace(".md", "")}"?`,
    );
    if (!confirm) return;

    try {
      // Get metadata before deleting to know the parent
      const metadata = getFileMetadata(path);

      await backend.deleteEntry(path);
      await persistNow();

      // Update CRDT - mark as deleted and remove from parent
      if (workspaceCrdtInitialized) {
        try {
          crdtDeleteFile(path);
          if (metadata?.partOf) {
            removeFromContents(metadata.partOf, path);
          }
        } catch (e) {
          console.error("[App] Failed to update CRDT on delete:", e);
        }
      }

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
    if (!backend) return;
    try {
      validationResult = await backend.validateWorkspace();
      console.log("[App] Validation result:", validationResult);
      console.log("[App] Warnings:", validationResult?.warnings);
    } catch (e) {
      console.error("[App] Validation error:", e);
    }
  }

  // Refresh the tree using the appropriate method based on showUnlinkedFiles setting
  async function refreshTree() {
    if (!backend) return;
    try {
      if (showUnlinkedFiles) {
        // "Show All Files" mode - use filesystem tree
        tree = await backend.getFilesystemTree(undefined, showHiddenFiles);
      } else {
        // Normal mode - use hierarchy tree
        tree = await backend.getWorkspaceTree();
      }
    } catch (e) {
      console.error("[App] Error refreshing tree:", e);
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
    if (!file || !backend || !pendingAttachmentPath) return;

    // Check size limit (5MB)
    const MAX_SIZE = 5 * 1024 * 1024;
    if (file.size > MAX_SIZE) {
      attachmentError = `File too large (${(file.size / 1024 / 1024).toFixed(1)}MB). Maximum is 5MB.`;
      input.value = "";
      return;
    }

    try {
      // Convert file to base64
      const dataBase64 = await fileToBase64(file);
      const entryPath = pendingAttachmentPath;

      // Upload attachment
      const attachmentPath = await backend.uploadAttachment(
        pendingAttachmentPath,
        file.name,
        dataBase64,
      );
      await persistNow();

      // Update CRDT with new attachment
      if (workspaceCrdtInitialized) {
        try {
          const attachmentRef: BinaryRef = {
            path: attachmentPath,
            source: "local",
            hash: "", // Could compute hash here if needed
            mimeType: file.type,
            size: file.size,
            deleted: false,
          };
          crdtAddAttachment(entryPath, attachmentRef);
        } catch (e) {
          console.error("[App] Failed to add attachment to CRDT:", e);
        }
      }

      // Refresh the entry if it's currently open
      if (currentEntry?.path === pendingAttachmentPath) {
        currentEntry = await backend.getEntry(pendingAttachmentPath);

        // If it's an image, also insert it into the editor at cursor
        if (file.type.startsWith("image/") && editorRef) {
          // Get the binary data and create blob URL
          const data = await backend.getAttachmentData(
            currentEntry.path,
            attachmentPath,
          );
          const blob = new Blob([new Uint8Array(data)], { type: file.type });
          const blobUrl = URL.createObjectURL(blob);

          // Track for cleanup
          blobUrlMap.set(attachmentPath, blobUrl);

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

  // Handle file drop in Editor - upload and return blob URL
  async function handleEditorFileDrop(
    file: File,
  ): Promise<{ blobUrl: string; attachmentPath: string } | null> {
    if (!backend || !currentEntry) return null;

    // Check size limit (5MB)
    const MAX_SIZE = 5 * 1024 * 1024;
    if (file.size > MAX_SIZE) {
      attachmentError = `File too large (${(file.size / 1024 / 1024).toFixed(1)}MB). Maximum is 5MB.`;
      return null;
    }

    try {
      const entryPath = currentEntry.path;
      const dataBase64 = await fileToBase64(file);
      const attachmentPath = await backend.uploadAttachment(
        currentEntry.path,
        file.name,
        dataBase64,
      );

      // Update CRDT with new attachment
      if (workspaceCrdtInitialized) {
        try {
          const attachmentRef: BinaryRef = {
            path: attachmentPath,
            source: "local",
            hash: "",
            mimeType: file.type,
            size: file.size,
            deleted: false,
          };
          crdtAddAttachment(entryPath, attachmentRef);
        } catch (e) {
          console.error("[App] Failed to add attachment to CRDT:", e);
        }
      }

      await persistNow();

      // Refresh the entry to update attachments list
      currentEntry = await backend.getEntry(currentEntry.path);

      // Get the binary data back and create blob URL
      const data = await backend.getAttachmentData(
        currentEntry.path,
        attachmentPath,
      );
      const ext = file.name.split(".").pop()?.toLowerCase() || "";
      const mimeTypes: Record<string, string> = {
        png: "image/png",
        jpg: "image/jpeg",
        jpeg: "image/jpeg",
        gif: "image/gif",
        webp: "image/webp",
        svg: "image/svg+xml",
      };
      const mimeType = mimeTypes[ext] || "image/png";
      const blob = new Blob([new Uint8Array(data)], { type: mimeType });
      const blobUrl = URL.createObjectURL(blob);

      // Track for cleanup
      blobUrlMap.set(attachmentPath, blobUrl);

      return { blobUrl, attachmentPath };
    } catch (e) {
      attachmentError = e instanceof Error ? e.message : String(e);
      return null;
    }
  }

  // Handle delete attachment from RightSidebar
  async function handleDeleteAttachment(attachmentPath: string) {
    if (!backend || !currentEntry) return;

    try {
      const entryPath = currentEntry.path;
      await backend.deleteAttachment(currentEntry.path, attachmentPath);
      await persistNow();

      // Update CRDT
      if (workspaceCrdtInitialized) {
        try {
          crdtRemoveAttachment(entryPath, attachmentPath);
        } catch (e) {
          console.error("[App] Failed to remove attachment from CRDT:", e);
        }
      }

      // Refresh current entry to update attachments list
      currentEntry = await backend.getEntry(currentEntry.path);
      attachmentError = null;
    } catch (e) {
      attachmentError = e instanceof Error ? e.message : String(e);
    }
  }

  // Handle drag-drop: attach entry to new parent
  async function handleMoveEntry(entryPath: string, newParentPath: string) {
    if (!backend) return;
    if (entryPath === newParentPath) return; // Can't attach to self

    console.log(
      `[Drag-Drop] entryPath="${entryPath}" -> newParentPath="${newParentPath}"`,
    );

    try {
      // Get old parent before moving
      const metadata = getFileMetadata(entryPath);
      const oldParentPath = metadata?.partOf ?? null;

      // Attach the entry to the new parent
      // This will:
      // - Add entry to newParent's `contents`
      // - Set entry's `part_of` to point to newParent
      await backend.attachEntryToParent(entryPath, newParentPath);
      await persistNow();

      // Update CRDT
      if (workspaceCrdtInitialized) {
        try {
          crdtMoveFile(entryPath, oldParentPath, newParentPath);
        } catch (e) {
          console.error("[App] Failed to move file in CRDT:", e);
        }
      }

      await refreshTree();
      await runValidation();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  // Handle frontmatter property changes
  async function handlePropertyChange(key: string, value: unknown) {
    if (!backend || !currentEntry) return;
    try {
      const path = currentEntry.path;
      // Special handling for title: need to check rename first
      if (key === "title" && typeof value === "string" && value.trim()) {
        const newFilename = backend.slugifyTitle(value);
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
            const newPath = await backend.renameEntry(oldPath, newFilename);
            // Rename succeeded, now update title in frontmatter (at new path)
            await backend.setFrontmatterProperty(newPath, key, value);
            await persistNow();

            // Transfer expanded state from old path to new path
            if (expandedNodes.has(oldPath)) {
              expandedNodes.delete(oldPath);
              expandedNodes.add(newPath);
              expandedNodes = expandedNodes; // trigger reactivity
            }

            // Update CRDT with rename
            if (workspaceCrdtInitialized) {
              try {
                crdtRenameFile(oldPath, newPath);
                updateCrdtFileMetadata(newPath, {
                  ...currentEntry.frontmatter,
                  [key]: value,
                });
              } catch (e) {
                console.error("[App] Failed to rename file in CRDT:", e);
              }
            }

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
          await backend.setFrontmatterProperty(currentEntry.path, key, value);
          await persistNow();
          currentEntry = {
            ...currentEntry,
            frontmatter: { ...currentEntry.frontmatter, [key]: value },
          };

          // Update CRDT
          updateCrdtFileMetadata(path, currentEntry.frontmatter);
          titleError = null;
        }
      } else {
        // Non-title properties: update normally
        await backend.setFrontmatterProperty(currentEntry.path, key, value);
        await persistNow();
        currentEntry = {
          ...currentEntry,
          frontmatter: { ...currentEntry.frontmatter, [key]: value },
        };

        // Update CRDT
        updateCrdtFileMetadata(path, currentEntry.frontmatter);
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function handlePropertyRemove(key: string) {
    if (!backend || !currentEntry) return;
    try {
      const path = currentEntry.path;
      await backend.removeFrontmatterProperty(currentEntry.path, key);
      await persistNow();
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
    if (!backend || !currentEntry) return;
    try {
      const path = currentEntry.path;
      await backend.setFrontmatterProperty(currentEntry.path, key, value);
      await persistNow();
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

  function getEntryTitle(entry: EntryData): string {
    // Prioritize frontmatter.title for live updates, fall back to cached title
    const frontmatterTitle = entry.frontmatter?.title as string | undefined;
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
      if (backend) {
        // Check if file exists by trying to get it
        const entry = await backend.getEntry(targetPath);
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
      if (create && backend) {
        try {
          // Create the file with basic frontmatter
          const title = fileName.replace(".md", "").replace(/-/g, " ");
          await backend.createEntry(targetPath, { title });
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
  bind:open={showCommandPalette}
  {tree}
  {backend}
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
/>

<!-- Export Dialog -->
<ExportDialog
  bind:open={showExportDialog}
  rootPath={exportPath}
  {backend}
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
  />

  <!-- Hidden file input for attachments -->
  <input
    type="file"
    bind:this={attachmentFileInput}
    onchange={handleAttachmentFileSelect}
    class="hidden"
    accept="image/*,.pdf,.doc,.docx,.txt,.md"
  />

  <!-- Main Content Area -->
  <main class="flex-1 flex flex-col overflow-hidden min-w-0">
    {#if currentEntry}
      <header
        class="flex items-center justify-between px-4 md:px-6 py-3 md:py-4 border-b border-border bg-card shrink-0"
      >
        <!-- Left side: toggle + title -->
        <div class="flex items-center gap-2 min-w-0 flex-1">
          <!-- Mobile menu button -->
          <Button
            variant="ghost"
            size="icon"
            onclick={toggleLeftSidebar}
            class="size-8 md:hidden shrink-0"
            aria-label="Toggle navigation"
          >
            <Menu class="size-4" />
          </Button>

          <!-- Desktop left sidebar toggle -->
          <Button
            variant="ghost"
            size="icon"
            onclick={toggleLeftSidebar}
            class="size-8 hidden md:flex shrink-0"
            aria-label="Toggle navigation sidebar"
          >
            <PanelLeft class="size-4" />
          </Button>

          <div class="min-w-0 flex-1">
            <h2
              class="text-lg md:text-xl font-semibold text-foreground truncate"
            >
              {getEntryTitle(currentEntry)}
            </h2>
            <p
              class="text-xs md:text-sm text-muted-foreground truncate hidden sm:block"
            >
              {currentEntry.path}
            </p>
          </div>
        </div>

        <!-- Right side: actions -->
        <div class="flex items-center gap-1 md:gap-2 ml-2 shrink-0">
          {#if isDirty}
            <span
              class="hidden sm:inline-flex px-2 py-1 text-xs font-medium rounded-md bg-amber-100 text-amber-800 dark:bg-amber-900/30 dark:text-amber-400"
            >
              Unsaved
            </span>
          {/if}
          <Button
            onclick={save}
            disabled={!isDirty}
            size="sm"
            class="gap-1 md:gap-2"
          >
            <Save class="size-4" />
            <span class="hidden sm:inline">Save</span>
          </Button>
          <Button
            onclick={exportEntry}
            variant="outline"
            size="sm"
            class="gap-1 md:gap-2 hidden sm:flex"
          >
            <Download class="size-4" />
            <span class="hidden md:inline">Export</span>
          </Button>

          <!-- Properties panel toggle -->
          <Button
            variant="ghost"
            size="icon"
            onclick={toggleRightSidebar}
            class="size-8"
            aria-label="Toggle properties panel"
          >
            <PanelRight class="size-4" />
          </Button>
        </div>
      </header>

      <div class="flex-1 overflow-y-auto p-4 md:p-6">
        {#if Editor}
          {#key `${currentCollaborationPath ?? currentEntry.path}:${collaborationEnabled ? "collab" : "local"}`}
            <Editor
              debugMenus={false}
              bind:this={editorRef}
              content={displayContent}
              onchange={handleContentChange}
              placeholder="Start writing..."
              onInsertImage={handleEditorImageInsert}
              onFileDrop={handleEditorFileDrop}
              onLinkClick={handleLinkClick}
              ydoc={collaborationEnabled
                ? (currentYDoc ?? undefined)
                : undefined}
              provider={collaborationEnabled
                ? (currentProvider ?? undefined)
                : undefined}
            />
          {/key}
        {:else}
          <div class="flex items-center justify-center h-full">
            <div
              class="animate-spin rounded-full h-8 w-8 border-b-2 border-primary"
            ></div>
          </div>
        {/if}
      </div>
    {:else}
      <!-- Empty state with sidebar toggles -->
      <header
        class="flex items-center justify-between px-4 py-3 border-b border-border bg-card shrink-0 md:hidden"
      >
        <Button
          variant="ghost"
          size="icon"
          onclick={toggleLeftSidebar}
          class="size-8"
          aria-label="Toggle navigation"
        >
          <Menu class="size-4" />
        </Button>
        <span class="text-lg font-semibold">Diaryx</span>
        <div class="size-8"></div>
      </header>

      <div class="flex-1 flex items-center justify-center">
        <div class="text-center max-w-md px-4">
          <!-- Desktop sidebar toggle when no entry -->
          <div class="hidden md:flex justify-center mb-4">
            {#if leftSidebarCollapsed}
              <Button
                variant="outline"
                size="sm"
                onclick={toggleLeftSidebar}
                class="gap-2"
              >
                <PanelLeft class="size-4" />
                Show Sidebar
              </Button>
            {/if}
          </div>
          <h2 class="text-2xl font-semibold text-foreground mb-2">
            Welcome to Diaryx
          </h2>
          <p class="text-muted-foreground">
            Select an entry from the sidebar to start editing, or create a new
            one.
          </p>
        </div>
      </div>
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
