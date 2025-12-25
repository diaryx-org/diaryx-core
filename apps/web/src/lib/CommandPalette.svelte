<script lang="ts">
  import * as Command from "$lib/components/ui/command";
  import type { TreeNode, SearchResults, Backend } from "./backend";
  import {
    Search,
    CalendarDays,
    Settings,
    FilePlus,
    FileText,
    Download,
    Paperclip,
  } from "@lucide/svelte";

  interface Props {
    open: boolean;
    tree: TreeNode | null;
    backend: Backend | null;
    onOpenEntry: (path: string) => void;
    onNewEntry: () => void;
    onDailyEntry: () => void;
    onSettings: () => void;
    onExport: () => void;
    onAddAttachment?: () => void;
  }

  let {
    open = $bindable(),
    tree,
    backend,
    onOpenEntry,
    onNewEntry,
    onDailyEntry,
    onSettings,
    onExport,
    onAddAttachment,
  }: Props = $props();

  let searchValue = $state("");
  let searchResults: SearchResults | null = $state(null);
  let searchTimeout: ReturnType<typeof setTimeout> | null = null;

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
    if (searchValue.trim() && backend) {
      if (searchTimeout) clearTimeout(searchTimeout);
      searchTimeout = setTimeout(async () => {
        try {
          searchResults = await backend.searchWorkspace(searchValue);
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
</script>

<Command.Dialog bind:open title="Command Palette" description="Search or run a command">
  <Command.Input
    placeholder="Search entries or type a command..."
    bind:value={searchValue}
  />
  <Command.List>
    <Command.Empty>No results found.</Command.Empty>

    <!-- Commands Group -->
    <Command.Group heading="Commands">
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
      <Command.Item onSelect={() => handleCommand(onSettings)}>
        <Settings class="mr-2 size-4" />
        <span>Settings</span>
        <Command.Shortcut>Open settings</Command.Shortcut>
      </Command.Item>
      <Command.Item onSelect={() => handleCommand(onExport)}>
        <Download class="mr-2 size-4" />
        <span>Export...</span>
        <Command.Shortcut>Export workspace</Command.Shortcut>
      </Command.Item>
      {#if onAddAttachment}
        <Command.Item onSelect={() => handleCommand(onAddAttachment)}>
          <Paperclip class="mr-2 size-4" />
          <span>Add Attachment</span>
          <Command.Shortcut>Add file to entry</Command.Shortcut>
        </Command.Item>
      {/if}
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
</Command.Dialog>
