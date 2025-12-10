//! Template engine for creating entries with pre-defined structures
//!
//! Supports simple variable substitution using `{{variable}}` syntax.
//! Variables can include format specifiers for dates: `{{date:%Y-%m-%d}}`

use chrono::{Local, NaiveDate};
use indexmap::IndexMap;
use serde_yaml::Value;
use std::path::{Path, PathBuf};

use crate::error::{DiaryxError, Result};
use crate::fs::FileSystem;

/// Available template variables and their descriptions
pub const TEMPLATE_VARIABLES: &[(&str, &str)] = &[
    ("title", "The entry title"),
    ("filename", "The filename without extension"),
    (
        "date",
        "Current date (default: %Y-%m-%d). Use {{date:%B %d, %Y}} for custom format",
    ),
    (
        "time",
        "Current time (default: %H:%M). Use {{time:%H:%M:%S}} for custom format",
    ),
    (
        "datetime",
        "Current datetime (default: %Y-%m-%dT%H:%M:%S). Use {{datetime:FORMAT}} for custom",
    ),
    (
        "timestamp",
        "ISO 8601 timestamp with timezone (for created/updated)",
    ),
    ("year", "Current year (4 digits)"),
    ("month", "Current month (2 digits)"),
    ("month_name", "Current month name (e.g., January)"),
    ("day", "Current day (2 digits)"),
    ("weekday", "Current weekday name (e.g., Monday)"),
];

/// Built-in default template for notes
pub const DEFAULT_NOTE_TEMPLATE: &str = r#"---
title: "{{title}}"
created: {{timestamp}}
---

# {{title}}

"#;

/// Built-in default template for daily entries
pub const DEFAULT_DAILY_TEMPLATE: &str = r#"---
date: {{date}}
title: "{{title}}"
created: {{timestamp}}
part_of: {{part_of}}
---

# {{title}}

"#;

/// A parsed template with frontmatter and body
#[derive(Debug, Clone)]
pub struct Template {
    /// Template name (derived from filename)
    pub name: String,
    /// Raw template content (before variable substitution)
    pub raw_content: String,
}

impl Template {
    /// Create a new template from raw content
    pub fn new(name: impl Into<String>, raw_content: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            raw_content: raw_content.into(),
        }
    }

    /// Load a template from a file
    pub fn from_file<FS: FileSystem>(fs: &FS, path: &Path) -> Result<Self> {
        let content = fs.read_to_string(path).map_err(|e| DiaryxError::FileRead {
            path: path.to_path_buf(),
            source: e,
        })?;

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(Self::new(name, content))
    }

    /// Get the built-in note template
    pub fn builtin_note() -> Self {
        Self::new("note", DEFAULT_NOTE_TEMPLATE)
    }

    /// Get the built-in daily template
    pub fn builtin_daily() -> Self {
        Self::new("daily", DEFAULT_DAILY_TEMPLATE)
    }

    /// Render the template with the given context
    pub fn render(&self, context: &TemplateContext) -> String {
        substitute_variables(&self.raw_content, context)
    }

    /// Render and parse into frontmatter and body
    pub fn render_parsed(
        &self,
        context: &TemplateContext,
    ) -> Result<(IndexMap<String, Value>, String)> {
        let rendered = self.render(context);
        parse_rendered_template(&rendered)
    }
}

/// Context for template variable substitution
#[derive(Debug, Clone, Default)]
pub struct TemplateContext {
    /// Title for the entry
    pub title: Option<String>,
    /// Filename (without extension)
    pub filename: Option<String>,
    /// Date to use (defaults to today)
    pub date: Option<NaiveDate>,
    /// Part of reference (for hierarchical entries)
    pub part_of: Option<String>,
    /// Custom variables
    pub custom: IndexMap<String, String>,
}

impl TemplateContext {
    /// Create a new empty context
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the title
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the filename
    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }

    /// Set the date
    pub fn with_date(mut self, date: NaiveDate) -> Self {
        self.date = Some(date);
        self
    }

    /// Set the part_of reference
    pub fn with_part_of(mut self, part_of: impl Into<String>) -> Self {
        self.part_of = Some(part_of.into());
        self
    }

    /// Add a custom variable
    pub fn with_custom(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.custom.insert(key.into(), value.into());
        self
    }

    /// Get the effective date (provided or today)
    fn effective_date(&self) -> NaiveDate {
        self.date.unwrap_or_else(|| Local::now().date_naive())
    }

    /// Get the effective title (provided, filename, or "Untitled")
    fn effective_title(&self) -> String {
        self.title
            .clone()
            .or_else(|| self.filename.clone())
            .unwrap_or_else(|| "Untitled".to_string())
    }
}

