<script lang="ts">
  /**
   * TemplateEditorDialog - Modal for creating/editing templates
   *
   * Shows a textarea for template content and a variable reference panel.
   */
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { FileText, Info, Save, X } from "@lucide/svelte";

  interface Props {
    open: boolean;
    template: { name: string; content: string } | null;
    isNew: boolean;
    readOnly?: boolean;
    onSave: (name: string, content: string) => void;
    onClose: () => void;
  }

  let {
    open = $bindable(),
    template,
    isNew,
    readOnly = false,
    onSave,
    onClose,
  }: Props = $props();

  // Local state for editing
  let name = $state("");
  let content = $state("");
  let error = $state<string | null>(null);

  // Template variables reference
  const templateVariables = [
    { name: "{{title}}", desc: "Entry title" },
    { name: "{{filename}}", desc: "Filename without extension" },
    { name: "{{date}}", desc: "Current date (YYYY-MM-DD)" },
    { name: "{{date:%B %d, %Y}}", desc: "Formatted date" },
    { name: "{{time}}", desc: "Current time (HH:MM)" },
    { name: "{{timestamp}}", desc: "ISO 8601 timestamp" },
    { name: "{{year}}", desc: "Current year" },
    { name: "{{month}}", desc: "Current month (2 digits)" },
    { name: "{{month_name}}", desc: "Month name (e.g., January)" },
    { name: "{{day}}", desc: "Current day (2 digits)" },
    { name: "{{weekday}}", desc: "Weekday name (e.g., Monday)" },
    { name: "{{part_of}}", desc: "Parent index reference" },
  ];

  // Reset local state when template changes
  $effect(() => {
    if (template) {
      name = template.name;
      content = template.content;
      error = null;
    }
  });

  function handleSave() {
    const trimmedName = name.trim();
    if (!trimmedName) {
      error = "Template name is required";
      return;
    }
    if (!content.trim()) {
      error = "Template content is required";
      return;
    }
    // Validate name (alphanumeric, dashes, underscores only)
    if (!/^[a-zA-Z0-9_-]+$/.test(trimmedName)) {
      error = "Name can only contain letters, numbers, dashes, and underscores";
      return;
    }
    onSave(trimmedName, content);
  }

  function insertVariable(variable: string) {
    content += variable;
  }
</script>

<Dialog.Root bind:open onOpenChange={(v) => !v && onClose()}>
  <Dialog.Content class="sm:max-w-[700px] max-h-[85vh] flex flex-col">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        <FileText class="size-5" />
        {isNew ? "New Template" : readOnly ? "View Template" : "Edit Template"}
      </Dialog.Title>
      <Dialog.Description>
        {#if readOnly}
          Built-in templates cannot be edited.
        {:else if isNew}
          Create a new template for your entries.
        {:else}
          Edit the template content and save.
        {/if}
      </Dialog.Description>
    </Dialog.Header>

    <div class="flex-1 overflow-hidden flex flex-col gap-4 py-4">
      <!-- Template Name -->
      <div class="space-y-2">
        <Label for="template-name">Name</Label>
        <Input
          id="template-name"
          bind:value={name}
          placeholder="my-template"
          disabled={!isNew || readOnly}
          class="font-mono"
        />
      </div>

      <!-- Content Editor and Variable Reference -->
      <div class="flex-1 flex gap-4 min-h-0">
        <!-- Editor -->
        <div class="flex-1 flex flex-col min-h-0">
          <Label for="template-content" class="mb-2">Content</Label>
          <textarea
            id="template-content"
            bind:value={content}
            disabled={readOnly}
            placeholder={`---
title: "{{title}}"
created: {{timestamp}}
---

# {{title}}

`}
            class="flex-1 w-full p-3 text-sm font-mono border rounded-md bg-background resize-none focus:outline-none focus:ring-2 focus:ring-ring disabled:opacity-50"
          ></textarea>
        </div>

        <!-- Variable Reference -->
        <div class="w-56 flex-shrink-0 flex flex-col min-h-0">
          <div class="flex items-center gap-1 mb-2">
            <Info class="size-4 text-muted-foreground" />
            <span class="text-sm font-medium">Variables</span>
          </div>
          <div class="flex-1 overflow-y-auto border rounded-md p-2 space-y-1">
            {#each templateVariables as v}
              <button
                type="button"
                class="w-full text-left p-1.5 rounded text-xs hover:bg-muted transition-colors disabled:opacity-50"
                onclick={() => insertVariable(v.name)}
                disabled={readOnly}
                title={`Click to insert ${v.name}`}
              >
                <code class="text-primary font-mono">{v.name}</code>
                <span class="text-muted-foreground block mt-0.5">{v.desc}</span>
              </button>
            {/each}
          </div>
        </div>
      </div>

      {#if error}
        <p class="text-sm text-destructive">{error}</p>
      {/if}
    </div>

    <div class="flex justify-end gap-2 pt-4 border-t">
      <Button variant="outline" onclick={onClose}>
        <X class="size-4 mr-1" />
        {readOnly ? "Close" : "Cancel"}
      </Button>
      {#if !readOnly}
        <Button onclick={handleSave}>
          <Save class="size-4 mr-1" />
          Save
        </Button>
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>
