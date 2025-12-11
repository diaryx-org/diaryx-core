---
title: Roadmap
description: The plan for future Diaryx features
author: adammharris
created: 2025-12-05T12:06:55-07:00
updated: 2025-12-11T01:55:40-07:00
audience:
  - public
part_of: README.md
---

# Roadmap

## v0.5.0

- Improved publish and export (including attachments as described below)
- Link validation

## Future considerations

## Better documentation

We have just one README file right now.

### Attachments/Images

When exporting, referenced images and attachments (e.g., `![photo](./images/vacation.jpg)`) are not currently copied. This could be a future enhancement.

### Link validation

A command to validate that all `part_of`/`contents` references are still valid, and that exported workspaces have no broken internal links.

### Sync

Via git? When to fetch? Update on edit?

### Undo/redo

I would like `diaryx undo` and `diaryx redo` commands to undo/redo any command that was previously done, because it is easy to make mistakes.
