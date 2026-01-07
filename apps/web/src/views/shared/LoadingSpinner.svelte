<script lang="ts">
  /**
   * LoadingSpinner - A simple animated loading spinner
   * 
   * Pure presentational component for loading states.
   */

  interface Props {
    size?: 'sm' | 'md' | 'lg';
  }

  let { size = 'md' }: Props = $props();

  const sizeClasses = {
    sm: 'h-4 w-4',
    md: 'h-8 w-8',
    lg: 'h-12 w-12',
  };
</script>

<div class="flex items-center justify-center h-full">
  <div
    class="animate-spin rounded-full border-b-2 border-primary {sizeClasses[size]}"
  ></div>
</div>