/// Substitute template variables in a string
fn substitute_variables(content: &str, context: &TemplateContext) -> String {
    let mut result = content.to_string();
    let now = Local::now();
    let date = context.effective_date();

    // Process variables with format specifiers first (e.g., {{date:%Y-%m-%d}})
    result = substitute_formatted_variables(&result, "date", |fmt| date.format(fmt).to_string());
    result = substitute_formatted_variables(&result, "time", |fmt| now.format(fmt).to_string());
    result = substitute_formatted_variables(&result, "datetime", |fmt| now.format(fmt).to_string());

    // Simple variable substitutions
    let replacements: Vec<(&str, String)> = vec![
        ("title", context.effective_title()),
        ("filename", context.filename.clone().unwrap_or_default()),
        ("date", date.format("%Y-%m-%d").to_string()),
        ("time", now.format("%H:%M").to_string()),
        ("datetime", now.format("%Y-%m-%dT%H:%M:%S").to_string()),
        ("timestamp", now.format("%Y-%m-%dT%H:%M:%S%:z").to_string()),
        ("year", date.format("%Y").to_string()),
        ("month", date.format("%m").to_string()),
        ("month_name", date.format("%B").to_string()),
        ("day", date.format("%d").to_string()),
        ("weekday", date.format("%A").to_string()),
        ("part_of", context.part_of.clone().unwrap_or_default()),
    ];

    for (var, value) in replacements {
        let pattern = format!("{{{{{}}}}}", var);
        result = result.replace(&pattern, &value);
    }

    // Custom variables
    for (key, value) in &context.custom {
        let pattern = format!("{{{{{}}}}}", key);
        result = result.replace(&pattern, value);
    }

    result
}

/// Substitute variables with format specifiers like {{var:FORMAT}}
fn substitute_formatted_variables<F>(content: &str, var_name: &str, formatter: F) -> String
where
    F: Fn(&str) -> String,
{
    let mut result = content.to_string();
    let prefix = format!("{{{{{}:", var_name);

    while let Some(start) = result.find(&prefix) {
        let rest = &result[start + prefix.len()..];
        if let Some(end) = rest.find("}}") {
            let format_str = &rest[..end];
            let full_pattern = format!("{{{{{}:{}}}}}", var_name, format_str);
            let replacement = formatter(format_str);
            result = result.replace(&full_pattern, &replacement);
        } else {
            break;
        }
    }

    result
}

/// Parse rendered template content into frontmatter and body
fn parse_rendered_template(content: &str) -> Result<(IndexMap<String, Value>, String)> {
    // Check if content starts with frontmatter delimiter
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        // No frontmatter, entire content is body
        return Ok((IndexMap::new(), content.to_string()));
    }

    // Find the closing delimiter
    let rest = &content[4..]; // Skip first "---\n"
    let end_idx = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n"));

    match end_idx {
        Some(idx) => {
            let frontmatter_str = &rest[..idx];
            let body = &rest[idx + 5..]; // Skip "\n---\n"

            let frontmatter: IndexMap<String, Value> = serde_yaml::from_str(frontmatter_str)?;
            Ok((frontmatter, body.to_string()))
        }
        None => {
            // Malformed frontmatter (no closing delimiter) - treat as no frontmatter
            Ok((IndexMap::new(), content.to_string()))
        }
    }
}

/// Template manager for loading and managing templates
pub struct TemplateManager<FS> {
    fs: FS,
    /// User templates directory (~/.config/diaryx/templates/)
    user_templates_dir: Option<PathBuf>,
    /// Workspace templates directory (<workspace>/.diaryx/templates/)
    workspace_templates_dir: Option<PathBuf>,
}

impl<FS: FileSystem> TemplateManager<FS> {
    /// Create a new template manager
    /// On native platforms, uses the system config directory for user templates
    /// On WASM, user_templates_dir will be None (use with_user_templates_dir to set)
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(fs: FS) -> Self {
        let user_templates_dir = dirs::config_dir().map(|d| d.join("diaryx").join("templates"));

        Self {
            fs,
            user_templates_dir,
            workspace_templates_dir: None,
        }
    }

