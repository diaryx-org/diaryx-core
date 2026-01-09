//! Async search operations for WASM with native Promise support.
//!
//! This module provides async search operations that work directly with
//! `JsAsyncFileSystem`, returning native JavaScript Promises. This enables
//! proper async/await patterns in the web frontend without the need for
//! synchronous wrappers or `block_on`.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { JsAsyncFileSystem, DiaryxAsyncSearch } from './wasm/diaryx_wasm.js';
//!
//! // Create filesystem with your storage backend callbacks
//! const fs = new JsAsyncFileSystem({ /* callbacks */ });
//!
//! // Create async search instance
//! const search = new DiaryxAsyncSearch(fs);
//!
//! // All methods return native Promises
//! const results = await search.search('workspace', 'my query', {
//!     searchFrontmatter: true,
//!     caseSensitive: false
//! });
//! ```

use std::path::PathBuf;

use diaryx_core::search::{SearchQuery, Searcher};
use js_sys::Promise;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::error::IntoJsResult;
use crate::js_async_fs::JsAsyncFileSystem;

// ============================================================================
// Types
// ============================================================================

/// A single match within a file
#[derive(Debug, Serialize)]
pub struct JsAsyncSearchMatch {
    /// Line number (1-indexed)
    pub line_number: usize,
    /// The matching line content
    pub line_content: String,
    /// Column where match starts (0-based)
    pub match_start: usize,
    /// Column where match ends (0-based, exclusive)
    pub match_end: usize,
}

impl From<diaryx_core::search::SearchMatch> for JsAsyncSearchMatch {
    fn from(m: diaryx_core::search::SearchMatch) -> Self {
        JsAsyncSearchMatch {
            line_number: m.line_number,
            line_content: m.line_content,
            match_start: m.match_start,
            match_end: m.match_end,
        }
    }
}

/// Search result for a single file
#[derive(Debug, Serialize)]
pub struct JsAsyncFileSearchResult {
    /// Path to the file
    pub path: String,
    /// Title from frontmatter, if available
    pub title: Option<String>,
    /// Matches found in this file
    pub matches: Vec<JsAsyncSearchMatch>,
}

impl From<diaryx_core::search::FileSearchResult> for JsAsyncFileSearchResult {
    fn from(r: diaryx_core::search::FileSearchResult) -> Self {
        JsAsyncFileSearchResult {
            path: r.path.to_string_lossy().to_string(),
            title: r.title,
            matches: r.matches.into_iter().map(JsAsyncSearchMatch::from).collect(),
        }
    }
}

/// Complete search results
#[derive(Debug, Serialize)]
pub struct JsAsyncSearchResults {
    /// Files with matches
    pub files: Vec<JsAsyncFileSearchResult>,
    /// Total number of matches across all files
    pub total_matches: usize,
    /// Number of files searched
    pub files_searched: usize,
}

impl From<diaryx_core::search::SearchResults> for JsAsyncSearchResults {
    fn from(r: diaryx_core::search::SearchResults) -> Self {
        let total_matches = r.total_matches();
        JsAsyncSearchResults {
            files: r.files.into_iter().map(JsAsyncFileSearchResult::from).collect(),
            total_matches,
            files_searched: r.files_searched,
        }
    }
}

/// Search options from JavaScript
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JsAsyncSearchOptions {
    /// Search in frontmatter (default: false, searches content)
    #[serde(default)]
    pub search_frontmatter: bool,
    /// Search only in a specific frontmatter property
    pub property: Option<String>,
    /// Case-sensitive search (default: false)
    #[serde(default)]
    pub case_sensitive: bool,
}

// ============================================================================
// DiaryxAsyncSearch Class
// ============================================================================

/// Async search operations with native Promise support.
///
/// Unlike `DiaryxSearch` which uses `block_on` internally, this class
/// returns true JavaScript Promises that can be properly awaited.
#[wasm_bindgen]
pub struct DiaryxAsyncSearch {
    fs: JsAsyncFileSystem,
}

#[wasm_bindgen]
impl DiaryxAsyncSearch {
    /// Create a new DiaryxAsyncSearch with the provided filesystem.
    #[wasm_bindgen(constructor)]
    pub fn new(fs: JsAsyncFileSystem) -> Self {
        Self { fs }
    }

