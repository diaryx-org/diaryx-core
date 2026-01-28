//! Metadata-to-frontmatter conversion and file writing utilities.
//!
//! This module provides functions to convert `FileMetadata` (from CRDT sync)
//! into YAML frontmatter format and write files with proper structure.
//!
//! # Link Formats
//!
//! When writing `part_of` and `contents` to frontmatter, this module creates
//! portable markdown links in the format: `[Title](/workspace/path.md)`
//!
//! These links are:
//! - **Clickable** in editors like Obsidian
//! - **Unambiguous** with workspace-root paths
//! - **Self-documenting** with human-readable titles

use std::collections::HashMap;
use std::path::Path;

use crate::error::{DiaryxError, Result};
use crate::frontmatter;
use crate::fs::AsyncFileSystem;
use crate::link_parser;

/// Metadata structure for file frontmatter.
/// This mirrors the CRDT FileMetadata but with simpler types for serialization.
///
/// When serialized to YAML, `part_of` and `contents` are formatted as markdown links
/// with workspace-root paths: `[Title](/path/to/file.md)`
#[derive(Debug, Clone, Default)]
pub struct FrontmatterMetadata {
    /// Display title from frontmatter
    pub title: Option<String>,
    /// Markdown link to parent index file (e.g., `[Parent](/folder/index.md)`)
    pub part_of: Option<String>,
    /// Markdown links to child files (e.g., `[Child](/folder/child.md)`)
    pub contents: Option<Vec<String>>,
    /// Binary attachment paths
    pub attachments: Option<Vec<String>>,
    /// Visibility/access control tags
    pub audience: Option<Vec<String>>,
    /// File description
    pub description: Option<String>,
    /// Additional frontmatter properties (excluding internal keys like _body)
    pub extra: HashMap<String, serde_json::Value>,
}

impl FrontmatterMetadata {
    /// Parse from a JSON value (typically from CRDT FileMetadata).
    ///
    /// Note: This basic version doesn't convert paths. For writing files to disk
    /// with proper markdown links, use `from_json_with_markdown_links`.
    pub fn from_json(value: &serde_json::Value) -> Self {
        Self::from_json_with_file_path(value, None)
    }

    /// Parse from a JSON value with the canonical file path.
    ///
    /// This method formats `part_of` and `contents` as markdown links with
    /// workspace-root paths: `[Title](/path/to/file.md)`
    ///
    /// # Arguments
    /// * `value` - The JSON value containing FileMetadata
    /// * `canonical_file_path` - The canonical path of the file being written (e.g., "folder/index.md")
    pub fn from_json_with_file_path(
        value: &serde_json::Value,
        _canonical_file_path: Option<&str>,
    ) -> Self {
        // Use the markdown links version with a path_to_title fallback for titles
        Self::from_json_with_markdown_links(value, link_parser::path_to_title)
    }

    /// Parse from a JSON value, formatting links with titles from a resolver function.
    ///
    /// This method formats `part_of` and `contents` as markdown links with
    /// workspace-root paths: `[Title](/path/to/file.md)`
    ///
    /// # Arguments
    /// * `value` - The JSON value containing FileMetadata
    /// * `title_resolver` - A function that returns a display title for a canonical path
    ///
    /// # Example
    /// ```ignore
    /// let metadata = FrontmatterMetadata::from_json_with_markdown_links(
    ///     &json_value,
    ///     |path| crdt.get_file(path).and_then(|m| m.title).unwrap_or_else(|| path_to_title(path))
    /// );
    /// ```
    pub fn from_json_with_markdown_links<F>(value: &serde_json::Value, title_resolver: F) -> Self
    where
        F: Fn(&str) -> String,
    {
        let obj = value.as_object();

        let title = obj
            .and_then(|o| o.get("title"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let part_of = obj
            .and_then(|o| o.get("part_of"))
            .and_then(|v| v.as_str())
            .map(|raw_value| {
                // Parse the incoming value - handles both plain paths and markdown links
                // This prevents double-wrapping if the value is already formatted
                let parsed = link_parser::parse_link(raw_value);
                let canonical_path = &parsed.path;
                // Use existing title from markdown link if available, otherwise resolve
                let link_title = parsed
                    .title
                    .clone()
                    .unwrap_or_else(|| title_resolver(canonical_path));
                link_parser::format_link(canonical_path, &link_title)
            });

        let contents = obj
            .and_then(|o| o.get("contents"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|raw_value| {
                        // Parse the incoming value - handles both plain paths and markdown links
                        // This prevents double-wrapping if the value is already formatted
                        let parsed = link_parser::parse_link(raw_value);
                        let canonical_path = &parsed.path;
                        // Use existing title from markdown link if available, otherwise resolve
                        let link_title = parsed
                            .title
                            .clone()
                            .unwrap_or_else(|| title_resolver(canonical_path));
                        link_parser::format_link(canonical_path, &link_title)
                    })
                    .collect()
            });

        let attachments = obj
            .and_then(|o| o.get("attachments"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        // Handle both string and object (BinaryRef) formats
                        if let Some(s) = v.as_str() {
                            Some(s.to_string())
                        } else if let Some(obj) = v.as_object() {
                            obj.get("path").and_then(|p| p.as_str()).map(String::from)
                        } else {
                            None
                        }
                    })
                    .collect()
            });

