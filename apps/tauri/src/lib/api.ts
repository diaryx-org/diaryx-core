// Tauri API wrapper functions
import { invoke } from "@tauri-apps/api/core";
import type {
  Config,
  TreeNode,
  EntryData,
  SearchResults,
  SerializableError,
} from "./types";

/**
 * Handle errors from Tauri commands
 */
function handleError(error: unknown): never {
  if (typeof error === "object" && error !== null && "message" in error) {
    throw new Error((error as SerializableError).message);
  }
  throw new Error(String(error));
}

/**
 * Get the current configuration
 */
export async function getConfig(): Promise<Config> {
  try {
    return await invoke<Config>("get_config");
  } catch (e) {
    handleError(e);
  }
}

/**
 * Get the workspace tree structure
 */
export async function getWorkspaceTree(
  workspacePath?: string,
  depth?: number
): Promise<TreeNode> {
  try {
    return await invoke<TreeNode>("get_workspace_tree", {
      workspacePath,
      depth,
    });
  } catch (e) {
    handleError(e);
  }
}

/**
 * Get an entry's content and metadata
 */
export async function getEntry(path: string): Promise<EntryData> {
  try {
    return await invoke<EntryData>("get_entry", { path });
  } catch (e) {
    handleError(e);
  }
}

/**
 * Save an entry's content
 */
export async function saveEntry(path: string, content: string): Promise<void> {
  try {
    await invoke("save_entry", { request: { path, content } });
  } catch (e) {
    handleError(e);
  }
}

/**
 * Search the workspace
 */
export async function searchWorkspace(
  pattern: string,
  options?: {
    workspacePath?: string;
    searchFrontmatter?: boolean;
    property?: string;
    caseSensitive?: boolean;
  }
): Promise<SearchResults> {
  try {
    return await invoke<SearchResults>("search_workspace", {
      pattern,
      workspacePath: options?.workspacePath,
      searchFrontmatter: options?.searchFrontmatter,
      property: options?.property,
      caseSensitive: options?.caseSensitive,
    });
  } catch (e) {
    handleError(e);
  }
}

/**
 * Create a new entry
 */
export async function createEntry(
  path: string,
  title?: string,
  partOf?: string
): Promise<string> {
  try {
    return await invoke<string>("create_entry", { path, title, partOf });
  } catch (e) {
    handleError(e);
  }
}

/**
 * Get frontmatter for an entry
 */
export async function getFrontmatter(
  path: string
): Promise<Record<string, unknown>> {
  try {
    return await invoke<Record<string, unknown>>("get_frontmatter", { path });
  } catch (e) {
    handleError(e);
  }
}

/**
 * Set a frontmatter property
 */
export async function setFrontmatterProperty(
  path: string,
  key: string,
  value: unknown
): Promise<void> {
  try {
    await invoke("set_frontmatter_property", { path, key, value });
  } catch (e) {
    handleError(e);
  }
}
