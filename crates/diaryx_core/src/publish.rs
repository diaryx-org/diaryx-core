//! Publishing functionality for diaryx workspaces
//!
//! Converts workspace markdown files to HTML for sharing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{DiaryxError, Result};
use crate::export::{ExportPlan, Exporter};
use crate::fs::FileSystem;
use crate::workspace::Workspace;

/// Options for publishing
#[derive(Debug, Default, Clone, Serialize)]
pub struct PublishOptions {
    /// Output as a single HTML file instead of multiple files
    pub single_file: bool,
    /// Site title (defaults to workspace title)
    pub title: Option<String>,
    /// Include audience filtering
    pub audience: Option<String>,
    /// Overwrite existing destination
    pub force: bool,
}

/// A navigation link
#[derive(Debug, Clone, Serialize)]
pub struct NavLink {
    /// Link href (relative path or anchor)
    pub href: String,
    /// Display title
    pub title: String,
}

/// A processed file ready for publishing
#[derive(Debug, Clone, Serialize)]
pub struct PublishedPage {
    /// Original source path
    pub source_path: PathBuf,
    /// Destination filename (e.g., "index.html" or "my-entry.html")
    pub dest_filename: String,
    /// Page title
    pub title: String,
    /// HTML content (body only, no wrapper)
    pub html_body: String,
    /// Original markdown body
    pub markdown_body: String,
    /// Navigation links to children (from contents property)
    pub contents_links: Vec<NavLink>,
    /// Navigation link to parent (from part_of property)
    pub parent_link: Option<NavLink>,
    /// Whether this is the root index
    pub is_root: bool,
}

/// Result of publishing operation
#[derive(Debug, Serialize)]
pub struct PublishResult {
    /// Pages that were published
    pub pages: Vec<PublishedPage>,
    /// Total files processed
    pub files_processed: usize,
}

/// Publisher for converting workspace to HTML
pub struct Publisher<FS: FileSystem + Clone> {
    fs: FS,
}

impl<FS: FileSystem + Clone> Publisher<FS> {
    /// Create a new publisher
    pub fn new(fs: FS) -> Self {
        Self { fs }
    }

    /// Publish a workspace to HTML
    pub fn publish(
        &self,
        workspace_root: &Path,
        destination: &Path,
        options: &PublishOptions,
    ) -> Result<PublishResult> {
        // Collect files to publish
        let pages = if let Some(ref audience) = options.audience {
            self.collect_with_audience(workspace_root, destination, audience)?
        } else {
            self.collect_all(workspace_root)?
        };

        if pages.is_empty() {
            return Ok(PublishResult {
                pages: vec![],
                files_processed: 0,
            });
        }

        let files_processed = pages.len();

        // Generate output
        if options.single_file {
            self.write_single_file(&pages, destination, options)?;
        } else {
            self.write_multi_file(&pages, destination, options)?;
        }

        Ok(PublishResult {
            pages,
            files_processed,
        })
    }

    /// Collect all workspace files without audience filtering
    fn collect_all(&self, workspace_root: &Path) -> Result<Vec<PublishedPage>> {
        let workspace = Workspace::new(self.fs.clone());
        let mut files = workspace.collect_workspace_files(workspace_root)?;

        // Ensure the workspace root is always first (it becomes index.html)
        // collect_workspace_files sorts alphabetically, so we need to move root to front
        let root_canonical = workspace_root
            .canonicalize()
            .unwrap_or_else(|_| workspace_root.to_path_buf());
        if let Some(pos) = files
            .iter()
            .position(|p| p.canonicalize().unwrap_or_else(|_| p.clone()) == root_canonical)
        {
            if pos != 0 {
                let root_file = files.remove(pos);
                files.insert(0, root_file);
            }
        }

        let mut pages = Vec::new();
        let mut path_to_filename: HashMap<PathBuf, String> = HashMap::new();

        // First pass: assign filenames
        for (idx, file_path) in files.iter().enumerate() {
            let filename = if idx == 0 {
                "index.html".to_string()
            } else {
                self.path_to_html_filename(file_path)
            };
            path_to_filename.insert(file_path.clone(), filename);
        }

        // Second pass: process files
        for (idx, file_path) in files.iter().enumerate() {
            if let Some(page) =
                self.process_file(file_path, idx == 0, &path_to_filename, workspace_root)?
            {
                pages.push(page);
            }
        }

        Ok(pages)
    }

