//! Main event loop for the navigation TUI

use std::io;
use std::path::Path;
use std::time::Duration;

use crossterm::event::{self, Event};
use ratatui::DefaultTerminal;

use super::keys::handle_key;
use super::state::NavState;
use super::ui::render;
use diaryx_core::config::Config;
use diaryx_core::workspace::TreeNode;

use crate::editor::launch_editor;

/// Run the navigation TUI event loop
pub fn run(
    terminal: &mut DefaultTerminal,
    state: &mut NavState,
    tree: &TreeNode,
    config: &Config,
) -> io::Result<()> {
    // Initial preview load
    state.update_preview();

    loop {
        // Draw UI
        terminal.draw(|frame| render(frame, state))?;

        // Check for quit
        if state.should_quit {
            break;
        }

        // Check for pending editor action
        if let Some(path) = state.pending_edit.take() {
            open_in_editor(terminal, &path, config)?;
            // After editor closes, refresh preview in case file was modified
            state.update_preview();
            continue;
        }

        // Handle events (with timeout for responsiveness)
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                handle_key(state, key, tree);
            }
        }
    }

    Ok(())
}

/// Suspend TUI, open file in editor, then resume TUI
fn open_in_editor(terminal: &mut DefaultTerminal, path: &Path, config: &Config) -> io::Result<()> {
    // Restore terminal to normal mode
    ratatui::restore();

    // Launch editor (this blocks until editor closes)
    if let Err(e) = launch_editor(path, config) {
        eprintln!("Error opening editor: {}", e);
    }

    // Re-initialize terminal for TUI
    *terminal = ratatui::init();

    Ok(())
}
