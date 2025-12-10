//! CLI handler for search command

use std::path::PathBuf;

use diaryx_core::fs::RealFileSystem;
use diaryx_core::search::{SearchMode, SearchQuery, SearchResults, Searcher};
use diaryx_core::workspace::Workspace;

/// Handle the search command
#[allow(clippy::too_many_arguments)]
pub fn handle_search(
    pattern: String,
    workspace_override: Option<PathBuf>,
    frontmatter: bool,
    property: Option<String>,
    case_sensitive: bool,
    limit: Option<usize>,
    context: usize,
    count_only: bool,
) {
    // Resolve workspace root
    let workspace_root = match resolve_workspace_for_search(workspace_override) {
        Ok(root) => root,
        Err(e) => {
            eprintln!("✗ {}", e);
            return;
        }
    };

    // Build search query
    let query = build_query(&pattern, frontmatter, property.as_deref(), case_sensitive);

    // Execute search
    let fs = RealFileSystem;
    let searcher = Searcher::new(fs);

    let results = match searcher.search_workspace(&workspace_root, &query) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("✗ Search failed: {}", e);
            return;
        }
    };

    // Display results
    if count_only {
        display_count_results(&results);
    } else {
        display_results(&results, limit, context, &pattern, case_sensitive);
    }
}

/// Build a SearchQuery from CLI arguments
fn build_query(
    pattern: &str,
    frontmatter: bool,
    property: Option<&str>,
    case_sensitive: bool,
) -> SearchQuery {
    let mode = if let Some(prop) = property {
        SearchMode::Property(prop.to_string())
    } else if frontmatter {
        SearchMode::Frontmatter
    } else {
        SearchMode::Content
    };

    SearchQuery {
        pattern: pattern.to_string(),
        case_sensitive,
        mode,
    }
}

/// Display results with match context
fn display_results(
    results: &SearchResults,
    limit: Option<usize>,
    context: usize,
    pattern: &str,
    case_sensitive: bool,
) {
    if results.files.is_empty() {
        println!("No matches found.");
        println!("Searched {} files.", results.files_searched);
        return;
    }

    let mut total_shown = 0;
    let max_results = limit.unwrap_or(usize::MAX);

    for file_result in &results.files {
        if total_shown >= max_results {
            break;
        }

        // Display file header
        let display_name = file_result.title.as_deref().unwrap_or_else(|| {
            file_result
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
        });

        println!(
            "\x1b[1;34m{}\x1b[0m: {} ({} match{})",
            file_result.path.display(),
            display_name,
            file_result.match_count(),
            if file_result.match_count() == 1 {
                ""
            } else {
                "es"
            }
        );

        // Display match lines (context support would require re-reading files)
        let _ = context; // Context parameter reserved for future use

        let mut last_line: Option<usize> = None;
        for search_match in &file_result.matches {
            if total_shown >= max_results {
                break;
            }

            // Show separator if there's a gap between matches
            if let Some(last) = last_line {
                if search_match.line_number > last + 1 {
                    println!("  \x1b[90m...\x1b[0m");
                }
            }

            // Highlight and display the match line
            let highlighted =
                highlight_matches(&search_match.line_content, pattern, case_sensitive);
            println!(
                "  \x1b[90m{:>4}:\x1b[0m {}",
                search_match.line_number, highlighted
            );
            total_shown += 1;

            last_line = Some(search_match.line_number);
        }

        println!();
    }

    // Summary
    println!(
        "\x1b[1mFound {} match{} in {} file{}\x1b[0m (searched {} files)",
        results.total_matches(),
        if results.total_matches() == 1 {
            ""
        } else {
            "es"
        },
        results.files_with_matches(),
        if results.files_with_matches() == 1 {
            ""
        } else {
            "s"
        },
        results.files_searched
    );

    if let Some(max) = limit {
        if total_shown >= max && results.total_matches() > max {
            println!("(showing first {} results, use --limit to see more)", max);
        }
    }
}

/// Highlight matches in a line
fn highlight_matches(line: &str, pattern: &str, case_sensitive: bool) -> String {
    let search_line = if case_sensitive {
        line.to_string()
    } else {
        line.to_lowercase()
    };
    let search_pattern = if case_sensitive {
        pattern.to_string()
    } else {
        pattern.to_lowercase()
    };

    // Work with character indices, not byte indices, to handle Unicode properly
    let line_chars: Vec<char> = line.chars().collect();
    let search_chars: Vec<char> = search_line.chars().collect();
    let pattern_chars: Vec<char> = search_pattern.chars().collect();

    let mut result = String::new();
    let mut last_end = 0;
    let mut i = 0;

    while i <= search_chars.len().saturating_sub(pattern_chars.len()) {
        // Check if pattern matches at position i
        let matches = search_chars[i..i + pattern_chars.len()]
            .iter()
            .zip(pattern_chars.iter())
            .all(|(a, b)| a == b);

        if matches {
            // Add text before match
            for c in &line_chars[last_end..i] {
                result.push(*c);
            }
            // Add highlighted match
            result.push_str("\x1b[1;33m");
            for c in &line_chars[i..i + pattern_chars.len()] {
                result.push(*c);
            }
            result.push_str("\x1b[0m");
            last_end = i + pattern_chars.len();
            i = last_end;
        } else {
            i += 1;
        }
    }

    // Add remaining text
    for c in &line_chars[last_end..] {
        result.push(*c);
    }

    result
}

/// Display count-only results
fn display_count_results(results: &SearchResults) {
    if results.files.is_empty() {
        println!("No matches found.");
        println!("Searched {} files.", results.files_searched);
        return;
    }

    for file_result in &results.files {
        println!(
            "{}: {} match{}",
            file_result.path.display(),
            file_result.match_count(),
            if file_result.match_count() == 1 {
                ""
            } else {
                "es"
            }
        );
    }

    println!();
    println!(
        "Total: {} match{} in {} file{}",
        results.total_matches(),
        if results.total_matches() == 1 {
            ""
        } else {
            "es"
        },
        results.files_with_matches(),
        if results.files_with_matches() == 1 {
            ""
        } else {
            "s"
        }
    );
}

/// Resolve the workspace root for search
fn resolve_workspace_for_search(workspace_override: Option<PathBuf>) -> Result<PathBuf, String> {
    let ws = Workspace::new(RealFileSystem);

    // If workspace is explicitly provided, use it
    if let Some(workspace_path) = workspace_override {
        if workspace_path.is_file() {
            return Ok(workspace_path);
        }
        // If it's a directory, find the root index in it
        if let Ok(Some(root)) = ws.find_root_index_in_dir(&workspace_path) {
            return Ok(root);
        }
        return Err(format!(
            "No workspace found at '{}'",
            workspace_path.display()
        ));
    }

    // Try current directory first
    let current_dir =
        std::env::current_dir().map_err(|e| format!("Cannot get current directory: {}", e))?;

    if let Ok(Some(root)) = ws.detect_workspace(&current_dir) {
        return Ok(root);
    }

    // Fall back to config default
    let config =
        diaryx_core::config::Config::load().map_err(|e| format!("Failed to load config: {}", e))?;

    if let Ok(Some(root)) = ws.find_root_index_in_dir(&config.default_workspace) {
        return Ok(root);
    }

    Err("No workspace found. Run 'diaryx init' first or specify --workspace".to_string())
}
