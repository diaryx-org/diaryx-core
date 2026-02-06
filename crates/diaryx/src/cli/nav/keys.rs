//! Key binding handling for the navigation TUI

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::commands;
use super::state::{ConfirmAction, InputMode, NavState, PickAction, TextAction};
use crate::cli::CliWorkspace;

/// Handle a key event, dispatching by current input mode.
pub fn handle_key(state: &mut NavState, key: KeyEvent, ws: &CliWorkspace) {
    match &state.mode {
        InputMode::Normal => handle_normal_key(state, key, ws),
        InputMode::TextInput { .. } => handle_text_input_key(state, key, ws),
        InputMode::Confirm { .. } => handle_confirm_key(state, key, ws),
        InputMode::NodePick { .. } => handle_node_pick_key(state, key, ws),
    }
}

/// Handle keys in normal browsing mode
fn handle_normal_key(state: &mut NavState, key: KeyEvent, ws: &CliWorkspace) {
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
            state.update_selection_from_tree_self();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.tree_state.key_up();
            state.update_selection_from_tree_self();
        }

        // Expand/Collapse or navigate
        KeyCode::Char('l') | KeyCode::Right => {
            state.tree_state.key_right();
            state.update_selection_from_tree_self();
        }
        KeyCode::Char('h') | KeyCode::Left => {
            state.tree_state.key_left();
            state.update_selection_from_tree_self();
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
            for _ in 0..10 {
                state.scroll_preview_down();
            }
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            for _ in 0..10 {
                state.scroll_preview_up();
            }
        }

        // === Workspace commands ===

        // Create child entry
        KeyCode::Char('a') => {
            if state.selected_path.is_some() {
                state.mode = InputMode::TextInput {
                    prompt: "New entry title".to_string(),
                    buffer: String::new(),
                    cursor: 0,
                    action: TextAction::Create,
                };
            }
        }

        // Delete selected entry
        KeyCode::Char('x') => {
            if let Some(title) = &state.selected_title {
                let message = format!("Delete '{}'?", title);
                state.mode = InputMode::Confirm {
                    message,
                    action: ConfirmAction::Delete,
                };
            }
        }

        // Rename selected entry
        KeyCode::Char('r') => {
            if let Some(title) = &state.selected_title {
                let title = title.clone();
                let cursor = title.len();
                state.mode = InputMode::TextInput {
                    prompt: "Rename".to_string(),
                    buffer: title,
                    cursor,
                    action: TextAction::Rename,
                };
            }
        }

        // Duplicate selected entry
        KeyCode::Char('p') => {
            if let Some(path) = state.selected_path.clone() {
                match commands::exec_duplicate(ws, &path) {
                    Ok(new_path) => {
                        state.set_status("Duplicated".to_string(), false);
                        state.rebuild_tree_and_select(ws, new_path);
                    }
                    Err(e) => {
                        state.set_status(e, true);
                    }
                }
            }
        }

        // Move entry (reparent)
        KeyCode::Char('m') => {
            if let Some(path) = state.selected_path.clone() {
                state.mode = InputMode::NodePick {
                    prompt: "Move: select target parent".to_string(),
                    source_path: path,
                    action: PickAction::Move,
                };
            }
        }

        // Combine/merge indices
        KeyCode::Char('M') => {
            if let Some(path) = state.selected_path.clone() {
                state.mode = InputMode::NodePick {
                    prompt: "Merge: select target index".to_string(),
                    source_path: path,
                    action: PickAction::Merge,
                };
            }
        }

        _ => {}
    }
}

