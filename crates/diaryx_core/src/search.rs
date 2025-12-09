//! Search functionality for diaryx workspaces
//!
//! Provides searching through workspace files by content or frontmatter properties.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::fs::FileSystem;
use crate::workspace::Workspace;

/// Represents a search query configuration
#[derive(Debug, Clone, Serialize)]
pub struct SearchQuery {
    /// The pattern to search for
    pub pattern: String,
    /// Whether the search is case-sensitive
    pub case_sensitive: bool,
    /// Search mode: content, frontmatter, or specific property
    pub mode: SearchMode,
}

/// What to search in files
#[derive(Debug, Clone, Serialize)]
pub enum SearchMode {
    /// Search only the body content (after frontmatter)
    Content,
    /// Search all frontmatter properties
    Frontmatter,
    /// Search a specific frontmatter property
    Property(String),
}

impl SearchQuery {
    /// Create a new content search query
    pub fn content(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            case_sensitive: false,
            mode: SearchMode::Content,
        }
    }

    /// Create a new frontmatter search query
    pub fn frontmatter(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            case_sensitive: false,
            mode: SearchMode::Frontmatter,
        }
    }

    /// Create a search query for a specific property
    pub fn property(pattern: impl Into<String>, property_name: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            case_sensitive: false,
            mode: SearchMode::Property(property_name.into()),
        }
    }

    /// Set case sensitivity
    pub fn case_sensitive(mut self, case_sensitive: bool) -> Self {
        self.case_sensitive = case_sensitive;
        self
    }
}

/// A single match within a file
#[derive(Debug, Clone, Serialize)]
pub struct SearchMatch {
    /// Line number (1-based)
    pub line_number: usize,
    /// The full line content
    pub line_content: String,
    /// Column where match starts (0-based)
    pub match_start: usize,
    /// Column where match ends (0-based, exclusive)
    pub match_end: usize,
}

/// Search results for a single file
#[derive(Debug, Clone, Serialize)]
pub struct FileSearchResult {
    /// Path to the file
    pub path: PathBuf,
    /// Title from frontmatter (if available)
    pub title: Option<String>,
    /// All matches found in this file
    pub matches: Vec<SearchMatch>,
}

impl FileSearchResult {
    /// Returns true if this result has any matches
    pub fn has_matches(&self) -> bool {
        !self.matches.is_empty()
    }

    /// Returns the number of matches
    pub fn match_count(&self) -> usize {
        self.matches.len()
    }
}

/// Aggregated search results
#[derive(Debug, Clone, Serialize)]
pub struct SearchResults {
    /// Results per file (only files with matches)
    pub files: Vec<FileSearchResult>,
    /// Total number of files searched
    pub files_searched: usize,
}

impl SearchResults {
    /// Create empty results
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            files_searched: 0,
        }
    }

    /// Total number of matches across all files
    pub fn total_matches(&self) -> usize {
        self.files.iter().map(|f| f.match_count()).sum()
    }

    /// Number of files with matches
    pub fn files_with_matches(&self) -> usize {
        self.files.len()
    }
}

impl Default for SearchResults {
    fn default() -> Self {
        Self::new()
    }
}

/// Searcher for workspace files
pub struct Searcher<FS: FileSystem> {
    fs: FS,
}

impl<FS: FileSystem + Clone> Searcher<FS> {
    /// Create a new searcher
    pub fn new(fs: FS) -> Self {
        Self { fs }
    }

    /// Search the entire workspace starting from the root index
    pub fn search_workspace(
        &self,
        workspace_root: &Path,
        query: &SearchQuery,
    ) -> crate::error::Result<SearchResults> {
        let workspace = Workspace::new(self.fs.clone());
        let files = workspace.collect_workspace_files(workspace_root)?;

        let mut results = SearchResults::new();
        results.files_searched = files.len();

        for file_path in files {
            if let Some(file_result) = self.search_file(&file_path, query)? {
                if file_result.has_matches() {
                    results.files.push(file_result);
                }
            }
        }

        Ok(results)
    }

    /// Search a single file
    pub fn search_file(
        &self,
        path: &Path,
        query: &SearchQuery,
    ) -> crate::error::Result<Option<FileSearchResult>> {
        let content = match self.fs.read_to_string(path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(crate::error::DiaryxError::FileRead {
                    path: path.to_path_buf(),
                    source: e,
                })
            }
        };

        let (frontmatter_str, body, title) = self.parse_file_parts(&content);

        let matches = match &query.mode {
            SearchMode::Content => self.search_text(&body, &query.pattern, query.case_sensitive),
            SearchMode::Frontmatter => {
                self.search_text(&frontmatter_str, &query.pattern, query.case_sensitive)
            }
            SearchMode::Property(prop_name) => self.search_property(
                &frontmatter_str,
                prop_name,
                &query.pattern,
                query.case_sensitive,
            ),
        };

