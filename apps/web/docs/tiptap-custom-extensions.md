---
title: TipTap Custom Extensions
description: Guide to creating custom TipTap extensions with markdown support
author: adammharris
audience:
- developers
part_of: '[README](/apps/web/README.md)'
---

# TipTap Custom Extensions

This guide covers creating custom TipTap extensions with `@tiptap/markdown` integration, based on lessons learned implementing the spoiler syntax (`||hidden text||`).

## Key Concepts

### Extension Lifecycle

When creating a TipTap editor with the Markdown extension:

1. All extensions are instantiated and configured
2. The Markdown extension's `onCreate` hook builds a `MarkdownManager`
3. The `MarkdownManager` iterates through all extensions and registers:
   - `markdownTokenizer` - for parsing custom syntax
   - `parseMarkdown` - for converting tokens to TipTap content
   - `renderMarkdown` - for serializing back to markdown

### Important: Tokenizer Persistence

**The `@tiptap/markdown` extension registers tokenizers with a shared marked.js instance.** This has a critical implication:

> Once a tokenizer is registered, it persists even after the editor is destroyed.

If you conditionally load an extension:
1. First editor loads with extension → tokenizer registered with marked.js
2. Editor destroyed, new editor created WITHOUT the extension
3. marked.js still has the tokenizer
4. Content is parsed into tokens but there's no extension to handle them
5. **Content is lost or corrupted**

**Solution:** Always load the extension, but use an `enabled` option to control behavior:

```typescript
// Always load, configure behavior
SpoilerMark.configure({ enabled: enableSpoilers })
```

## Creating a Custom Mark Extension

### Basic Structure

```typescript
import { Mark, mergeAttributes } from "@tiptap/core";
import { markInputRule, markPasteRule } from "@tiptap/core";

export const CustomMark = Mark.create({
  name: "customMark",

  addOptions() {
    return {
      HTMLAttributes: {},
      enabled: true, // For conditional behavior
    };
  },

  // HTML parsing (for clipboard/drag-drop)
  parseHTML() {
    return [{ tag: "span[data-custom]" }];
  },

  // HTML rendering
  renderHTML({ HTMLAttributes }) {
    return [
      "span",
      mergeAttributes(this.options.HTMLAttributes, HTMLAttributes, {
        "data-custom": "",
        class: "custom-mark",
      }),
      0, // 0 = render children here
    ];
  },

  // ... other methods
});
```

### Adding Markdown Support

#### 1. Tokenizer (Parsing)

The tokenizer tells marked.js how to recognize your syntax:

```typescript
// @ts-expect-error - custom field for @tiptap/markdown
markdownTokenizer: {
  name: "customMark",
  level: "inline", // or "block"
  start: "||", // Quick check before running full regex
  tokenize(
    src: string,
    _tokens: unknown[],
    helper: { inlineTokens: (src: string) => unknown[] }
  ) {
    const match = /^\|\|([^|]+)\|\|/.exec(src);
    if (!match) return undefined;

    return {
      type: "customMark", // Must match extension name
      raw: match[0],      // Full matched string
      tokens: helper.inlineTokens(match[1]), // Parse inner content
    };
  },
},
```

#### 2. Parse Handler (Token → TipTap)

Converts the parsed token into TipTap content:

```typescript
// @ts-expect-error - custom field for @tiptap/markdown
parseMarkdown(
  token: { tokens?: unknown[] },
  helpers: {
    parseInline: (tokens: unknown[]) => unknown[];
    applyMark: (markType: string, content: unknown[], attrs?: unknown) => unknown;
  }
) {
  const content = token.tokens ? helpers.parseInline(token.tokens) : [];
  return helpers.applyMark("customMark", content);
},
```

#### 3. Render Handler (TipTap → Markdown)

**This must be added via `.extend()` to be discovered by `@tiptap/markdown`:**

```typescript
export const CustomMark = Mark.create({
  // ... base config
}).extend({
  // Render back to markdown
  renderMarkdown(
    node: unknown,
    helpers: { renderChildren: (node: unknown) => string }
  ) {
    const content = helpers.renderChildren(node); // IMPORTANT: pass node!
    return `||${content}||`;
  },
});
```

**Critical:** For marks, you must call `helpers.renderChildren(node)` with the `node` parameter. Calling `helpers.renderChildren()` with no arguments will not work.

## Conditional Behavior Pattern

When you need to toggle extension behavior without unloading it:

```typescript
export const CustomMark = Mark.create({
  addOptions() {
    return {
      enabled: true,
    };
  },

  renderHTML({ HTMLAttributes }) {
    if (!this.options.enabled) {
      // Render differently when disabled
      return ["span", { class: "custom-disabled" }, 0];
    }
    return ["span", { class: "custom-enabled" }, 0];
  },

  addInputRules() {
    if (!this.options.enabled) return [];
    // ... return rules
  },

  addKeyboardShortcuts() {
    if (!this.options.enabled) return {};
    // ... return shortcuts
  },

  addProseMirrorPlugins() {
    if (!this.options.enabled) return [];
    // ... return plugins
  },

  // Tokenizer, parseMarkdown, renderMarkdown - ALWAYS active
  // These ensure content is preserved regardless of enabled state
});
```

## Debugging Tips

### Check Extension Config

```typescript
console.log("Extension config:", MyExtension.config);
console.log("Has renderMarkdown:", "renderMarkdown" in MyExtension.config);
```

### Check MarkdownManager Registration

```typescript
Markdown.configure({}).extend({
  onCreate() {
    this.parent?.();

    const manager = this.storage?.manager;
    console.log("Handler registered:", manager?.nodeTypeRegistry?.has?.("myMark"));
    console.log("Handlers:", manager?.getHandlersForNodeType?.("myMark"));
  },
});
```

### Check Document Structure

```typescript
onUpdate: ({ editor }) => {
  console.log("JSON:", JSON.stringify(editor.getJSON(), null, 2));
  console.log("Markdown:", editor.getMarkdown());
}
```

## Common Pitfalls

1. **Forgetting to pass `node` to `renderChildren`** - For marks, always use `helpers.renderChildren(node)`, not `helpers.renderChildren()`.

2. **Defining `renderMarkdown` in the base config** - Use `.extend()` to ensure it's discoverable by `getExtensionField`.

3. **Conditionally loading extensions with tokenizers** - The tokenizer persists in marked.js. Always load the extension, use options for conditional behavior.

4. **Extension order** - Generally doesn't matter since registration happens in `onCreate`, but keep extensions organized logically.

## Reference Implementation

See `src/lib/extensions/SpoilerMark.ts` for a complete working example of a custom mark with:
- Custom syntax (`||text||`)
- Input/paste rules
- Keyboard shortcut
- Click-to-reveal behavior
- Conditional enable/disable
- Full markdown round-trip support
