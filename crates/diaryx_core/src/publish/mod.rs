//! Publishing functionality for diaryx workspaces
//!
//! Converts workspace markdown files to HTML for sharing.
//!
//! # Async-first Design
//!
//! This module uses `AsyncFileSystem` for all filesystem operations.
//! For synchronous contexts (CLI, tests), wrap a sync filesystem with
//! `SyncToAsyncFs` and use `futures_lite::future::block_on()`.

mod types;

// Re-export types for backwards compatibility
pub use types::{NavLink, PublishOptions, PublishResult, PublishedPage};

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::entry::slugify;
use crate::error::{DiaryxError, Result};
use crate::export::{ExportPlan, Exporter};
use crate::frontmatter;
use crate::fs::AsyncFileSystem;
use crate::workspace::Workspace;

/// Publisher for converting workspace to HTML (async-first)
pub struct Publisher<FS: AsyncFileSystem> {
    fs: FS,
}

impl<FS: AsyncFileSystem + Clone> Publisher<FS> {
    /// Create a new publisher
    pub fn new(fs: FS) -> Self {
        Self { fs }
    }

    /// Publish a workspace to HTML
    /// Only available on native platforms (not WASM) since it writes to the filesystem
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn publish(
        &self,
        workspace_root: &Path,
        destination: &Path,
        options: &PublishOptions,
    ) -> Result<PublishResult> {
        // Collect files to publish
        let pages = if let Some(ref audience) = options.audience {
            self.collect_with_audience(workspace_root, destination, audience)
                .await?
        } else {
            self.collect_all(workspace_root).await?
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
            self.write_single_file(&pages, destination, options).await?;
        } else {
            self.write_multi_file(&pages, destination, options).await?;
        }

        Ok(PublishResult {
            pages,
            files_processed,
        })
    }

    /// Collect all workspace files without audience filtering
    async fn collect_all(&self, workspace_root: &Path) -> Result<Vec<PublishedPage>> {
        let workspace = Workspace::new(self.fs.clone());
        let mut files = workspace.collect_workspace_files(workspace_root).await?;

        // Ensure the workspace root is always first (it becomes index.html)
        // collect_workspace_files sorts alphabetically, so we need to move root to front
        let root_canonical = workspace_root
            .canonicalize()
            .unwrap_or_else(|_| workspace_root.to_path_buf());
        if let Some(pos) = files
            .iter()
            .position(|p| p.canonicalize().unwrap_or_else(|_| p.clone()) == root_canonical)
            && pos != 0
        {
            let root_file = files.remove(pos);
            files.insert(0, root_file);
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
            path_to_filename.insert(file_path.to_path_buf(), filename);
        }

        // Second pass: process files
        for (idx, file_path) in files.iter().enumerate() {
            if let Some(page) = self
                .process_file(file_path, idx == 0, &path_to_filename, workspace_root)
                .await?
            {
                pages.push(page);
            }
        }

        Ok(pages)
    }

    /// Collect files with audience filtering
    async fn collect_with_audience(
        &self,
        workspace_root: &Path,
        destination: &Path,
        audience: &str,
    ) -> Result<Vec<PublishedPage>> {
        let exporter = Exporter::new(self.fs.clone());
        let plan = exporter
            .plan_export(workspace_root, audience, destination)
            .await?;

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
            if let Some(page) = self
                .process_file(
                    &export_file.source_path,
                    idx == 0,
                    &path_to_filename,
                    workspace_root,
                )
                .await?
            {
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
    async fn process_file(
        &self,
        path: &Path,
        is_root: bool,
        path_to_filename: &HashMap<PathBuf, String>,
        _workspace_root: &Path,
    ) -> Result<Option<PublishedPage>> {
        let content = match self.fs.read_to_string(path).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(DiaryxError::FileRead {
                    path: path.to_path_buf(),
                    source: e,
                });
            }
        };

        let parsed = frontmatter::parse_or_empty(&content)?;
        let title = frontmatter::get_string(&parsed.frontmatter, "title")
            .map(String::from)
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
        let contents_links = self
            .build_contents_links(&parsed.frontmatter, path, path_to_filename)
            .await;

        // Build parent link
        let parent_link = self
            .build_parent_link(&parsed.frontmatter, path, path_to_filename)
            .await;

        // Convert markdown to HTML
        let html_body = self.markdown_to_html(&parsed.body);

        Ok(Some(PublishedPage {
            source_path: path.to_path_buf(),
            dest_filename,
            title,
            html_body,
            markdown_body: parsed.body,
            contents_links,
            parent_link,
            is_root,
        }))
    }