    /// Collect files with audience filtering
    fn collect_with_audience(
        &self,
        workspace_root: &Path,
        destination: &Path,
        audience: &str,
    ) -> Result<Vec<PublishedPage>> {
        let exporter = Exporter::new(self.fs.clone());
        let plan = exporter.plan_export(workspace_root, audience, destination)?;

        let mut pages = Vec::new();
        let mut path_to_filename: HashMap<PathBuf, String> = HashMap::new();

        // First pass: assign filenames
        for (idx, export_file) in plan.included.iter().enumerate() {
            let filename = if idx == 0 {
                "index.html".to_string()
            } else {
                self.path_to_html_filename(&export_file.source_path)
            };
            path_to_filename.insert(export_file.source_path.clone(), filename);
        }

        // Second pass: process files
        for (idx, export_file) in plan.included.iter().enumerate() {
            if let Some(page) = self.process_file(
                &export_file.source_path,
                idx == 0,
                &path_to_filename,
                workspace_root,
            )? {
                // Filter out excluded children from contents_links
                let filtered_page = self.filter_contents_links(page, &plan);
                pages.push(filtered_page);
            }
        }

        Ok(pages)
    }

    /// Filter contents links to only include files that are in the export plan
    fn filter_contents_links(&self, mut page: PublishedPage, plan: &ExportPlan) -> PublishedPage {
        let included_filenames: std::collections::HashSet<String> = plan
            .included
            .iter()
            .map(|f| self.path_to_html_filename(&f.source_path))
            .collect();

        // Also include index.html for the root
        let mut allowed = included_filenames;
        allowed.insert("index.html".to_string());

        page.contents_links
            .retain(|link| allowed.contains(&link.href));

        page
    }

    /// Process a single file into a PublishedPage
    fn process_file(
        &self,
        path: &Path,
        is_root: bool,
        path_to_filename: &HashMap<PathBuf, String>,
        _workspace_root: &Path,
    ) -> Result<Option<PublishedPage>> {
        let content = match self.fs.read_to_string(path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(DiaryxError::FileRead {
                    path: path.to_path_buf(),
                    source: e,
                })
            }
        };