        Ok(Some(FileSearchResult {
            path: path.to_path_buf(),
            title,
            matches,
        }))
    }

    /// Parse file into frontmatter string, body, and title
    fn parse_file_parts(&self, content: &str) -> (String, String, Option<String>) {
        // Check for frontmatter
        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            return (String::new(), content.to_string(), None);
        }

        let rest = &content[4..]; // Skip "---\n"
        let end_idx = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n"));

        match end_idx {
            Some(idx) => {
                let frontmatter_str = rest[..idx].to_string();
                let body = rest[idx + 5..].to_string(); // Skip "\n---\n"

                // Extract title from frontmatter
                let title = self.extract_title(&frontmatter_str);

                (frontmatter_str, body, title)
            }
            None => {
                // Malformed frontmatter, treat entire content as body
                (String::new(), content.to_string(), None)
            }
        }
    }

    /// Extract title from frontmatter string
    fn extract_title(&self, frontmatter: &str) -> Option<String> {
        for line in frontmatter.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("title:") {
                let title = rest.trim();
                // Remove quotes if present
                let title = title.trim_matches('"').trim_matches('\'');
                if !title.is_empty() {
                    return Some(title.to_string());
                }
            }
        }
        None
    }

    /// Search text for pattern, returning all matches with line info
    fn search_text(&self, text: &str, pattern: &str, case_sensitive: bool) -> Vec<SearchMatch> {
        let mut matches = Vec::new();

        let search_pattern = if case_sensitive {
            pattern.to_string()
        } else {
            pattern.to_lowercase()
        };

        for (line_idx, line) in text.lines().enumerate() {
            let search_line = if case_sensitive {
                line.to_string()
            } else {
                line.to_lowercase()
            };

            // Find all occurrences in this line
            let mut start = 0;
            while let Some(pos) = search_line[start..].find(&search_pattern) {
                let match_start = start + pos;
                let match_end = match_start + pattern.len();

                matches.push(SearchMatch {
                    line_number: line_idx + 1,
                    line_content: line.to_string(),
                    match_start,
                    match_end,
                });

                start = match_end;
            }
        }

        matches
    }

    /// Search for pattern within a specific frontmatter property
    fn search_property(
        &self,
        frontmatter: &str,
        property: &str,
        pattern: &str,
        case_sensitive: bool,
    ) -> Vec<SearchMatch> {
        let mut matches = Vec::new();
        let mut in_property = false;
        let mut property_indent: Option<usize> = None;

        let prop_prefix = format!("{}:", property);
        let search_pattern = if case_sensitive {
            pattern.to_string()
        } else {
            pattern.to_lowercase()
        };

        for (line_idx, line) in frontmatter.lines().enumerate() {
            let trimmed = line.trim_start();
            let indent = line.len() - trimmed.len();

            // Check if this line starts a new property
            if trimmed.contains(':') && !trimmed.starts_with('-') && !trimmed.starts_with('#') {
                // Check if it's our target property
                if trimmed.starts_with(&prop_prefix) {
                    in_property = true;
                    property_indent = Some(indent);

                    // Check value on same line
                    let value_part = trimmed[prop_prefix.len()..].trim();
                    if !value_part.is_empty() {
                        let search_value = if case_sensitive {
                            value_part.to_string()
                        } else {
                            value_part.to_lowercase()
                        };

                        if let Some(pos) = search_value.find(&search_pattern) {
                            let offset = line.find(value_part).unwrap_or(0);
                            matches.push(SearchMatch {
                                line_number: line_idx + 1,
                                line_content: line.to_string(),
                                match_start: offset + pos,
                                match_end: offset + pos + pattern.len(),
                            });
                        }
                    }
                } else if indent <= property_indent.unwrap_or(0) {
                    // Different property at same or lower indent level
                    in_property = false;
                    property_indent = None;
                }
            } else if in_property {
                // Continuation of property value (array items, multiline, etc.)
                if let Some(prop_indent) = property_indent {
                    if indent <= prop_indent && !trimmed.is_empty() {
                        // Back to same or lower indent, property ended
                        in_property = false;
                        property_indent = None;
                    } else {
                        // Still in property, search this line
                        let search_line = if case_sensitive {
                            line.to_string()
                        } else {
                            line.to_lowercase()
                        };

                        if let Some(pos) = search_line.find(&search_pattern) {
                            matches.push(SearchMatch {
                                line_number: line_idx + 1,
                                line_content: line.to_string(),
                                match_start: pos,
                                match_end: pos + pattern.len(),
                            });
                        }
                    }
                }
            }
        }

        matches
    }
}

