<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Editor } from "@tiptap/core";
  import StarterKit from "@tiptap/starter-kit";
  import { Markdown } from "@tiptap/markdown";
  import Link from "@tiptap/extension-link";
  import TaskList from "@tiptap/extension-task-list";
  import TaskItem from "@tiptap/extension-task-item";
  import Placeholder from "@tiptap/extension-placeholder";
  import CodeBlock from "@tiptap/extension-code-block";
  import Highlight from "@tiptap/extension-highlight";
  import Typography from "@tiptap/extension-typography";
  import Image from "@tiptap/extension-image";
  // Y.js collaboration
  import Collaboration from "@tiptap/extension-collaboration";
  import CollaborationCursor from "@tiptap/extension-collaboration-caret";
  import type * as Y from "yjs";
  import type { HocuspocusProvider } from "@hocuspocus/provider";
  import {
    Bold,
    Italic,
    Strikethrough,
    Code,
    Heading1,
    Heading2,
    Heading3,
    List,
    ListOrdered,
    CheckSquare,
    Quote,
    Braces,
    Link as LinkIcon,
    ImageIcon,
  } from "@lucide/svelte";

  interface Props {
    content?: string;
    placeholder?: string;
    onchange?: (markdown: string) => void;
    readonly?: boolean;
    onInsertImage?: () => void;
    onFileDrop?: (
      file: File,
    ) => Promise<{ blobUrl: string; attachmentPath: string } | null>;
    // Y.js collaboration options
    ydoc?: Y.Doc;
    provider?: HocuspocusProvider;
    userName?: string;
    userColor?: string;
  }

  let {
    content = "",
    placeholder = "Start writing...",
    onchange,
    readonly = false,
    onInsertImage,
    onFileDrop,
    ydoc,
    provider,
    userName = "Anonymous",
    userColor = "#958DF1",
  }: Props = $props();

  let element: HTMLDivElement;
  let editor: Editor | null = $state(null);
  let isUpdatingContent = false; // Flag to skip onchange during programmatic updates

  // Collaboration gating:
  // We show local markdown content first, then enable Collaboration after provider has synced once.
  let collabReady = $state(false);
  let providerSyncedUnsub: (() => void) | null = null;

  // Track what kind of editor we built last, so we only rebuild when it truly changes.
  // This avoids constantly recreating the editor (which can lead to blank content/races).
  let lastCollabKey: string | null = null;
  let lastReadonly: boolean | null = null;
  let lastPlaceholder: string | null = null;
  let lastCollabReady: boolean | null = null;

  function cleanupProviderSyncedHook() {
    if (providerSyncedUnsub) {
      providerSyncedUnsub();
      providerSyncedUnsub = null;
    }
  }

  function hookProviderSyncedOnce() {
    cleanupProviderSyncedHook();
    if (!provider || collabReady) return;

    // HocuspocusProvider is an EventEmitter-like object.
    // We listen once for "synced" and then flip collabReady.
    const anyProvider = provider as any;

    if (
      typeof anyProvider?.on === "function" &&
      typeof anyProvider?.off === "function"
    ) {
      const handler = () => {
        collabReady = true;
        anyProvider.off("synced", handler);
        providerSyncedUnsub = null;
      };
      anyProvider.on("synced", handler);
      providerSyncedUnsub = () => {
        try {
          anyProvider.off("synced", handler);
        } catch {
          // ignore
        }
      };
      return;
    }

    // Fallback: poll provider.synced boolean
    if (typeof (provider as any).synced === "boolean") {
      const interval = window.setInterval(() => {
        if ((provider as any)?.synced) {
          window.clearInterval(interval);
          collabReady = true;
          providerSyncedUnsub = null;
        }
      }, 50);
      providerSyncedUnsub = () => window.clearInterval(interval);
    }
  }

  function destroyEditor() {
    cleanupProviderSyncedHook();
    editor?.destroy();
    editor = null;
  }

  function createEditor() {
    destroyEditor();

    // Build extensions array
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const extensions: any[] = [
      StarterKit.configure({
        codeBlock: false, // We'll use the separate extension
        link: false, // Disable Link in StarterKit; we register Link explicitly below
        // Disable undoRedo when using Y.js - Collaboration extension handles undo/redo
        ...(ydoc ? { undoRedo: false } : {}),
      }),
      Markdown.configure({
        //transformPastedText: true,
        //transformCopiedText: true,
      }),
      Link.configure({
        openOnClick: false,
        HTMLAttributes: {
          class: "editor-link",
        },
      }),
      TaskList,
      TaskItem.configure({
        nested: true,
      }),
      Placeholder.configure({
        placeholder,
      }),
      CodeBlock.configure({
        HTMLAttributes: {
          class: "editor-code-block",
        },
      }),
      Highlight,
      Typography,
      Image.configure({
        inline: true,
        allowBase64: true,
        HTMLAttributes: {
          class: "editor-image",
        },
      }),
    ];

    // Only enable Collaboration after provider has reported initial sync.
    // Until then, show the local markdown `content` in a regular editor.
    if (ydoc && provider && collabReady) {
      extensions.push(
        Collaboration.configure({
          document: ydoc,
        }),
      );

      extensions.push(
        CollaborationCursor.configure({
          provider,
          user: {
            name: userName,
            color: userColor,
          },
        }),
      );
    }

    editor = new Editor({
      element,
      extensions,
      // Always start by showing local content; once collabReady flips,
      // the editor will be rebuilt with Collaboration enabled.
      content: content,
      contentType: "markdown",
      editable: !readonly,
      onUpdate: ({ editor }) => {
        if (onchange && !isUpdatingContent) {
          const markdown = editor.getMarkdown();
          onchange(markdown);
        }
      },
      editorProps: {
        attributes: {
          class: "editor-content",
        },
      },
    });
  }

  export function getMarkdown(): string | undefined {
    return editor?.getMarkdown();
  }

  /**
   * Set content from markdown
   */
  export function setContent(markdown: string): void {
    if (!editor) return;
    editor.commands.setContent(markdown, { contentType: "markdown" });
  }

  /**
   * Focus the editor
   */
  export function focus(): void {
    editor?.commands.focus();
  }

  /**
   * Check if editor is empty
   */
  export function isEmpty(): boolean {
    return editor?.isEmpty ?? true;
  }

  /**
   * Insert an image at cursor position
   */
  export function insertImage(src: string, alt?: string): void {
    if (!editor) return;
    editor
      .chain()
      .focus()
      .setImage({ src, alt: alt || "" })
      .run();
  }

  onMount(() => {
    // Begin in non-collab mode; enable collab after provider syncs.
    collabReady = false;
    hookProviderSyncedOnce();

    createEditor();
    lastCollabKey = ydoc ? "yjs" : "local";
    lastReadonly = readonly;
    lastPlaceholder = placeholder;
    lastCollabReady = collabReady;
  });

  onDestroy(() => {
    destroyEditor();
  });

  // Scope rebuild:
  // - Rebuild when switching between local <-> Yjs, or when readonly/placeholder changes,
  //   OR when collabReady flips after initial provider sync.
  $effect(() => {
    if (!element) return;

    // Keep the sync hook current when switching providers/docs.
    hookProviderSyncedOnce();

    const collabKey = ydoc ? "yjs" : "local";
    const needsRebuild =
      lastCollabKey === null ||
      lastReadonly === null ||
      lastPlaceholder === null ||
      lastCollabReady === null ||
      collabKey !== lastCollabKey ||
      readonly !== lastReadonly ||
      placeholder !== lastPlaceholder ||
      collabReady !== lastCollabReady;

    if (!needsRebuild) return;

    lastCollabKey = collabKey;
    lastReadonly = readonly;
    lastPlaceholder = placeholder;
    lastCollabReady = collabReady;

    createEditor();
  });

  // Update editor content when the content prop changes (e.g., switching files)
  $effect(() => {
    if (!editor) return;
    if (content === undefined) return;

    const currentEditorContent = editor.getMarkdown();
    if (content !== currentEditorContent) {
      isUpdatingContent = true;
      editor.commands.setContent(content, { contentType: "markdown" });
      setTimeout(() => {
        isUpdatingContent = false;
      }, 0);
    }
  });

  // Toolbar button handlers
  function toggleBold() {
    editor?.chain().focus().toggleBold().run();
  }

  function toggleItalic() {
    editor?.chain().focus().toggleItalic().run();
  }

  function toggleStrike() {
    editor?.chain().focus().toggleStrike().run();
  }

  function toggleCode() {
    editor?.chain().focus().toggleCode().run();
  }

  function toggleHeading(level: 1 | 2 | 3) {
    editor?.chain().focus().toggleHeading({ level }).run();
  }

  function toggleBulletList() {
    editor?.chain().focus().toggleBulletList().run();
  }

  function toggleOrderedList() {
    editor?.chain().focus().toggleOrderedList().run();
  }

  function toggleTaskList() {
    editor?.chain().focus().toggleTaskList().run();
  }

  function toggleBlockquote() {
    editor?.chain().focus().toggleBlockquote().run();
  }

  function toggleCodeBlock() {
    editor?.chain().focus().toggleCodeBlock().run();
  }

  function setLink() {
    const url = window.prompt("Enter URL");
    if (url) {
      editor
        ?.chain()
        .focus()
        .extendMarkRange("link")
        .setLink({ href: url })
        .run();
    }
  }

  function unsetLink() {
    editor?.chain().focus().unsetLink().run();
  }

  function isActive(name: string, attrs?: Record<string, unknown>) {
    return editor?.isActive(name, attrs) ?? false;
  }
