<script lang="ts">
  import type { Editor } from "@tiptap/core";
  import {
    Plus,
    Heading1,
    Heading2,
    Heading3,
    List,
    ListOrdered,
    CheckSquare,
    Quote,
    Braces,
    ImageIcon,
    Minus,
  } from "@lucide/svelte";

  interface Props {
    editor: Editor | null;
    onInsertImage?: () => void;
    element?: HTMLDivElement;
  }

  let { editor, onInsertImage, element = $bindable() }: Props = $props();

  let isExpanded = $state(false);

  function collapseMenu() {
    isExpanded = false;
  }

  function expandMenu(event: MouseEvent | TouchEvent) {
    // Prevent the event from bubbling to the window click handler
    event.stopPropagation();
    event.preventDefault();
    isExpanded = true;
  }

  function handleHeading(level: 1 | 2 | 3) {
    editor?.chain().focus().toggleHeading({ level }).run();
    collapseMenu();
  }

  function handleBulletList() {
    editor?.chain().focus().toggleBulletList().run();
    collapseMenu();
  }

  function handleOrderedList() {
    editor?.chain().focus().toggleOrderedList().run();
    collapseMenu();
  }

  function handleTaskList() {
    editor?.chain().focus().toggleTaskList().run();
    collapseMenu();
  }

  function handleBlockquote() {
    editor?.chain().focus().toggleBlockquote().run();
    collapseMenu();
  }

  function handleCodeBlock() {
    editor?.chain().focus().toggleCodeBlock().run();
    collapseMenu();
  }

  function handleHorizontalRule() {
    editor?.chain().focus().setHorizontalRule().run();
    collapseMenu();
  }

  function handleImage() {
    onInsertImage?.();
    collapseMenu();
  }

  // Handle menu item clicks - stop propagation to prevent closing
  function handleMenuItemClick(
    event: MouseEvent | TouchEvent,
    action: () => void,
  ) {
    event.stopPropagation();
    action();
  }

  // Close expanded menu when clicking outside
  function handleClickOutside(event: MouseEvent | TouchEvent) {
    if (!isExpanded) return;
    if (!element) return;

    const target = event.target as Node;
    if (!element.contains(target)) {
      isExpanded = false;
    }
  }
</script>

<svelte:window onclick={handleClickOutside} ontouchend={handleClickOutside} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={element}
  class="floating-menu"
  role="toolbar"
  aria-label="Block formatting"
  tabindex="-1"
  onmousedown={(e) => {
    // Prevent focus loss when clicking on the menu
    // This keeps the editor focused so the FloatingMenu extension doesn't hide it
    e.preventDefault();
  }}
  ontouchstart={(e) => {
    // Same for touch events
    e.preventDefault();
  }}
