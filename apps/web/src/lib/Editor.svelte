<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import TurndownService from "turndown";
  import type { Editor as EditorType } from "@tiptap/core";
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
    Link,
  } from "@lucide/svelte";

  interface Props {
    content?: string;
    placeholder?: string;
    onchange?: (markdown: string) => void;
    readonly?: boolean;
  }

  let {
    content = "",
    placeholder = "Start writing...",
    onchange,
    readonly = false,
  }: Props = $props();

  let element: HTMLDivElement;
  let editor: EditorType | null = $state(null);
  let isUpdatingContent = false; // Flag to skip onchange during programmatic updates

  // Turndown service for HTML -> Markdown conversion
  const turndownService = new TurndownService({
    headingStyle: "atx",
    codeBlockStyle: "fenced",
    bulletListMarker: "-",
  });

  // Custom rules for better markdown output
  turndownService.addRule("taskList", {
    filter: (node) => {
      return (
        node.nodeName === "LI" &&
        node.parentElement?.getAttribute("data-type") === "taskList"
      );
    },
    replacement: (content, node) => {
      const checkbox = (node as HTMLElement).querySelector(
        'input[type="checkbox"]',
      );
      const checked = checkbox?.hasAttribute("checked") ? "x" : " ";
      return `- [${checked}] ${content.trim()}\n`;
    },
  });

  turndownService.addRule("strikethrough", {
    filter: ["s", "strike", "del"] as any,
    replacement: (content) => `~~${content}~~`,
  });

  /**
   * Convert markdown to HTML for the editor
   */
  function markdownToHtml(markdown: string): string {
    // Basic markdown to HTML conversion
    // TipTap will handle most of this, but we need initial conversion
    let html = markdown
      // Headers
      .replace(/^### (.*$)/gm, "<h3>$1</h3>")
      .replace(/^## (.*$)/gm, "<h2>$1</h2>")
      .replace(/^# (.*$)/gm, "<h1>$1</h1>")
      // Bold and italic
      .replace(/\*\*\*(.*?)\*\*\*/g, "<strong><em>$1</em></strong>")
      .replace(/\*\*(.*?)\*\*/g, "<strong>$1</strong>")
      .replace(/\*(.*?)\*/g, "<em>$1</em>")
      // Strikethrough
      .replace(/~~(.*?)~~/g, "<s>$1</s>")
      // Inline code
      .replace(/`([^`]+)`/g, "<code>$1</code>")
      // Links
      .replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2">$1</a>')
      // Task lists
      .replace(
        /^- \[x\] (.*$)/gm,
        '<ul data-type="taskList"><li data-type="taskItem" data-checked="true">$1</li></ul>',
      )
      .replace(
        /^- \[ \] (.*$)/gm,
        '<ul data-type="taskList"><li data-type="taskItem" data-checked="false">$1</li></ul>',
      )
      // Unordered lists
      .replace(/^- (.*$)/gm, "<li>$1</li>")
      // Blockquotes
      .replace(/^> (.*$)/gm, "<blockquote>$1</blockquote>")
      // Horizontal rules
      .replace(/^---$/gm, "<hr>")
      // Paragraphs (lines with content)
      .replace(/^(?!<[hlubo]|<li|<hr)(.*$)/gm, (match) => {
        if (match.trim() === "") return "";
        if (match.startsWith("<")) return match;
        return `<p>${match}</p>`;
      });

    return html;
  }

  /**
   * Get the current content as markdown
   */
  export function getMarkdown(): string {
    if (!editor) return "";
    const html = editor.getHTML();
    return turndownService.turndown(html);
  }

  /**
   * Set content from markdown
   */
  export function setContent(markdown: string): void {
    if (!editor) return;
    const html = markdownToHtml(markdown);
    editor.commands.setContent(html);
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

  onMount(async () => {
    // Dynamically import TipTap modules to avoid SSR issues
    const [
      { Editor },
      { default: StarterKit },
      { default: Link },
      { default: TaskList },
      { default: TaskItem },
      { default: Placeholder },
      { default: CodeBlock },
      { default: Highlight },
      { default: Typography },
    ] = await Promise.all([
      import("@tiptap/core"),
      import("@tiptap/starter-kit"),
      import("@tiptap/extension-link"),
      import("@tiptap/extension-task-list"),
      import("@tiptap/extension-task-item"),
      import("@tiptap/extension-placeholder"),
      import("@tiptap/extension-code-block"),
      import("@tiptap/extension-highlight"),
      import("@tiptap/extension-typography"),
    ]);

    editor = new Editor({
      element,
      extensions: [
        StarterKit.configure({
          codeBlock: false, // We'll use the separate extension
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
      ],
      content: markdownToHtml(content),
      editable: !readonly,
      onUpdate: ({ editor }) => {
        // Skip onchange during programmatic content updates (e.g., switching files)
        if (onchange && !isUpdatingContent) {
          const markdown = turndownService.turndown(editor.getHTML());
          onchange(markdown);
        }
      },
      editorProps: {
        attributes: {
          class: "editor-content",
        },
      },
    });
  });

  onDestroy(() => {
    editor?.destroy();
  });

  // Update editor content when the content prop changes (e.g., switching files)
  $effect(() => {
    if (editor) {
      // Only update if the new content is different from what's currently in the editor
      const currentEditorContent = turndownService.turndown(editor.getHTML());
      if (content !== currentEditorContent) {
        // Set flag to prevent onchange from firing during programmatic update
        isUpdatingContent = true;
        editor.commands.setContent(markdownToHtml(content));
        // Reset flag after a tick to allow future user edits to trigger onchange
        setTimeout(() => {
          isUpdatingContent = false;
        }, 0);
      }
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
    const url = window.prompt("Enter URL:");
    if (url) {
      editor?.chain().focus().setLink({ href: url }).run();
    }
  }

  function unsetLink() {
    editor?.chain().focus().unsetLink().run();
  }

  // Check if format is active
  function isActive(name: string, attrs?: Record<string, unknown>): boolean {
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
          <Link class="size-4" />
        </button>
      </div>
    </div>
  {/if}

  <div class="flex-1 overflow-y-auto p-4" bind:this={element}></div>
</div>

<style>
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
</style>