</script>

<div
  class="flex flex-col h-full border border-border rounded-lg overflow-hidden bg-card"
>
  {#if !readonly}
    <div
      class="flex flex-wrap items-center gap-1 px-2 py-1.5 border-b border-border bg-muted/50"
    >
      <!-- Headings -->
      <div class="flex items-center gap-0.5">
        <button
          type="button"
          class="p-1.5 rounded-md text-sm font-medium transition-colors hover:bg-accent hover:text-accent-foreground {isActive(
            'heading',
            { level: 1 },
          )
            ? 'bg-accent text-accent-foreground'
            : 'text-muted-foreground'}"
          onclick={() => toggleHeading(1)}
          title="Heading 1"
        >
          <Heading1 class="size-4" />
        </button>
        <button
          type="button"
          class="p-1.5 rounded-md text-sm font-medium transition-colors hover:bg-accent hover:text-accent-foreground {isActive(
            'heading',
            { level: 2 },
          )
            ? 'bg-accent text-accent-foreground'
            : 'text-muted-foreground'}"
          onclick={() => toggleHeading(2)}
          title="Heading 2"
        >
          <Heading2 class="size-4" />
        </button>
        <button
          type="button"
          class="p-1.5 rounded-md text-sm font-medium transition-colors hover:bg-accent hover:text-accent-foreground {isActive(
            'heading',
            { level: 3 },
          )
            ? 'bg-accent text-accent-foreground'
            : 'text-muted-foreground'}"
          onclick={() => toggleHeading(3)}
          title="Heading 3"
        >
          <Heading3 class="size-4" />
        </button>
      </div>

      <div class="w-px h-5 bg-border mx-1"></div>

      <!-- Text formatting -->
      <div class="flex items-center gap-0.5">
        <button
          type="button"
          class="p-1.5 rounded-md text-sm font-medium transition-colors hover:bg-accent hover:text-accent-foreground {isActive(
            'bold',
          )
            ? 'bg-accent text-accent-foreground'
            : 'text-muted-foreground'}"
          onclick={toggleBold}
          title="Bold (Ctrl+B)"
        >
          <Bold class="size-4" />
        </button>
        <button
          type="button"
          class="p-1.5 rounded-md text-sm font-medium transition-colors hover:bg-accent hover:text-accent-foreground {isActive(
            'italic',
          )
            ? 'bg-accent text-accent-foreground'
            : 'text-muted-foreground'}"
          onclick={toggleItalic}
          title="Italic (Ctrl+I)"
        >
          <Italic class="size-4" />
        </button>
        <button
          type="button"
          class="p-1.5 rounded-md text-sm font-medium transition-colors hover:bg-accent hover:text-accent-foreground {isActive(
            'strike',
          )
            ? 'bg-accent text-accent-foreground'
            : 'text-muted-foreground'}"
          onclick={toggleStrike}
          title="Strikethrough"
        >
          <Strikethrough class="size-4" />
        </button>
        <button
          type="button"
          class="p-1.5 rounded-md text-sm font-medium transition-colors hover:bg-accent hover:text-accent-foreground {isActive(
            'code',
          )
            ? 'bg-accent text-accent-foreground'
            : 'text-muted-foreground'}"
          onclick={toggleCode}
          title="Inline Code"
        >
          <Code class="size-4" />
        </button>
      </div>

      <div class="w-px h-5 bg-border mx-1"></div>

      <!-- Lists -->
      <div class="flex items-center gap-0.5">
        <button
          type="button"
          class="p-1.5 rounded-md text-sm font-medium transition-colors hover:bg-accent hover:text-accent-foreground {isActive(
            'bulletList',
          )
            ? 'bg-accent text-accent-foreground'
            : 'text-muted-foreground'}"
          onclick={toggleBulletList}
          title="Bullet List"
        >
          <List class="size-4" />
        </button>
        <button
          type="button"
          class="p-1.5 rounded-md text-sm font-medium transition-colors hover:bg-accent hover:text-accent-foreground {isActive(
            'orderedList',
          )
            ? 'bg-accent text-accent-foreground'
            : 'text-muted-foreground'}"
          onclick={toggleOrderedList}
          title="Numbered List"
        >
          <ListOrdered class="size-4" />
        </button>
        <button
          type="button"
          class="p-1.5 rounded-md text-sm font-medium transition-colors hover:bg-accent hover:text-accent-foreground {isActive(
            'taskList',
          )
            ? 'bg-accent text-accent-foreground'
            : 'text-muted-foreground'}"
          onclick={toggleTaskList}
          title="Task List"
        >
          <CheckSquare class="size-4" />
        </button>
      </div>

      <div class="w-px h-5 bg-border mx-1"></div>

      <!-- Blocks -->
      <div class="flex items-center gap-0.5">
        <button
          type="button"
          class="p-1.5 rounded-md text-sm font-medium transition-colors hover:bg-accent hover:text-accent-foreground {isActive(
            'blockquote',
          )
            ? 'bg-accent text-accent-foreground'
            : 'text-muted-foreground'}"
          onclick={toggleBlockquote}
          title="Quote"
        >
          <Quote class="size-4" />
        </button>
        <button
          type="button"
          class="p-1.5 rounded-md text-sm font-medium transition-colors hover:bg-accent hover:text-accent-foreground {isActive(
            'codeBlock',
          )
            ? 'bg-accent text-accent-foreground'
            : 'text-muted-foreground'}"
          onclick={toggleCodeBlock}
          title="Code Block"
        >
          <Braces class="size-4" />
        </button>
        <button
          type="button"
          class="p-1.5 rounded-md text-sm font-medium transition-colors hover:bg-accent hover:text-accent-foreground {isActive(
            'link',
          )
            ? 'bg-accent text-accent-foreground'
            : 'text-muted-foreground'}"
          onclick={isActive("link") ? unsetLink : setLink}
          title="Link"
        >
          <LinkIcon class="size-4" />
        </button>
        <button
          type="button"
          class="p-1.5 rounded-md text-sm font-medium transition-colors hover:bg-accent hover:text-accent-foreground text-muted-foreground"
          onclick={() => onInsertImage?.()}
          title="Insert Image"
        >
          <ImageIcon class="size-4" />
        </button>
      </div>
    </div>
  {/if}

  <div
    class="flex-1 overflow-y-auto p-4"
    bind:this={element}
    role="application"
    ondragover={(e) => {
      e.preventDefault();
      e.dataTransfer && (e.dataTransfer.dropEffect = "copy");
    }}
    ondrop={async (e) => {
      e.preventDefault();
      const file = e.dataTransfer?.files?.[0];
      if (file && file.type.startsWith("image/") && onFileDrop) {
        const result = await onFileDrop(file);
        if (result && editor) {
          // Insert image at cursor position
          editor
            .chain()
            .focus()
            .setImage({ src: result.blobUrl, alt: file.name })
            .run();
        }
      }
    }}
  ></div>
