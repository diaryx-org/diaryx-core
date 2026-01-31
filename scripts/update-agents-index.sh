#!/usr/bin/env bash
# Updates the workspace index in AGENTS.md using diaryx workspace info
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
AGENTS_FILE="$REPO_ROOT/AGENTS.md"
TEMP_FILE="$AGENTS_FILE.tmp"
TREE_FILE="$AGENTS_FILE.tree"

# Generate full workspace tree
diaryx workspace info --depth 0 "$REPO_ROOT/README.md" > "$TREE_FILE"

# Get line numbers for markers
BEGIN_LINE=$(grep -n '<!-- BEGIN:WORKSPACE_INDEX -->' "$AGENTS_FILE" | cut -d: -f1)
END_LINE=$(grep -n '<!-- END:WORKSPACE_INDEX -->' "$AGENTS_FILE" | cut -d: -f1)

# Build new file: head + marker + tree + marker + tail
head -n "$BEGIN_LINE" "$AGENTS_FILE" > "$TEMP_FILE"
cat "$TREE_FILE" >> "$TEMP_FILE"
tail -n +"$END_LINE" "$AGENTS_FILE" >> "$TEMP_FILE"

if cmp -s "$TEMP_FILE" "$AGENTS_FILE"; then
  rm "$TEMP_FILE"
  rm "$TREE_FILE"
else
  mv "$TEMP_FILE" "$AGENTS_FILE"
  rm -f "$TREE_FILE"

  # Update frontmatter timestamp
  NOW=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
  diaryx property set "$AGENTS_FILE" updated "$NOW"

  echo "Updated AGENTS.md workspace index"
fi
