//! Utility functions for date parsing and path calculations.
//!
//! This module consolidates various utility functions used across the crate.

/// Date parsing and path generation utilities.
pub mod date;
/// Path calculation utilities for relative paths.
pub mod path;

// Re-export commonly used items for convenience
pub use date::{date_to_path, parse_date, path_to_date};
pub use path::{relative_path_from_dir_to_target, relative_path_from_file_to_target};

/// Simple glob pattern matching for exclude patterns.
/// Supports:
/// - `*` matches any sequence of non-separator characters
/// - `**` matches any sequence of characters including separators (recursive)
/// - Patterns without wildcards match exact filenames
///
/// Examples:
/// - `*.lock` matches `Cargo.lock`, `package-lock.json`
/// - `*.toml` matches `Cargo.toml`, `release.toml`
/// - `build/*` matches `build/output.js` but not `build/sub/file.js`
/// - `build/**` matches `build/output.js` and `build/sub/file.js`
pub fn matches_glob_pattern(pattern: &str, path: &str) -> bool {
    // Normalize path separators for consistent matching
    let pattern = pattern.replace('\\', "/");
    let path = path.replace('\\', "/");

    matches_glob_recursive(&pattern, &path)
}

fn matches_glob_recursive(pattern: &str, text: &str) -> bool {
    // Handle ** pattern for recursive matching
    if pattern.contains("**") {
        // Split on ** and handle the recursive case
        let parts: Vec<&str> = pattern.splitn(2, "**").collect();
        if parts.len() == 2 {
            let prefix = parts[0];
            let suffix = parts[1].trim_start_matches('/');

            // Check if prefix matches
            if !prefix.is_empty() && !text.starts_with(prefix.trim_end_matches('/')) {
                return false;
            }

            // For **, try matching suffix at any position
            let remaining = if prefix.is_empty() {
                text
            } else {
                let prefix_trimmed = prefix.trim_end_matches('/');
                if text.len() > prefix_trimmed.len() {
                    text[prefix_trimmed.len()..].trim_start_matches('/')
                } else {
                    return suffix.is_empty();
                }
            };

            if suffix.is_empty() {
                return true;
            }

            // Try matching the suffix at every possible position
            for i in 0..=remaining.len() {
                if matches_glob_recursive(suffix, &remaining[i..]) {
                    return true;
                }
            }
            return false;
        }
    }

    let pattern_bytes = pattern.as_bytes();
    let text_bytes = text.as_bytes();

    let mut pi = 0; // pattern index
    let mut ti = 0; // text index
    let mut star_pi = None; // position after last * in pattern
    let mut star_ti = None; // position in text when * was matched

    while ti < text_bytes.len() {
        if pi < pattern_bytes.len() && pattern_bytes[pi] == b'*' {
            // Found *, remember position
            star_pi = Some(pi + 1);
            star_ti = Some(ti);
            pi += 1;
        } else if pi < pattern_bytes.len()
            && (pattern_bytes[pi] == text_bytes[ti] || pattern_bytes[pi] == b'?')
        {
            // Characters match or pattern has ?
            pi += 1;
            ti += 1;
        } else if let (Some(sp), Some(st)) = (star_pi, star_ti) {
            // Backtrack: * should match one more character
            // But * should NOT match path separator /
            if text_bytes[st] == b'/' {
                return false;
            }
            pi = sp;
            star_ti = Some(st + 1);
            ti = st + 1;
        } else {
            return false;
        }
    }

    // Check remaining pattern characters (should all be *)
    while pi < pattern_bytes.len() && pattern_bytes[pi] == b'*' {
        pi += 1;
    }

    pi == pattern_bytes.len()
}

#[cfg(test)]
mod glob_tests {
    use super::*;

    #[test]
    fn test_simple_extension_patterns() {
        assert!(matches_glob_pattern("*.lock", "Cargo.lock"));
        assert!(matches_glob_pattern("*.lock", "package-lock.json") == false);
        assert!(matches_glob_pattern("*.toml", "Cargo.toml"));
        assert!(matches_glob_pattern("*.toml", "release.toml"));
        assert!(matches_glob_pattern("*.md", "README.md"));
        assert!(!matches_glob_pattern("*.md", "file.txt"));
    }

    #[test]
    fn test_exact_match() {
        assert!(matches_glob_pattern("Cargo.lock", "Cargo.lock"));
        assert!(!matches_glob_pattern("Cargo.lock", "cargo.lock"));
        assert!(!matches_glob_pattern("Cargo.lock", "Cargo.toml"));
    }

    #[test]
    fn test_directory_patterns() {
        assert!(matches_glob_pattern("build/*", "build/output.js"));
        assert!(!matches_glob_pattern("build/*", "build/sub/file.js"));
        assert!(matches_glob_pattern("build/**", "build/output.js"));
        assert!(matches_glob_pattern("build/**", "build/sub/file.js"));
    }

    #[test]
    fn test_star_at_start() {
        assert!(matches_glob_pattern("*lock*", "Cargo.lock"));
        assert!(matches_glob_pattern("*lock*", "package-lock.json"));
    }
}
