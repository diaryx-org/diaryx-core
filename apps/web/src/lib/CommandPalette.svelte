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
  } from "@lucide/svelte";

  interface Props {
    open: boolean;
    tree: TreeNode | null;
    api: Api | null;
    onOpenEntry: (path: string) => void;
    onNewEntry: () => void;
    onDailyEntry: () => void;
    onSettings: () => void;
    onExport: () => void;
  }

  let {
    open = $bindable(),
    tree,
    api,
    onOpenEntry,
    onNewEntry,
    onDailyEntry,
    onSettings,
    onExport,
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
