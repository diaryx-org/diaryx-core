<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import TurndownService from "turndown";
  import type { Editor as EditorType } from "@tiptap/core";

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
  let isMounted = $state(false);

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
    filter: ["s", "strike", "del"],
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
        if (onchange) {
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

  // Set mounted state after editor is ready
  $effect(() => {
    if (editor) {
      isMounted = true;
    }
  });

  // Track previous content to detect external changes
  let previousContent = content;

  // Update editor content when the content prop changes (e.g., switching files)
  $effect(() => {
    if (editor && content !== previousContent) {
      // Only update if the new content is different from what's currently in the editor
      const currentEditorContent = turndownService.turndown(editor.getHTML());
      if (content !== currentEditorContent) {
        editor.commands.setContent(markdownToHtml(content));
      }
      previousContent = content;
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

<div class="editor-wrapper">
  {#if !readonly}
    <div class="toolbar">
      <div class="toolbar-group">
        <button
          type="button"
          class:active={isActive("heading", { level: 1 })}
          onclick={() => toggleHeading(1)}
          title="Heading 1"
        >
          H1
        </button>
        <button
          type="button"
          class:active={isActive("heading", { level: 2 })}
          onclick={() => toggleHeading(2)}
          title="Heading 2"
        >
          H2
        </button>
        <button
          type="button"
          class:active={isActive("heading", { level: 3 })}
          onclick={() => toggleHeading(3)}
          title="Heading 3"
        >
          H3
        </button>
      </div>

      <div class="toolbar-divider"></div>

      <div class="toolbar-group">
        <button
          type="button"
          class:active={isActive("bold")}
          onclick={toggleBold}
          title="Bold (Ctrl+B)"
        >
          <strong>B</strong>
        </button>
        <button
          type="button"
          class:active={isActive("italic")}
          onclick={toggleItalic}
          title="Italic (Ctrl+I)"
        >
          <em>I</em>
        </button>
        <button
          type="button"
          class:active={isActive("strike")}
          onclick={toggleStrike}
          title="Strikethrough"
        >
          <s>S</s>
        </button>
        <button
          type="button"
          class:active={isActive("code")}
          onclick={toggleCode}
          title="Inline Code"
        >
          &lt;/&gt;
        </button>
      </div>

      <div class="toolbar-divider"></div>

      <div class="toolbar-group">
        <button
          type="button"
          class:active={isActive("bulletList")}
          onclick={toggleBulletList}
          title="Bullet List"
        >
          •
        </button>
        <button
          type="button"
          class:active={isActive("orderedList")}
          onclick={toggleOrderedList}
          title="Numbered List"
        >
          1.
        </button>
        <button
          type="button"
          class:active={isActive("taskList")}
          onclick={toggleTaskList}
          title="Task List"
        >
          ☑
        </button>
      </div>

      <div class="toolbar-divider"></div>

      <div class="toolbar-group">
        <button
          type="button"
          class:active={isActive("blockquote")}
          onclick={toggleBlockquote}
          title="Quote"
        >
          "
        </button>
        <button
          type="button"
          class:active={isActive("codeBlock")}
          onclick={toggleCodeBlock}
          title="Code Block"
        >
          &lbrace;&rbrace;
        </button>
        <button
          type="button"
          class:active={isActive("link")}
          onclick={isActive("link") ? unsetLink : setLink}
          title="Link"
        >
          🔗
        </button>
      </div>
    </div>
  {/if}

  <div class="editor-container" bind:this={element}></div>
</div>

<style>
  .editor-wrapper {
    display: flex;
    flex-direction: column;
    height: 100%;
    border: 1px solid var(--border-color, #e5e7eb);
    border-radius: 8px;
    overflow: hidden;
    background: var(--bg-color, #ffffff);
  }

  .toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    padding: 8px;
    border-bottom: 1px solid var(--border-color, #e5e7eb);
    background: var(--toolbar-bg, #f9fafb);
  }

  .toolbar-group {
    display: flex;
    gap: 2px;
  }

  .toolbar-divider {
    width: 1px;
    background: var(--border-color, #e5e7eb);
    margin: 0 4px;
  }

  .toolbar button {
    padding: 6px 10px;
    border: none;
    border-radius: 4px;
    background: transparent;
    cursor: pointer;
    font-size: 14px;
    color: var(--text-color, #374151);
    transition: background-color 0.15s;
  }

  .toolbar button:hover {
    background: var(--hover-bg, #e5e7eb);
  }

  .toolbar button.active {
    background: var(--active-bg, #dbeafe);
    color: var(--active-color, #2563eb);
  }

  .editor-container {
    flex: 1;
    overflow-y: auto;
    padding: 16px;
  }

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
  }

  :global(.editor-content h2) {
    font-size: 1.5em;
    font-weight: 600;
    line-height: 1.3;
  }

  :global(.editor-content h3) {
    font-size: 1.25em;
    font-weight: 600;
    line-height: 1.4;
  }

  :global(.editor-content p) {
    line-height: 1.6;
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
  }

  :global(.editor-content blockquote) {
    border-left: 3px solid var(--accent-color, #2563eb);
    padding-left: 1em;
    margin-left: 0;
    color: var(--muted-color, #6b7280);
    font-style: italic;
  }

  :global(.editor-content code) {
    background: var(--code-bg, #f3f4f6);
    padding: 2px 6px;
    border-radius: 4px;
    font-family: "SF Mono", Monaco, "Cascadia Code", monospace;
    font-size: 0.9em;
  }

  :global(.editor-code-block) {
    background: var(--code-bg, #f3f4f6);
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
    color: var(--accent-color, #2563eb);
    text-decoration: underline;
    cursor: pointer;
  }

  :global(.editor-content p.is-editor-empty:first-child::before) {
    content: attr(data-placeholder);
    float: left;
    color: var(--placeholder-color, #9ca3af);
    pointer-events: none;
    height: 0;
  }

  /* Dark mode support */
  @media (prefers-color-scheme: dark) {
    .editor-wrapper {
      --bg-color: #1f2937;
      --border-color: #374151;
      --toolbar-bg: #111827;
      --text-color: #e5e7eb;
      --hover-bg: #374151;
      --active-bg: #1e3a5f;
      --active-color: #60a5fa;
      --muted-color: #9ca3af;
      --code-bg: #111827;
      --placeholder-color: #6b7280;
      --accent-color: #60a5fa;
    }
  }
</style>