        let audience = obj
            .and_then(|o| o.get("audience"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });

        let description = obj
            .and_then(|o| o.get("description"))
            .and_then(|v| v.as_str())
            .map(String::from);

        // Extract extra properties, excluding internal keys
        let mut extra = HashMap::new();
        if let Some(extra_obj) = obj.and_then(|o| o.get("extra")).and_then(|v| v.as_object()) {
            for (key, value) in extra_obj {
                // Skip internal keys (starting with _)
                if !key.starts_with('_') {
                    extra.insert(key.clone(), value.clone());
                }
            }
        }

        Self {
            title,
            part_of,
            contents,
            attachments,
            audience,
            description,
            extra,
        }
    }

    /// Convert to YAML frontmatter string.
    pub fn to_yaml(&self) -> String {
        let mut lines: Vec<String> = Vec::new();

        if let Some(title) = &self.title {
            lines.push(format!("title: {}", yaml_string(title)));
        }

        if let Some(part_of) = &self.part_of {
            lines.push(format!("part_of: {}", yaml_string(part_of)));
        }

        if let Some(contents) = &self.contents {
            if contents.is_empty() {
                // Write empty array explicitly to preserve index file identity
                lines.push("contents: []".to_string());
            } else {
                lines.push("contents:".to_string());
                for item in contents {
                    lines.push(format!("  - {}", yaml_string(item)));
                }
            }
        }

        if let Some(audience) = &self.audience
            && !audience.is_empty()
        {
            lines.push("audience:".to_string());
            for item in audience {
                lines.push(format!("  - {}", yaml_string(item)));
            }
        }

        if let Some(description) = &self.description {
            lines.push(format!("description: {}", yaml_string(description)));
        }

        if let Some(attachments) = &self.attachments
            && !attachments.is_empty()
        {
            lines.push("attachments:".to_string());
            for item in attachments {
                lines.push(format!("  - {}", yaml_string(item)));
            }
        }

        // Add extra properties
        for (key, value) in &self.extra {
            lines.push(format!("{}: {}", key, yaml_value(value)));
        }

        lines.join("\n")
    }
}

/// Format a string for YAML (quote if necessary).
fn yaml_string(value: &str) -> String {
    // Check if the string needs quoting
    if value.is_empty()
        || value.contains(':')
        || value.contains('#')
        || value.contains('[')
        || value.contains(']')
        || value.contains('{')
        || value.contains('}')
        || value.contains('|')
        || value.contains('>')
        || value.contains('&')
        || value.contains('*')
        || value.contains('!')
        || value.contains('?')
        || value.contains('\'')
        || value.contains('"')
        || value.contains('%')
        || value.contains('@')
        || value.contains('`')
        || value.contains('\n')
        || value.starts_with(' ')
        || value.ends_with(' ')
        || looks_like_number(value)
        || is_yaml_keyword(value)
    {
        // Use double quotes and escape internal quotes
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

/// Check if a string looks like a number.
fn looks_like_number(s: &str) -> bool {
    s.parse::<f64>().is_ok()
}

/// Check if a string is a YAML keyword.
fn is_yaml_keyword(s: &str) -> bool {
    matches!(
        s.to_lowercase().as_str(),
        "true" | "false" | "null" | "yes" | "no" | "on" | "off"
    )
}

/// Format a JSON value for YAML.
fn yaml_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => yaml_string(s),
        serde_json::Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(yaml_value).collect();
            format!("[{}]", items.join(", "))
        }
        serde_json::Value::Object(_) => {
            // For objects, use JSON format as YAML flow style
            serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
        }
    }
}

