<script lang="ts">
  /**
   * BottomSheet - A mobile-friendly bottom sheet component
   *
   * Features:
   * - Slides up from the bottom of the screen
   * - Has a drag handle for intuitive interaction
   * - Closes on backdrop tap or escape key
   * - Respects safe areas on notched devices
   */

  import type { Snippet } from 'svelte';
  import { X } from '@lucide/svelte';

  interface Props {
    open: boolean;
    onClose: () => void;
    title?: string;
    children: Snippet;
  }

  let { open, onClose, title, children }: Props = $props();

  // Handle escape key
  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && open) {
      onClose();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open}
  <!-- Backdrop -->
  <button
    type="button"
    class="fixed inset-0 bg-black/50 z-50 animate-fade-in"
    onclick={onClose}
    aria-label="Close bottom sheet"
  ></button>

  <!-- Sheet -->
  <div
    class="fixed bottom-0 left-0 right-0 z-50 bg-background
           rounded-t-2xl max-h-[80vh] overflow-hidden
           animate-slide-up shadow-2xl"
    role="dialog"
    aria-modal="true"
    aria-labelledby={title ? 'bottom-sheet-title' : undefined}
  >
    <!-- Drag Handle -->
    <div class="flex justify-center pt-3 pb-2">
      <div class="w-10 h-1 rounded-full bg-muted-foreground/30"></div>
    </div>

    {#if title}
      <div class="flex items-center justify-between px-4 py-2 border-b border-border">
        <h2 id="bottom-sheet-title" class="text-lg font-semibold">{title}</h2>
        <button
          type="button"
          class="p-2 -mr-2 rounded-full hover:bg-muted transition-colors"
          onclick={onClose}
          aria-label="Close"
        >
          <X class="size-5" />
        </button>
      </div>
    {/if}

    <!-- Content area with safe area padding -->
    <div class="overflow-y-auto max-h-[calc(80vh-4rem)] pb-[env(safe-area-inset-bottom)]">
      {@render children()}
    </div>
  </div>
{/if}

<style>
  @keyframes fade-in {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

  @keyframes slide-up {
    from {
      transform: translateY(100%);
    }
    to {
      transform: translateY(0);
    }
  }

  .animate-fade-in {
    animation: fade-in 0.2s ease-out;
  }

  .animate-slide-up {
    animation: slide-up 0.3s ease-out;
  }
</style>
