<script lang="ts">
  /**
   * EditorContent - The main editor content area
   *
   * Wraps the TipTap editor with loading states.
   * This component handles the editor rendering logic.
   */

  import LoadingSpinner from "../shared/LoadingSpinner.svelte";
  import type { Api } from "$lib/backend/api";

  interface Props {
    Editor: typeof import("$lib/Editor.svelte").default | null;
    editorRef: any;
    content: string;
    editorKey: string;
    readableLineLength?: boolean;
    readonly?: boolean;
    onchange: (markdown: string) => void;
    onblur: () => void;
    // These match the Editor component prop types
    onFileDrop?: (file: File) => Promise<{ blobUrl: string; attachmentPath: string } | null>;
    onLinkClick?: (href: string) => void;
    // Attachment picker props
    entryPath?: string;
    api?: Api | null;
    onAttachmentInsert?: (selection: {
      path: string;
      isImage: boolean;
      blobUrl?: string;
      sourceEntryPath: string;
    }) => void;
  }

  let {
    Editor,
    editorRef = $bindable(),
    content,
    editorKey,
    readableLineLength = true,
    readonly = false,
    onchange,
    onblur,
    onFileDrop,
    onLinkClick,
    entryPath,
    api,
    onAttachmentInsert,
  }: Props = $props();
</script>

<!-- Outer container: scrollable area -->
<div class="flex-1 overflow-y-auto">
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- Inner wrapper: padding and optional max-width for readability -->
  <div
    class="px-4 py-8 md:px-6 md:py-12 min-h-full"
    class:max-w-prose={readableLineLength}
    class:mx-auto={readableLineLength}
    onclick={(e) => {
      // Only handle clicks directly on this container (not bubbled from editor content)
      // This allows clicking in the empty space below the editor to focus at the end
      if (e.target === e.currentTarget) {
        editorRef?.focusAtEnd?.();
      }
    }}
  >
    {#if Editor}
      {#key editorKey}
        <Editor
          debugMenus={false}
          bind:this={editorRef}
          {content}
          {onchange}
          {onblur}
          placeholder={readonly ? "" : "Start writing..."}
          {readonly}
          {onFileDrop}
          {onLinkClick}
          {entryPath}
          {api}
          {onAttachmentInsert}
        />
      {/key}
    {:else}
      <LoadingSpinner />
    {/if}
  </div>
</div>
