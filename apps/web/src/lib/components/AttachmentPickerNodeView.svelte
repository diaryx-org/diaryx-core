<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import {
    Upload,
    FileIcon,
    FileText,
    FileSpreadsheet,
    X,
  } from "@lucide/svelte";
  import type { Api } from "$lib/backend/api";
  import { isHeicFile, convertHeicToJpeg } from "$lib/../models/services/attachmentService";

  interface Props {
    entryPath: string;
    api: Api | null;
    onSelect: (result: AttachmentResult) => void;
    onCancel: () => void;
  }

  interface AttachmentResult {
    path: string;
    isImage: boolean;
    blobUrl?: string;
    sourceEntryPath: string;
  }

  let { entryPath, api, onSelect, onCancel }: Props = $props();

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
  let loading = $state(true);
  let error = $state<string | null>(null);
  let activeTab = $state<"existing" | "upload">("existing");
  let isDragging = $state(false);
  let fileInput: HTMLInputElement | null = $state(null);

  // Load attachments on mount
  $effect(() => {
    if (api && entryPath) {
      loadAttachments();
    }
  });

  async function loadAttachments() {
    if (!api) return;
    loading = true;
    error = null;

    try {
      const ancestorResult = await api.getAncestorAttachments(entryPath);
      const newGroups: AttachmentGroup[] = [];

      for (let i = 0; i < ancestorResult.entries.length; i++) {
        const entry = ancestorResult.entries[i];
        const isCurrentEntry = i === 0;

        const attachments = entry.attachments.map((path: string) => ({
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

      groups = newGroups;
      loading = false;

      if (newGroups.length === 0) {
        activeTab = "upload";
      }

      // Load thumbnails in background
      for (const group of newGroups) {
        for (const attachment of group.attachments) {
          if (attachment.isImage) {
            loadThumbnail(group.entryPath, attachment.path).then((url) => {
              if (url) {
                attachment.thumbnail = url;
                groups = [...groups]; // trigger reactivity
              }
            });
          }
        }
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

  function isImageFile(path: string): boolean {
    const ext = path.split(".").pop()?.toLowerCase() || "";
    return ["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "heic", "heif"].includes(
      ext
    );
  }

  function getMimeType(path: string): string {
    const ext = path.split(".").pop()?.toLowerCase() || "";
    const mimeTypes: Record<string, string> = {
      png: "image/png",
      jpg: "image/jpeg",
      jpeg: "image/jpeg",
      gif: "image/gif",
      webp: "image/webp",
      svg: "image/svg+xml",
      bmp: "image/bmp",
      ico: "image/x-icon",
      heic: "image/heic",
      heif: "image/heif",
      pdf: "application/pdf",
    };
    return mimeTypes[ext] || "application/octet-stream";
  }

  function getFilename(path: string): string {
    return path.split("/").pop() || path;
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

  // Convert bytes to base64 in chunks to avoid stack overflow
  function bytesToBase64(bytes: Uint8Array): string {
    const chunkSize = 8192;
    let binary = '';
    for (let i = 0; i < bytes.length; i += chunkSize) {
      const chunk = bytes.subarray(i, Math.min(i + chunkSize, bytes.length));
      binary += String.fromCharCode(...chunk);
    }
    return btoa(binary);
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
  }

  async function handleUpload(file: File) {
    if (!api) return;

    try {
      loading = true;
      error = null;

      const arrayBuffer = await file.arrayBuffer();
      const bytes = new Uint8Array(arrayBuffer);
      const base64 = bytesToBase64(bytes);

      const attachmentPath = await api.uploadAttachment(
        entryPath,
        file.name,
        base64
      );
      const blobUrl = URL.createObjectURL(file);
      const isImage = isImageFile(file.name);

      onSelect({
        path: attachmentPath,
        isImage,
        blobUrl,
        sourceEntryPath: entryPath,
      });
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      loading = false;
    }
  }

  function handleFileInputChange(e: Event) {
    const target = e.target as HTMLInputElement;
    const file = target.files?.[0];
    if (file) handleUpload(file);
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
    if (file) handleUpload(file);
  }
</script>

<div class="picker-container">
  <div class="picker-header">
    <span class="picker-title">Insert Attachment</span>
    <button type="button" class="close-btn" onclick={onCancel}>
      <X class="size-4" />
    </button>
  </div>

  <div class="picker-tabs">
    <button
      type="button"
      class="tab-btn"
      class:active={activeTab === "existing"}
      onclick={() => (activeTab = "existing")}
    >
      Select Existing
    </button>
    <button
      type="button"
      class="tab-btn"
      class:active={activeTab === "upload"}
      onclick={() => (activeTab = "upload")}
    >
      Upload New
    </button>
  </div>

  <div class="picker-content">
    {#if activeTab === "existing"}
      {#if loading}
        <div class="status-message">Loading...</div>
      {:else if error}
        <div class="status-message error">{error}</div>
      {:else if groups.length === 0}
        <div class="status-message">No attachments found. Upload one first.</div>
      {:else}
        <div class="attachments-grid">
          {#each groups as group}
            <div class="attachment-group">
              <h4 class="group-title">{group.entryTitle}</h4>
              <div class="group-items">
                {#each group.attachments as attachment}
                  <button
                    type="button"
                    class="attachment-item"
                    onclick={() => handleSelect(attachment, group.entryPath)}
                  >
                    {#if attachment.isImage && attachment.thumbnail}
                      <img
                        src={attachment.thumbnail}
                        alt=""
                        class="thumbnail"
                      />
                    {:else}
                      {@const IconComponent = getFileIcon(attachment.path)}
                      <div class="file-icon">
                        <IconComponent class="size-6" />
                      </div>
                    {/if}
                    <span class="filename">{getFilename(attachment.path)}</span>
                  </button>
                {/each}
              </div>
            </div>
          {/each}
        </div>
      {/if}
    {:else}
      <div
        class="upload-zone"
        class:dragging={isDragging}
        ondragover={handleDragOver}
        ondragleave={handleDragLeave}
        ondrop={handleDrop}
        role="presentation"
      >
        <Upload class="size-8 text-muted-foreground" />
        <p class="upload-text">Drag and drop a file here, or click to select</p>
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
          <p class="error-text">{error}</p>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .picker-container {
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--card);
    overflow: hidden;
    margin: 1em 0;
  }

  .picker-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
    background: var(--muted);
  }

  .picker-title {
    font-weight: 600;
    font-size: 14px;
    color: var(--foreground);
  }

  .close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 4px;
    border: none;
    background: transparent;
    border-radius: 4px;
    cursor: pointer;
    color: var(--muted-foreground);
  }

  .close-btn:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .picker-tabs {
    display: flex;
    gap: 8px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
  }

  .tab-btn {
    padding: 6px 12px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: transparent;
    cursor: pointer;
    font-size: 13px;
    color: var(--foreground);
  }

  .tab-btn.active {
    background: var(--primary);
    color: var(--primary-foreground);
    border-color: var(--primary);
  }

  .tab-btn:hover:not(.active) {
    background: var(--accent);
  }

  .picker-content {
    padding: 16px;
    max-height: 300px;
    overflow-y: auto;
  }

  .status-message {
    text-align: center;
    padding: 24px;
    color: var(--muted-foreground);
  }

  .status-message.error {
    color: var(--destructive);
  }

  .attachments-grid {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .group-title {
    font-size: 12px;
    font-weight: 500;
    color: var(--muted-foreground);
    margin-bottom: 8px;
  }

  .group-items {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(80px, 1fr));
    gap: 8px;
  }

  .attachment-item {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 8px;
    border: 1px solid transparent;
    border-radius: 6px;
    background: transparent;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .attachment-item:hover {
    border-color: var(--primary);
    background: var(--accent);
  }

  .thumbnail {
    width: 60px;
    height: 60px;
    object-fit: cover;
    border-radius: 4px;
  }

  .file-icon {
    width: 60px;
    height: 60px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--muted);
    border-radius: 4px;
    color: var(--muted-foreground);
  }

  .filename {
    font-size: 11px;
    margin-top: 4px;
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 80px;
    color: var(--foreground);
  }

  .upload-zone {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    padding: 32px;
    border: 2px dashed var(--border);
    border-radius: 8px;
    text-align: center;
    transition: all 0.15s ease;
  }

  .upload-zone.dragging {
    border-color: var(--primary);
    background: var(--accent);
  }

  .upload-text {
    color: var(--muted-foreground);
    font-size: 14px;
  }

  .hidden {
    display: none;
  }

  .error-text {
    color: var(--destructive);
    font-size: 13px;
  }
</style>
