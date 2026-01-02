//! Shared frontmatter parsing and manipulation utilities.
//!
//! This module provides low-level functions for working with YAML frontmatter
//! in markdown files. It extracts common parsing logic used across the codebase.

use indexmap::IndexMap;
use serde_yaml::Value;
use std::path::PathBuf;

use crate::error::{DiaryxError, Result};

/// Result of parsing a markdown file with frontmatter.
#[derive(Debug, Clone)]
pub struct ParsedFile {
    /// The parsed frontmatter as an ordered map.
    pub frontmatter: IndexMap<String, Value>,
    /// The body content after the frontmatter.
    pub body: String,
}

/// Parse frontmatter and body from markdown content.
///
/// Returns `Ok(ParsedFile)` with the frontmatter and body.
/// Returns `Err(NoFrontmatter)` if the content doesn't have valid frontmatter delimiters.
pub fn parse(content: &str) -> Result<ParsedFile> {
    // Check if content starts with frontmatter delimiter
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        return Err(DiaryxError::NoFrontmatter(PathBuf::new()));
    }

    // Find the closing delimiter
    let rest = &content[4..]; // Skip first "---\n"
    let end_idx = rest
        .find("\n---\n")
        .or_else(|| rest.find("\n---\r\n"))
        .ok_or_else(|| DiaryxError::NoFrontmatter(PathBuf::new()))?;

    let frontmatter_str = &rest[..end_idx];
    let body = &rest[end_idx + 5..]; // Skip "\n---\n"

    // Parse YAML frontmatter into IndexMap to preserve order
    let frontmatter: IndexMap<String, Value> = serde_yaml::from_str(frontmatter_str)?;

    Ok(ParsedFile {
        frontmatter,
        body: body.to_string(),
    })
}

/// Parse frontmatter and body, returning empty frontmatter if none exists.
///
/// Unlike `parse()`, this function never returns an error for missing frontmatter.
/// Use this for operations that should work on files without frontmatter.
pub fn parse_or_empty(content: &str) -> Result<ParsedFile> {
    // Check if content starts with frontmatter delimiter
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        // No frontmatter - return empty frontmatter and entire content as body
        return Ok(ParsedFile {
            frontmatter: IndexMap::new(),
            body: content.to_string(),
        });
    }

    // Find the closing delimiter
    let rest = &content[4..]; // Skip first "---\n"
    let end_idx = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n"));

    match end_idx {
        Some(idx) => {
            let frontmatter_str = &rest[..idx];
            let body = &rest[idx + 5..]; // Skip "\n---\n"

            // Parse YAML frontmatter into IndexMap to preserve order
            let frontmatter: IndexMap<String, Value> = serde_yaml::from_str(frontmatter_str)?;

            Ok(ParsedFile {
                frontmatter,
                body: body.to_string(),
            })
        }
        None => {
            // Malformed frontmatter (no closing delimiter) - treat as no frontmatter
            Ok(ParsedFile {
                frontmatter: IndexMap::new(),
                body: content.to_string(),
            })
        }
    }
}

/// Serialize frontmatter and body back to markdown content.
pub fn serialize(frontmatter: &IndexMap<String, Value>, body: &str) -> Result<String> {
    let yaml_str = serde_yaml::to_string(frontmatter)?;
    Ok(format!("---\n{}---\n{}", yaml_str, body))
}

/// Extract only the body from markdown content, stripping frontmatter.
///
/// If no frontmatter exists, returns the content unchanged.
pub fn extract_body(content: &str) -> &str {
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        return content;
    }

    let rest = &content[4..];
    if let Some(end_idx) = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n")) {
        // Skip past the closing delimiter
        let body_start = end_idx + 5;
        if body_start < rest.len() {
            &rest[body_start..]
        } else {
            ""
        }
    } else {
        content
    }
}

/// Get a property from frontmatter.
pub fn get_property<'a>(frontmatter: &'a IndexMap<String, Value>, key: &str) -> Option<&'a Value> {
    frontmatter.get(key)
}

/// Set a property in frontmatter (in place).
pub fn set_property(frontmatter: &mut IndexMap<String, Value>, key: &str, value: Value) {
    frontmatter.insert(key.to_string(), value);
}