</div>

<style global>
  :global(.editor-content) {
    outline: none;
    min-height: 100%;
  }

  :global(.editor-content > * + *) {
    margin-top: 0.75em;
  }

  :global(.editor-content h1) {
    font-size: 2em;
    font-weight: 700;
    line-height: 1.2;
    color: var(--foreground);
  }

  :global(.editor-content h2) {
    font-size: 1.5em;
    font-weight: 600;
    line-height: 1.3;
    color: var(--foreground);
  }

  :global(.editor-content h3) {
    font-size: 1.25em;
    font-weight: 600;
    line-height: 1.4;
    color: var(--foreground);
  }

  :global(.editor-content p) {
    line-height: 1.6;
    color: var(--foreground);
  }

  :global(.editor-content ul),
  :global(.editor-content ol) {
    padding-left: 1.5em;
  }

  :global(.editor-content li) {
    margin: 0.25em 0;
  }

  :global(.editor-content ul[data-type="taskList"]) {
    list-style: none;
    padding-left: 0;
  }

  :global(.editor-content ul[data-type="taskList"] li) {
    display: flex;
    align-items: flex-start;
    gap: 8px;
  }

  :global(.editor-content ul[data-type="taskList"] li input) {
    margin-top: 4px;
    accent-color: var(--primary);
  }

  :global(.editor-content blockquote) {
    border-left: 3px solid var(--primary);
    padding-left: 1em;
    margin-left: 0;
    color: var(--muted-foreground);
    font-style: italic;
  }

  :global(.editor-content code) {
    background: var(--muted);
    padding: 2px 6px;
    border-radius: 4px;
    font-family: "SF Mono", Monaco, "Cascadia Code", monospace;
    font-size: 0.9em;
  }

  :global(.editor-code-block) {
    background: var(--muted);
    padding: 12px 16px;
    border-radius: 6px;
    font-family: "SF Mono", Monaco, "Cascadia Code", monospace;
    font-size: 0.9em;
    overflow-x: auto;
  }

  :global(.editor-code-block code) {
    background: none;
    padding: 0;
  }

  :global(.editor-link) {
    color: var(--primary);
    text-decoration: underline;
    cursor: pointer;
  }

  :global(.editor-content p.is-editor-empty:first-child::before) {
    content: attr(data-placeholder);
    float: left;
    color: var(--muted-foreground);
    pointer-events: none;
    height: 0;
  }

  :global(.editor-content hr) {
    border: none;
    border-top: 1px solid var(--border);
    margin: 1.5em 0;
  }

  :global(.editor-content strong) {
    font-weight: 600;
  }

  :global(.editor-content em) {
    font-style: italic;
  }

  :global(.editor-content s) {
    text-decoration: line-through;
  }

  :global(.editor-content a) {
    color: var(--primary);
    text-decoration: underline;
  }

  :global(.editor-content a:hover) {
    opacity: 0.8;
  }

  :global(.editor-image) {
    max-width: 100%;
    height: auto;
    border-radius: 6px;
    margin: 0.5em 0;
  }

  /* Collaborative cursor styles */
  :global(.collaboration-carets__caret) {
    border-left: 1px solid;
    border-right: 1px solid;
    margin-left: -1px;
    margin-right: -1px;
    pointer-events: none;
    position: relative;
    word-break: normal;
  }

  :global(.collaboration-carets__label) {
    border-radius: 3px 3px 3px 0;
    color: #fff;
    font-size: 12px;
    font-weight: 600;
    left: -1px;
    line-height: normal;
    padding: 0.1rem 0.3rem;
    position: absolute;
    top: -1.4em;
    user-select: none;
    white-space: nowrap;
  }
</style>
