<script lang="ts">
  /**
   * LinkSettings - Link format configuration for the workspace
   *
   * Allows users to configure how links in part_of and contents properties
   * are formatted, and to convert existing links to a different format.
   */
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import * as Select from "$lib/components/ui/select";
  import { Link, RefreshCw, AlertCircle, Check } from "@lucide/svelte";
  import { getLinkFormatStore, type LinkFormatValue } from "../stores/linkFormatStore.svelte";

  interface Props {
    workspaceRootIndex?: string | null;
  }

  let { workspaceRootIndex = null }: Props = $props();

  const linkFormatStore = getLinkFormatStore();

  // Conversion state
  let isConverting = $state(false);
  let conversionResult = $state<{
    files_modified: number;
    links_converted: number;
  } | null>(null);
  let showResult = $state(false);

  // Load the link format when workspace root index changes
  $effect(() => {
    if (workspaceRootIndex) {
      linkFormatStore.load(workspaceRootIndex);
    }
  });

  async function handleFormatChange(value: string | undefined) {
    if (value && value !== linkFormatStore.format) {
      await linkFormatStore.setFormat(value as LinkFormatValue);
    }
  }

  async function handleConvertLinks() {
    isConverting = true;
    conversionResult = null;
    showResult = false;

    try {
      const result = await linkFormatStore.convertLinks(linkFormatStore.format, {
        dryRun: false,
      });

      if (result) {
        conversionResult = {
          files_modified: result.files_modified,
          links_converted: result.links_converted,
        };
        showResult = true;

        // Hide result after 5 seconds
        setTimeout(() => {
          showResult = false;
        }, 5000);
      }
    } finally {
      isConverting = false;
    }
  }

  // Get selected option for display
  $effect(() => {
    // Update selected when format changes
  });
</script>

<div class="space-y-4">
  <div class="space-y-3">
    <h3 class="font-medium flex items-center gap-2">
      <Link class="size-4" />
      Link Format
    </h3>

    <p class="text-xs text-muted-foreground px-1">
      Choose how links in <code class="bg-muted px-1 rounded">part_of</code> and
      <code class="bg-muted px-1 rounded">contents</code> properties are formatted.
    </p>

    <div class="space-y-3 px-1">
      <div class="space-y-2">
        <Label for="link-format" class="text-xs text-muted-foreground">
          Link Format
        </Label>
        <Select.Root
          type="single"
          value={linkFormatStore.format}
          onValueChange={handleFormatChange}
          disabled={linkFormatStore.loading || !workspaceRootIndex}
        >
          <Select.Trigger id="link-format" class="w-full">
            {linkFormatStore.getFormatLabel(linkFormatStore.format)}
          </Select.Trigger>
          <Select.Content>
            {#each linkFormatStore.options as option}
              <Select.Item value={option.value}>
                <div class="flex flex-col gap-0.5">
                  <span>{option.label}</span>
                  <span class="text-xs text-muted-foreground font-mono">
                    {option.example}
                  </span>
                </div>
              </Select.Item>
            {/each}
          </Select.Content>
        </Select.Root>
        <p class="text-xs text-muted-foreground">
          {linkFormatStore.getFormatDescription(linkFormatStore.format)}
        </p>
      </div>

      {#if linkFormatStore.error}
        <div class="flex items-center gap-2 text-xs text-destructive">
          <AlertCircle class="size-3" />
          <span>{linkFormatStore.error}</span>
        </div>
      {/if}

      <div class="pt-2 border-t space-y-2">
        <p class="text-xs text-muted-foreground">
          Convert all existing links in your workspace to the selected format.
          This will update <code class="bg-muted px-1 rounded">part_of</code> and
          <code class="bg-muted px-1 rounded">contents</code> in all files.
        </p>

        <Button
          variant="outline"
          size="sm"
          class="w-full"
          onclick={handleConvertLinks}
          disabled={isConverting || linkFormatStore.loading || !workspaceRootIndex}
        >
          {#if isConverting}
            <RefreshCw class="size-4 mr-2 animate-spin" />
            Converting...
          {:else}
            <RefreshCw class="size-4 mr-2" />
            Convert All Links
          {/if}
        </Button>

        {#if showResult && conversionResult}
          <div class="flex items-center gap-2 text-xs text-green-600 dark:text-green-400">
            <Check class="size-3" />
            <span>
              Converted {conversionResult.links_converted} link{conversionResult.links_converted !== 1 ? 's' : ''}
              in {conversionResult.files_modified} file{conversionResult.files_modified !== 1 ? 's' : ''}.
            </span>
          </div>
        {/if}
      </div>
    </div>
  </div>
</div>