        let (frontmatter, body) = self.parse_frontmatter(&content);
        let title = self
            .extract_property(&frontmatter, "title")
            .unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Untitled")
                    .to_string()
            });

        let dest_filename = path_to_filename
            .get(path)
            .cloned()
            .unwrap_or_else(|| self.path_to_html_filename(path));

        // Build contents links
        let contents_links = self.build_contents_links(&frontmatter, path, path_to_filename);

        // Build parent link
        let parent_link = self.build_parent_link(&frontmatter, path, path_to_filename);

        // Convert markdown to HTML
        let html_body = self.markdown_to_html(&body);

        Ok(Some(PublishedPage {
            source_path: path.to_path_buf(),
            dest_filename,
            title,
            html_body,
            markdown_body: body,
            contents_links,
            parent_link,
            is_root,
        }))
    }

    /// Parse frontmatter from content
    fn parse_frontmatter(&self, content: &str) -> (String, String) {
        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            return (String::new(), content.to_string());
        }

        let rest = &content[4..];
        let end_idx = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n"));

        match end_idx {
            Some(idx) => {
                let frontmatter = rest[..idx].to_string();
                let body = rest[idx + 5..].to_string();
                (frontmatter, body)
            }
            None => (String::new(), content.to_string()),
        }
    }

    /// Extract a property from frontmatter
    fn extract_property(&self, frontmatter: &str, key: &str) -> Option<String> {
        let prefix = format!("{}:", key);
        for line in frontmatter.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix(&prefix) {
                let value = rest.trim().trim_matches('"').trim_matches('\'');
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
        None
    }

    /// Extract array property from frontmatter (e.g., contents)
    fn extract_array_property(&self, frontmatter: &str, key: &str) -> Vec<String> {
        let mut result = Vec::new();
        let mut in_property = false;
        let prefix = format!("{}:", key);

        for line in frontmatter.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with(&prefix) {
                in_property = true;
                // Check for inline array: contents: [a.md, b.md]
                let value_part = trimmed[prefix.len()..].trim();
                if value_part.starts_with('[') && value_part.ends_with(']') {
                    let inner = &value_part[1..value_part.len() - 1];
                    for item in inner.split(',') {
                        let item = item.trim().trim_matches('"').trim_matches('\'');
                        if !item.is_empty() {
                            result.push(item.to_string());
                        }
                    }
                    return result;
                }
                continue;
            }

            if in_property {
                if let Some(stripped) = trimmed.strip_prefix('-') {
                    let item = stripped.trim().trim_matches('"').trim_matches('\'');
                    if !item.is_empty() {
                        result.push(item.to_string());
                    }
                } else if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    // Hit another property
                    break;
                }
            }
        }

        result
    }

    /// Build navigation links from contents property
    fn build_contents_links(
        &self,
        frontmatter: &str,
        current_path: &Path,
        path_to_filename: &HashMap<PathBuf, String>,
    ) -> Vec<NavLink> {
        let contents = self.extract_array_property(frontmatter, "contents");
        let current_dir = current_path.parent();

        contents
            .into_iter()
            .map(|child_ref| {
                let child_path = current_dir
                    .map(|d| d.join(&child_ref))
                    .unwrap_or_else(|| PathBuf::from(&child_ref));

                // Try to find the HTML filename for this path
                let href = path_to_filename
                    .get(&child_path)
                    .cloned()
                    .unwrap_or_else(|| self.path_to_html_filename(&child_path));

                // Try to get the title from the file
                let title = self
                    .get_title_from_file(&child_path)
                    .unwrap_or_else(|| self.filename_to_title(&child_ref));

                NavLink { href, title }
            })
            .collect()
    }

    /// Build parent navigation link from part_of property
    fn build_parent_link(
        &self,
        frontmatter: &str,
        current_path: &Path,
        path_to_filename: &HashMap<PathBuf, String>,
    ) -> Option<NavLink> {
        let part_of = self.extract_property(frontmatter, "part_of")?;
        let current_dir = current_path.parent();

        let parent_path = current_dir
            .map(|d| d.join(&part_of))
            .unwrap_or_else(|| PathBuf::from(&part_of));

        let href = path_to_filename
            .get(&parent_path)
            .cloned()
            .unwrap_or_else(|| self.path_to_html_filename(&parent_path));

        let title = self
            .get_title_from_file(&parent_path)
            .unwrap_or_else(|| self.filename_to_title(&part_of));

        Some(NavLink { href, title })
    }

    /// Get title from a file's frontmatter
    fn get_title_from_file(&self, path: &Path) -> Option<String> {
        let content = self.fs.read_to_string(path).ok()?;
        let (frontmatter, _) = self.parse_frontmatter(&content);
        self.extract_property(&frontmatter, "title")
    }

    /// Convert a path to an HTML filename
    fn path_to_html_filename(&self, path: &Path) -> String {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("page");

        format!("{}.html", self.slugify(stem))
    }

    /// Convert a filename to a display title
    fn filename_to_title(&self, filename: &str) -> String {
        let stem = Path::new(filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(filename);

        // Convert snake_case or kebab-case to Title Case
        stem.split(['_', '-'])
            .filter(|s| !s.is_empty())
            .map(|word| {
                let mut chars: Vec<char> = word.chars().collect();
                if let Some(first) = chars.first_mut() {
                    *first = first.to_ascii_uppercase();
                }
                chars.into_iter().collect::<String>()
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Slugify a string for use in URLs
    fn slugify(&self, s: &str) -> String {
        s.to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-")
    }

    /// Convert markdown to HTML using comrak
    #[cfg(feature = "markdown")]
    fn markdown_to_html(&self, markdown: &str) -> String {
        use comrak::{markdown_to_html, Options};

        let mut options = Options::default();
        options.extension.strikethrough = true;
        options.extension.table = true;
        options.extension.autolink = true;
        options.extension.tasklist = true;
        options.render.r#unsafe = true; // Allow raw HTML

        markdown_to_html(markdown, &options)
    }

    #[cfg(not(feature = "markdown"))]
    fn markdown_to_html(&self, markdown: &str) -> String {
        // Basic fallback without comrak
        format!("<pre>{}</pre>", markdown)
    }

    /// Write multiple HTML files
    fn write_multi_file(
        &self,
        pages: &[PublishedPage],
        destination: &Path,
        options: &PublishOptions,
    ) -> Result<()> {
        // Create destination directory
        std::fs::create_dir_all(destination)?;

        let site_title = options.title.clone().unwrap_or_else(|| {
            pages
                .first()
                .map(|p| p.title.clone())
                .unwrap_or_else(|| "Journal".to_string())
        });

        for page in pages {
            let html = self.render_page(page, &site_title, false);
            let dest_path = destination.join(&page.dest_filename);
            std::fs::write(&dest_path, html)?;
        }

        // Write CSS file
        let css_path = destination.join("style.css");
        std::fs::write(&css_path, Self::get_css())?;

        Ok(())
    }

    /// Write a single HTML file containing all pages
    fn write_single_file(
        &self,
        pages: &[PublishedPage],
        destination: &Path,
        options: &PublishOptions,
    ) -> Result<()> {
        let site_title = options.title.clone().unwrap_or_else(|| {
            pages
                .first()
                .map(|p| p.title.clone())
                .unwrap_or_else(|| "Journal".to_string())
        });

        let html = self.render_single_file(pages, &site_title);

        // Ensure parent directory exists
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(destination, html)?;

        Ok(())
    }

    /// Render a single page to HTML
    fn render_page(&self, page: &PublishedPage, site_title: &str, single_file: bool) -> String {
        let nav_html = self.render_navigation(page, single_file);
        let css_link = if single_file {
            format!("<style>{}</style>", Self::get_css())
        } else {
            r#"<link rel="stylesheet" href="style.css">"#.to_string()
        };

        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{page_title} - {site_title}</title>
    {css_link}
</head>
<body>
    <header>
        <h1 class="site-title"><a href="index.html">{site_title}</a></h1>
    </header>
    <main>
        <article>
            <h1 class="page-title">{page_title}</h1>
            {nav_html}
            <div class="content">
                {content}
            </div>
        </article>
    </main>
    <footer>
        <p>Generated by <a href="https://github.com/diaryx-org/diaryx-core">diaryx</a></p>
    </footer>
</body>
</html>"#,
            page_title = html_escape(&page.title),
            site_title = html_escape(site_title),
            css_link = css_link,
            nav_html = nav_html,
            content = page.html_body,
        )
    }

    /// Render navigation links
    fn render_navigation(&self, page: &PublishedPage, single_file: bool) -> String {
        let mut nav_parts = Vec::new();

        // Parent link (breadcrumb style)
        if let Some(ref parent) = page.parent_link {
            let href = if single_file {
                format!("#{}", self.title_to_anchor(&parent.title))
            } else {
                parent.href.clone()
            };
            nav_parts.push(format!(
                r#"<div class="parent-link">↑ <a href="{}">{}</a></div>"#,
                html_escape(&href),
                html_escape(&parent.title)
            ));
        }

        // Contents links
        if !page.contents_links.is_empty() {
            let mut contents_html = String::from(r#"<nav class="contents"><h3>Contents</h3><ul>"#);
            for link in &page.contents_links {
                let href = if single_file {
                    format!("#{}", self.title_to_anchor(&link.title))
                } else {
                    link.href.clone()
                };
                contents_html.push_str(&format!(
                    r#"<li><a href="{}">{}</a></li>"#,
                    html_escape(&href),
                    html_escape(&link.title)
                ));
            }
            contents_html.push_str("</ul></nav>");
            nav_parts.push(contents_html);
        }

        nav_parts.join("\n")
    }

    /// Render all pages into a single HTML file
    fn render_single_file(&self, pages: &[PublishedPage], site_title: &str) -> String {
        let mut sections = Vec::new();

        for page in pages {
            let anchor = self.title_to_anchor(&page.title);
            let nav_html = self.render_navigation(page, true);

            sections.push(format!(
                r#"<section id="{anchor}">
    <h2 class="page-title">{title}</h2>
    {nav_html}
    <div class="content">
        {content}
    </div>
</section>"#,
                anchor = html_escape(&anchor),
                title = html_escape(&page.title),
                nav_html = nav_html,
                content = page.html_body,
            ));
        }

        // Build table of contents
        let mut toc = String::from(r#"<nav class="toc"><h2>Table of Contents</h2><ul>"#);
        for page in pages {
            let anchor = self.title_to_anchor(&page.title);
            toc.push_str(&format!(
                r##"<li><a href="#{}">{}</a></li>"##,
                html_escape(&anchor),
                html_escape(&page.title)
            ));
        }
        toc.push_str("</ul></nav>");

        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{site_title}</title>
    <style>{css}</style>
</head>
<body>
    <header>
        <h1 class="site-title">{site_title}</h1>
    </header>
    <main>
        {toc}
        {sections}
    </main>
    <footer>
        <p>Generated by <a href="https://github.com/diaryx-org/diaryx-core">diaryx</a></p>
    </footer>
</body>
</html>"#,
            site_title = html_escape(site_title),
            css = Self::get_css(),
            toc = toc,
            sections = sections.join("\n<hr>\n"),
        )
    }

    /// Convert a title to an anchor ID
    fn title_to_anchor(&self, title: &str) -> String {
        self.slugify(title)
    }

    /// Get the CSS stylesheet
    fn get_css() -> &'static str {
        r#"
:root {
    --bg: #fafafa;
    --text: #333;
    --text-muted: #666;
    --accent: #2563eb;
    --accent-hover: #1d4ed8;
    --border: #e5e7eb;
    --code-bg: #f3f4f6;
}

@media (prefers-color-scheme: dark) {
    :root {
        --bg: #1a1a1a;
        --text: #e5e5e5;
        --text-muted: #a3a3a3;
        --accent: #60a5fa;
        --accent-hover: #93c5fd;
        --border: #404040;
        --code-bg: #262626;
    }
}

* {
    box-sizing: border-box;
}

html {
    font-size: 16px;
}

body {
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Oxygen, Ubuntu, sans-serif;
    line-height: 1.6;
    color: var(--text);
    background: var(--bg);
    max-width: 48rem;
    margin: 0 auto;
    padding: 2rem 1rem;
}

header {
    margin-bottom: 2rem;
    padding-bottom: 1rem;
    border-bottom: 1px solid var(--border);
}

.site-title {
    font-size: 1.5rem;
    margin: 0;
}

.site-title a {
    color: var(--text);
    text-decoration: none;
}

.site-title a:hover {
    color: var(--accent);
}

.page-title {
    font-size: 2rem;
    margin-top: 0;
    margin-bottom: 1rem;
}

.parent-link {
    margin-bottom: 1rem;
    font-size: 0.9rem;
}

.parent-link a {
    color: var(--accent);
}

nav.contents {
    background: var(--code-bg);
    padding: 1rem;
    border-radius: 0.5rem;
    margin-bottom: 1.5rem;
}

nav.contents h3 {
    margin-top: 0;
    margin-bottom: 0.5rem;
    font-size: 1rem;
}

nav.contents ul {
    margin: 0;
    padding-left: 1.5rem;
}

nav.contents li {
    margin: 0.25rem 0;
}

nav.toc {
    background: var(--code-bg);
    padding: 1.5rem;
    border-radius: 0.5rem;
    margin-bottom: 2rem;
}

nav.toc h2 {
    margin-top: 0;
}

nav.toc ul {
    margin: 0;
    padding-left: 1.5rem;
}

nav.toc li {
    margin: 0.5rem 0;
}

a {
    color: var(--accent);
    text-decoration: none;
}

a:hover {
    color: var(--accent-hover);
    text-decoration: underline;
}

.content {
    margin-top: 1.5rem;
}

.content h1, .content h2, .content h3, .content h4, .content h5, .content h6 {
    margin-top: 2rem;
    margin-bottom: 0.5rem;
}

.content p {
    margin: 1rem 0;
}

.content ul, .content ol {
    margin: 1rem 0;
    padding-left: 2rem;
}

.content li {
    margin: 0.25rem 0;
}

.content pre {
    background: var(--code-bg);
    padding: 1rem;
    border-radius: 0.5rem;
    overflow-x: auto;
}

.content code {
    background: var(--code-bg);
    padding: 0.2rem 0.4rem;
    border-radius: 0.25rem;
    font-size: 0.9em;
}

.content pre code {
    background: none;
    padding: 0;
}

.content blockquote {
    border-left: 4px solid var(--border);
    margin: 1rem 0;
    padding-left: 1rem;
    color: var(--text-muted);
}

.content table {
    width: 100%;
    border-collapse: collapse;
    margin: 1rem 0;
}

.content th, .content td {
    border: 1px solid var(--border);
    padding: 0.5rem;
    text-align: left;
}

.content th {
    background: var(--code-bg);
}

.content img {
    max-width: 100%;
    height: auto;
}

hr {
    border: none;
    border-top: 1px solid var(--border);
    margin: 3rem 0;
}

section {
    margin-bottom: 2rem;
}

footer {
    margin-top: 3rem;
    padding-top: 1rem;
    border-top: 1px solid var(--border);
    color: var(--text-muted);
    font-size: 0.9rem;
}

footer a {
    color: var(--text-muted);
}

footer a:hover {
    color: var(--accent);
}

@media (max-width: 600px) {
    html {
        font-size: 14px;
    }

    body {
        padding: 1rem;
    }

    .page-title {
        font-size: 1.5rem;
    }
}
"#
    }
}

/// Escape HTML special characters
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a & b"), "a &amp; b");
        assert_eq!(html_escape(r#"say "hi""#), "say &quot;hi&quot;");
    }
}
