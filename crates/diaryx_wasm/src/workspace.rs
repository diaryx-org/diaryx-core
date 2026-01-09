//! Workspace operations for WASM.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use diaryx_core::fs::{AsyncFileSystem, FileSystem};
use diaryx_core::workspace::Workspace;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::error::IntoJsResult;
use crate::state::{block_on, with_async_fs, with_fs};

// ============================================================================
// Types
// ============================================================================

/// Tree node returned to JavaScript
#[derive(Debug, Serialize)]
pub struct JsTreeNode {
    pub name: String,
    pub description: Option<String>,
    pub path: String,
    pub children: Vec<JsTreeNode>,
}

impl From<diaryx_core::workspace::TreeNode> for JsTreeNode {
    fn from(node: diaryx_core::workspace::TreeNode) -> Self {
        JsTreeNode {
            name: node.name,
            description: node.description,
            path: node.path.to_string_lossy().to_string(),
            children: node.children.into_iter().map(JsTreeNode::from).collect(),
        }
    }
}

// ============================================================================
// DiaryxWorkspace Class
// ============================================================================

/// Workspace operations for managing workspace structure.
#[wasm_bindgen]
pub struct DiaryxWorkspace;

#[wasm_bindgen]
impl DiaryxWorkspace {
    /// Create a new DiaryxWorkspace instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Get the workspace tree structure.
    #[wasm_bindgen]
    pub fn get_tree(&self, workspace_path: &str, depth: Option<u32>) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let ws = Workspace::new(fs);
            let root_path = PathBuf::from(workspace_path);

            // Find root index in the workspace
            let root_index = block_on(ws.find_root_index_in_dir(&root_path))
                .js_err()?
                .or_else(|| block_on(ws.find_any_index_in_dir(&root_path)).ok().flatten())
                .ok_or_else(|| {
                    JsValue::from_str(&format!("No workspace found at '{}'", workspace_path))
                })?;

            let max_depth = depth.map(|d| d as usize);
            let mut visited = HashSet::new();

            let tree = block_on(ws.build_tree_with_depth(&root_index, max_depth, &mut visited))
                .js_err()?;

            let js_tree: JsTreeNode = tree.into();
            serde_wasm_bindgen::to_value(&js_tree).js_err()
        })
    }

    /// Initialize a new workspace with an index.md file.
    #[wasm_bindgen]
    pub fn create(&self, path: &str, name: &str) -> Result<(), JsValue> {
        use crate::state::with_fs_mut;

        with_fs_mut(|fs| {
            let index_path = PathBuf::from(path).join("index.md");

            if fs.exists(&index_path) {
                return Err(JsValue::from_str(&format!(
                    "Workspace already exists at '{}'",
                    path
                )));
            }

            let content = format!(
                "---\ntitle: \"{}\"\ncontents: []\n---\n\n# {}\n",
                name, name
            );

            fs.write_file(&index_path, &content).js_err()
        })
    }

    /// Get the filesystem tree structure (for "Show All Files" mode).
    #[wasm_bindgen]
    pub fn get_filesystem_tree(
        &self,
        workspace_path: &str,
        show_hidden: bool,
    ) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let root_path = PathBuf::from(workspace_path);

            fn build_tree<FS: AsyncFileSystem>(
                fs: &FS,
                path: &Path,
                show_hidden: bool,
            ) -> Result<JsTreeNode, String> {
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string());

                if !show_hidden && name.starts_with('.') {
                    return Err("hidden".to_string());
                }

                let mut children = Vec::new();

                if block_on(fs.is_dir(path)) {
                    if let Ok(entries) = block_on(fs.list_files(path)) {
                        for entry in entries {
                            if let Ok(child) = build_tree(fs, &entry, show_hidden) {
                                children.push(child);
                            }
                        }
                    }
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

                Ok(JsTreeNode {
                    name,
                    description: None,
                    path: path.to_string_lossy().to_string(),
                    children,
                })
            }

            let tree =
                build_tree(&fs, &root_path, show_hidden).map_err(|e| JsValue::from_str(&e))?;

            serde_wasm_bindgen::to_value(&tree).js_err()
        })
    }
}

impl Default for DiaryxWorkspace {
    fn default() -> Self {
        Self::new()
    }
}