    /// Search for content in the workspace.
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @param pattern - The search pattern (string or regex)
    /// @param options - Optional search options
    /// @returns Promise resolving to search results
    #[wasm_bindgen]
    pub fn search(
        &self,
        workspace_path: String,
        pattern: String,
        options: JsValue,
    ) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let searcher = Searcher::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            // Parse options
            let opts: JsAsyncSearchOptions = if options.is_undefined() || options.is_null() {
                JsAsyncSearchOptions::default()
            } else {
                serde_wasm_bindgen::from_value(options)
                    .map_err(|e| JsValue::from_str(&format!("Invalid options: {}", e)))?
            };

            // Build search query
            let query = if let Some(prop) = opts.property {
                SearchQuery::property(&pattern, &prop)
            } else if opts.search_frontmatter {
                SearchQuery::frontmatter(&pattern)
            } else {
                SearchQuery::content(&pattern)
            };

            let query = query.case_sensitive(opts.case_sensitive);

            // Execute search
            let results = searcher
                .search_workspace(&root_path, &query)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_results: JsAsyncSearchResults = results.into();
            serde_wasm_bindgen::to_value(&js_results).js_err()
        })
    }

    /// Search for content in the workspace (content only).
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @param pattern - The search pattern
    /// @returns Promise resolving to search results
    #[wasm_bindgen(js_name = "searchContent")]
    pub fn search_content(&self, workspace_path: String, pattern: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let searcher = Searcher::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            let query = SearchQuery::content(&pattern);
            let results = searcher
                .search_workspace(&root_path, &query)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_results: JsAsyncSearchResults = results.into();
            serde_wasm_bindgen::to_value(&js_results).js_err()
        })
    }

    /// Search for a specific frontmatter property value.
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @param property - The frontmatter property to search
    /// @param pattern - The search pattern
    /// @returns Promise resolving to search results
    #[wasm_bindgen(js_name = "searchProperty")]
    pub fn search_property(
        &self,
        workspace_path: String,
        property: String,
        pattern: String,
    ) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let searcher = Searcher::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            let query = SearchQuery::property(&pattern, &property);
            let results = searcher
                .search_workspace(&root_path, &query)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_results: JsAsyncSearchResults = results.into();
            serde_wasm_bindgen::to_value(&js_results).js_err()
        })
    }

    /// Search with case sensitivity.
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @param pattern - The search pattern
    /// @param case_sensitive - Whether to match case
    /// @returns Promise resolving to search results
    #[wasm_bindgen(js_name = "searchWithCase")]
    pub fn search_with_case(
        &self,
        workspace_path: String,
        pattern: String,
        case_sensitive: bool,
    ) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let searcher = Searcher::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            let query = SearchQuery::content(&pattern).case_sensitive(case_sensitive);

            let results = searcher
                .search_workspace(&root_path, &query)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_results: JsAsyncSearchResults = results.into();
            serde_wasm_bindgen::to_value(&js_results).js_err()
        })
    }
}

impl Default for DiaryxAsyncSearch {
    fn default() -> Self {
        Self {
            fs: JsAsyncFileSystem::new(JsValue::NULL),
        }
    }
}

// ============================================================================
// TypeScript Type Definitions
// ============================================================================

#[wasm_bindgen(typescript_custom_section)]
const TS_APPEND_CONTENT: &'static str = r#"
/**
 * Options for search operations.
 */
export interface AsyncSearchOptions {
    /** Search in frontmatter instead of content (default: false) */
    searchFrontmatter?: boolean;
    /** Search only in a specific frontmatter property */
    property?: string;
    /** Case-sensitive search (default: false) */
    caseSensitive?: boolean;
}

/**
 * A single match within a file.
 */
export interface AsyncSearchMatch {
    /** Line number (1-indexed) */
    line_number: number;
    /** The matching line content */
    line_content: string;
    /** Column where match starts (0-based) */
    match_start: number;
    /** Column where match ends (0-based, exclusive) */
    match_end: number;
}

/**
 * Search result for a single file.
 */
export interface AsyncFileSearchResult {
    /** Path to the file */
    path: string;
    /** Title from frontmatter, if available */
    title: string | null;
    /** Matches found in this file */
    matches: AsyncSearchMatch[];
}

/**
 * Complete search results.
 */
export interface AsyncSearchResults {
    /** Files with matches */
    files: AsyncFileSearchResult[];
    /** Total number of matches across all files */
    total_matches: number;
    /** Number of files searched */
    files_searched: number;
}
"#;