/// Handle keys in text input mode (create, rename)
fn handle_text_input_key(state: &mut NavState, key: KeyEvent, ws: &CliWorkspace) {
    // Extract mode fields (we need to take ownership to avoid borrow issues)
    let (prompt, mut buffer, mut cursor, action) = match state.mode.clone() {
        InputMode::TextInput {
            prompt,
            buffer,
            cursor,
            action,
        } => (prompt, buffer, cursor, action),
        _ => return,
    };

    match key.code {
        KeyCode::Esc => {
            state.mode = InputMode::Normal;
        }
        KeyCode::Enter => {
            let trimmed = buffer.trim().to_string();
            if trimmed.is_empty() {
                state.mode = InputMode::Normal;
                return;
            }
            state.mode = InputMode::Normal;

            match action {
                TextAction::Create => {
                    if let Some(parent) = state.selected_path.clone() {
                        match commands::exec_create(ws, &parent, &trimmed) {
                            Ok(new_path) => {
                                state.set_status(format!("Created '{}'", trimmed), false);
                                state.rebuild_tree_and_select(ws, new_path);
                            }
                            Err(e) => {
                                state.set_status(e, true);
                            }
                        }
                    }
                }
                TextAction::Rename => {
                    if let Some(path) = state.selected_path.clone() {
                        match commands::exec_rename(ws, &path, &trimmed) {
                            Ok(new_path) => {
                                state.set_status(format!("Renamed to '{}'", trimmed), false);
                                state.rebuild_tree_and_select(ws, new_path);
                            }
                            Err(e) => {
                                state.set_status(e, true);
                            }
                        }
                    }
                }
            }
        }
        KeyCode::Backspace => {
            if cursor > 0 {
                buffer.remove(cursor - 1);
                cursor -= 1;
            }
            state.mode = InputMode::TextInput {
                prompt,
                buffer,
                cursor,
                action,
            };
        }
        KeyCode::Delete => {
            if cursor < buffer.len() {
                buffer.remove(cursor);
            }
            state.mode = InputMode::TextInput {
                prompt,
                buffer,
                cursor,
                action,
            };
        }
        KeyCode::Left => {
            cursor = cursor.saturating_sub(1);
            state.mode = InputMode::TextInput {
                prompt,
                buffer,
                cursor,
                action,
            };
        }
        KeyCode::Right => {
            if cursor < buffer.len() {
                cursor += 1;
            }
            state.mode = InputMode::TextInput {
                prompt,
                buffer,
                cursor,
                action,
            };
        }
        KeyCode::Home => {
            cursor = 0;
            state.mode = InputMode::TextInput {
                prompt,
                buffer,
                cursor,
                action,
            };
        }
        KeyCode::End => {
            cursor = buffer.len();
            state.mode = InputMode::TextInput {
                prompt,
                buffer,
                cursor,
                action,
            };
        }
        KeyCode::Char(c) => {
            buffer.insert(cursor, c);
            cursor += 1;
            state.mode = InputMode::TextInput {
                prompt,
                buffer,
                cursor,
                action,
            };
        }
        _ => {}
    }
}

/// Handle keys in confirmation mode (delete)
fn handle_confirm_key(state: &mut NavState, key: KeyEvent, ws: &CliWorkspace) {
    let action = match &state.mode {
        InputMode::Confirm { action, .. } => action.clone(),
        _ => return,
    };

    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            state.mode = InputMode::Normal;
            match action {
                ConfirmAction::Delete => {
                    if let Some(path) = state.selected_path.clone() {
                        match commands::exec_delete(ws, &path) {
                            Ok(()) => {
                                state.set_status("Deleted".to_string(), false);
                                state.rebuild_tree(ws);
                            }
                            Err(e) => {
                                state.set_status(e, true);
                            }
                        }
                    }
                }
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            state.mode = InputMode::Normal;
        }
        _ => {}
    }
}

/// Handle keys in node-pick mode (move, merge)
fn handle_node_pick_key(state: &mut NavState, key: KeyEvent, ws: &CliWorkspace) {
    let (source_path, action) = match &state.mode {
        InputMode::NodePick {
            source_path,
            action,
            ..
        } => (source_path.clone(), action.clone()),
        _ => return,
    };

    match key.code {
        KeyCode::Esc => {
            // Cancel: restore selection to source node
            state.mode = InputMode::Normal;
            state.rebuild_tree(ws);
        }

        // Navigation while picking
        KeyCode::Char('j') | KeyCode::Down => {
            state.tree_state.key_down();
            state.update_selection_from_tree_self();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.tree_state.key_up();
            state.update_selection_from_tree_self();
        }
        KeyCode::Char('l') | KeyCode::Right => {
            state.tree_state.key_right();
            state.update_selection_from_tree_self();
        }
        KeyCode::Char('h') | KeyCode::Left => {
            state.tree_state.key_left();
            state.update_selection_from_tree_self();
        }
        KeyCode::Char(' ') | KeyCode::Tab => {
            state.tree_state.toggle_selected();
        }

        // Confirm pick
        KeyCode::Enter => {
            let target = match &state.selected_path {
                Some(p) => p.clone(),
                None => return,
            };

            if target == source_path {
                state.set_status("Cannot target the same node".to_string(), true);
                state.mode = InputMode::Normal;
                return;
            }

            state.mode = InputMode::Normal;

            match action {
                PickAction::Move => match commands::exec_move(ws, &source_path, &target) {
                    Ok(new_path) => {
                        state.set_status("Moved".to_string(), false);
                        state.rebuild_tree_and_select(ws, new_path);
                    }
                    Err(e) => {
                        state.set_status(e, true);
                        state.rebuild_tree(ws);
                    }
                },
                PickAction::Merge => match commands::exec_merge(ws, &source_path, &target) {
                    Ok(()) => {
                        state.set_status("Merged".to_string(), false);
                        state.rebuild_tree_and_select(ws, target);
                    }
                    Err(e) => {
                        state.set_status(e, true);
                        state.rebuild_tree(ws);
                    }
                },
            }
        }

        _ => {}
    }
}
