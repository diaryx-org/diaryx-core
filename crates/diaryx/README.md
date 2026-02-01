---
title: diaryx
description: CLI frontend
author: adammharris
audience:
- public
part_of: '[README](/crates/README.md)'
contents:
  - '[README](/crates/diaryx/src/README.md)'
attachments:
  - '[Cargo.toml](/crates/diaryx/Cargo.toml)'
  - '[build.rs](/crates/diaryx/build.rs)'
exclude:
  - '*.lock'
---

# Diaryx CLI

A command line interface for the diaryx_core library. Allows command-line journaling.

## Installation

Install via `cargo`:

```bash
cargo install diaryx
```

## A Brief Introduction (to the CLI)

Diaryx saves entries as markdown files in a folder, and provides tools for modifying frontmatter properties. It also provides a "workspace" feature for defining relationships between different entries. In this way it is similar to other "knowledge management" tools like Obsidian. But it differs by defining these relationships primarily in the frontmatter in the form of `part_of` and `contents` properties.

It distinguishes between "index" files and "content" or "leaf" files. An "index" file describes the contents of a certain area of the workspace. A "content" file is simply a regular file that belongs to a group described by an index file. For example:

```bash
> cd ~/path/to/workspace
> diaryx init
✓ Initialized diaryx configuration
  Default workspace: /Users/your_username/diaryx
  Config file: /Users/your_username/<config-path>/diaryx/config.toml
✓ Initialized workspace
  Index file: /Users/your_username/diaryx/README.md
> diaryx w info
diaryx - A diaryx workspace
```

`diaryx` has initiated a default workspace with a single index file. You can look at it using `diaryx open README`, which opens the file in your default editor as defined by the `$EDITOR` environment variable:

```md
---
title: diaryx
description: A diaryx workspace
contents: []
---

# diaryx

A diaryx workspace
```

The **frontmatter** is the space at the top enclosed with three-hyphen delimiters (`---`). It describes certain aspects of the file, so it is a kind of readable metadata. Here you see "title" and "description," as well as an empty "contents." These are called **properties**, and there are many more possible properties you could have in a file, but these are enough to get you started.

To add a file, type `diaryx workspace add <filename>.md`, replacing `<filename>` with whatever you want your new file to be called. You can also simply type the letter `w` instead of `workspace` as a shortcut, or alias.

```bash
> diaryx w add test.md
✓ Added 'test.md' to contents of '/Users/adamharris/diaryx/README.md'
✓ Set part_of to 'README.md' in 'test.md'
```

If you look in README, a new property has been added:

```yaml
contents:
  - test.md
```

And you can use `diaryx open test` to look at the new file. It has a corresponding property:

```yaml
part_of: README.md
```

These two properties, `contents` and `part_of`, define a hierarchal relationship. You can see it using `diaryx workspace info`:

```bash
> diaryx workspace info
diaryx - A diaryx workspace
└── test
```

The README file has a title, "diaryx," and a description, "A diaryx workspace." The "test" file does not have a title or description, so it just uses the file name.

These `contents`/`part_of` relationships can be deeply nested, and don't necessarily need folders to function. You can use it for chapters of a book, phases of a project, recipes in a recipe book, or whatever you like!

From here, you can learn about the tool using `--help` menus. Try `diaryx --help`, `diaryx workspace --help`, or `diaryx property --help` to learn more about what you can do with Diaryx.

## Navigation

Interactively browse your workspace hierarchy with a TUI:

```bash
> diaryx nav

┌─────────────── Workspace ───────────────┬─────────────── notes/ideas.md ───────────────┐
│ ▶ README - A diaryx workspace           │                                              │
│   ├── notes - Personal notes            │   Ideas                                      │
│   │   ├── meeting-notes                 │   notes/ideas.md                             │
│   │   └── ideas - Random thoughts       │                                              │
│   └── ▶ projects                        │ # Ideas                                      │
│                                         │                                              │
│                                         │ Random thoughts and ideas go here.           │
├─────────────────────────────────────────┴──────────────────────────────────────────────┤
│ j/k: navigate  h/l: collapse/expand  Enter: open  J/K: scroll preview  q: quit        │
└────────────────────────────────────────────────────────────────────────────────────────┘
```

