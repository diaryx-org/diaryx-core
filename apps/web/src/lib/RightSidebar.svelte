<script lang="ts">
  import type { EntryData } from "./backend";
  import type { RustCrdtApi } from "$lib/crdt/rustCrdtApi";
  import type { CrdtHistoryEntry, FileDiff } from "$lib/backend/generated";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import * as Alert from "$lib/components/ui/alert";
  import {
    Calendar,
    Clock,
    Tag,
    FileText,
    Link,
    Hash,
    List,
    ToggleLeft,
    Type,
    PanelRightClose,
    Plus,
    X,
    Check,
    AlertCircle,
    Paperclip,
    Trash2,
    File,
    FileImage,
    FileArchive,
    FileSpreadsheet,
    FileCode,
    History,
    RefreshCw,
    RotateCcw,
  } from "@lucide/svelte";
  import type { Component } from "svelte";
  import VersionDiff from "./history/VersionDiff.svelte";
  import ShareTab from "./share/ShareTab.svelte";
  import * as Tooltip from "$lib/components/ui/tooltip";
  import { getMobileState } from "$lib/hooks/useMobile.svelte";

  // Platform detection for keyboard shortcut display
  const isMac =
    typeof navigator !== "undefined" &&
    navigator.platform.toUpperCase().indexOf("MAC") >= 0;
  const modKey = isMac ? "⌘" : "Ctrl+";

  // Mobile state for hiding tooltips
  const mobileState = getMobileState();

  interface Props {
    entry: EntryData | null;
    collapsed: boolean;
    onToggleCollapse: () => void;
    onPropertyChange?: (key: string, value: unknown) => void;
    onPropertyRemove?: (key: string) => void;
    onPropertyAdd?: (key: string, value: unknown) => void;
    titleError?: string | null;
    onTitleErrorClear?: () => void;
    onDeleteAttachment?: (attachmentPath: string) => void;
    attachmentError?: string | null;
    onAttachmentErrorClear?: () => void;
    // History props
    rustApi?: RustCrdtApi | null;
    onHistoryRestore?: () => void;
    // Share props
    onBeforeHost?: () => Promise<void>;
    onOpenEntry?: (path: string) => Promise<void>;
  }

  let {
    entry,
    collapsed,
    onToggleCollapse,
    onPropertyChange,
    onPropertyRemove,
    onPropertyAdd,
    titleError = null,
    onTitleErrorClear,
    onDeleteAttachment,
    attachmentError = null,
    onAttachmentErrorClear,
    rustApi = null,
    onHistoryRestore,
    onBeforeHost,
    onOpenEntry,
  }: Props = $props();

  // Tab state: "properties" | "history" | "share"
  type TabType = "properties" | "history" | "share";
  let activeTab: TabType = $state("properties");

  // History state
  let history: CrdtHistoryEntry[] = $state([]);
  let historyLoading = $state(false);
  let historyError = $state<string | null>(null);
  let selectedEntry: CrdtHistoryEntry | null = $state(null);
  let diffs: FileDiff[] = $state([]);
  let loadingDiff = $state(false);

  // Load history for current file (combines workspace metadata + body content changes)
  async function loadHistory() {
    if (!rustApi || !entry) return;

    historyLoading = true;
    historyError = null;
    selectedEntry = null;
    diffs = [];

    try {
      // Use file-specific history that combines workspace and body doc changes
      history = await rustApi.getFileHistory(entry.path, 100);
    } catch (e) {
      historyError = e instanceof Error ? e.message : "Failed to load history";
      console.error("[RightSidebar] Error loading history:", e);
    } finally {
      historyLoading = false;
    }
  }

  // Select a history entry and load its diff
  async function selectHistoryEntry(historyEntry: CrdtHistoryEntry) {
    if (!rustApi) return;

    if (selectedEntry?.update_id === historyEntry.update_id) {
      // Deselect
      selectedEntry = null;
      diffs = [];
      return;
    }

    selectedEntry = historyEntry;
    loadingDiff = true;
    diffs = [];

    try {
      const idx = history.findIndex((h) => h.update_id === historyEntry.update_id);
      if (idx < history.length - 1) {
        const previousEntry = history[idx + 1];
        // Diff operates on workspace document for metadata changes
        diffs = await rustApi.getVersionDiff(previousEntry.update_id, historyEntry.update_id, "workspace");
      }
    } catch (e) {
      console.error("[RightSidebar] Error loading diff:", e);
    } finally {
      loadingDiff = false;
    }
  }

  // Restore to a specific version
  async function restoreVersion(historyEntry: CrdtHistoryEntry) {
    if (!rustApi || !entry) return;

    const confirmRestore = confirm(`Restore to version from ${formatTimestamp(historyEntry.timestamp)}?`);
    if (!confirmRestore) return;

    try {
      // Restore operates on workspace document for metadata
      await rustApi.restoreVersion(historyEntry.update_id, "workspace");
      onHistoryRestore?.();
      await loadHistory();
    } catch (e) {
      console.error("[RightSidebar] Error restoring version:", e);
      alert("Failed to restore version");
    }
  }

  function formatTimestamp(timestamp: bigint): string {
    const date = new Date(Number(timestamp));
    return date.toLocaleString();
  }

  function formatRelativeTime(timestamp: bigint): string {
    const now = Date.now();
    const diff = now - Number(timestamp);
    const minutes = Math.floor(diff / 60000);
    const hours = Math.floor(diff / 3600000);
    const days = Math.floor(diff / 86400000);

    if (minutes < 1) return "Just now";
    if (minutes < 60) return `${minutes}m ago`;
    if (hours < 24) return `${hours}h ago`;
    return `${days}d ago`;
  }

  function getOriginLabel(entry: CrdtHistoryEntry): string {
    // Show device name if available
    if (entry.device_name) {
      if (entry.origin === "local") {
        return `You (${entry.device_name})`;
      }
      return entry.device_name;
    }
    // Fallback to origin-based label
    switch (entry.origin) {
      case "local": return "You";
      case "remote": return "Remote";
      case "sync": return "Sync";
      default: return entry.origin;
    }
  }

  function getOriginClass(origin: string): string {
    switch (origin) {
      case "Local": return "bg-primary text-primary-foreground";
      case "Remote": return "bg-secondary text-secondary-foreground";
      case "Sync": return "bg-accent text-accent-foreground";
      default: return "bg-muted text-muted-foreground";
    }
  }

  // Load history when switching to history tab or when entry changes
  $effect(() => {
    if (activeTab === "history" && entry && rustApi) {
      loadHistory();
    }
  });

  // Reset history state when entry changes
  $effect(() => {
    if (entry) {
      history = [];
      selectedEntry = null;
      diffs = [];
    }
  });

  // Get attachments from frontmatter
  $effect(() => {
    if (attachmentError && entry) {
      // Auto-clear error after 5 seconds
      const timeout = setTimeout(() => onAttachmentErrorClear?.(), 5000);
      return () => clearTimeout(timeout);
    }
  });

  // Get attachments list from frontmatter
  function getAttachments(): string[] {
    if (!entry?.frontmatter?.attachments) return [];
    const attachments = entry.frontmatter.attachments;
    if (Array.isArray(attachments)) {
      return attachments.filter((a): a is string => typeof a === "string");
    }
    return [];
  }

  function getFilename(path: string): string {
    return path.split("/").pop() ?? path;
  }

  // Get file type icon based on extension
  function getFileIcon(filename: string): Component {
    const ext = filename.split('.').pop()?.toLowerCase() || '';
    const imageExts = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'ico'];
    const docExts = ['pdf', 'doc', 'docx', 'txt', 'md', 'rtf'];
    const spreadsheetExts = ['xls', 'xlsx', 'csv'];
    const archiveExts = ['zip', 'tar', 'gz', '7z', 'rar'];
    const codeExts = ['json', 'xml', 'html', 'css', 'js', 'ts'];

    if (imageExts.includes(ext)) return FileImage;
    if (docExts.includes(ext)) return FileText;
    if (spreadsheetExts.includes(ext)) return FileSpreadsheet;
    if (archiveExts.includes(ext)) return FileArchive;
    if (codeExts.includes(ext)) return FileCode;
    return File;
  }

  // State for adding new properties
  let showAddProperty = $state(false);
  let newPropertyKey = $state("");
  let newPropertyValue = $state("");

  // State for adding new array items
  let addingArrayItemKey = $state<string | null>(null);
  let newArrayItem = $state("");

  // Get an icon for a frontmatter key
  function getIcon(key: string) {
    const lowerKey = key.toLowerCase();
    if (lowerKey === "title") return Type;
    if (lowerKey === "created" || lowerKey === "date") return Calendar;
    if (lowerKey === "updated" || lowerKey === "modified") return Clock;
    if (lowerKey === "tags" || lowerKey === "categories") return Tag;
    if (lowerKey === "part_of" || lowerKey === "parent") return Link;
    if (lowerKey === "contents" || lowerKey === "children") return List;
    return Hash;
  }

  // Check if a value is an array
  function isArray(value: unknown): value is unknown[] {
    return Array.isArray(value);
  }

  // Check if a value looks like a date
  function isDateValue(key: string, value: unknown): boolean {
    if (typeof value !== "string") return false;
    const lowerKey = key.toLowerCase();
    const dateKeys = ["created", "updated", "date", "modified"];
    return dateKeys.includes(lowerKey) || /^\d{4}-\d{2}-\d{2}/.test(value);
  }

  // Get frontmatter entries sorted with common fields first
  function getSortedFrontmatter(
    frontmatter: Record<string, unknown>,
  ): [string, unknown][] {
    const priorityKeys = [
      "title",
      "created",
      "updated",
      "date",
      "tags",
      "part_of",
      "contents",
    ];
    const entries = Object.entries(frontmatter);

    return entries.sort(([a], [b]) => {
      const aIndex = priorityKeys.indexOf(a.toLowerCase());
      const bIndex = priorityKeys.indexOf(b.toLowerCase());

      if (aIndex !== -1 && bIndex !== -1) return aIndex - bIndex;
      if (aIndex !== -1) return -1;
      if (bIndex !== -1) return 1;
      return a.localeCompare(b);
    });
  }

  // Format a key for display (convert snake_case to Title Case)
  function formatKey(key: string): string {
    return key.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
  }

  // Handle string property change
  function handleStringChange(key: string, event: Event) {
    const target = event.target as HTMLInputElement;
    onPropertyChange?.(key, target.value);
  }

  // Handle boolean toggle
  function handleBooleanToggle(key: string, currentValue: boolean) {
    onPropertyChange?.(key, !currentValue);
  }

  // Handle array item removal
  function handleArrayItemRemove(key: string, index: number) {
    if (!entry) return;
    const currentArray = entry.frontmatter[key] as unknown[];
    const newArray = [...currentArray];
    newArray.splice(index, 1);
    onPropertyChange?.(key, newArray);
  }

  // Handle adding new array item
  function handleAddArrayItem(key: string) {
    if (!entry || !newArrayItem.trim()) return;
    const currentArray = (entry.frontmatter[key] as unknown[]) || [];
    const newArray = [...currentArray, newArrayItem.trim()];
    onPropertyChange?.(key, newArray);
    newArrayItem = "";
    addingArrayItemKey = null;
  }

  // Handle adding new property
  function handleAddProperty() {
    if (!newPropertyKey.trim()) return;

    // Try to parse as JSON for complex values, otherwise use as string
    let value: unknown = newPropertyValue;
    try {
      value = JSON.parse(newPropertyValue);
    } catch {
      // Keep as string
    }

    onPropertyAdd?.(newPropertyKey.trim(), value);
    newPropertyKey = "";
    newPropertyValue = "";
    showAddProperty = false;
  }

  // Handle key press in inputs
  function handleKeyPress(event: KeyboardEvent, callback: () => void) {
    if (event.key === "Enter") {
      event.preventDefault();
      callback();
    }
    if (event.key === "Escape") {
      showAddProperty = false;
      addingArrayItemKey = null;
      newArrayItem = "";
    }
  }

  // Format date for datetime-local input
  function formatDateForInput(value: string): string {
    try {
      const date = new Date(value);
      // Format as YYYY-MM-DDTHH:mm
      return date.toISOString().slice(0, 16);
    } catch {
      return value;
    }
  }

  // Parse datetime-local input back to ISO string
  function parseDateFromInput(value: string): string {
    try {
      const date = new Date(value);
      return date.toISOString();
    } catch {
      return value;
    }
  }
