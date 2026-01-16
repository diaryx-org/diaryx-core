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
  // FloatingMenu extension for block formatting
  import FloatingMenu from "@tiptap/extension-floating-menu";
  // BubbleMenu extension for inline formatting on selection
  import BubbleMenu from "@tiptap/extension-bubble-menu";
  // ProseMirror Plugin for link click handling
  import { Plugin as ProseMirrorPlugin } from "@tiptap/pm/state";

  // FloatingMenu for block formatting (headings, lists, etc.)
  import FloatingMenuComponent from "./components/FloatingMenuComponent.svelte";
  // BubbleMenu for inline formatting when text is selected
  import BubbleMenuComponent from "./components/BubbleMenuComponent.svelte";

  // Custom extension for inline attachment picker node
  import { AttachmentPickerNode } from "./extensions/AttachmentPickerNode";
  import type { Api } from "$lib/backend/api";

  interface Props {
    content?: string;
    placeholder?: string;
    onchange?: (markdown: string) => void;
    onblur?: () => void;
    readonly?: boolean;
    onFileDrop?: (
      file: File,
    ) => Promise<{ blobUrl: string; attachmentPath: string } | null>;
    // Debug mode for menus (logs shouldShow decisions to console)
    debugMenus?: boolean;
    // Callback when a link is clicked (for handling relative links to other notes)
    onLinkClick?: (href: string) => void;
    // Attachment picker options
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
    content = "",
    placeholder = "Start writing...",
    onchange,
    onblur,
    readonly = false,
    onFileDrop,
    debugMenus = false,
    onLinkClick,
    entryPath = "",
    api = null,
    onAttachmentInsert,
  }: Props = $props();

  let element: HTMLDivElement;
  let editor: Editor | null = $state(null);

  // FloatingMenu element ref - must exist before editor creation
  let floatingMenuElement: HTMLDivElement | undefined = $state();
  // FloatingMenu component ref - for programmatic expansion
  let floatingMenuRef: { expand: () => void } | undefined = $state();
  // BubbleMenu element ref - must exist before editor creation
  let bubbleMenuElement: HTMLDivElement | undefined = $state();
  let isUpdatingContent = false; // Flag to skip onchange during programmatic updates

  // Track the last content prop value we synced FROM, so we only sync when it actually changes
  // This prevents resetting editor content when the user is typing and the prop hasn't changed
  let lastSyncedContent: string | undefined = undefined;

  // Track what kind of editor we built last, so we only rebuild when it truly changes.
  // This avoids constantly recreating the editor (which can lead to blank content/races).
  let lastReadonly: boolean | null = null;
  let lastPlaceholder: string | null = null;

  function destroyEditor() {
    editor?.destroy();
    editor = null;
  }

  function createEditor() {
    destroyEditor();

    // In non-readonly mode, require FloatingMenu element
    if (!readonly && !floatingMenuElement) {
      if (debugMenus) {
        console.log(
          "[Editor] FloatingMenu element not ready, deferring editor creation",
        );
      }
      return;
    }

    // Build extensions array
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const extensions: any[] = [
      StarterKit.configure({
        codeBlock: false, // We'll use the separate extension
        link: false, // Disable Link in StarterKit; we register Link explicitly below
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
      }).extend({
        // Add click handler for links
        addProseMirrorPlugins() {
          const plugins = this.parent?.() ?? [];
          return [
            ...plugins,
            new ProseMirrorPlugin({
              props: {
                handleClick: (_view, _pos, event) => {
                  const target = event.target as HTMLElement;
                  const link = target.closest("a");
                  if (link && link.href) {
                    event.preventDefault();
                    const href = link.getAttribute("href") || "";
                    if (onLinkClick) {
                      onLinkClick(href);
                    } else if (
                      href.startsWith("http://") ||
                      href.startsWith("https://")
                    ) {
                      // External link - open in new tab
                      window.open(href, "_blank", "noopener,noreferrer");
                    }
                    return true;
                  }
                  return false;
                },
              },
            }),
          ];
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
      // Inline attachment picker node extension
      AttachmentPickerNode.configure({
        entryPath,
        api,
        onAttachmentSelect: (selection) => {
          onAttachmentInsert?.(selection);
        },
      }),
    ];

    // Add FloatingMenu extension (for block formatting on empty lines)
    if (!readonly) {
      extensions.push(
        FloatingMenu.configure({
          element: floatingMenuElement,
          appendTo: () => document.body,
          options: {
            strategy: "fixed",
            placement: "left-start",
            offset: 10,
            flip: {
              fallbackPlacements: ["right-start", "left", "right"],
            },
            shift: {
              padding: 8,
            },
            // Manually control visibility to prevent flash on initial load
            onShow: () => {
              if (floatingMenuElement) {
                floatingMenuElement.style.display = "block";
              }
            },
            onHide: () => {
              if (floatingMenuElement) {
                floatingMenuElement.style.display = "none";
              }
            },
          },
          shouldShow: ({ editor: ed, view, state }) => {
            const { selection } = state;
            const { empty } = selection;
            const anchor = selection.$anchor;

            if (debugMenus) {
              console.log("[FloatingMenu] shouldShow check", {
                empty,
                editable: ed.isEditable,
                hasFocus: view.hasFocus(),
                parentType: anchor.parent.type.name,
                contentSize: anchor.parent.content.size,
              });
            }

            // Must be editable
            if (!ed.isEditable) return false;

            // Must have focus - prevents menu from showing on initial load
            // before user has interacted with the editor
            if (!view.hasFocus()) return false;

            // Must be an empty selection (cursor, not a range)
            if (!empty) return false;

            // Only show on empty paragraph lines
            const isEmptyParagraph =
              anchor.parent.type.name === "paragraph" &&
              anchor.parent.content.size === 0;
            if (!isEmptyParagraph) return false;

            // Don't show in code blocks
            if (ed.isActive("codeBlock")) return false;

            // Don't show in lists
            if (
              ed.isActive("bulletList") ||
              ed.isActive("orderedList") ||
              ed.isActive("taskList")
            ) {
              return false;
            }

            if (debugMenus) {
              console.log("[FloatingMenu] shouldShow: true");
            }
            return true;
          },
        }),
      );

      // Add BubbleMenu extension (for inline formatting when text is selected)
      // Only show on desktop - mobile uses the bottom InlineToolbar
      if (bubbleMenuElement) {
        extensions.push(
          BubbleMenu.configure({
            element: bubbleMenuElement,
            options: {
              offset: 10,
            },
            shouldShow: ({ editor: ed, view, state, from, to }) => {
              // Must be editable
              if (!ed.isEditable) return false;

              // Must have focus
              if (!view.hasFocus()) return false;

              // Must have a selection (not just cursor)
              const { empty } = state.selection;
              if (empty) return false;

              // Don't show in code blocks
              if (ed.isActive("codeBlock")) return false;

              // Check if the selection contains actual text content
              const text = state.doc.textBetween(from, to, " ");
              if (!text.trim()) return false;

              if (debugMenus) {
                console.log("[BubbleMenu] shouldShow: true");
              }
              return true;
            },
          }),
        );
      }
    }

    editor = new Editor({
      element,
      extensions,
      content: content,
      contentType: "markdown",
      editable: !readonly,
      onCreate: () => {
        // Track the initial content so we don't reset it on the first effect run
        lastSyncedContent = content;
      },
      onUpdate: ({ editor }) => {
        if (onchange && !isUpdatingContent) {
          const markdown = editor.getMarkdown();
          onchange(markdown);
        }
      },
      onBlur: () => {
        onblur?.();
      },
      editorProps: {
        attributes: {
          class: "editor-content",
        },
        handleKeyDown: (view, event) => {
          // Right Arrow on empty paragraph opens floating menu
          if (event.key === "ArrowRight" && floatingMenuRef) {
            const { state } = view;
            const { selection } = state;
            const { empty } = selection;
            const anchor = selection.$anchor;

            // Check if we're on an empty paragraph (same conditions as floating menu shouldShow)
            const isEmptyParagraph =
              anchor.parent.type.name === "paragraph" &&
              anchor.parent.content.size === 0;

            if (empty && isEmptyParagraph) {
              event.preventDefault();
              floatingMenuRef.expand();
              return true;
            }
          }
          return false;
        },
        handlePaste: (_view, event) => {
          const items = event.clipboardData?.items;
          if (!items) return false;

          for (const item of items) {
            // Handle pasted images
            if (item.type.startsWith('image/')) {
              const file = item.getAsFile();
              if (file && onFileDrop) {
                event.preventDefault();
                onFileDrop(file).then(result => {
                  if (result && result.blobUrl && editor) {
                    editor.chain().focus()
                      .setImage({ src: result.blobUrl, alt: file.name })
                      .run();
                  }
                });
                return true;
              }
            }
          }
          return false;
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
   * Focus the editor at the end of the document and create a new paragraph if needed
   */
  export function focusAtEnd(): void {
    if (!editor) return;

    // Move cursor to end of document
    editor.commands.focus("end");

    // Check if we're on an empty paragraph - if not, create one
    const { selection } = editor.state;
    const currentNode = selection.$anchor.parent;
    const isEmptyParagraph = currentNode.type.name === "paragraph" && currentNode.content.size === 0;

    if (!isEmptyParagraph) {
      // Insert a new paragraph at the end
      editor.chain().focus("end").createParagraphNear().focus("end").run();
    }
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
    // Don't create editor here - let the $effect handle it once menu elements are ready
    // This ensures BubbleMenu and FloatingMenu extensions have elements to bind to
  });

  // Track if we've done initial editor creation
  let editorInitialized = $state(false);

  // Wait for menu elements to be available before creating editor
  // This effect runs when bubbleMenuElement or floatingMenuElement change
  $effect(() => {
    // Explicitly track the element refs so Svelte knows to re-run when they change
    const hasEditorElement = !!element;
    const hasFloatingMenu = !!floatingMenuElement;
    const hasBubbleMenu = !!bubbleMenuElement;
    const isReadonly = readonly;

    if (debugMenus) {
      console.log("[Editor] Init effect check", {
        hasEditorElement,
        hasFloatingMenu,
        hasBubbleMenu,
        isReadonly,
        editorInitialized,
      });
    }

    // In readonly mode, we don't need menu elements
    if (isReadonly) {
      if (!editorInitialized && hasEditorElement) {
        if (debugMenus) {
          console.log("[Editor] Creating editor (readonly mode)");
        }
        createEditor();
        editorInitialized = true;
        lastReadonly = readonly;
        lastPlaceholder = placeholder;
      }
      return;
    }

    // In edit mode, wait for menu elements
    if (!editorInitialized && hasEditorElement && hasFloatingMenu && hasBubbleMenu) {
      if (debugMenus) {
        console.log("[Editor] Menu elements ready, creating editor", {
          floatingMenuElement,
          bubbleMenuElement,
        });
      }
      createEditor();
      editorInitialized = true;
      lastReadonly = readonly;
      lastPlaceholder = placeholder;
    }
  });

  // Reset editorInitialized when readonly changes so the effect can re-run
  $effect(() => {
    if (lastReadonly !== null && lastReadonly !== readonly) {
      editorInitialized = false;
    }
  });

  onDestroy(() => {
    destroyEditor();
  });

  // Rebuild editor when readonly or placeholder changes
  $effect(() => {
    if (!element) return;
    // Skip if we haven't done initial creation yet
    if (!editorInitialized) return;

    const needsRebuild =
      readonly !== lastReadonly ||
      placeholder !== lastPlaceholder;

    if (!needsRebuild) return;

    // Update tracking for what we're about to build
    lastReadonly = readonly;
    lastPlaceholder = placeholder;

    createEditor();
  });

  // Update editor content when the content prop changes (e.g., switching files)
  // Only sync when the content PROP has actually changed from what we last synced
  // This prevents resetting user's typing when the prop hasn't changed
  $effect(() => {
    if (!editor) return;
    if (content === undefined) return;

    // Only sync if the content prop has actually changed from what we last synced
    // This prevents resetting the editor when the user is typing (prop stays the same,
    // but editor content changes)
    if (content === lastSyncedContent) return;

    // Content prop changed - sync it to the editor
    lastSyncedContent = content;
    isUpdatingContent = true;
    editor.commands.setContent(content, { contentType: "markdown" });
    setTimeout(() => {
      isUpdatingContent = false;
    }, 0);
  });
</script>

<!-- Editor content area - scrolling handled by parent EditorContent -->
<div
  bind:this={element}
  class="min-h-full"
  role="application"
  ondragover={(e) => {
    e.preventDefault();
    e.dataTransfer && (e.dataTransfer.dropEffect = "copy");
  }}
  ondrop={async (e) => {
    e.preventDefault();
    const file = e.dataTransfer?.files?.[0];
    if (file && onFileDrop) {
      const result = await onFileDrop(file);
      // Only insert into editor if it's an image with a blob URL
      if (result && result.blobUrl && editor && file.type.startsWith("image/")) {
        editor
          .chain()
          .focus()
          .setImage({ src: result.blobUrl, alt: file.name })
          .run();
      }
    }
  }}
></div>


<!-- FloatingMenu for block formatting (appears on empty lines) -->
<!-- Element must exist before editor creation for extension to bind to it -->
{#if !readonly}
  <FloatingMenuComponent
    bind:this={floatingMenuRef}
    {editor}
    onInsertAttachment={() => editor?.commands.insertAttachmentPicker()}
    bind:element={floatingMenuElement}
  />
{/if}

<!-- BubbleMenu for inline formatting (appears when text is selected) -->
{#if !readonly}
  <BubbleMenuComponent {editor} bind:element={bubbleMenuElement} />
{/if}

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

  /* Mobile-specific styles */
  @media (max-width: 767px) {
    :global(.editor-content) {
      /* Slightly larger touch targets on mobile */
      font-size: 16px; /* Prevents iOS zoom on focus */
    }

    :global(.editor-content h1) {
      font-size: 1.75em;
    }

    :global(.editor-content h2) {
      font-size: 1.35em;
    }

    :global(.editor-content h3) {
      font-size: 1.15em;
    }
  }
</style>
