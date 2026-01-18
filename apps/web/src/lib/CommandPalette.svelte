<script lang="ts">
  import * as Command from "$lib/components/ui/command";
  import * as Drawer from "$lib/components/ui/drawer";
  import type { TreeNode, SearchResults } from "./backend";
  import type { Api } from "./backend/api";
  import { getMobileState } from "./hooks/useMobile.svelte";
  import {
    Search,
    CalendarDays,
    Settings,
    FilePlus,
    FileText,
    Download,
    ShieldCheck,
    RefreshCw,
    Copy,
    Pencil,
    Trash2,
    FolderInput,
    FilePlus2,
    Share2,
    UserPlus,
    FileSearch,
    ClipboardPaste,
    LetterText,
  } from "@lucide/svelte";

  interface Props {
    open: boolean;
    tree: TreeNode | null;
    api: Api | null;
    currentEntryPath: string | null;
    onOpenEntry: (path: string) => void;
    onNewEntry: () => void;
    onDailyEntry: () => void;
    onSettings: () => void;
    onExport: () => void;
    onValidate: () => void;
    onRefreshTree: () => void;
    onDuplicateEntry: () => void;
    onRenameEntry: () => void;
    onDeleteEntry: () => void;
    onMoveEntry: () => void;
    onCreateChildEntry: () => void;
    onStartShare: () => void;
    onJoinSession: () => void;
    onFindInFile: () => void;
    onWordCount: () => void;
    onImportFromClipboard: () => void;
    onCopyAsMarkdown: () => void;
  }

  let {
    open = $bindable(),
    tree,
    api,
    currentEntryPath,
    onOpenEntry,
    onNewEntry,
    onDailyEntry,
    onSettings,
    onExport,
    onValidate,
    onRefreshTree,
    onDuplicateEntry,
    onRenameEntry,
    onDeleteEntry,
    onMoveEntry,
    onCreateChildEntry,
    onStartShare,
    onJoinSession,
    onFindInFile,
    onWordCount,
    onImportFromClipboard,
    onCopyAsMarkdown,
  }: Props = $props();

  let searchValue = $state("");
  let searchResults: SearchResults | null = $state(null);
  let searchTimeout: ReturnType<typeof setTimeout> | null = null;

  // Check if we have a current entry for entry-specific commands
  const hasCurrentEntry = $derived(!!currentEntryPath);

  // Collect all entry paths from tree for quick navigation
  function getAllEntries(node: TreeNode | null): { path: string; name: string }[] {
    if (!node) return [];
    const entries: { path: string; name: string }[] = [];

    function traverse(n: TreeNode) {
      entries.push({ path: n.path, name: n.name });
      for (const child of n.children) {
        traverse(child);
      }
    }
    traverse(node);
    return entries;
  }

  $effect(() => {
    // Debounced search
    if (searchValue.trim() && api) {
      if (searchTimeout) clearTimeout(searchTimeout);
      searchTimeout = setTimeout(async () => {
        try {
          searchResults = await api.searchWorkspace(searchValue);
        } catch (e) {
          console.error("Search failed:", e);
          searchResults = null;
        }
      }, 200);
    } else {
      searchResults = null;
    }
  });

  function handleSelect(path: string) {
    onOpenEntry(path);
    open = false;
    searchValue = "";
    searchResults = null;
  }

  function handleCommand(action: () => void) {
    action();
    open = false;
    searchValue = "";
    searchResults = null;
  }

  // Filter entries based on search
  const allEntries = $derived(getAllEntries(tree));
  const filteredEntries = $derived(
    searchValue.trim()
      ? allEntries.filter(
          (e) =>
            e.name.toLowerCase().includes(searchValue.toLowerCase()) ||
            e.path.toLowerCase().includes(searchValue.toLowerCase())
        )
      : []
  );

  const mobileState = getMobileState();
</script>

