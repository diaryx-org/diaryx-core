//! Normalize filename command - renames files to match their title property

use serde_yaml::Value;
use std::path::PathBuf;

use crate::cli::util::{
    ConfirmResult, load_config, prompt_confirm, rename_file_with_refs, resolve_paths,
};
use crate::cli::{CliDiaryxAppSync, block_on};

/// Convert a filename (without extension) to a human-readable title
/// Replaces underscores and hyphens with spaces, applies title case
fn filename_to_title(filename: &str) -> String {
    filename
        .replace(['_', '-'], " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Convert a title string to a snake_case slug suitable for filenames
pub fn slugify(title: &str) -> String {
    let mut result = String::new();
    let mut last_was_underscore = true; // Start true to avoid leading underscore

    for c in title.chars() {
        if c.is_alphanumeric() {
            result.push(c.to_lowercase().next().unwrap_or(c));
            last_was_underscore = false;
        } else if !last_was_underscore {
            // Replace non-alphanumeric with underscore, but avoid consecutive underscores
            result.push('_');
            last_was_underscore = true;
        }
    }

    // Remove trailing underscore
    if result.ends_with('_') {
        result.pop();
    }

    result
}

/// Handle the normalize-filename command
pub fn handle_normalize_filename(
    app: &CliDiaryxAppSync,
    path: &str,
    new_title: Option<String>,
    yes: bool,
    dry_run: bool,
) {
    let config = match load_config() {
        Some(c) => c,
        None => return,
    };

    let paths = resolve_paths(path, &config, app);

    if paths.is_empty() {
        eprintln!("✗ No files matched: {}", path);
        return;
    }

    let multiple = paths.len() > 1;
    let mut confirm_all = yes;

    for file_path in &paths {
        if !file_path.exists() {
            eprintln!("✗ File does not exist: {}", file_path.display());
            continue;
        }

        // Get current title, use new one if provided, or derive from filename
        let path_str = file_path.to_string_lossy();
        let (title, needs_title_set) = if let Some(ref t) = new_title {
            (t.clone(), true)
        } else {
            // Read title from frontmatter
            match app.get_frontmatter_property(&path_str, "title") {
                Ok(Some(Value::String(t))) => (t, false),
                Ok(Some(_)) => {
                    eprintln!(
                        "✗ Title property is not a string in '{}'",
                        file_path.display()
                    );
                    continue;
                }
                Ok(None) => {
                    // No title property - derive from filename
                    let derived = file_path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .map(filename_to_title)
                        .unwrap_or_default();
                    if derived.is_empty() {
                        eprintln!(
                            "✗ Could not derive title from filename '{}'",
                            file_path.display()
                        );
                        continue;
                    }
                    (derived, true)
                }
                Err(e) => {
                    eprintln!(
                        "✗ Error reading frontmatter from '{}': {}",
                        file_path.display(),
                        e
                    );
                    continue;
                }
            }
        };

        let slug = slugify(&title);
        let new_filename = format!("{}.md", slug);

        // Calculate the new path
        let new_path = file_path
            .parent()
            .map(|p| p.join(&new_filename))
            .unwrap_or_else(|| PathBuf::from(&new_filename));

        // Check if already normalized
        if file_path == &new_path {
            if !dry_run {
                println!("✓ '{}' already normalized", file_path.display());
            }
            continue;
        }

        // Check if destination exists
        if new_path.exists() {
            eprintln!(
                "✗ Cannot rename '{}' to '{}': destination already exists",
                file_path.display(),
                new_path.display()
            );
            continue;
        }

        // Confirm if multiple files and not auto-confirming
        if multiple && !confirm_all && !dry_run {
            println!(
                "Rename '{}' -> '{}'?",
                file_path.display(),
                new_path.display()
            );
            match prompt_confirm("Proceed?") {
                ConfirmResult::Yes => {}
                ConfirmResult::No => {
                    println!("Skipped");
                    continue;
                }
                ConfirmResult::All => {
                    confirm_all = true;
                }
                ConfirmResult::Quit => {
                    println!("Aborted");
                    return;
                }
            }
        }

        if dry_run {
            println!(
                "Would rename '{}' -> '{}'",
                file_path.display(),
                new_path.display()
            );
            if needs_title_set {
                println!("  Would set title: '{}'", title);
            }
            continue;
        }

        // Set the title if needed (new title provided or derived from filename)
        if needs_title_set {
            if let Err(e) =
                app.set_frontmatter_property(&path_str, "title", Value::String(title.clone()))
            {
                eprintln!("✗ Error setting title in '{}': {}", file_path.display(), e);
                continue;
            }
            println!("✓ Set title in '{}': '{}'", file_path.display(), title);
        }

        // Perform the rename using shared utility that updates all workspace references
        rename_file_with_refs(app, file_path, &new_path, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify_simple() {
        assert_eq!(slugify("Hello World"), "hello_world");
    }

    #[test]
    fn test_slugify_special_chars() {
        assert_eq!(slugify("Hello, World!"), "hello_world");
    }

    #[test]
    fn test_slugify_multiple_spaces() {
        assert_eq!(slugify("Hello   World"), "hello_world");
    }

    #[test]
    fn test_slugify_leading_trailing() {
        assert_eq!(slugify("  Hello World  "), "hello_world");
    }

    #[test]
    fn test_slugify_numbers() {
        assert_eq!(slugify("Chapter 1: Introduction"), "chapter_1_introduction");
    }

    #[test]
    fn test_slugify_mixed_case() {
        assert_eq!(slugify("MyProjectIdeas"), "myprojectideas");
    }

    #[test]
    fn test_slugify_apostrophe() {
        assert_eq!(slugify("Adam's Notes"), "adam_s_notes");
    }

    #[test]
    fn test_slugify_empty() {
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn test_slugify_only_special() {
        assert_eq!(slugify("!@#$%"), "");
    }

    #[test]
    fn test_filename_to_title_underscores() {
        assert_eq!(filename_to_title("my_project_ideas"), "My Project Ideas");
    }

    #[test]
    fn test_filename_to_title_hyphens() {
        assert_eq!(filename_to_title("my-project-ideas"), "My Project Ideas");
    }

    #[test]
    fn test_filename_to_title_mixed() {
        assert_eq!(filename_to_title("my_project-ideas"), "My Project Ideas");
    }

    #[test]
    fn test_filename_to_title_already_spaced() {
        assert_eq!(filename_to_title("My Project Ideas"), "My Project Ideas");
    }
}