    /// Create a new template manager (WASM version)
    /// User templates directory must be set explicitly with with_user_templates_dir
    #[cfg(target_arch = "wasm32")]
    pub fn new(fs: FS) -> Self {
        Self {
            fs,
            user_templates_dir: None,
            workspace_templates_dir: None,
        }
    }

    /// Set the user templates directory explicitly
    /// Useful for WASM or when you want to override the default location
    pub fn with_user_templates_dir(mut self, dir: PathBuf) -> Self {
        self.user_templates_dir = Some(dir);
        self
    }

    /// Set the workspace templates directory
    pub fn with_workspace_dir(mut self, workspace_dir: &Path) -> Self {
        self.workspace_templates_dir = Some(workspace_dir.join(".diaryx").join("templates"));
        self
    }

    /// Get the user templates directory path
    pub fn user_templates_dir(&self) -> Option<&Path> {
        self.user_templates_dir.as_deref()
    }

    /// Get the workspace templates directory path
    pub fn workspace_templates_dir(&self) -> Option<&Path> {
        self.workspace_templates_dir.as_deref()
    }

    /// Get a template by name
    /// Search order: workspace templates, user templates, built-in templates
    pub fn get(&self, name: &str) -> Option<Template> {
        // Try workspace templates first
        if let Some(template) = self.load_from_dir(&self.workspace_templates_dir, name) {
            return Some(template);
        }

        // Try user templates
        if let Some(template) = self.load_from_dir(&self.user_templates_dir, name) {
            return Some(template);
        }

        // Fall back to built-in templates
        self.get_builtin(name)
    }

    /// Get a built-in template by name
    pub fn get_builtin(&self, name: &str) -> Option<Template> {
        match name {
            "note" => Some(Template::builtin_note()),
            "daily" => Some(Template::builtin_daily()),
            _ => None,
        }
    }

    /// Load a template from a directory
    fn load_from_dir(&self, dir: &Option<PathBuf>, name: &str) -> Option<Template> {
        let dir = dir.as_ref()?;
        let path = dir.join(format!("{}.md", name));

        if self.fs.exists(&path) {
            Template::from_file(&self.fs, &path).ok()
        } else {
            None
        }
    }

    /// List all available templates
    pub fn list(&self) -> Vec<TemplateInfo> {
        let mut templates = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // Workspace templates (highest priority)
        if let Some(ref dir) = self.workspace_templates_dir {
            for info in self.list_templates_in_dir(dir, TemplateSource::Workspace) {
                if seen.insert(info.name.clone()) {
                    templates.push(info);
                }
            }
        }

        // User templates
        if let Some(ref dir) = self.user_templates_dir {
            for info in self.list_templates_in_dir(dir, TemplateSource::User) {
                if seen.insert(info.name.clone()) {
                    templates.push(info);
                }
            }
        }

        // Built-in templates
        for (name, source) in [
            ("note", TemplateSource::Builtin),
            ("daily", TemplateSource::Builtin),
        ] {
            if seen.insert(name.to_string()) {
                templates.push(TemplateInfo {
                    name: name.to_string(),
                    source,
                    path: None,
                });
            }
        }

        templates.sort_by(|a, b| a.name.cmp(&b.name));
        templates
    }

    /// List templates in a directory
    fn list_templates_in_dir(&self, dir: &Path, source: TemplateSource) -> Vec<TemplateInfo> {
        let mut templates = Vec::new();

        if let Ok(files) = self.fs.list_md_files(dir) {
            for path in files {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    templates.push(TemplateInfo {
                        name: name.to_string(),
                        source: source.clone(),
                        path: Some(path),
                    });
                }
            }
        }

        templates
    }

    /// Create a new template file in the user templates directory
    pub fn create_template(&self, name: &str, content: &str) -> Result<PathBuf> {
        let dir = self
            .user_templates_dir
            .as_ref()
            .ok_or(DiaryxError::NoConfigDir)?;

        // Create templates directory if it doesn't exist
        self.fs.create_dir_all(dir)?;

        let path = dir.join(format!("{}.md", name));
        self.fs.create_new(&path, content)?;

        Ok(path)
    }

    /// Save a template to the user templates directory (overwrites if exists)
    pub fn save_template(&self, name: &str, content: &str) -> Result<PathBuf> {
        let dir = self
            .user_templates_dir
            .as_ref()
            .ok_or(DiaryxError::NoConfigDir)?;

        // Create templates directory if it doesn't exist
        self.fs.create_dir_all(dir)?;

        let path = dir.join(format!("{}.md", name));
        self.fs.write_file(&path, content)?;

        Ok(path)
    }
}