    /// Build navigation links from contents property
    async fn build_contents_links(
        &self,
        fm: &indexmap::IndexMap<String, serde_yaml::Value>,
        current_path: &Path,
        path_to_filename: &HashMap<PathBuf, String>,
    ) -> Vec<NavLink> {
        let contents = frontmatter::get_string_array(fm, "contents");
        let current_dir = current_path.parent();

        let mut links = Vec::new();
        for child_ref in contents {
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
                .await
                .unwrap_or_else(|| self.filename_to_title(&child_ref));

            links.push(NavLink { href, title });
        }
        links
    }

    /// Build parent navigation link from part_of property
    async fn build_parent_link(
        &self,
        fm: &indexmap::IndexMap<String, serde_yaml::Value>,
        current_path: &Path,
        path_to_filename: &HashMap<PathBuf, String>,
    ) -> Option<NavLink> {
        let part_of = frontmatter::get_string(fm, "part_of")?;
        let current_dir = current_path.parent();

        let parent_path = current_dir
            .map(|d| d.join(part_of))
            .unwrap_or_else(|| PathBuf::from(part_of));

        let href = path_to_filename
            .get(&parent_path)
            .cloned()
            .unwrap_or_else(|| self.path_to_html_filename(&parent_path));

        let title = self
            .get_title_from_file(&parent_path)
            .await
            .unwrap_or_else(|| self.filename_to_title(part_of));

        Some(NavLink { href, title })
    }

    /// Get title from a file's frontmatter
    async fn get_title_from_file(&self, path: &Path) -> Option<String> {
        let content = self.fs.read_to_string(path).await.ok()?;
        let parsed = frontmatter::parse_or_empty(&content).ok()?;
        frontmatter::get_string(&parsed.frontmatter, "title").map(String::from)
    }

    /// Convert a path to an HTML filename
    fn path_to_html_filename(&self, path: &Path) -> String {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("page");

        format!("{}.html", slugify(stem))
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

    /// Convert markdown to HTML using comrak
    #[cfg(feature = "markdown")]
    fn markdown_to_html(&self, markdown: &str) -> String {
        use comrak::{Options, markdown_to_html};

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
    #[cfg(not(target_arch = "wasm32"))]
    async fn write_multi_file(
        &self,
        pages: &[PublishedPage],
        destination: &Path,
        options: &PublishOptions,
    ) -> Result<()> {
        // Create destination directory
        self.fs.create_dir_all(destination).await?;

        let site_title = options.title.clone().unwrap_or_else(|| {
            pages
                .first()
                .map(|p| p.title.clone())
                .unwrap_or_else(|| "Journal".to_string())
        });

        for page in pages {
            let html = self.render_page(page, &site_title, false);
            let dest_path = destination.join(&page.dest_filename);
            self.fs.write_file(&dest_path, &html).await?;
        }

        // Write CSS file
        let css_path = destination.join("style.css");
        self.fs.write_file(&css_path, Self::get_css()).await?;

        Ok(())
    }

    /// Write a single HTML file containing all pages
    #[cfg(not(target_arch = "wasm32"))]
    async fn write_single_file(
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
            self.fs.create_dir_all(parent).await?;
        }

        self.fs.write_file(destination, &html).await?;

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
        slugify(title)
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
