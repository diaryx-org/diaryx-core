//! Async workspace operations for WASM with native Promise support.
//!
//! This module provides async workspace operations that work directly with
//! `JsAsyncFileSystem`, returning native JavaScript Promises. This enables
//! proper async/await patterns in the web frontend without the need for
//! synchronous wrappers or `block_on`.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { JsAsyncFileSystem, DiaryxAsyncWorkspace } from './wasm/diaryx_wasm.js';
//!
//! // Create filesystem with your storage backend callbacks
//! const fs = new JsAsyncFileSystem({ /* callbacks */ });
//!
//! // Create async workspace instance
//! const workspace = new DiaryxAsyncWorkspace(fs);
//!
//! // All methods return native Promises
//! const tree = await workspace.getTree('workspace');
//! const created = await workspace.create('new-workspace', 'My New Workspace');
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use diaryx_core::fs::AsyncFileSystem;
use diaryx_core::workspace::Workspace;
use js_sys::Promise;
use serde::Serialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::error::IntoJsResult;
use crate::js_async_fs::JsAsyncFileSystem;

// ============================================================================
// Types
// ============================================================================

/// Tree node returned to JavaScript
#[derive(Debug, Serialize)]
pub struct JsAsyncTreeNode {
    pub name: String,
    pub description: Option<String>,
    pub path: String,
    pub children: Vec<JsAsyncTreeNode>,
}

impl From<diaryx_core::workspace::TreeNode> for JsAsyncTreeNode {
    fn from(node: diaryx_core::workspace::TreeNode) -> Self {
        JsAsyncTreeNode {
            name: node.name,
            description: node.description,
            path: node.path.to_string_lossy().to_string(),
            children: node
                .children
                .into_iter()
                .map(JsAsyncTreeNode::from)
                .collect(),
        }
    }
}

// ============================================================================
// DiaryxAsyncWorkspace Class
// ============================================================================

/// Async workspace operations with native Promise support.
///
/// Unlike `DiaryxWorkspace` which uses `block_on` internally, this class
/// returns true JavaScript Promises that can be properly awaited. This enables:
///
/// - Proper async/await patterns in JavaScript
/// - Non-blocking operations in the browser
/// - Integration with truly async storage backends (IndexedDB, OPFS)
#[wasm_bindgen]
pub struct DiaryxAsyncWorkspace {
    fs: JsAsyncFileSystem,
}

#[wasm_bindgen]
impl DiaryxAsyncWorkspace {
    /// Create a new DiaryxAsyncWorkspace with the provided filesystem.
    #[wasm_bindgen(constructor)]
    pub fn new(fs: JsAsyncFileSystem) -> Self {
        Self { fs }
    }

    /// Get the workspace tree structure.
    ///
    /// Returns a Promise that resolves to the tree structure starting from
    /// the workspace root index.
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @param depth - Optional maximum depth to traverse (null for unlimited)
    /// @returns Promise resolving to the tree structure
    #[wasm_bindgen(js_name = "getTree")]
    pub fn get_tree(&self, workspace_path: String, depth: Option<u32>) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            // Find root index in the workspace
            let root_index = ws
                .find_root_index_in_dir(&root_path)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?
                .or_else(|| {
                    // Try to find any index if root index not found
                    // We can't use async in or_else, so we handle this differently
                    None
                });

