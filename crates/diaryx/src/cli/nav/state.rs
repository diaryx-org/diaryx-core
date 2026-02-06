//! Navigation application state management

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;
use tui_tree_widget::TreeState;

use diaryx_core::workspace::TreeNode;

use crate::cli::{CliWorkspace, block_on};

/// What kind of text input action is in progress
#[derive(Debug, Clone)]
pub enum TextAction {
    Create,
    Rename,
}

/// What kind of confirmation action is in progress
#[derive(Debug, Clone)]
pub enum ConfirmAction {
    Delete,
}

/// What kind of node-pick action is in progress
#[derive(Debug, Clone)]
pub enum PickAction {
    Move,
    Merge,
}

/// Current input mode for the TUI
#[derive(Debug, Clone)]
pub enum InputMode {
    /// Normal browsing mode
    Normal,
    /// Text input mode (create, rename)
    TextInput {
        prompt: String,
        buffer: String,
        cursor: usize,
        action: TextAction,
    },
    /// Confirmation prompt (delete)
    Confirm {
        message: String,
        action: ConfirmAction,
    },
    /// Pick a second node (move, merge)
    NodePick {
        prompt: String,
        source_path: PathBuf,
        action: PickAction,
    },
}

impl Default for InputMode {
    fn default() -> Self {
        Self::Normal
    }
}

/// Navigation application state
pub struct NavState {
    /// The workspace tree data
    pub tree: TreeNode,

    /// Tree widget state (selection, expanded nodes)
    pub tree_state: TreeState<PathBuf>,

    /// Currently selected file path
    pub selected_path: Option<PathBuf>,

    /// Title of the selected file (from frontmatter)
    pub selected_title: Option<String>,

    /// Cached preview content for selected file (body only, no frontmatter)
    pub preview_content: String,

    /// Workspace root directory
    pub workspace_root: PathBuf,

    /// Whether to quit the app
    pub should_quit: bool,

    /// Path to open in editor (TUI will resume after editor closes)
    pub pending_edit: Option<PathBuf>,

    /// Preview scroll offset (line number)
    pub preview_scroll: u16,

    /// Current input mode
    pub mode: InputMode,

    /// Transient status message: (message, timestamp, is_error)
    pub status_message: Option<(String, Instant, bool)>,

    /// Root index path (for rebuilding tree after mutations)
    pub root_path: PathBuf,

    /// Depth limit for tree building (None = unlimited)
    pub depth_limit: Option<usize>,
}

impl NavState {
    /// Create a new navigation state from a tree
    pub fn new(
        tree: TreeNode,
        workspace_root: PathBuf,
        root_path: PathBuf,
        depth_limit: Option<usize>,
    ) -> Self {
        let mut tree_state = TreeState::default();
        // Open the root node by default
        tree_state.open(vec![tree.path.clone()]);
        // Select the root node
        tree_state.select(vec![tree.path.clone()]);

        Self {
            selected_path: Some(tree.path.clone()),
            selected_title: Some(tree.name.clone()),
            preview_content: String::new(),
            tree,
            tree_state,
            workspace_root,
            should_quit: false,
            pending_edit: None,
            preview_scroll: 0,
            mode: InputMode::Normal,
            status_message: None,
            root_path,
            depth_limit,
        }
    }