/// Remove a property from frontmatter (in place).
pub fn remove_property(frontmatter: &mut IndexMap<String, Value>, key: &str) -> Option<Value> {
    frontmatter.shift_remove(key)
}

/// Get a string property value.
pub fn get_string<'a>(frontmatter: &'a IndexMap<String, Value>, key: &str) -> Option<&'a str> {
    frontmatter.get(key).and_then(|v| v.as_str())
}

/// Get an array property as a Vec of strings.
pub fn get_string_array(frontmatter: &IndexMap<String, Value>, key: &str) -> Vec<String> {
    match frontmatter.get(key) {
        Some(Value::Sequence(seq)) => seq
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

/// Sort frontmatter keys alphabetically.
pub fn sort_alphabetically(frontmatter: IndexMap<String, Value>) -> IndexMap<String, Value> {
    let mut pairs: Vec<_> = frontmatter.into_iter().collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs.into_iter().collect()
}

/// Sort frontmatter keys according to a pattern.
///
/// Pattern is comma-separated keys, with "*" meaning "rest alphabetically".
/// Example: "title,description,*" puts title first, description second, rest alphabetically
pub fn sort_by_pattern(
    frontmatter: IndexMap<String, Value>,
    pattern: &str,
) -> IndexMap<String, Value> {
    let priority_keys: Vec<&str> = pattern.split(',').map(|s| s.trim()).collect();

    let mut result = IndexMap::new();
    let mut remaining = frontmatter;

    for key in &priority_keys {
        if *key == "*" {
            // Insert remaining keys alphabetically
            let mut rest: Vec<_> = remaining.drain(..).collect();
            rest.sort_by(|a, b| a.0.cmp(&b.0));
            for (k, v) in rest {
                result.insert(k, v);
            }
            break;
        } else if let Some(value) = remaining.shift_remove(*key) {
            result.insert(key.to_string(), value);
        }
    }

    // If no "*" was in pattern, append any remaining keys alphabetically.
    if !remaining.is_empty() {
        let mut rest: Vec<_> = remaining.drain(..).collect();
        rest.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in rest {
            result.insert(k, v);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_frontmatter() {
        let content = "---\ntitle: Test\n---\n\nBody content";
        let parsed = parse(content).unwrap();
        assert_eq!(parsed.frontmatter.get("title").unwrap().as_str().unwrap(), "Test");
        assert_eq!(parsed.body.trim(), "Body content");
    }

    #[test]
    fn test_parse_no_frontmatter() {
        let content = "Just body content";
        let result = parse(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_or_empty_no_frontmatter() {
        let content = "Just body content";
        let parsed = parse_or_empty(content).unwrap();
        assert!(parsed.frontmatter.is_empty());
        assert_eq!(parsed.body, content);
    }

    #[test]
    fn test_serialize() {
        let mut fm = IndexMap::new();
        fm.insert("title".to_string(), Value::String("Test".to_string()));
        let result = serialize(&fm, "\nBody").unwrap();
        assert!(result.starts_with("---\n"));
        assert!(result.contains("title: Test"));
        assert!(result.contains("---\n\nBody"));
    }

    #[test]
    fn test_extract_body() {
        let content = "---\ntitle: Test\n---\n\nBody content";
        assert_eq!(extract_body(content).trim(), "Body content");
    }

    #[test]
    fn test_extract_body_no_frontmatter() {
        let content = "Just body content";
        assert_eq!(extract_body(content), content);
    }

    #[test]
    fn test_sort_alphabetically() {
        let mut fm = IndexMap::new();
        fm.insert("zebra".to_string(), Value::Null);
        fm.insert("apple".to_string(), Value::Null);
        fm.insert("banana".to_string(), Value::Null);

        let sorted = sort_alphabetically(fm);
        let keys: Vec<_> = sorted.keys().collect();
        assert_eq!(keys, vec!["apple", "banana", "zebra"]);
    }

    #[test]
    fn test_sort_by_pattern() {
        let mut fm = IndexMap::new();
        fm.insert("zebra".to_string(), Value::Null);
        fm.insert("title".to_string(), Value::Null);
        fm.insert("apple".to_string(), Value::Null);

        let sorted = sort_by_pattern(fm, "title,*");
        let keys: Vec<_> = sorted.keys().collect();
        assert_eq!(keys, vec!["title", "apple", "zebra"]);
    }
}
