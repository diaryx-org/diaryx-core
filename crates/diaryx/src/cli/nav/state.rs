//! Navigation application state management

use std::path::PathBuf;
use tui_tree_widget::TreeState;

use diaryx_core::workspace::TreeNode;

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
}

impl NavState {
    /// Create a new navigation state from a tree
    pub fn new(tree: TreeNode, workspace_root: PathBuf) -> Self {
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

    /// Update selection based on tree state
    pub fn update_selection_from_tree(&mut self, tree: &TreeNode) {
        if let Some(selected) = self.tree_state.selected().last() {
            self.selected_path = Some(selected.clone());
            // Find the node to get its title
            if let Some(node) = find_node_by_path(tree, selected) {
                self.selected_title = Some(node.name.clone());
            }
            self.update_preview();
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