/// Write a file with metadata as YAML frontmatter and body content.
///
/// Note: This function doesn't convert canonical paths to relative paths.
/// For proper path conversion, use `write_file_with_metadata_and_canonical_path`.
pub async fn write_file_with_metadata<FS: AsyncFileSystem>(
    fs: &FS,
    path: &Path,
    metadata: &serde_json::Value,
    body: &str,
) -> Result<()> {
    write_file_with_metadata_and_canonical_path(fs, path, metadata, body, None).await
}

/// Write a file with metadata as YAML frontmatter and body content.
///
/// When `canonical_path` is provided, `contents` and `part_of` paths in the metadata
/// are converted from canonical (workspace-relative) paths to file-relative paths.
///
/// # Arguments
/// * `fs` - The filesystem to write to
/// * `path` - The storage path to write the file to
/// * `metadata` - The JSON metadata (typically from CRDT FileMetadata)
/// * `body` - The body content of the file
/// * `canonical_path` - The canonical path of the file (e.g., "folder/index.md") for path conversion
pub async fn write_file_with_metadata_and_canonical_path<FS: AsyncFileSystem>(
    fs: &FS,
    path: &Path,
    metadata: &serde_json::Value,
    body: &str,
    canonical_path: Option<&str>,
) -> Result<()> {
    let fm = FrontmatterMetadata::from_json_with_file_path(metadata, canonical_path);
    let yaml = fm.to_yaml();

    // Combine frontmatter and body
    let content = if yaml.is_empty() {
        body.to_string()
    } else {
        format!("---\n{}\n---\n\n{}", yaml, body)
    };

    // Ensure parent directory exists
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs.create_dir_all(parent).await?;
    }

    fs.write_file(path, &content)
        .await
        .map_err(|e| DiaryxError::FileWrite {
            path: path.to_path_buf(),
            source: e,
        })?;

    Ok(())
}

