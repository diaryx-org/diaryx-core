//! Terminal progress indicator using OSC 9;4 escape sequences.
//!
//! This provides native progress bar support in terminals that support it
//! (like Ghostty, iTerm2, Windows Terminal). In unsupported terminals,
//! the escape sequences are simply ignored.
//!
//! Reference: https://martinemde.com/blog/ghostty-progress-bars.txt

#![allow(dead_code)]

use std::io::{self, Write};

/// Progress bar state values
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressState {
    /// Hide/clear the progress bar
    Hidden = 0,
    /// Normal progress display
    Normal = 1,
    /// Error state (typically red)
    Error = 2,
    /// Indeterminate/loading state (spinner)
    Indeterminate = 3,
    /// Warning state
    Warning = 4,
}

/// Set the terminal progress indicator.
///
/// - `state`: The visual state of the progress bar
/// - `progress`: Progress percentage (0-100), ignored for Indeterminate state
pub fn set_progress(state: ProgressState, progress: u8) {
    let progress = progress.min(100);
    let state_num = state as u8;

    // OSC 9;4 sequence: ESC ] 9 ; 4 ; <state> ; <progress> BEL
    let seq = if state == ProgressState::Indeterminate {
        format!("\x1b]9;4;{}\x07", state_num)
    } else {
        format!("\x1b]9;4;{};{}\x07", state_num, progress)
    };

    // Write directly to stderr (doesn't interfere with stdout)
    let _ = io::stderr().write_all(seq.as_bytes());
    let _ = io::stderr().flush();
}

/// Show indeterminate progress (loading spinner).
pub fn show_indeterminate() {
    set_progress(ProgressState::Indeterminate, 0);
}

/// Show normal progress percentage.
pub fn show_progress(percent: u8) {
    set_progress(ProgressState::Normal, percent);
}

/// Show error state with progress percentage.
pub fn show_error(percent: u8) {
    set_progress(ProgressState::Error, percent);
}

/// Show warning state with progress percentage.
pub fn show_warning(percent: u8) {
    set_progress(ProgressState::Warning, percent);
}

/// Hide/clear the progress indicator.
pub fn hide() {
    set_progress(ProgressState::Hidden, 0);
}

/// A guard that clears the progress bar when dropped.
/// Useful for ensuring cleanup on early returns or panics.
pub struct ProgressGuard;

impl ProgressGuard {
    pub fn new() -> Self {
        Self
    }
}

impl Drop for ProgressGuard {
    fn drop(&mut self) {
        hide();
    }
}

impl Default for ProgressGuard {
    fn default() -> Self {
        Self::new()
    }
}
