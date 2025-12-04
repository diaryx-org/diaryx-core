//! Shared utilities for CLI commands

use diaryx_core::config::Config;
use diaryx_core::entry::DiaryxApp;
use diaryx_core::fs::RealFileSystem;
use glob::glob;
use serde_yaml::Value;
use std::io::{self, Write};
use std::path::PathBuf;

/// Result of a confirmation prompt
pub enum ConfirmResult {
    Yes,
    No,
    All,
    Quit,
}

/// Prompt user for confirmation
pub fn prompt_confirm(message: &str) -> ConfirmResult {
    print!("{} [y/n/a/q] ", message);
    io::stdout().flush().unwrap();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return ConfirmResult::Quit;
    }

    match input.trim().to_lowercase().as_str() {
        "y" | "yes" => ConfirmResult::Yes,
        "n" | "no" => ConfirmResult::No,
        "a" | "all" => ConfirmResult::All,
        "q" | "quit" => ConfirmResult::Quit,
        _ => ConfirmResult::No,
    }
}

/// Check if a path pattern contains glob characters
pub fn is_glob_pattern(path: &str) -> bool {
    path.contains('*') || path.contains('?') || path.contains('[')
}

/// Resolve a path pattern to a list of files
/// Returns either a single resolved path (for dates/literals) or multiple paths (for globs)
pub fn resolve_paths(path: &str, config: &Config, app: &DiaryxApp<RealFileSystem>) -> Vec<PathBuf> {
    // First check if it's a glob pattern
    if is_glob_pattern(path) {
        match glob(path) {
            Ok(paths) => {
                let mut result: Vec<PathBuf> = paths
                    .filter_map(|p| p.ok())
                    .filter(|p| p.extension().map_or(false, |ext| ext == "md"))
                    .collect();
                result.sort();
                result
            }
            Err(e) => {
                eprintln!("✗ Invalid glob pattern: {}", e);
                vec![]
            }
        }
    } else {
        // Try to resolve as date or literal path
        vec![app.resolve_path(path, config)]
    }
}

/// Load config or print error message
pub fn load_config() -> Option<Config> {
    match Config::load() {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("✗ Error loading config: {}", e);
            eprintln!("  Run 'diaryx init' first");
            None
        }
    }
}

/// Format a YAML value for display
pub fn format_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Sequence(items) => {
            let items_str: Vec<String> = items.iter().map(|v| format_value(v)).collect();
            format!("[{}]", items_str.join(", "))
        }
        _ => serde_yaml::to_string(value)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}
