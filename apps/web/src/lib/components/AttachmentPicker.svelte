<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import {
    Upload,
    FileIcon,
    FileText,
    FileSpreadsheet,
    FolderOpen,
  } from "@lucide/svelte";
  import type { Api } from "$lib/backend/api";
  import { isHeicFile, convertHeicToJpeg, isImageFile, getFilename, getMimeType, bytesToBase64 } from "$lib/../models/services/attachmentService";

  interface Props {
    open: boolean;
    entryPath: string;
    api: Api | null;
    onSelect: (result: AttachmentSelection | null) => void;
    onClose: () => void;
  }

  export interface AttachmentSelection {
    path: string;
    isImage: boolean;
    blobUrl?: string;
    /** The entry path where this attachment lives (for getting data) */
    sourceEntryPath: string;
  }

  let { open = $bindable(), entryPath, api, onSelect, onClose }: Props = $props();

  interface AttachmentGroup {
    entryPath: string;
    entryTitle: string | null;
    attachments: Array<{
      path: string;
      isImage: boolean;
      thumbnail?: string;
    }>;
  }

  let groups = $state<AttachmentGroup[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let activeTab = $state<"existing" | "upload">("existing");
  let isDragging = $state(false);
  let fileInput: HTMLInputElement | null = $state(null);

  // Track previous open state to detect transitions
  let prevOpen = false;

  // Single effect to handle open/close transitions
  $effect(() => {
    const currentOpen = open;

    if (currentOpen && !prevOpen) {
      // Dialog just opened - load attachments
      prevOpen = true;
      if (api && entryPath) {
        loadAttachments();
      }
    } else if (!currentOpen && prevOpen) {
      // Dialog just closed - schedule cleanup
      prevOpen = false;
      // Use setTimeout to avoid state updates during effect
      setTimeout(() => {
        // Cleanup blob URLs from the current groups
        for (const group of groups) {
          for (const att of group.attachments) {
            if (att.thumbnail) {
              URL.revokeObjectURL(att.thumbnail);
            }
          }
        }
        groups = [];
        error = null;
      }, 0);
    }
  });

  async function loadAttachments() {
    if (!api) return;
    loading = true;
    error = null;

    try {
      // Load ancestor attachments (includes current entry)
      const ancestorResult = await api.getAncestorAttachments(entryPath);

      // Build groups WITHOUT thumbnails first (fast)
      const newGroups: AttachmentGroup[] = [];

      for (let i = 0; i < ancestorResult.entries.length; i++) {
        const entry = ancestorResult.entries[i];
        const isCurrentEntry = i === 0;

        const attachments = entry.attachments.map((path) => ({
          path,
          isImage: isImageFile(path),
          thumbnail: undefined as string | undefined,
        }));

        newGroups.push({
          entryPath: entry.entry_path,
          entryTitle: isCurrentEntry
            ? "Current Entry"
            : entry.entry_title || getFilename(entry.entry_path),
          attachments,
        });
      }

      // Show groups immediately
      groups = newGroups;
      loading = false;

      // Auto-switch to upload tab if no attachments found
      if (newGroups.length === 0) {
        activeTab = "upload";
      }

      // Then load thumbnails in the background (lazy loading)
      // Batch updates to avoid excessive re-renders
      const thumbnailPromises: Promise<void>[] = [];
      for (const group of newGroups) {
        for (const attachment of group.attachments) {
          if (attachment.isImage) {
            thumbnailPromises.push(
              loadThumbnail(group.entryPath, attachment.path).then((url) => {
                if (url) {
                  attachment.thumbnail = url;
                }
              })
            );
          }
        }
      }

      // Update once after all thumbnails are loaded
      if (thumbnailPromises.length > 0) {
        Promise.all(thumbnailPromises).then(() => {
          // Trigger a single re-render
          groups = [...newGroups];
        });
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      loading = false;
    }
  }

  async function loadThumbnail(
    sourceEntryPath: string,
    attachmentPath: string
  ): Promise<string | undefined> {
    if (!api) return undefined;
    try {
      const data = await api.getAttachmentData(sourceEntryPath, attachmentPath);
      const mimeType = getMimeType(attachmentPath);
      let blob = new Blob([new Uint8Array(data)], { type: mimeType });

      // Convert HEIC to JPEG for browser display
      if (isHeicFile(attachmentPath)) {
        blob = await convertHeicToJpeg(blob);
      }

      return URL.createObjectURL(blob);
    } catch {
      return undefined;
    }
  }


  function getFileIcon(path: string) {
    const ext = path.split(".").pop()?.toLowerCase();
    switch (ext) {
      case "pdf":
        return FileText;
      case "csv":
      case "xlsx":
      case "xls":
        return FileSpreadsheet;
      default:
        return FileIcon;
    }
  }

  async function handleSelect(
    attachment: AttachmentGroup["attachments"][0],
    sourceEntryPath: string
  ) {
    let blobUrl = attachment.thumbnail;

    // If thumbnail not loaded yet, load it now
    if (!blobUrl && attachment.isImage && api) {
      blobUrl = await loadThumbnail(sourceEntryPath, attachment.path);
    }

    onSelect({
      path: attachment.path,
      isImage: attachment.isImage,
      blobUrl,
      sourceEntryPath,
    });
    onClose();
  }

  async function handleUpload(file: File) {
    if (!api) return;

    try {
      loading = true;
      error = null;

      // Convert file to base64
      const arrayBuffer = await file.arrayBuffer();
      const bytes = new Uint8Array(arrayBuffer);
      const base64 = bytesToBase64(bytes);

      // Upload the attachment
      const attachmentPath = await api.uploadAttachment(
        entryPath,
        file.name,
        base64
      );

      // Create blob URL for immediate use
      const blobUrl = URL.createObjectURL(file);
      const isImage = isImageFile(file.name);

      // Select the newly uploaded attachment
      onSelect({
        path: attachmentPath,
        isImage,
        blobUrl,
        sourceEntryPath: entryPath,
      });
      onClose();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  function handleFileInputChange(e: Event) {
    const target = e.target as HTMLInputElement;
    const file = target.files?.[0];
    if (file) {
      handleUpload(file);
    }
  }

  function handleDragOver(e: DragEvent) {
    e.preventDefault();
    isDragging = true;
  }

  function handleDragLeave(e: DragEvent) {
    e.preventDefault();
    isDragging = false;
  }

  function handleDrop(e: DragEvent) {
    e.preventDefault();
    isDragging = false;
    const file = e.dataTransfer?.files?.[0];
    if (file) {
      handleUpload(file);
    }
  }

  function handleOpenChange(isOpen: boolean) {
    open = isOpen;
    if (!isOpen) {
      onClose();
    }
  }
</script>

<Dialog.Root {open} onOpenChange={handleOpenChange}>
  <Dialog.Content class="sm:max-w-2xl max-h-[80vh] flex flex-col">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        <FolderOpen class="size-5" />
        Insert Attachment
      </Dialog.Title>
      <Dialog.Description>
        Select an existing attachment or upload a new one.
      </Dialog.Description>
    </Dialog.Header>

    <!-- Tab buttons -->
    <div class="flex gap-2 border-b pb-2">
      <Button
        variant={activeTab === "existing" ? "default" : "outline"}
        size="sm"
        onclick={() => (activeTab = "existing")}
      >
        <FolderOpen class="size-4 mr-2" />
        Select Existing
      </Button>
      <Button
        variant={activeTab === "upload" ? "default" : "outline"}
        size="sm"
        onclick={() => (activeTab = "upload")}
      >
        <Upload class="size-4 mr-2" />
        Upload New
      </Button>
    </div>

    <!-- Content based on active tab -->
    <div class="flex-1 overflow-y-auto min-h-[200px]">
      {#if activeTab === "existing"}
        {#if loading}
          <div class="p-4 text-center text-muted-foreground">Loading...</div>
        {:else if error}
          <div class="p-4 text-center text-destructive">{error}</div>
        {:else if groups.length === 0}
          <div class="p-4 text-center text-muted-foreground">
            No attachments found. Upload one first.
          </div>
        {:else}
          <div class="p-2 space-y-4">
            {#each groups as group}
              <div>
                <h4
                  class="text-sm font-medium text-muted-foreground mb-2 px-2"
                >
                  {group.entryTitle}
                </h4>
                <div
                  class="grid gap-2"
                  style="grid-template-columns: repeat(auto-fill, minmax(120px, 1fr));"
                >
                  {#each group.attachments as attachment}
                    <button
                      type="button"
                      class="attachment-item group relative flex flex-col items-center p-2 rounded-lg border border-transparent hover:border-primary hover:bg-accent transition-colors cursor-pointer"
                      onclick={() => handleSelect(attachment, group.entryPath)}
                    >
                      {#if attachment.isImage && attachment.thumbnail}
                        <img
                          src={attachment.thumbnail}
                          alt=""
                          class="w-full h-20 object-cover rounded"
                        />
                      {:else}
                        {@const IconComponent = getFileIcon(attachment.path)}
                        <div
                          class="w-full h-20 flex items-center justify-center bg-muted rounded"
                        >
                          <IconComponent class="size-8 text-muted-foreground" />
                        </div>
                      {/if}
                      <span
                        class="text-xs mt-1 text-center truncate w-full px-1"
                        title={getFilename(attachment.path)}
                      >
                        {getFilename(attachment.path)}
                      </span>
                    </button>
                  {/each}
                </div>
              </div>
            {/each}
          </div>
        {/if}
      {:else}
        <!-- Upload tab content -->
        <div
          class="p-4 h-full"
          ondragover={handleDragOver}
          ondragleave={handleDragLeave}
          ondrop={handleDrop}
          role="presentation"
        >
          <div
            class="border-2 border-dashed rounded-lg p-8 text-center transition-colors h-full flex flex-col items-center justify-center"
            class:border-primary={isDragging}
            class:bg-accent={isDragging}
          >
            <Upload class="size-12 text-muted-foreground mb-4" />
            <p class="text-muted-foreground mb-4">
              Drag and drop a file here, or click to select
            </p>
            <input
              type="file"
              bind:this={fileInput}
              onchange={handleFileInputChange}
              class="hidden"
            />
            <Button onclick={() => fileInput?.click()} disabled={loading}>
              {loading ? "Uploading..." : "Choose File"}
            </Button>
            {#if error}
              <p class="text-destructive mt-4 text-sm">{error}</p>
            {/if}
          </div>
        </div>
      {/if}
    </div>

    </Dialog.Content>
</Dialog.Root>
