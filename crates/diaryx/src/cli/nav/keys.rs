//! Key binding handling for the navigation TUI

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::state::NavState;
use diaryx_core::workspace::TreeNode;

/// Handle a key event, updating state as needed.
/// Returns true if the app should continue, false if it should quit.
pub fn handle_key(state: &mut NavState, key: KeyEvent, tree: &TreeNode) {
    match key.code {
        // Quit
        KeyCode::Char('q') | KeyCode::Esc => {
            state.should_quit = true;
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.should_quit = true;
        }

        // Navigation (vim-style)
        KeyCode::Char('j') | KeyCode::Down => {
            state.tree_state.key_down();
            state.update_selection_from_tree(tree);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.tree_state.key_up();
            state.update_selection_from_tree(tree);
        }

        // Expand/Collapse or navigate
        KeyCode::Char('l') | KeyCode::Right => {
            // If node has children, expand it; otherwise do nothing
            state.tree_state.key_right();
            state.update_selection_from_tree(tree);
        }
        KeyCode::Char('h') | KeyCode::Left => {
            // Collapse or go to parent
            state.tree_state.key_left();
            state.update_selection_from_tree(tree);
        }

        // Toggle expand (space or tab)
        KeyCode::Char(' ') | KeyCode::Tab => {
            state.tree_state.toggle_selected();
        }

        // Open selected file in editor (TUI will resume after)
        KeyCode::Enter => {
            if let Some(path) = &state.selected_path {
                state.pending_edit = Some(path.clone());
            }
        }

        // Scroll preview (capital J/K)
        KeyCode::Char('J') => {
            state.scroll_preview_down();
        }
        KeyCode::Char('K') => {
            state.scroll_preview_up();
        }

        // Page down/up for preview
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Scroll down by 10 lines
            for _ in 0..10 {
                state.scroll_preview_down();
            }
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Scroll up by 10 lines
            for _ in 0..10 {
                state.scroll_preview_up();
            }
        }

        _ => {}
    }
}