### Key Bindings

| Key | Action |
|-----|--------|
| `j` / `↓` | Move selection down |
| `k` / `↑` | Move selection up |
| `h` / `←` | Collapse / Go to parent |
| `l` / `→` | Expand / Enter child |
| `Space` / `Tab` | Toggle expand/collapse |
| `J` / `K` | Scroll preview down/up |
| `Ctrl+d` / `Ctrl+u` | Page down/up in preview |
| `Enter` | Open selected file in `$EDITOR` |
| `q` / `Esc` | Quit |

### Navigation Options

- `diaryx nav` - Start from workspace root
- `diaryx nav .` - Start from current directory's index
- `diaryx nav notes/` - Start from a specific subdirectory

## Workspace Validation

Diaryx can validate your workspace to find broken links and other issues:

```bash
> diaryx workspace validate
✓ Workspace validation passed (5 files checked)
```

If issues are found, it reports them:

```bash
> diaryx workspace validate
Errors (1):
  ✗ Broken part_of: notes/orphan.md -> missing.md
Warnings (2):
  ⚠ Unlisted file: notes/forgot-to-add.md
  ⚠ Missing part_of (orphan): random-file.md

Summary: 1 error(s), 2 warning(s), 5 files checked
```

Use `--fix` to automatically repair issues:

```bash
> diaryx workspace validate --fix
  ✓ Fixed: Removed broken part_of 'missing.md' from notes/orphan.md
  ✓ Fixed: Added 'forgot-to-add.md' to notes/README.md
  ✓ Fixed: Set part_of to 'README.md' in random-file.md

Summary: 3 issue(s) fixed, 5 files checked
```

### Validation Errors

- **BrokenPartOf** - `part_of` points to a non-existent file
- **BrokenContentsRef** - `contents` references a non-existent file
- **BrokenAttachment** - `attachments` references a non-existent file

### Validation Warnings

- **OrphanFile** - Markdown file not in any index's `contents`
- **MissingPartOf** - Non-index file has no `part_of` property
- **NonPortablePath** - Path contains absolute or `.`/`..` components
- **OrphanBinaryFile** - Binary file not in any index's `attachments`
- **MultipleIndexes** - Multiple index files in same directory
- **CircularReference** - Circular reference in hierarchy

### Exclude Patterns

Use the `exclude` frontmatter property to suppress warnings for specific files:

```yaml
exclude:
  - "LICENSE.md"    # Exact match
  - "*.lock"        # Glob pattern
```

Patterns are inherited up the `part_of` hierarchy.

You can also validate specific files or directories:

```bash
> diaryx workspace validate notes/
> diaryx workspace validate notes/my-note.md
> diaryx workspace validate notes/ --recursive
```

## Sync

Diaryx can sync your workspace with a remote server for backup and multi-device access:

```bash
# Login with magic link authentication
> diaryx sync login your-email@example.com
Logging in to sync server...
Check your email for a magic link!

# Verify the magic link token
> diaryx sync verify <TOKEN_FROM_EMAIL>
Successfully logged in!

# Check sync status
> diaryx sync status
Sync Status
===========
Server: https://sync.diaryx.org
Account: your-email@example.com (logged in)
Workspace ID: abc-123-def

# Start continuous sync
> diaryx sync start
Connecting to sync server...
Sync is running. Press Ctrl+C to stop.

# Or do one-shot operations
> diaryx sync push    # Push local changes
> diaryx sync pull    # Pull remote changes
```

### Sync Commands

- `diaryx sync login <email>` - Authenticate via magic link
- `diaryx sync verify <token>` - Complete authentication
- `diaryx sync logout` - Clear credentials
- `diaryx sync status` - Show sync status
- `diaryx sync start` - Start continuous sync
- `diaryx sync push` - One-shot push local changes
- `diaryx sync pull` - One-shot pull remote changes
- `diaryx sync config` - Configure sync settings

## roadmap

See [the roadmap document here](../../roadmap.md).

## License

PolyForm Shield 1.0. Read it [here](../../LICENSE.md).