</script>

<!-- Mobile overlay backdrop -->
{#if !collapsed}
  <button
    type="button"
    class="fixed inset-0 bg-black/50 z-30 md:hidden"
    onclick={onToggleCollapse}
    aria-label="Close properties panel"
  ></button>
{/if}

<aside
  class="flex flex-col h-full border-l border-border bg-sidebar text-sidebar-foreground transition-all duration-300 ease-in-out
    {collapsed ? 'w-0 opacity-0 overflow-hidden md:w-0' : 'w-72'}
    fixed right-0 md:relative z-40 md:z-auto"
>
  <!-- Header with collapse button -->
  <div
    class="flex items-center justify-between px-4 py-3 border-b border-sidebar-border shrink-0"
  >
    <Tooltip.Root>
      <Tooltip.Trigger>
        <Button
          variant="ghost"
          size="icon"
          onclick={onToggleCollapse}
          class="size-8"
          aria-label="Collapse panel"
        >
          <PanelRightClose class="size-4" />
        </Button>
      </Tooltip.Trigger>
      {#if !mobileState.isMobile}
        <Tooltip.Content>Collapse panel ({modKey}])</Tooltip.Content>
      {/if}
    </Tooltip.Root>
    
    <!-- Tab Toggle -->
    <div class="flex items-center gap-1 bg-muted rounded-md p-0.5">
      <button
        type="button"
        class="px-2 py-1 text-xs font-medium rounded transition-colors {activeTab === 'properties' ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'}"
        onclick={() => activeTab = "properties"}
      >
        Props
      </button>
      <button
        type="button"
        class="px-2 py-1 text-xs font-medium rounded transition-colors {activeTab === 'history' ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'}"
        onclick={() => activeTab = "history"}
      >
        History
      </button>
      <button
        type="button"
        class="px-2 py-1 text-xs font-medium rounded transition-colors {activeTab === 'share' ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'}"
        onclick={() => activeTab = "share"}
      >
        Share
      </button>
    </div>
  </div>

  <!-- Content -->
  <div class="flex-1 overflow-y-auto">
    {#if activeTab === "properties"}
      <!-- Properties Tab -->
      {#if entry}
      {#if Object.keys(entry.frontmatter).length > 0}
        <div class="p-3 space-y-3">
          {#each getSortedFrontmatter(entry.frontmatter) as [key, value]}
            {@const Icon = getIcon(key)}
            <div class="space-y-1 group">
              <div
                class="flex items-center justify-between text-xs text-muted-foreground"
              >
                <div class="flex items-center gap-2">
                  <Icon class="size-3.5" />
                  <span class="font-medium">{formatKey(key)}</span>
                </div>
                <!-- Delete button -->
                <Button
                  variant="ghost"
                  size="icon"
                  class="size-5 opacity-0 group-hover:opacity-100 transition-opacity"
                  onclick={() => onPropertyRemove?.(key)}
                  aria-label="Remove property"
                >
                  <X class="size-3" />
                </Button>
              </div>
              <div class="pl-5.5">
                {#if isArray(value)}
                  <!-- Array editor -->
                  <div class="space-y-1">
                    <div class="flex flex-wrap gap-1">
                      {#each value as item, index}
                        <span
                          class="inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-xs font-medium bg-secondary text-secondary-foreground group/tag"
                        >
                          {item}
                          <button
                            type="button"
                            class="opacity-0 group-hover/tag:opacity-100 hover:text-destructive transition-opacity"
                            onclick={() => handleArrayItemRemove(key, index)}
                            aria-label="Remove item"
                          >
                            <X class="size-3" />
                          </button>
                        </span>
                      {/each}
                    </div>
                    {#if addingArrayItemKey === key}
                      <div class="flex items-center gap-1 mt-1">
                        <Input
                          type="text"
                          bind:value={newArrayItem}
                          class="h-7 text-base md:text-xs"
                          placeholder="New item..."
                          onkeydown={(e) =>
                            handleKeyPress(e, () => handleAddArrayItem(key))}
                        />
                        <Button
                          variant="ghost"
                          size="icon"
                          class="size-6"
                          onclick={() => handleAddArrayItem(key)}
                        >
                          <Check class="size-3" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          class="size-6"
                          onclick={() => {
                            addingArrayItemKey = null;
                            newArrayItem = "";
                          }}
                        >
                          <X class="size-3" />
                        </Button>
                      </div>
                    {:else}
                      <Button
                        variant="ghost"
                        size="sm"
                        class="h-6 text-xs px-2 mt-1"
                        onclick={() => (addingArrayItemKey = key)}
                      >
                        <Plus class="size-3 mr-1" />
                        Add
                      </Button>
                    {/if}
                  </div>
                {:else if typeof value === "boolean"}
                  <!-- Boolean toggle -->
                  <button
                    type="button"
                    class="flex items-center gap-1.5 cursor-pointer hover:opacity-80 transition-opacity"
                    onclick={() => handleBooleanToggle(key, value)}
                  >
                    <ToggleLeft
                      class="size-4 {value
                        ? 'text-primary'
                        : 'text-muted-foreground'}"
                    />
                    <span class="text-sm text-foreground"
                      >{value ? "Yes" : "No"}</span
                    >
                  </button>
                {:else if isDateValue(key, value)}
                  <!-- Date input -->
                  <Input
                    type="datetime-local"
                    value={formatDateForInput(String(value))}
                    class="h-8 text-base md:text-sm"
                    onchange={(e) => {
                      const target = e.target as HTMLInputElement;
                      onPropertyChange?.(key, parseDateFromInput(target.value));
                    }}
                  />
                {:else}
                  <!-- String input -->
                  <Input
                    type="text"
                    value={String(value ?? "")}
                    class="h-8 text-base md:text-sm {key.toLowerCase() ===
                      'title' && titleError
                      ? 'border-destructive'
                      : ''}"
                    onblur={(e) => handleStringChange(key, e)}
                    onfocus={() => {
                      if (key.toLowerCase() === "title") onTitleErrorClear?.();
                    }}
                    onkeydown={(e) => {
                      if (e.key === "Enter") {
                        handleStringChange(key, e);
                        (e.target as HTMLInputElement).blur();
                      }
                    }}
                  />
                  {#if key.toLowerCase() === "title" && titleError}
                    <Alert.Root variant="destructive" class="mt-2 py-2">
                      <AlertCircle class="size-4" />
                      <Alert.Description class="text-xs">
                        {titleError}
                      </Alert.Description>
                    </Alert.Root>
                  {/if}
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {:else}
        <div
          class="flex flex-col items-center justify-center py-8 px-4 text-center"
        >
          <FileText class="size-8 text-muted-foreground mb-2" />
          <p class="text-sm text-muted-foreground">No properties</p>
          <p class="text-xs text-muted-foreground mt-1">
            Add frontmatter properties below
          </p>
        </div>
      {/if}

      <!-- Add Property Section -->
      <div class="p-3 border-t border-sidebar-border">
        {#if showAddProperty}
          <div class="space-y-2">
            <Input
              type="text"
              bind:value={newPropertyKey}
              class="h-8 text-base md:text-sm"
              placeholder="Property name..."
              onkeydown={(e) => handleKeyPress(e, handleAddProperty)}
            />
            <Input
              type="text"
              bind:value={newPropertyValue}
              class="h-8 text-base md:text-sm"
              placeholder="Value..."
              onkeydown={(e) => handleKeyPress(e, handleAddProperty)}
            />
            <div class="flex gap-2">
              <Button
                variant="default"
                size="sm"
                class="flex-1 h-7 text-xs"
                onclick={handleAddProperty}
              >
                <Check class="size-3 mr-1" />
                Add
              </Button>
              <Button
                variant="ghost"
                size="sm"
                class="h-7 text-xs"
                onclick={() => {
                  showAddProperty = false;
                  newPropertyKey = "";
                  newPropertyValue = "";
                }}
              >
                Cancel
              </Button>
            </div>
          </div>
        {:else}
          <Button
            variant="outline"
            size="sm"
            class="w-full h-8 text-xs"
            onclick={() => (showAddProperty = true)}
          >
            <Plus class="size-3 mr-1" />
            Add Property
          </Button>
        {/if}
      </div>

      <!-- Attachments Section -->
      <div class="p-3 border-t border-sidebar-border">
        <div class="flex items-center justify-between mb-2">
          <div class="flex items-center gap-2 text-xs text-muted-foreground">
            <Paperclip class="size-3.5" />
            <span class="font-medium">Attachments</span>
          </div>
        </div>

        {#if attachmentError}
          <Alert.Root variant="destructive" class="mb-2 py-2">
            <AlertCircle class="size-4" />
            <Alert.Description class="text-xs">
              {attachmentError}
            </Alert.Description>
          </Alert.Root>
        {/if}

        {#if getAttachments().length > 0}
          <div class="space-y-1 mb-2">
            {#each getAttachments() as attachment}
              {@const Icon = getFileIcon(getFilename(attachment))}
              <div
                class="flex items-center justify-between gap-2 px-2 py-1.5 rounded-md bg-secondary/50 group cursor-grab active:cursor-grabbing"
                role="listitem"
                aria-label="Attachment: {getFilename(attachment)}, drag to move"
                draggable="true"
                ondragstart={(e) => {
                  if (e.dataTransfer && entry) {
                    e.dataTransfer.setData('text/x-diaryx-attachment', attachment);
                    e.dataTransfer.setData('text/x-diaryx-source-entry', entry.path);
                    e.dataTransfer.effectAllowed = 'move';
                  }
                }}
              >
                <div class="flex items-center gap-2 min-w-0">
                  <Icon class="size-3.5 shrink-0 text-muted-foreground" />
                  <span
                    class="text-xs text-foreground truncate"
                    title={attachment}
                  >
                    {getFilename(attachment)}
                  </span>
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  class="size-5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity"
                  onclick={() => onDeleteAttachment?.(attachment)}
                  aria-label="Remove attachment"
                >
                  <Trash2 class="size-3" />
                </Button>
              </div>
            {/each}
          </div>
        {:else}
          <p class="text-xs text-muted-foreground mb-2">No attachments</p>
        {/if}
      </div>
      {:else}
        <div
          class="flex flex-col items-center justify-center py-8 px-4 text-center"
        >
          <FileText class="size-8 text-muted-foreground mb-2" />
          <p class="text-sm text-muted-foreground">No entry selected</p>
          <p class="text-xs text-muted-foreground mt-1">
            Select an entry to view its properties
          </p>
        </div>
      {/if}
    {:else if activeTab === "history"}
      <!-- History Tab -->
      {#if entry}
        <div class="p-3">
          <!-- History Header -->
          <div class="flex items-center justify-between mb-3">
            <div class="flex items-center gap-2 text-xs text-muted-foreground">
              <History class="size-3.5" />
              <span class="font-medium">Version History</span>
            </div>
            <Button
              variant="ghost"
              size="icon"
              class="size-6"
              onclick={loadHistory}
              disabled={historyLoading}
              aria-label="Refresh history"
            >
              <RefreshCw class="size-3 {historyLoading ? 'animate-spin' : ''}" />
            </Button>
          </div>

          {#if historyError}
            <Alert.Root variant="destructive" class="mb-3 py-2">
              <AlertCircle class="size-4" />
              <Alert.Description class="text-xs">
                {historyError}
              </Alert.Description>
            </Alert.Root>
          {/if}

          {#if historyLoading && history.length === 0}
            <div class="flex items-center justify-center py-8">
              <RefreshCw class="size-5 animate-spin text-muted-foreground" />
            </div>
          {:else if history.length === 0}
            <div class="flex flex-col items-center justify-center py-8 text-center">
              <History class="size-8 text-muted-foreground mb-2" />
              <p class="text-sm text-muted-foreground">No history available</p>
              <p class="text-xs text-muted-foreground mt-1">
                Changes will appear here
              </p>
            </div>
          {:else}
            <!-- History Entries -->
            <div class="space-y-1">
              {#each history as historyEntry (historyEntry.update_id)}
                {@const isSelected = selectedEntry?.update_id === historyEntry.update_id}
                <div
                  class="rounded-md cursor-pointer transition-colors {isSelected ? 'bg-accent' : 'hover:bg-muted'}"
                  role="button"
                  tabindex="0"
                  onclick={() => selectHistoryEntry(historyEntry)}
                  onkeydown={(e) => e.key === 'Enter' && selectHistoryEntry(historyEntry)}
                >
                  <div class="flex items-center justify-between p-2">
                    <div class="flex-1 min-w-0">
                      <div class="flex items-center gap-2">
                        <span class="text-sm font-medium text-foreground">
                          {formatRelativeTime(historyEntry.timestamp)}
                        </span>
                        <span class="text-[10px] px-1.5 py-0.5 rounded {getOriginClass(historyEntry.origin)}">
                          {getOriginLabel(historyEntry)}
                        </span>
                      </div>
                      <div class="text-[10px] text-muted-foreground mt-0.5">
                        #{historyEntry.update_id.toString()}
                      </div>
                    </div>
                    {#if isSelected}
                      <Button
                        variant="default"
                        size="sm"
                        class="h-6 text-xs px-2 shrink-0"
                        onclick={(e) => { e.stopPropagation(); restoreVersion(historyEntry); }}
                      >
                        <RotateCcw class="size-3 mr-1" />
                        Restore
                      </Button>
                    {/if}
                  </div>
                </div>
              {/each}
            </div>

            <!-- Version Diff -->
            {#if selectedEntry && (diffs.length > 0 || loadingDiff)}
              <div class="mt-4 pt-3 border-t border-sidebar-border">
                <h4 class="text-xs font-medium text-muted-foreground mb-2">Changes in this version</h4>
                {#if loadingDiff}
                  <div class="flex items-center justify-center py-4">
                    <RefreshCw class="size-4 animate-spin text-muted-foreground" />
                  </div>
                {:else}
                  <VersionDiff {diffs} />
                {/if}
              </div>
            {/if}
          {/if}
        </div>
      {:else}
        <div
          class="flex flex-col items-center justify-center py-8 px-4 text-center"
        >
          <History class="size-8 text-muted-foreground mb-2" />
          <p class="text-sm text-muted-foreground">No entry selected</p>
          <p class="text-xs text-muted-foreground mt-1">
            Select an entry to view its history
          </p>
        </div>
      {/if}
    {:else if activeTab === "share"}
      <!-- Share Tab -->
      <ShareTab {onBeforeHost} {onOpenEntry} />
    {/if}
  </div>

  <!-- Footer with path -->
  {#if entry}
    <div class="px-4 py-3 border-t border-sidebar-border shrink-0">
      <p class="text-xs text-muted-foreground truncate" title={entry.path}>
        {entry.path}
      </p>
    </div>
  {/if}
</aside>
