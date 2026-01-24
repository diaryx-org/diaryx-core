//! Metadata-to-frontmatter conversion and file writing utilities.
//!
//! This module provides functions to convert `FileMetadata` (from CRDT sync)
//! into YAML frontmatter format and write files with proper structure.

use std::collections::HashMap;
use std::path::Path;

use crate::error::{DiaryxError, Result};
use crate::frontmatter;
use crate::fs::AsyncFileSystem;

/// Metadata structure for file frontmatter.
/// This mirrors the CRDT FileMetadata but with simpler types for serialization.
#[derive(Debug, Clone, Default)]
pub struct FrontmatterMetadata {
    /// Display title from frontmatter
    pub title: Option<String>,
    /// Relative path to parent index file (will be converted from absolute)
    pub part_of: Option<String>,
    /// Relative paths to child files
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
    pub fn from_json(value: &serde_json::Value) -> Self {
        let obj = value.as_object();

        let title = obj
            .and_then(|o| o.get("title"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let part_of = obj
            .and_then(|o| o.get("part_of"))
            .and_then(|v| v.as_str())
            .map(|s| {
                // Convert absolute path to relative (just filename)
                s.split('/').next_back().unwrap_or(s).to_string()
            });

        let contents = obj
            .and_then(|o| o.get("contents"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
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
pub async fn write_file_with_metadata<FS: AsyncFileSystem>(
    fs: &FS,
    path: &Path,
    metadata: &serde_json::Value,
    body: &str,
) -> Result<()> {
    let fm = FrontmatterMetadata::from_json(metadata);
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
        assert_eq!(fm.part_of, Some("parent.md".to_string())); // Converted to relative
        assert_eq!(
            fm.contents,
            Some(vec!["child1.md".to_string(), "child2.md".to_string()])
        );
        assert_eq!(fm.description, Some("A test file".to_string()));
        assert!(fm.extra.contains_key("custom_key"));
        assert!(!fm.extra.contains_key("_body")); // Internal key excluded
    }

    #[test]
    fn test_frontmatter_metadata_to_yaml() {
        let fm = FrontmatterMetadata {
            title: Some("Test Title".to_string()),
            part_of: Some("parent.md".to_string()),
            contents: Some(vec!["child1.md".to_string()]),
            audience: None,
            description: Some("A description".to_string()),
            attachments: None,
            extra: HashMap::new(),
        };

        let yaml = fm.to_yaml();
        assert!(yaml.contains("title: Test Title"));
        assert!(yaml.contains("part_of: parent.md"));
        assert!(yaml.contains("contents:"));
        assert!(yaml.contains("  - child1.md"));
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
}
