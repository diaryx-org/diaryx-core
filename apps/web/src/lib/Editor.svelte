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
  // ProseMirror Plugin for link click handling
  import { Plugin as ProseMirrorPlugin } from "@tiptap/pm/state";
  // Y.js collaboration
  import Collaboration from "@tiptap/extension-collaboration";
  import CollaborationCursor from "@tiptap/extension-collaboration-caret";
  import type * as Y from "yjs";
  import type { HocuspocusProvider } from "@hocuspocus/provider";

  // Mobile detection
  import { getMobileState } from "./hooks/useMobile.svelte";

  // InlineToolbar for inline formatting (Bold, Italic, etc.)
  import InlineToolbar from "./components/InlineToolbar.svelte";

  // FloatingMenu for block formatting (headings, lists, etc.)
  import FloatingMenuComponent from "./components/FloatingMenuComponent.svelte";

  interface Props {
    content?: string;
    placeholder?: string;
    onchange?: (markdown: string) => void;
    onblur?: () => void;
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
    // Debug mode for menus (logs shouldShow decisions to console)
    debugMenus?: boolean;
    // Callback when a link is clicked (for handling relative links to other notes)
    onLinkClick?: (href: string) => void;
  }

  let {
    content = "",
    placeholder = "Start writing...",
    onchange,
    onblur,
    readonly = false,
    onInsertImage,
    onFileDrop,
    ydoc,
    provider,
    userName = "Anonymous",
    userColor = "#958DF1",
    debugMenus = false,
    onLinkClick,
  }: Props = $props();

  let element: HTMLDivElement;
  let editor: Editor | null = $state(null);

  // FloatingMenu element ref - must exist before editor creation
  let floatingMenuElement: HTMLDivElement | undefined = $state();
  let isUpdatingContent = false; // Flag to skip onchange during programmatic updates

  // Track editor focus state for mobile toolbar visibility
  let editorHasFocus = $state(false);

  // Track the last content prop value we synced FROM, so we only sync when it actually changes
  // This prevents resetting editor content when the user is typing and the prop hasn't changed
  let lastSyncedContent: string | undefined = undefined;

  // Mobile state for responsive behavior
  const mobileState = getMobileState();

  // Collaboration gating:
  // We show local markdown content first, then enable Collaboration after provider has synced once.
  let collabReady = $state(false);
  let providerSyncedUnsub: (() => void) | null = null;

  // Track what kind of editor we built last, so we only rebuild when it truly changes.
  // This avoids constantly recreating the editor (which can lead to blank content/races).
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

    const anyProvider = provider as any;

    // HocuspocusProvider: check if already synced
    if (anyProvider.synced === true) {
      collabReady = true;
      return;
    }

    // HocuspocusProvider uses 'synced' event (not 'sync')
    if (typeof anyProvider.on === "function" && typeof anyProvider.off === "function") {
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
    if (typeof anyProvider.synced === "boolean") {
      const interval = window.setInterval(() => {
        if (anyProvider?.synced) {
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
    }

    // Only enable Collaboration after provider has reported initial sync.
    // Until then, show the local markdown `content` in a regular editor.
    if (ydoc && provider && collabReady) {
      // CRITICAL: If the Y.Doc fragment is empty after sync, we need to populate it
      // with the local markdown content. Without this, switching to Collaboration mode
      // would show empty content because TipTap ignores the `content` prop when
      // Collaboration is enabled.
      const fragment = ydoc.getXmlFragment("default");
      
      // Only seed content if:
      // 1. Fragment is truly empty (no content from server or cache)
      // 2. We have local content to seed
      // If server already has content, we should NOT seed - just use server content
      if (fragment.length === 0 && content) {
        console.log("[Editor] Y.Doc is empty after sync, initializing with local content");
        // We'll initialize the Y.Doc by creating a temporary editor and syncing it
        // This is done BEFORE we create our main editor
        const tempEditor = new Editor({
          extensions: [
            StarterKit.configure({
              codeBlock: false,
              link: false,
            }),
            Markdown,
            Collaboration.configure({
              document: ydoc,
            }),
          ],
          content: content,
          contentType: "markdown",
        });
        // Content is now in the Y.Doc, destroy temp editor
        tempEditor.destroy();
      } else if (fragment.length > 0) {
        console.log("[Editor] Y.Doc has content from sync, using that instead of local content");
      }

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

    // Determine what content to use:
    // - If Collaboration is NOT enabled: use local markdown content
    // - If Collaboration IS enabled: don't pass content prop (Y.Doc is source of truth)
    const useCollaboration = ydoc && provider && collabReady;
    
    editor = new Editor({
      element,
      extensions,
      // Only pass content when NOT using collaboration
      // When using collaboration, Y.Doc is the source of truth
      ...(useCollaboration ? {} : { content: content, contentType: "markdown" }),
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
      onFocus: () => {
        editorHasFocus = true;
      },
      onBlur: () => {
        editorHasFocus = false;
        onblur?.();
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
    const isReadonly = readonly;

    if (debugMenus) {
      console.log("[Editor] Init effect check", {
        hasEditorElement,
        hasFloatingMenu,
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
        lastCollabReady = collabReady;
      }
      return;
    }

    // In edit mode, wait for FloatingMenu element
    if (!editorInitialized && hasEditorElement && hasFloatingMenu) {
      if (debugMenus) {
        console.log("[Editor] Menu elements ready, creating editor", {
          floatingMenuElement,
        });
      }
      createEditor();
      editorInitialized = true;
      lastReadonly = readonly;
      lastPlaceholder = placeholder;
      lastCollabReady = collabReady;
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

  // Scope rebuild:
  // - Rebuild when collabReady flips (after sync completes), or when readonly/placeholder changes.
  // - Do NOT rebuild just because ydoc prop arrived - that happens before sync and would lose content.
  $effect(() => {
    if (!element) return;
    // Skip if we haven't done initial creation yet
    if (!editorInitialized) return;

    // Keep the sync hook current when switching providers/docs.
    hookProviderSyncedOnce();

    // Only rebuild when collabReady actually changes (after sync),
    // or when readonly/placeholder changes.
    // The key insight: we DON'T want to rebuild just because ydoc arrived -
    // we want to keep showing local content until sync completes.
    const needsRebuild =
      readonly !== lastReadonly ||
      placeholder !== lastPlaceholder ||
      collabReady !== lastCollabReady;

    if (!needsRebuild) return;

    // Update tracking for what we're about to build
    lastReadonly = readonly;
    lastPlaceholder = placeholder;
    lastCollabReady = collabReady;

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

<!--
  Container:
  - On desktop: bordered, rounded container
  - On mobile: edge-to-edge, no border/rounding for maximum writing space
-->
<div
  class="relative flex flex-col h-full overflow-hidden bg-card
         md:border md:border-border md:rounded-lg"
>
  <!-- Desktop inline toolbar: fixed at top, hidden on mobile -->
  {#if !readonly && !mobileState.isMobile}
    <InlineToolbar {editor} position="top" />
  {/if}

  <!-- Editor content area -->
  <div
    class="relative flex-1 overflow-y-auto px-4 py-3 md:p-4"
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

<!-- Mobile inline toolbar: appears above virtual keyboard when keyboard is visible AND editor is focused -->
{#if !readonly && mobileState.isMobile && mobileState.keyboardVisible && editorHasFocus}
  <InlineToolbar
    {editor}
    position="bottom"
    bottomOffset={mobileState.keyboardHeight}
    showDone={true}
  />
{/if}

<!-- FloatingMenu for block formatting (appears on empty lines) -->
<!-- Element must exist before editor creation for extension to bind to it -->
{#if !readonly}
  <FloatingMenuComponent
    {editor}
    {onInsertImage}
    bind:element={floatingMenuElement}
  />
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