/// Information about a template
#[derive(Debug, Clone)]
pub struct TemplateInfo {
    /// Template name
    pub name: String,
    /// Where the template comes from
    pub source: TemplateSource,
    /// Path to the template file (None for built-in)
    pub path: Option<PathBuf>,
}

/// Source of a template
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateSource {
    /// Built-in template
    Builtin,
    /// User template (~/.config/diaryx/templates/)
    User,
    /// Workspace template (<workspace>/.diaryx/templates/)
    Workspace,
}

impl std::fmt::Display for TemplateSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TemplateSource::Builtin => write!(f, "built-in"),
            TemplateSource::User => write!(f, "user"),
            TemplateSource::Workspace => write!(f, "workspace"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_variable_substitution() {
        let template = Template::new("test", "Hello {{title}}!");
        let context = TemplateContext::new().with_title("World");
        let result = template.render(&context);
        assert_eq!(result, "Hello World!");
    }

    #[test]
    fn test_date_variables() {
        let template = Template::new("test", "Date: {{date}}, Year: {{year}}, Month: {{month}}");
        let date = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let context = TemplateContext::new().with_date(date);
        let result = template.render(&context);
        assert_eq!(result, "Date: 2024-06-15, Year: 2024, Month: 06");
    }

    #[test]
    fn test_formatted_date_variable() {
        let template = Template::new("test", "{{date:%B %d, %Y}}");
        let date = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let context = TemplateContext::new().with_date(date);
        let result = template.render(&context);
        assert_eq!(result, "June 15, 2024");
    }

    #[test]
    fn test_custom_variables() {
        let template = Template::new("test", "Mood: {{mood}}, Weather: {{weather}}");
        let context = TemplateContext::new()
            .with_custom("mood", "happy")
            .with_custom("weather", "sunny");
        let result = template.render(&context);
        assert_eq!(result, "Mood: happy, Weather: sunny");
    }

    #[test]
    fn test_builtin_note_template() {
        let template = Template::builtin_note();
        let context = TemplateContext::new().with_title("My Note");
        let result = template.render(&context);

        assert!(result.contains("title: \"My Note\""));
        assert!(result.contains("# My Note"));
        assert!(result.contains("created:"));
    }

    #[test]
    fn test_builtin_daily_template() {
        let template = Template::builtin_daily();
        let date = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let context = TemplateContext::new()
            .with_title("June 15, 2024")
            .with_date(date)
            .with_part_of("06_june.md");
        let result = template.render(&context);

        assert!(result.contains("date: 2024-06-15"));
        assert!(result.contains("title: \"June 15, 2024\""));
        assert!(result.contains("part_of: 06_june.md"));
    }

    #[test]
    fn test_render_parsed() {
        let template = Template::new("test", "---\ntitle: \"{{title}}\"\n---\n\n# {{title}}\n");
        let context = TemplateContext::new().with_title("Test");
        let (frontmatter, body) = template.render_parsed(&context).unwrap();

        assert_eq!(frontmatter.get("title").unwrap().as_str().unwrap(), "Test");
        assert_eq!(body.trim(), "# Test");
    }

    #[test]
    fn test_effective_title_fallback() {
        // With title
        let ctx = TemplateContext::new().with_title("My Title");
        assert_eq!(ctx.effective_title(), "My Title");

        // Without title, with filename
        let ctx = TemplateContext::new().with_filename("my-file");
        assert_eq!(ctx.effective_title(), "my-file");

        // Without title or filename
        let ctx = TemplateContext::new();
        assert_eq!(ctx.effective_title(), "Untitled");
    }

    #[test]
    fn test_part_of_empty_when_not_set() {
        let template = Template::new("test", "part_of: {{part_of}}");
        let context = TemplateContext::new();
        let result = template.render(&context);
        assert_eq!(result, "part_of: ");
    }

    #[test]
    fn test_timestamp_format() {
        let template = Template::new("test", "{{timestamp}}");
        let context = TemplateContext::new();
        let result = template.render(&context);

        // Should match ISO 8601 with timezone like "2024-06-15T10:30:00-07:00"
        assert!(result.contains("T"));
        assert!(result.contains(":"));
        // Should have timezone offset
        assert!(result.contains("+") || result.contains("-"));
    }
}