            // If no root index found, try finding any index
            let root_index = match root_index {
                Some(idx) => idx,
                None => ws
                    .find_any_index_in_dir(&root_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?
                    .ok_or_else(|| {
                        JsValue::from_str(&format!("No workspace found at '{}'", workspace_path))
                    })?,
            };

            let max_depth = depth.map(|d| d as usize);
            let mut visited = HashSet::new();

            let tree = ws
                .build_tree_with_depth(&root_index, max_depth, &mut visited)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_tree: JsAsyncTreeNode = tree.into();
            serde_wasm_bindgen::to_value(&js_tree).js_err()
        })
    }

    /// Create a new workspace with an index.md file.
    ///
    /// @param path - Path where the workspace should be created
    /// @param name - Name of the workspace (used in the index title)
    /// @returns Promise that resolves when the workspace is created
    #[wasm_bindgen]
    pub fn create(&self, path: String, name: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let index_path = PathBuf::from(&path).join("index.md");

            if fs.exists(&index_path).await {
                return Err(JsValue::from_str(&format!(
                    "Workspace already exists at '{}'",
                    path
                )));
            }

            let content = format!(
                "---\ntitle: \"{}\"\ncontents: []\n---\n\n# {}\n",
                name, name
            );

            fs.write_file(&index_path, &content)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            Ok(JsValue::UNDEFINED)
        })
    }

    /// Get the filesystem tree structure (for "Show All Files" mode).
    ///
    /// This returns a tree of all files and directories, not just those
    /// in the workspace hierarchy.
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @param show_hidden - Whether to include hidden files (starting with .)
    /// @returns Promise resolving to the filesystem tree structure
    #[wasm_bindgen(js_name = "getFilesystemTree")]
    pub fn get_filesystem_tree(&self, workspace_path: String, show_hidden: bool) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let root_path = PathBuf::from(&workspace_path);

            async fn build_tree(
                fs: &JsAsyncFileSystem,
                path: &Path,
                show_hidden: bool,
            ) -> Result<JsAsyncTreeNode, String> {
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string());

                if !show_hidden && name.starts_with('.') {
                    return Err("hidden".to_string());
                }

                let mut children = Vec::new();

                if fs.is_dir(path).await {
                    if let Ok(entries) = fs.list_files(path).await {
                        for entry in entries {
                            if let Ok(child) = Box::pin(build_tree(fs, &entry, show_hidden)).await {
                                children.push(child);
                            }
                        }
                    }
                    // Sort: directories first, then alphabetically
                    children.sort_by(|a, b| {
                        let a_is_dir = !a.children.is_empty();
                        let b_is_dir = !b.children.is_empty();
                        match (a_is_dir, b_is_dir) {
                            (true, false) => std::cmp::Ordering::Less,
                            (false, true) => std::cmp::Ordering::Greater,
                            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                        }
                    });
                }

                Ok(JsAsyncTreeNode {
                    name,
                    description: None,
                    path: path.to_string_lossy().to_string(),
                    children,
                })
            }

            let tree = build_tree(&fs, &root_path, show_hidden)
                .await
                .map_err(|e| JsValue::from_str(&e))?;

            serde_wasm_bindgen::to_value(&tree).js_err()
        })
    }

    /// Find the root index file in a workspace directory.
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @returns Promise resolving to the root index path, or null if not found
    #[wasm_bindgen(js_name = "findRootIndex")]
    pub fn find_root_index(&self, workspace_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            match ws.find_root_index_in_dir(&root_path).await {
                Ok(Some(index_path)) => Ok(JsValue::from_str(&index_path.to_string_lossy())),
                Ok(None) => Ok(JsValue::NULL),
                Err(e) => Err(JsValue::from_str(&e.to_string())),
            }
        })
    }

    /// Check if a path is a workspace (contains an index file).
    ///
    /// @param path - Path to check
    /// @returns Promise resolving to true if the path is a workspace
    #[wasm_bindgen(js_name = "isWorkspace")]
    pub fn is_workspace(&self, path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&fs);
            let check_path = PathBuf::from(&path);

            let is_workspace = ws
                .find_any_index_in_dir(&check_path)
                .await
                .map(|opt| opt.is_some())
                .unwrap_or(false);

            Ok(JsValue::from_bool(is_workspace))
        })
    }

    /// Build the full tree starting from a specific index file.
    ///
    /// @param index_path - Path to the index file to start from
    /// @param depth - Optional maximum depth to traverse
    /// @returns Promise resolving to the tree structure
    #[wasm_bindgen(js_name = "buildTreeFromIndex")]
    pub fn build_tree_from_index(&self, index_path: String, depth: Option<u32>) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&fs);
            let root_index = PathBuf::from(&index_path);

            let max_depth = depth.map(|d| d as usize);
            let mut visited = HashSet::new();

            let tree = ws
                .build_tree_with_depth(&root_index, max_depth, &mut visited)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_tree: JsAsyncTreeNode = tree.into();
            serde_wasm_bindgen::to_value(&js_tree).js_err()
        })
    }
}

impl Default for DiaryxAsyncWorkspace {
    fn default() -> Self {
        // Create with an empty filesystem - caller should use new() with proper fs
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
 * Tree node representing a workspace entry.
 */
export interface AsyncTreeNode {
    /** Display name of the node */
    name: string;
    /** Optional description from frontmatter */
    description: string | null;
    /** Path to the file */
    path: string;
    /** Child nodes */
    children: AsyncTreeNode[];
}
"#;
