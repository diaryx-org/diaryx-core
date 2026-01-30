//! Sort command handler

use crate::cli::CliDiaryxAppSync;
use crate::cli::util::{ConfirmResult, load_config, prompt_confirm, resolve_paths};

/// Handle the sort command
/// Returns true on success, false on error
pub fn handle_sort_command(
    app: &CliDiaryxAppSync,
    path: String,
    pattern: Option<String>,
    default: bool,
    index: bool,
    yes: bool,
    dry_run: bool,
) -> bool {
    let config = match load_config() {
        Some(c) => c,
        None => return false,
    };

    // Determine sort pattern from flags or custom pattern
    let sort_pattern: Option<&str> = if let Some(ref p) = pattern {
        Some(p.as_str())
    } else if default {
        Some("title,description,author,date,tags,*")
    } else if index {
        Some("title,description,part_of,contents,*")
    } else {
        // abc is default (alphabetical), which is None
        None
    };

    let paths = resolve_paths(&path, &config, app);

    if paths.is_empty() {
        eprintln!("✗ No files matched pattern: {}", path);
        return false;
    }

    let multiple_files = paths.len() > 1;
    let mut skip_confirm = yes || !multiple_files;
    let mut had_error = false;

    for file_path in paths {
        let path_str = file_path.to_string_lossy();
        let display_path = file_path.display();

        // Describe the operation
        let desc = match sort_pattern {
            Some(p) => format!("Sort with pattern '{}'", p),
            None => "Sort alphabetically".to_string(),
        };

        // Handle confirmation for multiple files
        if !skip_confirm && !dry_run {
            let msg = format!("{} in {}?", desc, display_path);
            match prompt_confirm(&msg) {
                ConfirmResult::Yes => {}
                ConfirmResult::No => continue,
                ConfirmResult::All => skip_confirm = true,
                ConfirmResult::Quit => return !had_error,
            }
        }

        // Handle dry-run
        if dry_run {
            println!("Would {} in {}", desc.to_lowercase(), display_path);
            continue;
        }

        // Prefix output with filename for multiple files
        let prefix = if multiple_files {
            format!("{}: ", display_path)
        } else {
            String::new()
        };

        match app.sort_frontmatter(&path_str, sort_pattern) {
            Ok(_) => match sort_pattern {
                Some(p) => println!("{}✓ Sorted with pattern '{}'", prefix, p),
                None => println!("{}✓ Sorted alphabetically", prefix),
            },
            Err(e) => {
                eprintln!("{}✗ Error: {}", prefix, e);
                had_error = true;
            }
        }
    }

    !had_error
}