    /// Update preview content for currently selected file.
    /// Strips frontmatter and shows only the body content.
    pub fn update_preview(&mut self) {
        if let Some(path) = &self.selected_path {
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    // Strip frontmatter (content between --- markers)
                    self.preview_content = strip_frontmatter(&content);
                }
                Err(e) => {
                    self.preview_content = format!("(Cannot read file: {})", e);
                }
            }
            // Reset scroll when content changes
            self.preview_scroll = 0;
        }
    }

    /// Scroll preview down by one line
    pub fn scroll_preview_down(&mut self) {
        let max_scroll = self.preview_content.lines().count().saturating_sub(1) as u16;
        if self.preview_scroll < max_scroll {
            self.preview_scroll += 1;
        }
    }

    /// Scroll preview up by one line
    pub fn scroll_preview_up(&mut self) {
        self.preview_scroll = self.preview_scroll.saturating_sub(1);
    }

    /// Rebuild the tree from the filesystem after a mutation.
    /// Preserves opened nodes and restores selection.
    pub fn rebuild_tree(&mut self, ws: &CliWorkspace) {
        self.rebuild_tree_inner(ws, None);
    }

    /// Rebuild the tree and select a specific path (for create/dup/rename/move).
    pub fn rebuild_tree_and_select(&mut self, ws: &CliWorkspace, new_path: PathBuf) {
        self.rebuild_tree_inner(ws, Some(new_path));
    }

    fn rebuild_tree_inner(&mut self, ws: &CliWorkspace, select_path: Option<PathBuf>) {
        // Save opened nodes
        let saved_opened: Vec<Vec<PathBuf>> = self.tree_state.opened().iter().cloned().collect();
        let saved_selected = self.tree_state.selected().to_vec();

        // Rebuild tree
        let mut visited = HashSet::new();
        match block_on(ws.build_tree_with_depth(&self.root_path, self.depth_limit, &mut visited)) {
            Ok(new_tree) => {
                self.tree = new_tree;
            }
            Err(e) => {
                self.set_status(format!("Tree rebuild failed: {}", e), true);
                return;
            }
        }

        // Restore tree state
        let mut new_tree_state = TreeState::default();

        // Re-open saved nodes
        for path in saved_opened {
            new_tree_state.open(path);
        }

        // Select new path or restore previous selection
        let target = select_path.unwrap_or_else(|| {
            saved_selected
                .last()
                .cloned()
                .unwrap_or(self.root_path.clone())
        });
        // Build the full identifier path to the target
        if let Some(id_path) = find_id_path(&self.tree, &target) {
            new_tree_state.select(id_path);
        } else {
            // Fallback: select root
            new_tree_state.select(vec![self.tree.path.clone()]);
        }

        self.tree_state = new_tree_state;
        self.update_selection_from_tree_self();
    }

    /// Update selection from tree state using self.tree (no external borrow needed)
    pub fn update_selection_from_tree_self(&mut self) {
        if let Some(selected) = self.tree_state.selected().last() {
            let selected = selected.clone();
            self.selected_path = Some(selected.clone());
            if let Some(node) = find_node_by_path(&self.tree, &selected) {
                self.selected_title = Some(node.name.clone());
            }
            self.update_preview();
        }
    }

    /// Set a transient status message
    pub fn set_status(&mut self, message: String, is_error: bool) {
        self.status_message = Some((message, Instant::now(), is_error));
    }

    /// Clear status message if it has expired (3 seconds)
    pub fn clear_expired_status(&mut self) {
        if let Some((_, when, _)) = &self.status_message {
            if when.elapsed().as_secs() >= 3 {
                self.status_message = None;
            }
        }
    }
}

/// Strip YAML frontmatter from markdown content.
/// Frontmatter is content between `---` markers at the start of the file.
fn strip_frontmatter(content: &str) -> String {
    let trimmed = content.trim_start();

    // Check if content starts with frontmatter delimiter
    if !trimmed.starts_with("---") {
        return content.to_string();
    }

    // Find the closing delimiter
    let after_first = &trimmed[3..];
    if let Some(end_pos) = after_first.find("\n---") {
        // Skip past the closing --- and any following newline
        let body_start = end_pos + 4; // "\n---".len()
        let remainder = &after_first[body_start..];
        remainder.trim_start_matches('\n').to_string()
    } else {
        // No closing delimiter found, return original
        content.to_string()
    }
}

/// Build the full identifier path from root to a target node.
/// TreeState::select() needs the full path of identifiers [root, ..., target].
fn find_id_path(root: &TreeNode, target: &PathBuf) -> Option<Vec<PathBuf>> {
    fn helper(node: &TreeNode, target: &PathBuf, path: &mut Vec<PathBuf>) -> bool {
        path.push(node.path.clone());
        if &node.path == target {
            return true;
        }
        for child in &node.children {
            if helper(child, target, path) {
                return true;
            }
        }
        path.pop();
        false
    }
    let mut path = Vec::new();
    if helper(root, target, &mut path) {
        Some(path)
    } else {
        None
    }
}

/// Find a node in the tree by its path
fn find_node_by_path<'a>(node: &'a TreeNode, path: &PathBuf) -> Option<&'a TreeNode> {
    if &node.path == path {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find_node_by_path(child, path) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_frontmatter_with_frontmatter() {
        let content = r#"---
title: Test
description: A test file
---

# Hello

This is the body."#;

        let result = strip_frontmatter(content);
        assert_eq!(result, "# Hello\n\nThis is the body.");
    }

    #[test]
    fn test_strip_frontmatter_no_frontmatter() {
        let content = "# Hello\n\nNo frontmatter here.";
        let result = strip_frontmatter(content);
        assert_eq!(result, content);
    }

    #[test]
    fn test_strip_frontmatter_unclosed() {
        let content = "---\ntitle: Test\nNo closing delimiter";
        let result = strip_frontmatter(content);
        assert_eq!(result, content);
    }
}
