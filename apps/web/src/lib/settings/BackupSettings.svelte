<script lang="ts">
  /**
   * BackupSettings - Export workspace to zip file
   *
   * On Tauri: Uses native file save dialog
   * On Web: Downloads via browser
   */
  import { Button } from "$lib/components/ui/button";
  import { Download, Loader2, Check, AlertCircle } from "@lucide/svelte";
  import { getBackend } from "../backend";
  import { createApi } from "../backend/api";

  interface Props {
    workspacePath?: string | null;
  }

  let { workspacePath = null }: Props = $props();

  let isExporting: boolean = $state(false);
  let exportError: string | null = $state(null);
  let exportSuccess: { files: number; path?: string } | null = $state(null);

  async function handleExport() {
    if (!workspacePath) return;

    isExporting = true;
    exportError = null;
    exportSuccess = null;

    try {
      const backend = await getBackend();

      // Check if we're on Tauri (has getInvoke method)
      if ("getInvoke" in backend) {
        // Tauri: Use native save dialog
        const invoke = (backend as any).getInvoke();
        const result = await invoke("export_to_zip", {
          workspacePath: workspacePath.substring(
            0,
            workspacePath.lastIndexOf("/"),
          ),
        });

        if (result.cancelled) {
          // User cancelled - do nothing
          return;
        }

        if (result.success) {
          exportSuccess = {
            files: result.files_exported,
            path: result.output_path,
          };
        } else {
          exportError = result.error || "Export failed";
        }
      } else {
        // Web: Use workspace tree + file reads to build zip
        const api = createApi(backend);

        // Get workspace directory from index path
        const workspaceDir = workspacePath.substring(
          0,
          workspacePath.lastIndexOf("/"),
        );

        // Get workspace tree to find all files
        const tree = await api.getFilesystemTree(workspaceDir, false);

        // Create zip
        const JSZip = (await import("jszip")).default;
        const zip = new JSZip();

        // Helper to recursively collect files from tree
        async function addFilesToZip(
          node: { path: string; children?: { path: string; children?: unknown[] }[] },
          basePath: string,
        ): Promise<number> {
          let count = 0;

          // Skip hidden files/directories
          const name = node.path.split("/").pop() || "";
          if (name.startsWith(".")) {
            return 0;
          }

          if (node.children) {
            // It's a directory - recurse into children
            for (const child of node.children) {
              count += await addFilesToZip(child as typeof node, basePath);
            }
          } else {
            // It's a file - add to zip
            const relativePath =
              node.path.startsWith(basePath + "/")
                ? node.path.substring(basePath.length + 1)
                : node.path;

            try {
              // Determine if it's a text or binary file
              const ext = node.path.split(".").pop()?.toLowerCase() || "";
              const textExts = ["md", "txt", "json", "yaml", "yml", "toml"];

              if (textExts.includes(ext)) {
                const content = await api.readFile(node.path);
                zip.file(relativePath, content);
                count++;
              } else {
                // Binary file - use backend.execute or cast to any for readBinary
                const backendAny = backend as unknown as {
                  readBinary: (path: string) => Promise<Uint8Array>;
                };
                if (typeof backendAny.readBinary === "function") {
                  const data = await backendAny.readBinary(node.path);
                  zip.file(relativePath, data, { binary: true });
                  count++;
                }
              }
            } catch (e) {
              console.warn(`[Export] Failed to read ${node.path}:`, e);
            }
          }

          return count;
        }

        const fileCount = await addFilesToZip(tree, workspaceDir);

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

        exportSuccess = { files: fileCount };
      }
    } catch (e) {
      console.error("Export failed:", e);
      exportError = e instanceof Error ? e.message : String(e);
    } finally {
      isExporting = false;
    }
  }
</script>

<div class="space-y-3">
  <h3 class="font-medium flex items-center gap-2">
    <Download class="size-4" />
    Export
  </h3>

  <div class="px-1 space-y-2">
    <p class="text-xs text-muted-foreground">
      Export your workspace to a zip file for backup or transfer.
    </p>

    <Button
      variant="outline"
      size="sm"
      onclick={handleExport}
      disabled={isExporting || !workspacePath}
    >
      {#if isExporting}
        <Loader2 class="mr-2 size-4 animate-spin" />
        Exporting...
      {:else}
        Export to Zip...
      {/if}
    </Button>

    {#if exportError}
      <div
        class="flex items-center gap-2 text-sm text-destructive bg-destructive/10 p-2 rounded"
      >
        <AlertCircle class="size-4" />
        <span>{exportError}</span>
      </div>
    {/if}

    {#if exportSuccess}
      <div
        class="flex items-center gap-2 text-sm text-green-600 bg-green-50 dark:bg-green-950/20 p-2 rounded"
      >
        <Check class="size-4" />
        <span>
          Exported {exportSuccess.files} files
          {#if exportSuccess.path}
            to {exportSuccess.path.split("/").pop()}
          {/if}
        </span>
      </div>
    {/if}
  </div>
</div>
