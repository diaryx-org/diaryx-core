/**
 * Link format store for managing workspace link format preferences.
 * Uses Svelte 5 runes for reactive state management.
 * Persists preferences to the workspace root index frontmatter.
 */

import { getBackend } from "../backend";

/**
 * Link format type matching the Rust enum.
 */
export type LinkFormat =
  | "markdown_root"
  | "markdown_relative"
  | "plain_relative"
  | "plain_canonical";

/**
 * Link format options with labels and descriptions.
 */
export const LINK_FORMAT_OPTIONS = [
  {
    value: "markdown_root" as const,
    label: "Markdown Root",
    description: "[Title](/path/to/file.md) - Clickable in editors, absolute from workspace root",
    example: "[My Note](/Notes/my-note.md)",
  },
  {
    value: "markdown_relative" as const,
    label: "Markdown Relative",
    description: "[Title](../relative/path.md) - Clickable in editors, relative to current file",
    example: "[My Note](../Notes/my-note.md)",
  },
  {
    value: "plain_relative" as const,
    label: "Plain Relative",
    description: "../relative/path.md - Simple path relative to current file",
    example: "../Notes/my-note.md",
  },
  {
    value: "plain_canonical" as const,
    label: "Plain Canonical",
    description: "path/to/file.md - Simple path from workspace root",
    example: "Notes/my-note.md",
  },
] as const;

export type LinkFormatValue = (typeof LINK_FORMAT_OPTIONS)[number]["value"];

/**
 * Creates reactive link format state with backend persistence.
 */
export function createLinkFormatStore() {
  let format = $state<LinkFormatValue>("markdown_root");
  let loading = $state(false);
  let error = $state<string | null>(null);
  let rootIndexPath = $state<string | null>(null);

  /**
   * Load the current link format from the workspace root index.
   */
  async function load(workspaceRootIndex: string) {
    loading = true;
    error = null;
    rootIndexPath = workspaceRootIndex;

    try {
      const backend = await getBackend();
      const response = await backend.execute({
        type: "GetLinkFormat",
        params: { root_index_path: workspaceRootIndex },
      });

      if (response.type === "LinkFormat") {
        format = response.data as LinkFormatValue;
      }
    } catch (e) {
      console.error("[LinkFormatStore] Failed to load link format:", e);
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  /**
   * Set the link format and persist to workspace root index.
   */
  async function setFormat(newFormat: LinkFormatValue) {
    if (!rootIndexPath) {
      error = "No workspace root index loaded";
      return;
    }

    loading = true;
    error = null;

    try {
      const backend = await getBackend();
      await backend.execute({
        type: "SetLinkFormat",
        params: {
          root_index_path: rootIndexPath,
          format: newFormat,
        },
      });

      format = newFormat;
    } catch (e) {
      console.error("[LinkFormatStore] Failed to set link format:", e);
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  /**
   * Convert all links in the workspace to the target format.
   */
  async function convertLinks(
    targetFormat: LinkFormatValue,
    options: { path?: string; dryRun?: boolean } = {}
  ) {
    if (!rootIndexPath) {
      error = "No workspace root index loaded";
      return null;
    }

    loading = true;
    error = null;

    try {
      const backend = await getBackend();
      const response = await backend.execute({
        type: "ConvertLinks",
        params: {
          root_index_path: rootIndexPath,
          format: targetFormat,
          path: options.path ?? null,
          dry_run: options.dryRun ?? false,
        },
      });

      if (response.type === "ConvertLinksResult") {
        // Update format after successful conversion (if not dry run)
        if (!options.dryRun) {
          format = targetFormat;
        }
        return response.data as {
          files_modified: number;
          links_converted: number;
          modified_files: string[];
          dry_run: boolean;
        };
      }
      return null;
    } catch (e) {
      console.error("[LinkFormatStore] Failed to convert links:", e);
      error = e instanceof Error ? e.message : String(e);
      return null;
    } finally {
      loading = false;
    }
  }

  /**
   * Get the label for a format value.
   */
  function getFormatLabel(value: LinkFormatValue): string {
    const option = LINK_FORMAT_OPTIONS.find((o) => o.value === value);
    return option?.label ?? value;
  }

  /**
   * Get the description for a format value.
   */
  function getFormatDescription(value: LinkFormatValue): string {
    const option = LINK_FORMAT_OPTIONS.find((o) => o.value === value);
    return option?.description ?? "";
  }

  return {
    get format() {
      return format;
    },
    get loading() {
      return loading;
    },
    get error() {
      return error;
    },
    get rootIndexPath() {
      return rootIndexPath;
    },
    get options() {
      return LINK_FORMAT_OPTIONS;
    },
    load,
    setFormat,
    convertLinks,
    getFormatLabel,
    getFormatDescription,
  };
}

/**
 * Singleton instance for shared link format state across components.
 */
let sharedLinkFormatStore: ReturnType<typeof createLinkFormatStore> | null = null;

export function getLinkFormatStore() {
  if (typeof window === "undefined") {
    // SSR fallback
    return {
      get format() {
        return "markdown_root" as LinkFormatValue;
      },
      get loading() {
        return false;
      },
      get error() {
        return null as string | null;
      },
      get rootIndexPath() {
        return null as string | null;
      },
      get options() {
        return LINK_FORMAT_OPTIONS;
      },
      load: async () => {},
      setFormat: async () => {},
      convertLinks: async () => null,
      getFormatLabel: (value: LinkFormatValue) => value,
      getFormatDescription: () => "",
    };
  }

  if (!sharedLinkFormatStore) {
    sharedLinkFormatStore = createLinkFormatStore();
  }
  return sharedLinkFormatStore;
}
