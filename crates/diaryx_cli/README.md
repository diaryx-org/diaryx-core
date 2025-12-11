---
title: Diaryx CLI
author: adammharris
audience:
  - public
part_of: ../../README.md
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

## roadmap

See [the roadmap document here](../../roadmap.md).

## License

PolyForm Shield 1.0. Read it [here](../../LICENSE.md).
