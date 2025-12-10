// Types matching the Rust backend structures

export interface Config {
  default_workspace: string;
  daily_entry_folder?: string;
  editor?: string;
  default_template?: string;
}

export interface TreeNode {
  name: string;
  description?: string;
  path: string;
  children: TreeNode[];
}

export interface EntryData {
  path: string;
  title?: string;
  frontmatter: Record<string, unknown>;
  content: string;
}

export interface SaveEntryRequest {
  path: string;
  content: string;
}

export interface SearchMatch {
  line_number: number;
  line_content: string;
  match_start: number;
  match_end: number;
}

export interface FileSearchResult {
  path: string;
  title?: string;
  matches: SearchMatch[];
}

export interface SearchResults {
  files: FileSearchResult[];
  files_searched: number;
}

export interface SerializableError {
  kind: string;
  message: string;
  path?: string;
}

// Frontend-specific types

export interface EditorState {
  path: string | null;
  content: string;
  isDirty: boolean;
  title?: string;
}

export interface SidebarItem {
  node: TreeNode;
  depth: number;
  isExpanded: boolean;
}

export type ViewMode = "edit" | "preview" | "split";
