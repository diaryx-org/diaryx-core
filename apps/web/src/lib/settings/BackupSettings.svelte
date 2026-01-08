<script lang="ts">
  /**
   * BackupSettings - Backup and export settings section
   *
   * Extracted from SettingsDialog for modularity.
   */
  import { Button } from "$lib/components/ui/button";
  import { Save, Download, Loader2, Check, AlertCircle } from "@lucide/svelte";
  import { getBackend } from "../backend";
  import type { BackupStatus } from "../backend/interface";

  interface Props {
    workspacePath?: string | null;
  }

  let { workspacePath = null }: Props = $props();

  // Backup state
  let backupTargets: string[] = $state([]);
  let backupStatus: BackupStatus[] | null = $state(null);
  let isBackingUp: boolean = $state(false);
  let isExporting: boolean = $state(false);
  let backupError: string | null = $state(null);

  // Load backup targets on mount
  $effect(() => {
    loadBackupTargets();
  });

  async function loadBackupTargets() {
    try {
      const backend = await getBackend();
      if ("getInvoke" in backend) {
        const invoke = (backend as any).getInvoke();
        backupTargets = await invoke("list_backup_targets", {});
      } else {
        backupTargets = ["IndexedDB (Local)"];
      }
    } catch (e) {
      backupTargets = [];
    }
  }

  async function performBackup() {
    isBackingUp = true;
    backupError = null;
    backupStatus = null;
    try {
      const backend = await getBackend();
      if ("getInvoke" in backend) {
        const invoke = (backend as any).getInvoke();
        backupStatus = await invoke("run_backups", {});
      }
    } catch (e) {
      backupError = e instanceof Error ? e.message : String(e);
    } finally {
      isBackingUp = false;
    }
  }

  async function handleQuickExport() {
    if (!workspacePath) return;

    isExporting = true;
    try {
      const backend = await getBackend();

      // Export all markdown files
      const files = await backend.exportToMemory(workspacePath, "*");

      // Export all binary attachments
      const binaryFiles = await backend.exportBinaryAttachments(
        workspacePath,
        "*",
      );

      // Create zip
      const JSZip = (await import("jszip")).default;
      const zip = new JSZip();

      // Add text files
      for (const file of files) {
        zip.file(file.path, file.content);
      }

      // Add binary files (attachments)
      for (const file of binaryFiles) {
        zip.file(file.path, new Uint8Array(file.data), { binary: true });
      }

      const blob = await zip.generateAsync({ type: "blob" });
      const url = URL.createObjectURL(blob);

      const a = document.createElement("a");
      a.href = url;
      const baseName =
        workspacePath.split("/").pop()?.replace(".md", "") || "workspace";
      const timestamp = new Date().toISOString().slice(0, 10);
      a.download = `${baseName}-${timestamp}.zip`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
    } catch (e) {
      console.error("Export failed:", e);
      backupError = e instanceof Error ? e.message : String(e);
    } finally {
      isExporting = false;
    }
  }
</script>

<div class="space-y-3">
  <h3 class="font-medium flex items-center gap-2">
    <Save class="size-4" />
    Backup
  </h3>

  {#if backupTargets.length > 0}
    <div class="text-sm text-muted-foreground px-1">
      Configured targets: {backupTargets.join(", ")}
    </div>
  {/if}

  <div class="flex items-center gap-2 px-1">
    <Button
      variant="outline"
      size="sm"
      onclick={performBackup}
      disabled={isBackingUp}
    >
      {#if isBackingUp}
        <Loader2 class="mr-2 size-4 animate-spin" />
      {/if}
      Run Backup
    </Button>
    <Button
      variant="outline"
      size="sm"
      onclick={handleQuickExport}
      disabled={isExporting || !workspacePath}
    >
      {#if isExporting}
        <Loader2 class="mr-2 size-4 animate-spin" />
      {:else}
        <Download class="mr-2 size-4" />
      {/if}
      Download Zip
    </Button>
  </div>

  {#if backupError}
    <div
      class="flex items-center gap-2 text-sm text-destructive bg-destructive/10 p-2 rounded"
    >
      <AlertCircle class="size-4" />
      <span>{backupError}</span>
    </div>
  {/if}

  {#if backupStatus}
    <div class="space-y-1 px-1">
      {#each backupStatus as status}
        <div class="flex items-center gap-2 text-sm">
          {#if status.success}
            <Check class="size-4 text-green-500" />
          {:else}
            <AlertCircle class="size-4 text-destructive" />
          {/if}
          <span
            >{status.target_name}: {status.success
              ? `${status.files_processed} files`
              : status.error || "Failed"}</span
          >
        </div>
      {/each}
    </div>
  {/if}
</div>
