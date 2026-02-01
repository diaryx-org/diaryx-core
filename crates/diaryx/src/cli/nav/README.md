---
title: Navigation TUI module
description: Interactive TUI for navigating workspace hierarchy
part_of: '[README](/crates/diaryx/src/cli/README.md)'
author: adammharris
audience:
  - public
attachments:
  - '[mod.rs](/crates/diaryx/src/cli/nav/mod.rs)'
  - '[app.rs](/crates/diaryx/src/cli/nav/app.rs)'
  - '[state.rs](/crates/diaryx/src/cli/nav/state.rs)'
  - '[keys.rs](/crates/diaryx/src/cli/nav/keys.rs)'
  - '[tree.rs](/crates/diaryx/src/cli/nav/tree.rs)'
  - '[ui.rs](/crates/diaryx/src/cli/nav/ui.rs)'
---

# Navigation TUI Module

This module implements `diaryx nav` (alias `go`), an interactive TUI for browsing the workspace's `contents`/`part_of` hierarchy.

## Architecture

```
┌─────────────────────────────────────┬─────────────────────────────────────┐
│ Tree View (40%)                     │ Preview Pane (60%)                  │
│                                     │                                     │
│  Rendered by tui-tree-widget        │  Title + path header                │
│  TreeNode → TreeItem conversion     │  File body (frontmatter stripped)   │
│                                     │  Scrollable with J/K                │
├─────────────────────────────────────┴─────────────────────────────────────┤
│ Help Bar                                                                   │
└────────────────────────────────────────────────────────────────────────────┘
```

## Module Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Entry point, workspace resolution, terminal lifecycle |
| `app.rs` | Main event loop, editor suspend/resume logic |
| `state.rs` | `NavState` struct, preview content management, frontmatter stripping |
| `keys.rs` | Key binding handlers (vim-style navigation) |
| `tree.rs` | `TreeNode` → `TreeItem` conversion |
| `ui.rs` | Widget layout and rendering |

## Key Dependencies

- **ratatui** - TUI framework for rendering widgets
- **tui-tree-widget** - Tree widget with expand/collapse support
- **crossterm** - Terminal backend for keyboard events

## Key Bindings

| Key | Action |
|-----|--------|
| `j`/`k` | Navigate up/down |
| `h`/`l` | Collapse/expand or navigate parent/child |
| `Space`/`Tab` | Toggle expand |
| `J`/`K` | Scroll preview |
| `Ctrl+d`/`Ctrl+u` | Page down/up in preview |
| `Enter` | Open in editor (TUI resumes after) |
| `q`/`Esc` | Quit |

## Editor Integration

When Enter is pressed:
1. Terminal is restored to normal mode
2. Editor is launched with the selected file
3. After editor closes, terminal is re-initialized
4. TUI resumes with the same state (preview refreshes if file changed)

This mimics the behavior of file managers like `ranger` or `lf`.
