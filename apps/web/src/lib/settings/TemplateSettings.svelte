<script lang="ts">
  /**
   * TemplateSettings - Template management settings panel
   *
   * Allows users to:
   * - Configure template folder location
   * - Set default templates for new entries and daily entries
   * - View, create, edit, and delete workspace templates
   */
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import {
    FileText,
    Plus,
    Pencil,
    Trash2,
    Check,
    FolderOpen,
    File,
  } from "@lucide/svelte";
  import { getBackend } from "../backend";
  import { createApi } from "../backend/api";
  import type { TemplateInfo } from "../backend/generated/TemplateInfo";
  import TemplateEditorDialog from "../components/TemplateEditorDialog.svelte";

  // Settings from localStorage
  let templateFolder = $state(
    typeof window !== "undefined"
      ? localStorage.getItem("diaryx-template-folder") || "templates"
      : "templates"
  );
  let defaultTemplate = $state(
    typeof window !== "undefined"
      ? localStorage.getItem("diaryx-default-template") || "note"
      : "note"
  );
  let dailyTemplate = $state(
    typeof window !== "undefined"
      ? localStorage.getItem("diaryx-daily-template") || "daily"
      : "daily"
  );

  // UI state
  let templates = $state<TemplateInfo[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let folderSaved = $state(false);
  let defaultSaved = $state(false);
  let dailySaved = $state(false);

  // Editor dialog state
  let editorOpen = $state(false);
  let editingTemplate = $state<{ name: string; content: string } | null>(null);
  let isNewTemplate = $state(false);

  // Load templates on mount
  $effect(() => {
    loadTemplates();
  });

  async function loadTemplates() {
    loading = true;
    error = null;
    try {
      const backend = await getBackend();
      const api = createApi(backend);
      const workspacePath = await getWorkspaceDir();
      templates = await api.listTemplates(workspacePath || undefined);
    } catch (e) {
      console.error("[TemplateSettings] Failed to load templates:", e);
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  // Get workspace directory (not the index file path)
  async function getWorkspaceDir(): Promise<string> {
    const backend = await getBackend();
    const appPaths = backend.getAppPaths();
    const defaultWorkspace = (appPaths?.default_workspace ?? "") as string;
    // If it ends with .md, strip the filename to get the directory
    if (defaultWorkspace.endsWith(".md")) {
      const lastSlash = defaultWorkspace.lastIndexOf("/");
      if (lastSlash >= 0) {
        return defaultWorkspace.substring(0, lastSlash);
      }
    }
    return defaultWorkspace;
  }

  function saveTemplateFolderSetting() {
    const folder = templateFolder.trim() || "templates";
    templateFolder = folder;
    if (typeof window !== "undefined") {
      localStorage.setItem("diaryx-template-folder", folder);
    }
    folderSaved = true;
    setTimeout(() => {
      folderSaved = false;
    }, 2000);
  }

  function saveDefaultTemplateSetting() {
    if (typeof window !== "undefined") {
      localStorage.setItem("diaryx-default-template", defaultTemplate);
    }
    defaultSaved = true;
    setTimeout(() => {
      defaultSaved = false;
    }, 2000);
  }

  function saveDailyTemplateSetting() {
    if (typeof window !== "undefined") {
      localStorage.setItem("diaryx-daily-template", dailyTemplate);
    }
    dailySaved = true;
    setTimeout(() => {
      dailySaved = false;
    }, 2000);
  }

  function openNewTemplateEditor() {
    isNewTemplate = true;
    editingTemplate = {
      name: "",
      content: `---
title: "{{title}}"
created: {{timestamp}}
---

# {{title}}

`,
    };
    editorOpen = true;
  }

  async function openEditTemplateEditor(templateInfo: TemplateInfo) {
    if (templateInfo.source === "built-in") {
      // Can't edit built-in templates, but can view them
      isNewTemplate = false;
    } else {
      isNewTemplate = false;
    }

    try {
      const backend = await getBackend();
      const api = createApi(backend);
      const workspacePath = await getWorkspaceDir();
      const content = await api.getTemplate(templateInfo.name, workspacePath);
      editingTemplate = { name: templateInfo.name, content };
      editorOpen = true;
    } catch (e) {
      console.error("[TemplateSettings] Failed to load template:", e);
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function handleSaveTemplate(name: string, content: string) {
    try {
      const backend = await getBackend();
      const api = createApi(backend);
      const workspacePath = await getWorkspaceDir();
      await api.saveTemplate(name, content, workspacePath);
      editorOpen = false;
      editingTemplate = null;
      await loadTemplates();
    } catch (e) {
      console.error("[TemplateSettings] Failed to save template:", e);
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function deleteTemplate(templateInfo: TemplateInfo) {
    if (templateInfo.source === "built-in") return;

    if (!confirm(`Delete template "${templateInfo.name}"?`)) return;

    try {
      const backend = await getBackend();
      const api = createApi(backend);
      const workspacePath = await getWorkspaceDir();
      await api.deleteTemplate(templateInfo.name, workspacePath);
      await loadTemplates();
    } catch (e) {
      console.error("[TemplateSettings] Failed to delete template:", e);
      error = e instanceof Error ? e.message : String(e);
    }
  }

  function getSourceBadgeClass(source: string): string {
    switch (source) {
      case "built-in":
        return "bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-200";
      case "workspace":
        return "bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-200";
      default:
        return "bg-gray-100 text-gray-800 dark:bg-gray-800 dark:text-gray-200";
    }
  }
</script>

<div class="space-y-4 min-w-0 overflow-hidden">
  <!-- Template Folder -->
  <div class="space-y-3">
    <h3 class="font-medium flex items-center gap-2">
      <FolderOpen class="size-4" />
      Template Location
    </h3>

    <p class="text-xs text-muted-foreground px-1">
      Folder where templates are stored in your workspace.
    </p>

    <div class="space-y-2 px-1">
      <Label for="template-folder" class="text-xs text-muted-foreground">
        Template Folder
      </Label>
      <div class="flex gap-2">
        <Input
          id="template-folder"
          type="text"
          bind:value={templateFolder}
          placeholder="e.g., templates or .diaryx/templates"
          class="text-sm"
          onkeydown={(e) => e.key === "Enter" && saveTemplateFolderSetting()}
        />
        <Button variant="secondary" size="sm" onclick={saveTemplateFolderSetting}>
          {#if folderSaved}
            <Check class="size-4 text-green-600" />
          {:else}
            Save
          {/if}
        </Button>
      </div>
    </div>
  </div>

  <!-- Default Templates -->
  <div class="space-y-3 pt-2 border-t">
    <h3 class="font-medium flex items-center gap-2">
      <File class="size-4" />
      Default Templates
    </h3>

    <!-- Default for New Entries -->
    <div class="flex items-center justify-between gap-4 px-1">
      <Label for="default-template" class="text-sm flex flex-col gap-0.5">
        <span>New entries</span>
        <span class="font-normal text-xs text-muted-foreground">
          Template used when clicking the + button.
        </span>
      </Label>
      <div class="flex items-center gap-2">
        <select
          id="default-template"
          class="w-auto px-2 py-1 text-sm border rounded bg-background"
          bind:value={defaultTemplate}
          onchange={saveDefaultTemplateSetting}
        >
          {#each templates as t}
            <option value={t.name}>{t.name}</option>
          {/each}
        </select>
        {#if defaultSaved}
          <Check class="size-4 text-green-600" />
        {/if}
      </div>
    </div>

    <!-- Default for Daily Entries -->
    <div class="flex items-center justify-between gap-4 px-1">
      <Label for="daily-template" class="text-sm flex flex-col gap-0.5">
        <span>Daily entries</span>
        <span class="font-normal text-xs text-muted-foreground">
          Template for daily journal entries.
        </span>
      </Label>
      <div class="flex items-center gap-2">
        <select
          id="daily-template"
          class="w-auto px-2 py-1 text-sm border rounded bg-background"
          bind:value={dailyTemplate}
          onchange={saveDailyTemplateSetting}
        >
          {#each templates as t}
            <option value={t.name}>{t.name}</option>
          {/each}
        </select>
        {#if dailySaved}
          <Check class="size-4 text-green-600" />
        {/if}
      </div>
    </div>
  </div>

  <!-- Template List -->
  <div class="space-y-3 pt-2 border-t">
    <div class="flex items-center justify-between">
      <h3 class="font-medium flex items-center gap-2">
        <FileText class="size-4" />
        Templates
      </h3>
      <Button variant="outline" size="sm" onclick={openNewTemplateEditor}>
        <Plus class="size-4 mr-1" />
        New
      </Button>
    </div>

    {#if error}
      <p class="text-xs text-destructive px-1">{error}</p>
    {/if}

    {#if loading}
      <p class="text-xs text-muted-foreground px-1">Loading templates...</p>
    {:else if templates.length === 0}
      <p class="text-xs text-muted-foreground px-1">No templates found.</p>
    {:else}
      <div class="space-y-1 px-1">
        {#each templates as template}
          <div
            class="flex items-center justify-between p-2 rounded-lg border border-border hover:bg-muted/50 transition-colors"
          >
            <div class="flex items-center gap-2">
              <FileText class="size-4 text-muted-foreground" />
              <span class="text-sm font-medium">{template.name}</span>
              <span
                class={`text-xs px-1.5 py-0.5 rounded ${getSourceBadgeClass(template.source)}`}
              >
                {template.source}
              </span>
            </div>
            <div class="flex items-center gap-1">
              <Button
                variant="ghost"
                size="sm"
                class="h-7 w-7 p-0"
                onclick={() => openEditTemplateEditor(template)}
                title={template.source === "built-in" ? "View template" : "Edit template"}
              >
                <Pencil class="size-3.5" />
              </Button>
              {#if template.source !== "built-in"}
                <Button
                  variant="ghost"
                  size="sm"
                  class="h-7 w-7 p-0 text-destructive hover:text-destructive"
                  onclick={() => deleteTemplate(template)}
                  title="Delete template"
                >
                  <Trash2 class="size-3.5" />
                </Button>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<TemplateEditorDialog
  bind:open={editorOpen}
  template={editingTemplate}
  isNew={isNewTemplate}
  readOnly={editingTemplate !== null && !isNewTemplate && templates.find(t => t.name === editingTemplate?.name)?.source === "built-in"}
  onSave={handleSaveTemplate}
  onClose={() => {
    editorOpen = false;
    editingTemplate = null;
  }}
/>