{#snippet commandContent()}
  <Command.Input
    placeholder="Search entries or type a command..."
    bind:value={searchValue}
  />
  <Command.List>
    <Command.Empty>No results found.</Command.Empty>

    <!-- General Commands -->
    <Command.Group heading="General">
      <Command.Item onSelect={() => handleCommand(onDailyEntry)}>
        <CalendarDays class="mr-2 size-4" />
        <span>Daily Entry</span>
        <Command.Shortcut>Open today's entry</Command.Shortcut>
      </Command.Item>
      <Command.Item onSelect={() => handleCommand(onNewEntry)}>
        <FilePlus class="mr-2 size-4" />
        <span>New Entry</span>
        <Command.Shortcut>Create new entry</Command.Shortcut>
      </Command.Item>
      <Command.Item onSelect={() => handleCommand(onImportFromClipboard)}>
        <ClipboardPaste class="mr-2 size-4" />
        <span>Import from Clipboard</span>
        <Command.Shortcut>Create entry from clipboard</Command.Shortcut>
      </Command.Item>
      <Command.Item onSelect={() => handleCommand(onSettings)}>
        <Settings class="mr-2 size-4" />
        <span>Settings</span>
        <Command.Shortcut>Open settings</Command.Shortcut>
      </Command.Item>
    </Command.Group>

    <!-- Current Entry Commands -->
    {#if hasCurrentEntry}
      <Command.Separator />
      <Command.Group heading="Current Entry">
        <Command.Item onSelect={() => handleCommand(onFindInFile)}>
          <FileSearch class="mr-2 size-4" />
          <span>Find in File</span>
          <Command.Shortcut>Cmd/Ctrl+F</Command.Shortcut>
        </Command.Item>
        <Command.Item onSelect={() => handleCommand(onWordCount)}>
          <LetterText class="mr-2 size-4" />
          <span>Word Count</span>
          <Command.Shortcut>Show statistics</Command.Shortcut>
        </Command.Item>
        <Command.Item onSelect={() => handleCommand(onCopyAsMarkdown)}>
          <Copy class="mr-2 size-4" />
          <span>Copy as Markdown</span>
          <Command.Shortcut>Copy to clipboard</Command.Shortcut>
        </Command.Item>
        <Command.Item onSelect={() => handleCommand(onCreateChildEntry)}>
          <FilePlus2 class="mr-2 size-4" />
          <span>Create Child Entry</span>
          <Command.Shortcut>New entry under this one</Command.Shortcut>
        </Command.Item>
        <Command.Item onSelect={() => handleCommand(onDuplicateEntry)}>
          <Copy class="mr-2 size-4" />
          <span>Duplicate Entry</span>
          <Command.Shortcut>Create a copy</Command.Shortcut>
        </Command.Item>
        <Command.Item onSelect={() => handleCommand(onRenameEntry)}>
          <Pencil class="mr-2 size-4" />
          <span>Rename Entry</span>
          <Command.Shortcut>Change filename</Command.Shortcut>
        </Command.Item>
        <Command.Item onSelect={() => handleCommand(onMoveEntry)}>
          <FolderInput class="mr-2 size-4" />
          <span>Move Entry...</span>
          <Command.Shortcut>Move to different parent</Command.Shortcut>
        </Command.Item>
        <Command.Item onSelect={() => handleCommand(onDeleteEntry)}>
          <Trash2 class="mr-2 size-4 text-destructive" />
          <span class="text-destructive">Delete Entry</span>
          <Command.Shortcut>Remove permanently</Command.Shortcut>
        </Command.Item>
      </Command.Group>
    {/if}

    <!-- Workspace Commands -->
    <Command.Separator />
    <Command.Group heading="Workspace">
      <Command.Item onSelect={() => handleCommand(onRefreshTree)}>
        <RefreshCw class="mr-2 size-4" />
        <span>Refresh Tree</span>
        <Command.Shortcut>Reload file tree</Command.Shortcut>
      </Command.Item>
      <Command.Item onSelect={() => handleCommand(onValidate)}>
        <ShieldCheck class="mr-2 size-4" />
        <span>Validate Workspace</span>
        <Command.Shortcut>Check for issues</Command.Shortcut>
      </Command.Item>
      <Command.Item onSelect={() => handleCommand(onExport)}>
        <Download class="mr-2 size-4" />
        <span>Export...</span>
        <Command.Shortcut>Export workspace</Command.Shortcut>
      </Command.Item>
    </Command.Group>

    <!-- Collaboration Commands -->
    <Command.Separator />
    <Command.Group heading="Collaboration">
      <Command.Item onSelect={() => handleCommand(onStartShare)}>
        <Share2 class="mr-2 size-4" />
        <span>Start Share Session</span>
        <Command.Shortcut>Host a session</Command.Shortcut>
      </Command.Item>
      <Command.Item onSelect={() => handleCommand(onJoinSession)}>
        <UserPlus class="mr-2 size-4" />
        <span>Join Session</span>
        <Command.Shortcut>Enter join code</Command.Shortcut>
      </Command.Item>
    </Command.Group>

    <!-- Quick Navigation (files matching query) -->
    {#if filteredEntries.length > 0}
      <Command.Separator />
      <Command.Group heading="Files">
        {#each filteredEntries.slice(0, 10) as entry}
          <Command.Item onSelect={() => handleSelect(entry.path)}>
            <FileText class="mr-2 size-4" />
            <span>{entry.name}</span>
            <Command.Shortcut class="text-xs opacity-50">{entry.path}</Command.Shortcut>
          </Command.Item>
        {/each}
      </Command.Group>
    {/if}

    <!-- Full-text Search Results -->
    {#if searchResults && searchResults.files.length > 0}
      <Command.Separator />
      <Command.Group heading="Content Matches">
        {#each searchResults.files.slice(0, 5) as result}
          <Command.Item onSelect={() => handleSelect(result.path)}>
            <Search class="mr-2 size-4" />
            <div class="flex flex-col">
              <span>{result.title || result.path.split("/").pop()}</span>
              {#if result.matches.length > 0}
                <span class="text-xs text-muted-foreground truncate max-w-[300px]">
                  ...{result.matches[0].line_content.trim()}...
                </span>
              {/if}
            </div>
          </Command.Item>
        {/each}
      </Command.Group>
    {/if}
  </Command.List>
{/snippet}

{#if mobileState.isMobile}
  <!-- Mobile: Use Drawer from top -->
  <Drawer.Root bind:open direction="top">
    <Drawer.Content>
      <div class="mx-auto w-full max-w-md px-4 pb-4">
        <Command.Root class="rounded-lg border-none shadow-none">
          {@render commandContent()}
        </Command.Root>
      </div>
    </Drawer.Content>
  </Drawer.Root>
{:else}
  <!-- Desktop: Use Dialog -->
  <Command.Dialog bind:open title="Command Palette" description="Search or run a command">
    {@render commandContent()}
  </Command.Dialog>
{/if}
