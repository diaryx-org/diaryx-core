---
trigger: model_decision
description: Interact with the Diaryx CLI to manage markdown journals, organize workspace hierarchies, manipulate frontmatter properties, attach files, sync, validate, publish, or export.
---

# Diaryx CLI Skill

This skill allows the agent to use the `diaryx` command-line tool. Diaryx is a structured system for managing markdown files with YAML frontmatter, allowing for hierarchical organization ("workspaces"), metadata management, and synchronization.

## Usage

**Global Arguments:**
* `--workspace <PATH>`: Override the default workspace location for any command.

### 1. Entry Creation & Navigation
* **Daily Journaling:**
    * `diaryx today`: Open (or create) today's entry.
    * `diaryx yesterday`: Open yesterday's entry.
* **Create Specific Entry:**
    * `diaryx create <PATH> [--title <TITLE>] [--template <NAME>]`: Create a new entry at a specific path.
* **Open Entry:**
    * `diaryx open <QUERY>`: Open an entry using a date, path, or fuzzy search term.

### 2. Workspace Organization (`diaryx workspace` or `diaryx w`)
Manage the hierarchy of the notebook (parent/child relationships via `contents` and `part_of` frontmatter).

* **View Hierarchy:**
    * `diaryx w info [PATH] [--depth <N>]`: Show the tree structure.
* **Add Existing File to Parent:**
    * `diaryx w add <PARENT> <CHILD>`: Links an existing child file to a parent index.
* **Create Child File:**
    * `diaryx w create <PARENT> [NAME]`: Creates a new file and automatically links it as a child of the parent.
* **Move/Rename:**
    * `diaryx w mv <SOURCE> <DEST>`: Moves a file and updates all hierarchy references.

### 3. Metadata & Properties (`diaryx property` or `diaryx p`)
Programmatically read or modify YAML frontmatter without opening the file.

* **Get Value:** `diaryx p get <PATH> <KEY>`
* **Set Value:** `diaryx p set <PATH> <KEY> <VALUE>`
* **List Properties:** `diaryx p list <PATH>`
* **List Operations:**
    * `diaryx p append <PATH> <KEY> <VALUE>`: Add to a list (e.g., tags).
    * `diaryx p remove-value <PATH> <KEY> <VALUE>`: Remove item from list.

### 4. Search (`diaryx search` or `diaryx s`)
* `diaryx s <PATTERN>`: Search content.
* `diaryx s <PATTERN> --frontmatter`: Search only within metadata.
* `diaryx s <PATTERN> --property <KEY>`: Search values of a specific property.

### 5. Content Manipulation (`diaryx content` or `diaryx c`)
* `diaryx c append <PATH> <CONTENT>`: Append text to the end of a file.
* `diaryx c get <PATH>`: Read the body content of a file.

### 6. Sync (`diaryx sync`)
* `diaryx sync status`: Check connection status.
* `diaryx sync push`: Force push local changes.
* `diaryx sync pull`: Force pull remote changes.
* `diaryx sync login <EMAIL>`: Authenticate with the sync server.

### 7. Maintenance & Validation
* **Validate Links:** `diaryx w validate --fix` (Scans workspace and fixes broken `part_of`/`contents` links).
* **Sort Frontmatter:** `diaryx sort <PATH>` (Organizes YAML keys; use `--index` for index files).
* **Normalize Filenames:** `diaryx norm <PATH>` (Renames files to match their title property, e.g., "My Title" -> "my_title.md").

### 8. Attachments & Publishing
* **Attachments:** `diaryx att add <ENTRY> <FILE> [--copy]` (Attach a file to an entry).
* **Publish:** `diaryx pub <DESTINATION> [--single-file]` (Generate HTML version of the workspace).
* **Export:** `diaryx export <DESTINATION> --audience <AUDIENCE>` (Export subset of files matching a specific audience).

## Examples

**User:** "Add a tag 'urgent' to today's entry."
**Code:**
```bash
# 'tags' is a list property, so we use append
diaryx p append today tags "urgent"
