<script lang="ts">
  /**
   * ImportSettings - Import from zip settings section
   *
   * Extracted from SettingsDialog for modularity.
   */
  import { Button } from "$lib/components/ui/button";
  import { Upload, Loader2, Check, AlertCircle } from "@lucide/svelte";
  import { getBackend } from "../backend";

  interface Props {
    workspacePath?: string | null;
  }

  let { workspacePath = null }: Props = $props();

  // Import state
  let isImporting: boolean = $state(false);
  let importResult: {
    success: boolean;
    files_imported: number;
    error?: string;
  } | null = $state(null);

  // Reference to hidden file input
  let fileInputRef: HTMLInputElement | null = $state(null);

  function triggerFileInput() {
    fileInputRef?.click();
  }

  async function handleFileSelected(event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;

    isImporting = true;
    importResult = null;

    try {
      const backend = await getBackend();
      const workspaceDir = workspacePath
        ? workspacePath.substring(0, workspacePath.lastIndexOf("/"))
        : undefined;

      const result = await backend.importFromZip(
        file,
        workspaceDir,
        (uploaded, total) => {
          if (uploaded % (10 * 1024 * 1024) < 1024 * 1024) {
            console.log(
              `[Import] Progress: ${(uploaded / 1024 / 1024).toFixed(1)} / ${(total / 1024 / 1024).toFixed(1)} MB`,
            );
          }
        },
      );

      importResult = result;

      if (result.success) {
        window.dispatchEvent(
          new CustomEvent("import:complete", { detail: result }),
        );
      }
    } catch (e) {
      console.error("Import failed:", e);
      importResult = {
        success: false,
        files_imported: 0,
        error: e instanceof Error ? e.message : String(e),
      };
    } finally {
      isImporting = false;
      if (input) input.value = "";
    }
  }
</script>

<div class="space-y-3">
  <h3 class="font-medium flex items-center gap-2">
    <Upload class="size-4" />
    Import
  </h3>
  <div class="px-1 space-y-2">
    <p class="text-xs text-muted-foreground">
      Import entries from a zip backup.
    </p>
    <input
      type="file"
      accept=".zip"
      class="hidden"
      bind:this={fileInputRef}
      onchange={handleFileSelected}
    />

    <Button
      variant="outline"
      size="sm"
      onclick={triggerFileInput}
      disabled={isImporting}
    >
      {#if isImporting}
        <Loader2 class="size-4 mr-2 animate-spin" />
        Importing...
      {:else}
        Select Zip File...
      {/if}
    </Button>

    {#if importResult}
      {#if importResult.success}
        <div
          class="flex items-center gap-2 text-sm text-green-600 bg-green-50 dark:bg-green-950/20 p-2 rounded"
        >
          <Check class="size-4" />
          <span
            >Imported {importResult.files_imported} files. Refresh to see
            changes.</span
          >
        </div>
      {:else}
        <div
          class="flex items-center gap-2 text-sm text-destructive bg-destructive/10 p-2 rounded"
        >
          <AlertCircle class="size-4" />
          <span>{importResult.error || "Import failed"}</span>
        </div>
      {/if}
    {/if}
  </div>
</div>