impl<FS: FileSystem + Clone> Clone for Searcher<FS> {
    fn clone(&self) -> Self {
        Self {
            fs: self.fs.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::io::{Error, ErrorKind, Result};

    #[derive(Clone)]
    struct MockFs {
        files: std::rc::Rc<RefCell<HashMap<PathBuf, String>>>,
    }

    impl MockFs {
        fn new() -> Self {
            Self {
                files: std::rc::Rc::new(RefCell::new(HashMap::new())),
            }
        }

        fn with_file(self, path: &str, content: &str) -> Self {
            self.files
                .borrow_mut()
                .insert(PathBuf::from(path), content.to_string());
            self
        }
    }

    impl FileSystem for MockFs {
        fn read_to_string(&self, path: &Path) -> Result<String> {
            self.files
                .borrow()
                .get(path)
                .cloned()
                .ok_or_else(|| Error::new(ErrorKind::NotFound, "file not found"))
        }

        fn write_file(&self, path: &Path, content: &str) -> Result<()> {
            self.files
                .borrow_mut()
                .insert(path.to_path_buf(), content.to_string());
            Ok(())
        }

        fn create_new(&self, path: &Path, content: &str) -> Result<()> {
            if self.files.borrow().contains_key(path) {
                return Err(Error::new(ErrorKind::AlreadyExists, "file exists"));
            }
            self.files
                .borrow_mut()
                .insert(path.to_path_buf(), content.to_string());
            Ok(())
        }

        fn delete_file(&self, path: &Path) -> Result<()> {
            self.files.borrow_mut().remove(path);
            Ok(())
        }

        fn list_md_files(&self, dir: &Path) -> Result<Vec<PathBuf>> {
            Ok(self
                .files
                .borrow()
                .keys()
                .filter(|p| p.starts_with(dir) && p.extension().is_some_and(|e| e == "md"))
                .cloned()
                .collect())
        }

        fn exists(&self, path: &Path) -> bool {
            self.files.borrow().contains_key(path)
        }
    }

    #[test]
    fn test_search_content() {
        let fs = MockFs::new().with_file(
            "/test/entry.md",
            "---\ntitle: Test Entry\n---\n\nThis is some content.\nWith multiple lines.\n",
        );

        let searcher = Searcher::new(fs);
        let query = SearchQuery::content("content");

        let result = searcher
            .search_file(Path::new("/test/entry.md"), &query)
            .unwrap()
            .unwrap();

        assert_eq!(result.title, Some("Test Entry".to_string()));
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].line_number, 2); // Line 2 of body
        assert!(result.matches[0].line_content.contains("content"));
    }

    #[test]
    fn test_search_content_case_insensitive() {
        let fs = MockFs::new().with_file(
            "/test/entry.md",
            "---\ntitle: Test\n---\n\nHello WORLD and world.\n",
        );

        let searcher = Searcher::new(fs);
        let query = SearchQuery::content("world");

        let result = searcher
            .search_file(Path::new("/test/entry.md"), &query)
            .unwrap()
            .unwrap();

        // Should find both "WORLD" and "world"
        assert_eq!(result.matches.len(), 2);
    }

    #[test]
    fn test_search_content_case_sensitive() {
        let fs = MockFs::new().with_file(
            "/test/entry.md",
            "---\ntitle: Test\n---\n\nHello WORLD and world.\n",
        );

        let searcher = Searcher::new(fs);
        let query = SearchQuery::content("world").case_sensitive(true);

        let result = searcher
            .search_file(Path::new("/test/entry.md"), &query)
            .unwrap()
            .unwrap();

        // Should only find lowercase "world"
        assert_eq!(result.matches.len(), 1);
    }

    #[test]
    fn test_search_frontmatter() {
        let fs = MockFs::new().with_file(
            "/test/entry.md",
            "---\ntitle: Important Meeting\ndescription: A very important meeting\n---\n\nBody content here.\n",
        );

        let searcher = Searcher::new(fs);
        let query = SearchQuery::frontmatter("important");

        let result = searcher
            .search_file(Path::new("/test/entry.md"), &query)
            .unwrap()
            .unwrap();

        // Should find in both title and description
        assert_eq!(result.matches.len(), 2);
    }

    #[test]
    fn test_search_specific_property() {
        let fs = MockFs::new().with_file(
            "/test/entry.md",
            "---\ntitle: Meeting Notes\ntags:\n  - important\n  - work\n---\n\nSome important content.\n",
        );

        let searcher = Searcher::new(fs);
        let query = SearchQuery::property("important", "tags");

        let result = searcher
            .search_file(Path::new("/test/entry.md"), &query)
            .unwrap()
            .unwrap();

        // Should only find in tags, not in body
        assert_eq!(result.matches.len(), 1);
        assert!(result.matches[0].line_content.contains("important"));
    }

    #[test]
    fn test_search_no_frontmatter() {
        let fs =
            MockFs::new().with_file("/test/entry.md", "Just plain content.\nNo frontmatter.\n");

        let searcher = Searcher::new(fs);
        let query = SearchQuery::content("plain");

        let result = searcher
            .search_file(Path::new("/test/entry.md"), &query)
            .unwrap()
            .unwrap();

        assert!(result.title.is_none());
        assert_eq!(result.matches.len(), 1);
    }

    #[test]
    fn test_extract_title_with_quotes() {
        let fs = MockFs::new().with_file(
            "/test/entry.md",
            "---\ntitle: \"Quoted Title\"\n---\n\nContent.\n",
        );

        let searcher = Searcher::new(fs);
        let query = SearchQuery::content("Content");

        let result = searcher
            .search_file(Path::new("/test/entry.md"), &query)
            .unwrap()
            .unwrap();

        assert_eq!(result.title, Some("Quoted Title".to_string()));
    }
}
