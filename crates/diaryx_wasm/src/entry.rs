//! Entry operations for WASM.

use std::path::{Path, PathBuf};

use chrono::Utc;
use diaryx_core::entry::{DiaryxApp, DiaryxAppSync};
use diaryx_core::fs::{FileSystem, SyncToAsyncFs};
use diaryx_core::template::TemplateManager;
use diaryx_core::workspace::Workspace;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::error::{IntoJsOption, IntoJsResult};
use crate::state::{block_on, with_fs, with_fs_mut};

// ============================================================================
// Types
// ============================================================================

/// Entry data returned to JavaScript
#[derive(Debug, Serialize)]
pub struct JsEntryData {
    pub path: String,
    pub title: Option<String>,
    pub frontmatter: serde_json::Map<String, serde_json::Value>,
    pub content: String,
}

/// Options for creating an entry
#[derive(Debug, Deserialize)]
pub struct CreateEntryOptions {
    pub title: Option<String>,
    pub part_of: Option<String>,
    pub template: Option<String>,
}

// ============================================================================
// DiaryxEntry Class
// ============================================================================

/// Entry operations for managing diary entries.
#[wasm_bindgen]
pub struct DiaryxEntry;

#[wasm_bindgen]
impl DiaryxEntry {
    /// Create a new DiaryxEntry instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Get an entry's content and metadata.
    #[wasm_bindgen]
    pub fn get(&self, path: &str) -> Result<JsValue, JsValue> {
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);

        with_fs(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);

            let frontmatter = block_on(app.get_all_frontmatter(path)).js_err()?;

            let mut json_frontmatter = serde_json::Map::new();
            for (key, value) in frontmatter {
                if let Ok(json_val) = serde_json::to_value(&value) {
                    json_frontmatter.insert(key, json_val);
                }
            }

            let title = json_frontmatter
                .get("title")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let content = block_on(app.get_content(path)).js_err()?;

            let entry = JsEntryData {
                path: path.to_string(),
                title,
                frontmatter: json_frontmatter,
                content,
            };

            entry.serialize(&serializer).js_err()
        })
    }

    /// Save an entry's content.
    #[wasm_bindgen]
    pub fn save(&self, path: &str, content: &str) -> Result<(), JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);
            block_on(app.save_content(path, content)).js_err()
        })
    }

    /// Get an entry's full raw file content.
    #[wasm_bindgen]
    pub fn read_raw(&self, path: &str) -> Result<String, JsValue> {
        with_fs(|fs| fs.read_to_string(Path::new(path)).js_err())
    }

    /// Create a new entry.
    #[wasm_bindgen]
    pub fn create(&self, path: &str, options: JsValue) -> Result<String, JsValue> {
        let options: Option<CreateEntryOptions> = if options.is_undefined() || options.is_null() {
            None
        } else {
            Some(serde_wasm_bindgen::from_value(options).js_err()?)
        };

        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);
            let path_buf = PathBuf::from(path);

            let title = options
                .as_ref()
                .and_then(|o| o.title.clone())
                .unwrap_or_else(|| {
                    path_buf
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Untitled")
                        .to_string()
                });

            // Check if template is requested
            if let Some(ref opts) = options
                && let Some(ref template_name) = opts.template
            {
                let manager = TemplateManager::new(fs);
                if let Some(template) = manager.get(template_name) {
                    let mut context = diaryx_core::template::TemplateContext::new()
                        .with_title(&title)
                        .with_date(Utc::now().date_naive());

                    if let Some(ref part_of) = opts.part_of {
                        context = context.with_part_of(part_of);
                    }

                    let content = template.render(&context);
                    fs.create_new(&path_buf, &content).js_err()?;
                    add_to_parent_index(fs, path)?;
                    return Ok(path.to_string());
                }
            }

            // Create entry without template
            block_on(app.create_entry(path)).js_err()?;

            // Set title and timestamps
            block_on(app.set_frontmatter_property(path, "title", serde_yaml::Value::String(title)))
                .js_err()?;

            let now = Utc::now().to_rfc3339();
            block_on(app.set_frontmatter_property(
                path,
                "created",
                serde_yaml::Value::String(now.clone()),
            ))
            .js_err()?;
            block_on(app.set_frontmatter_property(path, "updated", serde_yaml::Value::String(now)))
                .js_err()?;

            // Set part_of if provided
            if let Some(ref opts) = options
                && let Some(ref part_of) = opts.part_of
            {
                block_on(app.set_frontmatter_property(
                    path,
                    "part_of",
                    serde_yaml::Value::String(part_of.clone()),
                ))
                .js_err()?;
            }

            add_to_parent_index(fs, path)?;
            Ok(path.to_string())
        })
    }

    /// Delete an entry.
    #[wasm_bindgen]
    pub fn delete(&self, path: &str) -> Result<(), JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.delete_entry(&PathBuf::from(path))).js_err()
        })
    }

    /// Move/rename an entry.
    #[wasm_bindgen]
    pub fn move_entry(&self, from_path: &str, to_path: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.move_entry(&PathBuf::from(from_path), &PathBuf::from(to_path))).js_err()?;
            Ok(to_path.to_string())
        })
    }

    /// Attach an entry to a parent.
    #[wasm_bindgen]
    pub fn attach_to_parent(&self, entry_path: &str, parent_path: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.attach_and_move_entry_to_parent(
                &PathBuf::from(entry_path),
                &PathBuf::from(parent_path),
            ))
            .map(|p| p.to_string_lossy().to_string())
            .js_err()
        })
    }

    /// Convert a leaf file to an index.
    #[wasm_bindgen]
    pub fn convert_to_index(&self, path: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.convert_to_index(&PathBuf::from(path)))
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
        })
    }

    /// Convert an index back to a leaf file.
    #[wasm_bindgen]
    pub fn convert_to_leaf(&self, path: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.convert_to_leaf(&PathBuf::from(path)))
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
        })
    }

    /// Create a new child entry under a parent.
    #[wasm_bindgen]
    pub fn create_child(&self, parent_path: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.create_child_entry(&PathBuf::from(parent_path), None))
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
        })
    }

    /// Rename an entry.
    #[wasm_bindgen]
    pub fn rename(&self, path: &str, new_filename: &str) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let ws = Workspace::new(async_fs);
            block_on(ws.rename_entry(&PathBuf::from(path), new_filename))
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
        })
    }

    /// Ensure today's daily entry exists.
    #[wasm_bindgen]
    pub fn ensure_daily(&self) -> Result<String, JsValue> {
        use chrono::Local;
        use diaryx_core::config::Config;

        with_fs_mut(|fs| {
            // Use DiaryxAppSync for ensure_dated_entry (not yet async)
            let app = DiaryxAppSync::new(fs.clone());
            let today = Local::now().date_naive();

            let config = Config::with_options(
                PathBuf::from("workspace"),
                Some("Daily".to_string()),
                None,
                None,
                None,
            );

            app.ensure_dated_entry(&today, &config)
                .map(|p| p.to_string_lossy().to_string())
                .js_err()
        })
    }
}