/// Update a file's frontmatter metadata, preserving or replacing the body.
///
/// If `new_body` is `Some`, it replaces the existing body.
/// If `new_body` is `None`, the existing body is preserved.
pub async fn update_file_metadata<FS: AsyncFileSystem>(
    fs: &FS,
    path: &Path,
    metadata: &serde_json::Value,
    new_body: Option<&str>,
) -> Result<()> {
    // Determine the body content
    let body = if let Some(b) = new_body {
        b.to_string()
    } else {
        // Read existing body from file
        let content = fs
            .read_to_string(path)
            .await
            .map_err(|e| DiaryxError::FileRead {
                path: path.to_path_buf(),
                source: e,
            })?;

        let parsed = frontmatter::parse_or_empty(&content)?;
        parsed.body
    };

    write_file_with_metadata(fs, path, metadata, &body).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yaml_string_simple() {
        assert_eq!(yaml_string("hello"), "hello");
        assert_eq!(yaml_string("hello world"), "hello world");
    }

    #[test]
    fn test_yaml_string_needs_quoting() {
        assert_eq!(yaml_string("hello: world"), "\"hello: world\"");
        assert_eq!(yaml_string("has #comment"), "\"has #comment\"");
        assert_eq!(yaml_string("true"), "\"true\"");
        assert_eq!(yaml_string("123"), "\"123\"");
        assert_eq!(yaml_string(" leading space"), "\" leading space\"");
    }

    #[test]
    fn test_yaml_string_escaping() {
        assert_eq!(yaml_string("has \"quotes\""), "\"has \\\"quotes\\\"\"");
    }

    #[test]
    fn test_frontmatter_metadata_from_json() {
        let json = serde_json::json!({
            "title": "Test Title",
            "part_of": "workspace/parent.md",
            "contents": ["child1.md", "child2.md"],
            "description": "A test file",
            "extra": {
                "custom_key": "custom_value",
                "_body": "should be excluded"
            }
        });

        let fm = FrontmatterMetadata::from_json(&json);
        assert_eq!(fm.title, Some("Test Title".to_string()));
        // Now formatted as markdown link with workspace-root path
        assert_eq!(
            fm.part_of,
            Some("[Parent](/workspace/parent.md)".to_string())
        );
        assert_eq!(
            fm.contents,
            Some(vec![
                "[Child1](/child1.md)".to_string(),
                "[Child2](/child2.md)".to_string()
            ])
        );
        assert_eq!(fm.description, Some("A test file".to_string()));
        assert!(fm.extra.contains_key("custom_key"));
        assert!(!fm.extra.contains_key("_body")); // Internal key excluded
    }

    #[test]
    fn test_frontmatter_metadata_to_yaml() {
        let fm = FrontmatterMetadata {
            title: Some("Test Title".to_string()),
            part_of: Some("[Parent Index](/folder/parent.md)".to_string()),
            contents: Some(vec!["[Child](/folder/child.md)".to_string()]),
            audience: None,
            description: Some("A description".to_string()),
            attachments: None,
            extra: HashMap::new(),
        };

        let yaml = fm.to_yaml();
        assert!(yaml.contains("title: Test Title"));
        // Markdown links are quoted in YAML because they contain special characters
        assert!(yaml.contains("part_of: \"[Parent Index](/folder/parent.md)\""));
        assert!(yaml.contains("contents:"));
        assert!(yaml.contains("  - \"[Child](/folder/child.md)\""));
        assert!(yaml.contains("description: A description"));
    }

    #[test]
    fn test_empty_contents_written_as_empty_array() {
        // Empty contents (Some([])) should be written as "contents: []"
        // to preserve index file identity
        let fm = FrontmatterMetadata {
            title: Some("Root Index".to_string()),
            part_of: None,
            contents: Some(vec![]), // Empty but explicitly set
            audience: None,
            description: None,
            attachments: None,
            extra: HashMap::new(),
        };

        let yaml = fm.to_yaml();
        assert!(
            yaml.contains("contents: []"),
            "Empty contents should be written as 'contents: []', got: {}",
            yaml
        );
    }

    #[test]
    fn test_none_contents_not_written() {
        // None contents should NOT be written at all
        let fm = FrontmatterMetadata {
            title: Some("Regular File".to_string()),
            part_of: Some("parent.md".to_string()),
            contents: None, // Not an index file
            audience: None,
            description: None,
            attachments: None,
            extra: HashMap::new(),
        };

        let yaml = fm.to_yaml();
        assert!(
            !yaml.contains("contents"),
            "None contents should not be written, got: {}",
            yaml
        );
    }

    #[test]
    fn test_from_json_with_markdown_links_formats_part_of() {
        let json = serde_json::json!({
            "title": "Child File",
            "part_of": "folder/parent.md",
        });

        let fm = FrontmatterMetadata::from_json_with_file_path(&json, Some("folder/child.md"));
        // Now formatted as markdown link with workspace-root path
        assert_eq!(fm.part_of, Some("[Parent](/folder/parent.md)".to_string()));
    }

    #[test]
    fn test_from_json_with_markdown_links_formats_contents() {
        let json = serde_json::json!({
            "title": "Parent Index",
            "contents": ["folder/child1.md", "folder/sub/child2.md"],
        });

        let fm = FrontmatterMetadata::from_json_with_file_path(&json, Some("folder/index.md"));
        // Contents formatted as markdown links with workspace-root paths
        assert_eq!(
            fm.contents,
            Some(vec![
                "[Child1](/folder/child1.md)".to_string(),
                "[Child2](/folder/sub/child2.md)".to_string()
            ])
        );
    }

    #[test]
    fn test_from_json_with_custom_title_resolver() {
        let json = serde_json::json!({
            "title": "Index",
            "part_of": "root/parent.md",
            "contents": ["root/child.md"],
        });

        // Use a custom title resolver that returns a fixed title
        let fm = FrontmatterMetadata::from_json_with_markdown_links(&json, |path| {
            if path == "root/parent.md" {
                "My Custom Parent Title".to_string()
            } else if path == "root/child.md" {
                "My Custom Child Title".to_string()
            } else {
                link_parser::path_to_title(path)
            }
        });

        assert_eq!(
            fm.part_of,
            Some("[My Custom Parent Title](/root/parent.md)".to_string())
        );
        assert_eq!(
            fm.contents,
            Some(vec!["[My Custom Child Title](/root/child.md)".to_string()])
        );
    }

    #[test]
    fn test_yaml_string_quotes_markdown_links() {
        // Markdown links contain [ and ] which require quoting
        let link = "[Title](/path/to/file.md)";
        let quoted = yaml_string(link);
        assert_eq!(quoted, "\"[Title](/path/to/file.md)\"");
    }

    #[test]
    fn test_roundtrip_markdown_link_to_yaml() {
        let fm = FrontmatterMetadata {
            title: Some("Test Entry".to_string()),
            part_of: Some("[Parent Index](/Folder/index.md)".to_string()),
            contents: Some(vec!["[Child Entry](/Folder/child.md)".to_string()]),
            audience: None,
            description: None,
            attachments: None,
            extra: HashMap::new(),
        };

        let yaml = fm.to_yaml();
        // The markdown links should be properly quoted
        assert!(yaml.contains("part_of: \"[Parent Index](/Folder/index.md)\""));
        assert!(yaml.contains("  - \"[Child Entry](/Folder/child.md)\""));
    }
}
