//! WebAssembly bindings for Diaryx core functionality.
//!
//! This crate provides JavaScript-accessible functions for parsing frontmatter,
//! serializing YAML, rendering templates, and searching content.

use wasm_bindgen::prelude::*;

// When the `console_error_panic_hook` feature is enabled, we can call the
// `set_panic_hook` function at least once during initialization, and then
// we will get better error messages if our code ever panics.
#[cfg(feature = "console_error_panic_hook")]
pub fn set_panic_hook() {
    console_error_panic_hook::set_once();
}

/// Initialize the WASM module. Call this once before using other functions.
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    set_panic_hook();
}

// ============================================================================
// Frontmatter Parsing
// ============================================================================

/// Parse YAML frontmatter from markdown content.
///
/// Returns a JavaScript object with the frontmatter key-value pairs.
/// If no frontmatter is found, returns an empty object.
#[wasm_bindgen]
pub fn parse_frontmatter(content: &str) -> Result<JsValue, JsValue> {
    // Check for frontmatter delimiters
    if !content.starts_with("---\n") {
        return Ok(serde_wasm_bindgen::to_value(&serde_json::Map::<String, serde_json::Value>::new())?);
    }

    // Find the closing delimiter
    let rest = &content[4..]; // Skip "---\n"
    let end_idx = rest.find("\n---");

    let yaml_str = match end_idx {
        Some(idx) => &rest[..idx],
        None => {
            return Ok(serde_wasm_bindgen::to_value(&serde_json::Map::<String, serde_json::Value>::new())?);
        }
    };

    // Parse YAML into a JSON-compatible structure
    match serde_yaml::from_str::<serde_json::Value>(yaml_str) {
        Ok(value) => {
            if let serde_json::Value::Object(map) = value {
                Ok(serde_wasm_bindgen::to_value(&map)?)
            } else {
                Ok(serde_wasm_bindgen::to_value(&serde_json::Map::<String, serde_json::Value>::new())?)
            }
        }
        Err(_) => {
            Ok(serde_wasm_bindgen::to_value(&serde_json::Map::<String, serde_json::Value>::new())?)
        }
    }
}

/// Serialize a JavaScript object to YAML frontmatter format.
///
/// Returns a string in the format:
///
///     ---
///     key: value
///     ---
#[wasm_bindgen]
pub fn serialize_frontmatter(frontmatter: JsValue) -> Result<String, JsValue> {
    let map: serde_json::Map<String, serde_json::Value> =
        serde_wasm_bindgen::from_value(frontmatter)?;

    let yaml = serde_yaml::to_string(&map)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    // serde_yaml adds a trailing newline, and we want to wrap in delimiters
    let yaml = yaml.trim_end();

    Ok(format!("---\n{}\n---", yaml))
}

// ============================================================================
// Template Rendering
// ============================================================================

/// Render a template with the given context.
///
/// Supports `{{variable}}` and `{{variable:format}}` syntax.
/// For dates, supports strftime-like format specifiers.
#[wasm_bindgen]
pub fn render_template(template: &str, context: JsValue) -> Result<String, JsValue> {
    let ctx: std::collections::HashMap<String, String> =
        serde_wasm_bindgen::from_value(context)?;

    let mut result = template.to_string();

    // Find all {{...}} patterns
    let mut i = 0;
    while i < result.len() {
        if let Some(start) = result[i..].find("{{") {
            let start = start + i;
            if let Some(end) = result[start..].find("}}") {
                let end = start + end + 2;
                let placeholder = &result[start + 2..end - 2];

                // Check for format specifier
                let (var_name, format) = if let Some(colon_idx) = placeholder.find(':') {
                    (&placeholder[..colon_idx], Some(&placeholder[colon_idx + 1..]))
                } else {
                    (placeholder, None)
                };

                if let Some(value) = ctx.get(var_name) {
                    let replacement = if let Some(fmt) = format {
                        if var_name == "date" || var_name == "timestamp" {
                            format_date_string(value, fmt)
                        } else {
                            value.clone()
                        }
                    } else {
                        value.clone()
                    };

                    result.replace_range(start..end, &replacement);
                    i = start + replacement.len();
                } else {
                    // Keep the placeholder if variable not found
                    i = end;
                }
            } else {
                i = start + 2;
            }
        } else {
            break;
        }
    }

    Ok(result)
}

/// Format a date string using strftime-like format specifiers.
fn format_date_string(date_str: &str, format: &str) -> String {
    // Try to parse as ISO 8601
    let parsed = chrono::DateTime::parse_from_rfc3339(date_str)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .or_else(|_| {
            // Try parsing as a date only
            chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc())
        });

    match parsed {
        Ok(dt) => dt.format(format).to_string(),
        Err(_) => date_str.to_string(),
    }
}

// ============================================================================
// Content Search
// ============================================================================

/// A match found during content search.
#[derive(serde::Serialize)]
pub struct SearchMatch {
    pub line_number: u32,
    pub line_content: String,
    pub match_start: u32,
    pub match_end: u32,
}

/// Search content for a pattern.
///
/// Returns an array of match objects with line numbers and positions.
#[wasm_bindgen]
pub fn search_content(
    content: &str,
    pattern: &str,
    case_sensitive: bool
) -> Result<JsValue, JsValue> {
    use regex::RegexBuilder;

    let regex = RegexBuilder::new(pattern)
        .case_insensitive(!case_sensitive)
        .build()
        .map_err(|e| JsValue::from_str(&format!("Invalid regex: {}", e)))?;

    let mut matches = Vec::new();

    for (line_idx, line) in content.lines().enumerate() {
        for mat in regex.find_iter(line) {
            matches.push(SearchMatch {
                line_number: (line_idx + 1) as u32,
                line_content: line.to_string(),
                match_start: mat.start() as u32,
                match_end: mat.end() as u32,
            });
        }
    }

    Ok(serde_wasm_bindgen::to_value(&matches)?)
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Extract the body content from a markdown file (everything after frontmatter).
#[wasm_bindgen]
pub fn extract_body(content: &str) -> String {
    if !content.starts_with("---\n") {
        return content.to_string();
    }

    let rest = &content[4..]; // Skip "---\n"

    if let Some(end_idx) = rest.find("\n---") {
        let after_frontmatter = &rest[end_idx + 4..]; // Skip "\n---"
        // Skip newlines after the closing delimiter
        after_frontmatter.trim_start_matches('\n').to_string()
    } else {
        content.to_string()
    }
}

/// Generate an ISO 8601 timestamp for the current time.
#[wasm_bindgen]
pub fn now_timestamp() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Generate a formatted date string for the current date.
#[wasm_bindgen]
pub fn today_formatted(format: &str) -> String {
    chrono::Utc::now().format(format).to_string()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_body() {
        let content = "---\ntitle: Test\n---\n\n# Hello\n\nWorld";
        let body = extract_body(content);
        assert_eq!(body, "# Hello\n\nWorld");
    }

    #[test]
    fn test_extract_body_no_frontmatter() {
        let content = "# Hello\n\nWorld";
        let body = extract_body(content);
        assert_eq!(body, "# Hello\n\nWorld");
    }

    #[test]
    fn test_format_date() {
        let date = "2024-01-15T10:30:00Z";
        let formatted = format_date_string(date, "%B %d, %Y");
        assert_eq!(formatted, "January 15, 2024");
    }
}
