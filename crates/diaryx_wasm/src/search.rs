//! Search operations for WASM.

use std::path::PathBuf;

use diaryx_core::search::{SearchQuery, Searcher};
use diaryx_core::workspace::Workspace;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::error::IntoJsResult;
use crate::state::{block_on, with_async_fs};

// ============================================================================
// Types
// ============================================================================

/// Search result returned to JavaScript
#[derive(Debug, Serialize)]
pub struct JsSearchResults {
    pub files: Vec<JsFileSearchResult>,
    pub files_searched: u32,
}

#[derive(Debug, Serialize)]
pub struct JsFileSearchResult {
    pub path: String,
    pub title: Option<String>,
    pub matches: Vec<JsSearchMatch>,
}

#[derive(Debug, Serialize)]
pub struct JsSearchMatch {
    pub line_number: u32,
    pub line_content: String,
    pub match_start: u32,
    pub match_end: u32,
}

/// Search options
#[derive(Debug, Deserialize, Default)]
pub struct SearchOptions {
    pub workspace_path: Option<String>,
    pub search_frontmatter: Option<bool>,
    pub property: Option<String>,
    pub case_sensitive: Option<bool>,
}

// ============================================================================
// DiaryxSearch Class
// ============================================================================

/// Search operations for finding content in the workspace.
#[wasm_bindgen]
pub struct DiaryxSearch;

#[wasm_bindgen]
impl DiaryxSearch {
    /// Create a new DiaryxSearch instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Search the workspace for entries matching a pattern.
    #[wasm_bindgen]
    pub fn search(&self, pattern: &str, options: JsValue) -> Result<JsValue, JsValue> {
        let opts: SearchOptions = if options.is_undefined() || options.is_null() {
            SearchOptions::default()
        } else {
            serde_wasm_bindgen::from_value(options).js_err()?
        };

        with_async_fs(|fs| {
            let searcher = Searcher::new(fs.clone());
            let ws = Workspace::new(fs);

            let workspace_path = opts.workspace_path.as_deref().unwrap_or("workspace");
            let root_path = PathBuf::from(workspace_path);

            let root_index = block_on(ws.find_root_index_in_dir(&root_path))
                .js_err()?
                .or_else(|| {
                    block_on(ws.find_any_index_in_dir(&root_path))
                        .ok()
                        .flatten()
                })
                .ok_or_else(|| {
                    JsValue::from_str(&format!("No workspace found at '{}'", workspace_path))
                })?;

            let query = if let Some(ref prop) = opts.property {
                SearchQuery::property(pattern, prop)
            } else if opts.search_frontmatter.unwrap_or(false) {
                SearchQuery::frontmatter(pattern)
            } else {
                SearchQuery::content(pattern)
            };

            let query = query.case_sensitive(opts.case_sensitive.unwrap_or(false));

            let results = block_on(searcher.search_workspace(&root_index, &query)).js_err()?;

            let js_results = JsSearchResults {
                files_searched: results.files_searched as u32,
                files: results
                    .files
                    .into_iter()
                    .map(|f| JsFileSearchResult {
                        path: f.path.to_string_lossy().to_string(),
                        title: f.title,
                        matches: f
                            .matches
                            .into_iter()
                            .map(|m| JsSearchMatch {
                                line_number: m.line_number as u32,
                                line_content: m.line_content,
                                match_start: m.match_start as u32,
                                match_end: m.match_end as u32,
                            })
                            .collect(),
                    })
                    .collect(),
            };

            serde_wasm_bindgen::to_value(&js_results).js_err()
        })
    }
}

impl Default for DiaryxSearch {
    fn default() -> Self {
        Self::new()
    }
}