impl Default for DiaryxEntry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Add an entry to its parent index's contents array.
fn add_to_parent_index<FS: FileSystem>(fs: &FS, entry_path: &str) -> Result<(), JsValue> {
    let path = PathBuf::from(entry_path);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .js_ok_or("Invalid path")?;

    let parent = path.parent().js_ok_or("No parent directory")?;
    let index_path = parent.join("index.md");

    if !fs.exists(&index_path) {
        return Ok(());
    }

    let async_fs = SyncToAsyncFs::new(fs.clone());
    let app = DiaryxApp::new(async_fs);
    let index_path_str = index_path.to_string_lossy();

    let frontmatter = block_on(app.get_all_frontmatter(&index_path_str)).js_err()?;

    let mut contents: Vec<String> = frontmatter
        .get("contents")
        .and_then(|v| {
            if let serde_yaml::Value::Sequence(seq) = v {
                Some(
                    seq.iter()
                        .filter_map(|item| item.as_str().map(String::from))
                        .collect(),
                )
            } else {
                None
            }
        })
        .unwrap_or_default();

    if !contents.contains(&file_name.to_string()) {
        contents.push(file_name.to_string());
        contents.sort();

        let yaml_contents: Vec<serde_yaml::Value> = contents
            .into_iter()
            .map(serde_yaml::Value::String)
            .collect();

        block_on(app.set_frontmatter_property(
            &index_path_str,
            "contents",
            serde_yaml::Value::Sequence(yaml_contents),
        ))
        .js_err()?;
    }

    // Set part_of on entry if not present
    let entry_frontmatter = block_on(app.get_all_frontmatter(entry_path)).js_err()?;

    if !entry_frontmatter.contains_key("part_of") {
        block_on(app.set_frontmatter_property(
            entry_path,
            "part_of",
            serde_yaml::Value::String("index.md".to_string()),
        ))
        .js_err()?;
    }

    Ok(())
}

/// Convert a title to a kebab-case filename.
#[wasm_bindgen]
pub fn slugify_title(title: &str) -> String {
    diaryx_core::entry::slugify_title(title)
}