>
  {#if isExpanded}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="menu-expanded" onclick={(e) => e.stopPropagation()}>
      <div class="menu-section">
        <button
          type="button"
          onclick={(e) => handleMenuItemClick(e, () => handleHeading(1))}
          class="menu-item"
          title="Heading 1"
        >
          <Heading1 class="size-4" />
          <span>Heading 1</span>
        </button>
        <button
          type="button"
          onclick={(e) => handleMenuItemClick(e, () => handleHeading(2))}
          class="menu-item"
          title="Heading 2"
        >
          <Heading2 class="size-4" />
          <span>Heading 2</span>
        </button>
        <button
          type="button"
          onclick={(e) => handleMenuItemClick(e, () => handleHeading(3))}
          class="menu-item"
          title="Heading 3"
        >
          <Heading3 class="size-4" />
          <span>Heading 3</span>
        </button>
      </div>

      <div class="menu-divider"></div>

      <div class="menu-section">
        <button
          type="button"
          onclick={(e) => handleMenuItemClick(e, handleBulletList)}
          class="menu-item"
          title="Bullet List"
        >
          <List class="size-4" />
          <span>Bullet List</span>
        </button>
        <button
          type="button"
          onclick={(e) => handleMenuItemClick(e, handleOrderedList)}
          class="menu-item"
          title="Numbered List"
        >
          <ListOrdered class="size-4" />
          <span>Numbered List</span>
        </button>
        <button
          type="button"
          onclick={(e) => handleMenuItemClick(e, handleTaskList)}
          class="menu-item"
          title="Task List"
        >
          <CheckSquare class="size-4" />
          <span>Task List</span>
        </button>
      </div>

      <div class="menu-divider"></div>

      <div class="menu-section">
        <button
          type="button"
          onclick={(e) => handleMenuItemClick(e, handleBlockquote)}
          class="menu-item"
          title="Quote"
        >
          <Quote class="size-4" />
          <span>Blockquote</span>
        </button>
        <button
          type="button"
          onclick={(e) => handleMenuItemClick(e, handleCodeBlock)}
          class="menu-item"
          title="Code Block"
        >
          <Braces class="size-4" />
          <span>Code Block</span>
        </button>
        <button
          type="button"
          onclick={(e) => handleMenuItemClick(e, handleHorizontalRule)}
          class="menu-item"
          title="Horizontal Rule"
        >
          <Minus class="size-4" />
          <span>Divider</span>
        </button>
      </div>

      {#if onInsertImage}
        <div class="menu-divider"></div>

        <div class="menu-section">
          <button
            type="button"
            onclick={(e) => handleMenuItemClick(e, handleImage)}
            class="menu-item"
            title="Insert Image"
          >
            <ImageIcon class="size-4" />
            <span>Image</span>
          </button>
        </div>
      {/if}
    </div>
  {:else}
    <button
      type="button"
      class="trigger-button"
      onmousedown={(e) => {
        e.preventDefault();
        e.stopPropagation();
      }}
      onclick={expandMenu}
      title="Add block"
      aria-expanded={isExpanded}
    >
      <Plus class="size-5" />
    </button>
  {/if}
</div>

<style>
  .floating-menu {
    z-index: 20;
    /* TipTap will handle the display/positioning */
  }

  .trigger-button {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: 6px;
    background: var(--card);
    border: 1px solid var(--border);
    color: var(--muted-foreground);
    cursor: pointer;
    transition: all 0.15s ease;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.05);
    /* Prevent text selection on touch */
    -webkit-user-select: none;
    user-select: none;
    /* Improve touch responsiveness */
    touch-action: manipulation;
  }

  .trigger-button:hover {
    background: var(--accent);
    color: var(--accent-foreground);
    border-color: var(--accent);
  }

  .trigger-button:active {
    transform: scale(0.9);
  }

  .menu-expanded {
    display: flex;
    flex-direction: row;
    align-items: center;
    padding: 2px;
    background: var(--popover);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow:
      0 10px 15px -3px rgba(0, 0, 0, 0.1),
      0 4px 6px -2px rgba(0, 0, 0, 0.05);
    min-width: max-content;
    max-width: 90vw;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .menu-expanded::-webkit-scrollbar {
    display: none;
  }

  .menu-section {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 2px;
  }

  .menu-item {
    display: flex;
    flex-direction: row;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border-radius: 6px;
    background: transparent;
    border: none;
    color: var(--foreground);
    cursor: pointer;
    transition: all 0.1s ease;
    /* Prevent text selection on touch */
    -webkit-user-select: none;
    user-select: none;
    /* Improve touch responsiveness */
    touch-action: manipulation;
  }

  .menu-item span {
    display: none;
  }

  .menu-item:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .menu-item:active {
    background: var(--accent);
    transform: scale(0.95);
  }

  .menu-divider {
    width: 1px;
    height: 20px;
    background: var(--border);
    margin: 0 4px;
    opacity: 0.5;
    flex-shrink: 0;
  }

  /* Mobile-specific adjustments */
  @media (max-width: 767px) {
    .trigger-button {
      width: 28px;
      height: 28px;
    }

    .menu-item {
      width: 36px;
      height: 36px;
    }
  }
</style>
