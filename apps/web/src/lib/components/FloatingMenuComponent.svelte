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
        <span class="menu-section-label">Headings</span>
        <div class="menu-row">
          <button
            type="button"
            onclick={(e) => handleMenuItemClick(e, () => handleHeading(1))}
            class="menu-item"
            title="Heading 1"
          >
            <Heading1 class="size-4" />
            <span>H1</span>
          </button>
          <button
            type="button"
            onclick={(e) => handleMenuItemClick(e, () => handleHeading(2))}
            class="menu-item"
            title="Heading 2"
          >
            <Heading2 class="size-4" />
            <span>H2</span>
          </button>
          <button
            type="button"
            onclick={(e) => handleMenuItemClick(e, () => handleHeading(3))}
            class="menu-item"
            title="Heading 3"
          >
            <Heading3 class="size-4" />
            <span>H3</span>
          </button>
        </div>
      </div>

      <div class="menu-divider"></div>

      <div class="menu-section">
        <span class="menu-section-label">Lists</span>
        <div class="menu-row">
          <button
            type="button"
            onclick={(e) => handleMenuItemClick(e, handleBulletList)}
            class="menu-item"
            title="Bullet List"
          >
            <List class="size-4" />
            <span>Bullet</span>
          </button>
          <button
            type="button"
            onclick={(e) => handleMenuItemClick(e, handleOrderedList)}
            class="menu-item"
            title="Numbered List"
          >
            <ListOrdered class="size-4" />
            <span>Numbered</span>
          </button>
          <button
            type="button"
            onclick={(e) => handleMenuItemClick(e, handleTaskList)}
            class="menu-item"
            title="Task List"
          >
            <CheckSquare class="size-4" />
            <span>Tasks</span>
          </button>
        </div>
      </div>

      <div class="menu-divider"></div>

      <div class="menu-section">
        <span class="menu-section-label">Blocks</span>
        <div class="menu-row">
          <button
            type="button"
            onclick={(e) => handleMenuItemClick(e, handleBlockquote)}
            class="menu-item"
            title="Quote"
          >
            <Quote class="size-4" />
            <span>Quote</span>
          </button>
          <button
            type="button"
            onclick={(e) => handleMenuItemClick(e, handleCodeBlock)}
            class="menu-item"
            title="Code Block"
          >
            <Braces class="size-4" />
            <span>Code</span>
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
      </div>

      {#if onInsertImage}
        <div class="menu-divider"></div>

        <div class="menu-section">
          <span class="menu-section-label">Media</span>
          <div class="menu-row">
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
    z-index: 9999;
    display: none;
  }

  .trigger-button {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: 6px;
    background: var(--card);
    border: 1px solid var(--border);
    color: var(--muted-foreground);
    cursor: pointer;
    transition: all 0.15s ease;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.1);
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
    transform: scale(0.95);
  }

  .menu-expanded {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    background: var(--popover);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow:
      0 4px 16px rgba(0, 0, 0, 0.15),
      0 0 0 1px rgba(0, 0, 0, 0.05);
    min-width: 200px;
  }

  .menu-section {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .menu-section-label {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted-foreground);
    padding-left: 4px;
  }

  .menu-row {
    display: flex;
    gap: 4px;
  }

  .menu-item {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    padding: 8px 12px;
    border-radius: 6px;
    background: transparent;
    border: 1px solid transparent;
    color: var(--foreground);
    cursor: pointer;
    transition: all 0.15s ease;
    flex: 1;
    min-width: 60px;
    /* Prevent text selection on touch */
    -webkit-user-select: none;
    user-select: none;
    /* Improve touch responsiveness */
    touch-action: manipulation;
  }

  .menu-item span {
    font-size: 11px;
    font-weight: 500;
  }

  .menu-item:hover {
    background: var(--accent);
    color: var(--accent-foreground);
    border-color: var(--border);
  }

  .menu-item:active {
    transform: scale(0.97);
    background: var(--accent);
  }

  .menu-divider {
    height: 1px;
    background: var(--border);
    margin: 4px 0;
  }

  /* Mobile-specific adjustments */
  @media (max-width: 767px) {
    .trigger-button {
      width: 32px;
      height: 32px;
    }

    .menu-expanded {
      min-width: 180px;
      padding: 10px;
    }

    .menu-item {
      padding: 10px 8px;
      min-width: 54px;
    }
  }
</style>
