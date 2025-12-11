// src-tauri/src/main.rs

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Note: Do NOT add 'mod commands;' here.
// The library (diaryx_lib) now owns that module.

fn main() {
    // Call the run function from your library
    diaryx_lib::run();
}
