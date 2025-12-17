//! WebAssembly bindings for Diaryx core functionality.
//!
//! This crate provides a complete backend implementation for the web frontend,
//! using an in-memory filesystem that can be persisted to IndexedDB.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;

use diaryx_core::{
    entry::DiaryxApp,
    fs::{FileSystem, InMemoryFileSystem},
    search::{SearchQuery, Searcher},
    template::TemplateManager,
    workspace::Workspace,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

// ============================================================================
// Initialization
// ============================================================================

#[cfg(feature = "console_error_panic_hook")]
pub fn set_panic_hook() {
    console_error_panic_hook::set_once();
}

/// Initialize the WASM module. Called automatically on module load.
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    set_panic_hook();
}

// ============================================================================
// Global State
// ============================================================================

thread_local! {
    static FILESYSTEM: RefCell<InMemoryFileSystem> = RefCell::new(InMemoryFileSystem::new());
}

fn with_fs<F, R>(f: F) -> R
where
    F: FnOnce(&InMemoryFileSystem) -> R,
{
    FILESYSTEM.with(|fs| f(&fs.borrow()))
}

fn with_fs_mut<F, R>(f: F) -> R
where
    F: FnOnce(&InMemoryFileSystem) -> R,
{
    FILESYSTEM.with(|fs| f(&fs.borrow()))
}

// ============================================================================
// Filesystem Operations (for IndexedDB sync)
// ============================================================================

/// Load files into the in-memory filesystem from JavaScript.
/// Takes an array of [path, content] tuples.
#[wasm_bindgen]
pub fn load_files(entries: JsValue) -> Result<(), JsValue> {
    let entries: Vec<(String, String)> = serde_wasm_bindgen::from_value(entries)?;

    FILESYSTEM.with(|fs| {
        *fs.borrow_mut() = InMemoryFileSystem::load_from_entries(entries);
    });

    Ok(())
}

/// Export all files from the in-memory filesystem.
/// Returns an array of [path, content] tuples for persistence to IndexedDB.
#[wasm_bindgen]
pub fn export_files() -> Result<JsValue, JsValue> {
    let entries = with_fs(|fs| fs.export_entries());
    Ok(serde_wasm_bindgen::to_value(&entries)?)
}

/// Check if a file exists.
#[wasm_bindgen]
pub fn file_exists(path: &str) -> bool {
    with_fs(|fs| FileSystem::exists(fs, std::path::Path::new(path)))
}

/// Read a file's content.
#[wasm_bindgen]
pub fn read_file(path: &str) -> Result<String, JsValue> {
    use diaryx_core::fs::FileSystem;
    with_fs(|fs| {
        fs.read_to_string(std::path::Path::new(path))
            .map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

/// Write content to a file (creates or overwrites).
#[wasm_bindgen]
pub fn write_file(path: &str, content: &str) -> Result<(), JsValue> {
    use diaryx_core::fs::FileSystem;
    with_fs_mut(|fs| {
        fs.write_file(std::path::Path::new(path), content)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

/// Delete a file.
#[wasm_bindgen]
pub fn delete_file(path: &str) -> Result<(), JsValue> {
    use diaryx_core::fs::FileSystem;
    with_fs_mut(|fs| {
        fs.delete_file(std::path::Path::new(path))
            .map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

// ============================================================================
// Workspace Operations
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

/// Get the workspace tree structure.
#[wasm_bindgen]
pub fn get_workspace_tree(workspace_path: &str, depth: Option<u32>) -> Result<JsValue, JsValue> {
    with_fs(|fs| {
        let ws = Workspace::new(fs);
        let root_path = PathBuf::from(workspace_path);

        // Find root index in the workspace
        let root_index = ws
            .find_root_index_in_dir(&root_path)
            .map_err(|e| JsValue::from_str(&e.to_string()))?
            .or_else(|| ws.find_any_index_in_dir(&root_path).ok().flatten())
            .ok_or_else(|| {
                JsValue::from_str(&format!("No workspace found at '{}'", workspace_path))
            })?;

        let max_depth = depth.map(|d| d as usize);
        let mut visited = HashSet::new();

        let tree = ws
            .build_tree_with_depth(&root_index, max_depth, &mut visited)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let js_tree: JsTreeNode = tree.into();
        Ok(serde_wasm_bindgen::to_value(&js_tree)?)
    })
}

/// Initialize a new workspace with an index.md file.
#[wasm_bindgen]
pub fn create_workspace(path: &str, name: &str) -> Result<(), JsValue> {
    use diaryx_core::fs::FileSystem;

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

        fs.write_file(&index_path, &content)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

// ============================================================================
// Validation Operations
// ============================================================================

/// Validation error returned to JavaScript
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum JsValidationError {
    BrokenPartOf { file: String, target: String },
    BrokenContentsRef { index: String, target: String },
}

impl From<diaryx_core::validate::ValidationError> for JsValidationError {
    fn from(err: diaryx_core::validate::ValidationError) -> Self {
        use diaryx_core::validate::ValidationError;
        match err {
            ValidationError::BrokenPartOf { file, target } => JsValidationError::BrokenPartOf {
                file: file.to_string_lossy().to_string(),
                target,
            },
            ValidationError::BrokenContentsRef { index, target } => {
                JsValidationError::BrokenContentsRef {
                    index: index.to_string_lossy().to_string(),
                    target,
                }
            }
        }
    }
}

/// Validation warning returned to JavaScript
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum JsValidationWarning {
    OrphanFile { file: String },
    CircularReference { files: Vec<String> },
}

impl From<diaryx_core::validate::ValidationWarning> for JsValidationWarning {
    fn from(warn: diaryx_core::validate::ValidationWarning) -> Self {
        use diaryx_core::validate::ValidationWarning;
        match warn {
            ValidationWarning::OrphanFile { file } => JsValidationWarning::OrphanFile {
                file: file.to_string_lossy().to_string(),
            },
            ValidationWarning::CircularReference { files } => {
                JsValidationWarning::CircularReference {
                    files: files
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect(),
                }
            }
        }
    }
}

/// Validation result returned to JavaScript
#[derive(Debug, Serialize)]
pub struct JsValidationResult {
    pub errors: Vec<JsValidationError>,
    pub warnings: Vec<JsValidationWarning>,
    pub files_checked: usize,
}

/// Validate workspace links (contents and part_of references).
#[wasm_bindgen]
pub fn validate_workspace(workspace_path: &str) -> Result<JsValue, JsValue> {
    use diaryx_core::validate::Validator;

    with_fs(|fs| {
        let validator = Validator::new(fs);
        let root_path = PathBuf::from(workspace_path);

        // Find the root index
        let ws = Workspace::new(fs);
        let root_index = ws
            .find_root_index_in_dir(&root_path)
            .map_err(|e| JsValue::from_str(&e.to_string()))?
            .or_else(|| ws.find_any_index_in_dir(&root_path).ok().flatten())
            .ok_or_else(|| {
                JsValue::from_str(&format!("No workspace found at '{}'", workspace_path))
            })?;

        let result = validator
            .validate_workspace(&root_index)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let js_result = JsValidationResult {
            errors: result
                .errors
                .into_iter()
                .map(JsValidationError::from)
                .collect(),
            warnings: result
                .warnings
                .into_iter()
                .map(JsValidationWarning::from)
                .collect(),
            files_checked: result.files_checked,
        };

        Ok(serde_wasm_bindgen::to_value(&js_result)?)
    })
}

// ============================================================================
// Entry Operations
// ============================================================================

/// Entry data returned to JavaScript
#[derive(Debug, Serialize)]
pub struct JsEntryData {
    pub path: String,
    pub title: Option<String>,
    pub frontmatter: serde_json::Map<String, serde_json::Value>,
    pub content: String,
}

/// Get an entry's content and metadata.
#[wasm_bindgen]
pub fn get_entry(path: &str) -> Result<JsValue, JsValue> {
    // Use a serializer that converts Maps to plain JS objects
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);

    with_fs(|fs| {
        let app = DiaryxApp::new(fs);

        let frontmatter = app
            .get_all_frontmatter(path)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Convert YAML values to JSON
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

        let content = app
            .get_content(path)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let entry = JsEntryData {
            path: path.to_string(),
            title,
            frontmatter: json_frontmatter,
            content,
        };

        Ok(entry.serialize(&serializer)?)
    })
}

/// Save an entry's content (preserves frontmatter).
#[wasm_bindgen]
pub fn save_entry(path: &str, content: &str) -> Result<(), JsValue> {
    with_fs_mut(|fs| {
        let app = DiaryxApp::new(fs);

        // Save content and update the "updated" timestamp
        app.save_content(path, content)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

/// Options for creating an entry
#[derive(Debug, Deserialize)]
pub struct CreateEntryOptions {
    pub title: Option<String>,
    pub part_of: Option<String>,
    pub template: Option<String>,
}

/// Create a new entry.
#[wasm_bindgen]
pub fn create_entry(path: &str, options: JsValue) -> Result<String, JsValue> {
    let options: Option<CreateEntryOptions> = if options.is_undefined() || options.is_null() {
        None
    } else {
        Some(serde_wasm_bindgen::from_value(options)?)
    };

    with_fs_mut(|fs| {
        let app = DiaryxApp::new(fs);
        let path_buf = PathBuf::from(path);

        // Determine title
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
                    .with_date(chrono::Utc::now().date_naive());

                if let Some(ref part_of) = opts.part_of {
                    context = context.with_part_of(part_of);
                }

                let content = template.render(&context);

                use diaryx_core::fs::FileSystem;
                fs.create_new(&path_buf, &content)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                // Add to parent index contents
                add_to_parent_index(fs, path)?;

                return Ok(path.to_string());
            }
        }

        // Create entry without template
        app.create_entry(path)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Set title
        app.set_frontmatter_property(path, "title", serde_yaml::Value::String(title))
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Set timestamps
        let now = chrono::Utc::now().to_rfc3339();
        app.set_frontmatter_property(path, "created", serde_yaml::Value::String(now.clone()))
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        app.set_frontmatter_property(path, "updated", serde_yaml::Value::String(now))
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Set part_of if provided
        if let Some(ref opts) = options
            && let Some(ref part_of) = opts.part_of
        {
            app.set_frontmatter_property(
                path,
                "part_of",
                serde_yaml::Value::String(part_of.clone()),
            )
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        }

        // Add to parent index contents
        add_to_parent_index(fs, path)?;

        Ok(path.to_string())
    })
}

/// Helper to add an entry to its parent index's contents array and set part_of
fn add_to_parent_index(fs: &InMemoryFileSystem, entry_path: &str) -> Result<(), JsValue> {
    let path = PathBuf::from(entry_path);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| JsValue::from_str("Invalid path"))?;

    let parent = path
        .parent()
        .ok_or_else(|| JsValue::from_str("No parent directory"))?;
    let index_path = parent.join("index.md");

    // Check if parent index exists
    if !fs.exists(&index_path) {
        return Ok(()); // No index to update
    }

    let app = DiaryxApp::new(fs);
    let index_path_str = index_path.to_string_lossy();

    // Get current contents
    let frontmatter = app
        .get_all_frontmatter(&index_path_str)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

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

    // Add entry if not present
    if !contents.contains(&file_name.to_string()) {
        contents.push(file_name.to_string());
        contents.sort();

        let yaml_contents: Vec<serde_yaml::Value> = contents
            .into_iter()
            .map(serde_yaml::Value::String)
            .collect();

        app.set_frontmatter_property(
            &index_path_str,
            "contents",
            serde_yaml::Value::Sequence(yaml_contents),
        )
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    }

    // Set part_of on the entry to point back to the parent index (bidirectional link)
    // Only set if the entry doesn't already have a part_of
    let entry_frontmatter = app
        .get_all_frontmatter(entry_path)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    if !entry_frontmatter.contains_key("part_of") {
        app.set_frontmatter_property(
            entry_path,
            "part_of",
            serde_yaml::Value::String("index.md".to_string()),
        )
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    }

    Ok(())
}

/// Attach an existing entry to a parent index.
///
/// Uses core `Workspace::attach_entry_to_parent` which:
/// - Adds the entry to the parent index's `contents` (using a relative child path)
/// - Sets the entry's `part_of` to point back to the parent index (relative to the entry)
#[wasm_bindgen]
pub fn attach_entry_to_parent(entry_path: &str, parent_index_path: &str) -> Result<(), JsValue> {
    with_fs_mut(|fs| {
        let ws = Workspace::new(fs);
        let entry = PathBuf::from(entry_path);
        let parent_index = PathBuf::from(parent_index_path);

        ws.attach_entry_to_parent(&entry, &parent_index)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

/// Delete an entry.
#[wasm_bindgen]
pub fn delete_entry(path: &str) -> Result<(), JsValue> {
    use diaryx_core::fs::FileSystem;

    with_fs_mut(|fs| {
        fs.delete_file(std::path::Path::new(path))
            .map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

/// Move/rename an entry while updating workspace index references.
///
/// Uses core `Workspace::move_entry` which:
/// - Moves the file from `from_path` to `to_path`
/// - Removes the entry from old parent's `contents` (if `index.md` exists)
/// - Adds the entry to new parent's `contents` (if `index.md` exists)
/// - Updates the moved file's `part_of` to point to new parent index
#[wasm_bindgen]
pub fn move_entry(from_path: &str, to_path: &str) -> Result<String, JsValue> {
    with_fs_mut(|fs| {
        let ws = Workspace::new(fs);
        let from = PathBuf::from(from_path);
        let to = PathBuf::from(to_path);

        ws.move_entry(&from, &to)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        Ok(to_path.to_string())
    })
}

// ============================================================================
// Frontmatter Operations
// ============================================================================

/// Get all frontmatter for an entry.
#[wasm_bindgen]
pub fn get_frontmatter(path: &str) -> Result<JsValue, JsValue> {
    with_fs(|fs| {
        let app = DiaryxApp::new(fs);

        let frontmatter = app
            .get_all_frontmatter(path)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Convert to JSON
        let mut json_map = serde_json::Map::new();
        for (key, value) in frontmatter {
            if let Ok(json_val) = serde_json::to_value(&value) {
                json_map.insert(key, json_val);
            }
        }

        Ok(serde_wasm_bindgen::to_value(&json_map)?)
    })
}

/// Set a frontmatter property.
#[wasm_bindgen]
pub fn set_frontmatter_property(path: &str, key: &str, value: JsValue) -> Result<(), JsValue> {
    with_fs_mut(|fs| {
        let app = DiaryxApp::new(fs);

        // Convert JS value to YAML value
        let json_value: serde_json::Value = serde_wasm_bindgen::from_value(value)?;
        let yaml_value: serde_yaml::Value =
            serde_json::from_value(json_value).map_err(|e| JsValue::from_str(&e.to_string()))?;

        app.set_frontmatter_property(path, key, yaml_value)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

/// Remove a frontmatter property.
#[wasm_bindgen]
pub fn remove_frontmatter_property(path: &str, key: &str) -> Result<(), JsValue> {
    with_fs_mut(|fs| {
        let app = DiaryxApp::new(fs);

        app.remove_frontmatter_property(path, key)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

// ============================================================================
// Search Operations
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

/// Search the workspace for entries matching a pattern.
#[wasm_bindgen]
pub fn search_workspace(pattern: &str, options: JsValue) -> Result<JsValue, JsValue> {
    let opts: SearchOptions = if options.is_undefined() || options.is_null() {
        SearchOptions::default()
    } else {
        serde_wasm_bindgen::from_value(options)?
    };

    with_fs(|fs| {
        let searcher = Searcher::new(fs);
        let ws = Workspace::new(fs);

        let workspace_path = opts.workspace_path.as_deref().unwrap_or("workspace");
        let root_path = PathBuf::from(workspace_path);

        // Find root index
        let root_index = ws
            .find_root_index_in_dir(&root_path)
            .map_err(|e| JsValue::from_str(&e.to_string()))?
            .or_else(|| ws.find_any_index_in_dir(&root_path).ok().flatten())
            .ok_or_else(|| {
                JsValue::from_str(&format!("No workspace found at '{}'", workspace_path))
            })?;

        // Build query
        let query = if let Some(ref prop) = opts.property {
            SearchQuery::property(pattern, prop)
        } else if opts.search_frontmatter.unwrap_or(false) {
            SearchQuery::frontmatter(pattern)
        } else {
            SearchQuery::content(pattern)
        };

        let query = query.case_sensitive(opts.case_sensitive.unwrap_or(false));

        let results = searcher
            .search_workspace(&root_index, &query)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Convert to JS-friendly format
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

        Ok(serde_wasm_bindgen::to_value(&js_results)?)
    })
}

// ============================================================================
// Template Operations
// ============================================================================

/// Template info returned to JavaScript
#[derive(Debug, Serialize)]
pub struct JsTemplateInfo {
    pub name: String,
    pub path: String,
    pub source: String,
}

/// List available templates.
#[wasm_bindgen]
pub fn list_templates(workspace_path: Option<String>) -> Result<JsValue, JsValue> {
    with_fs(|fs| {
        let mut manager = TemplateManager::new(fs);

        if let Some(ref ws_path) = workspace_path {
            manager = manager.with_workspace_dir(std::path::Path::new(ws_path));
        }

        let templates: Vec<JsTemplateInfo> = manager
            .list()
            .into_iter()
            .map(|t| JsTemplateInfo {
                name: t.name,
                path: t
                    .path
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
                source: format!("{}", t.source),
            })
            .collect();

        Ok(serde_wasm_bindgen::to_value(&templates)?)
    })
}

/// Get a template's content.
#[wasm_bindgen]
pub fn get_template(name: &str, workspace_path: Option<String>) -> Result<String, JsValue> {
    with_fs(|fs| {
        let mut manager = TemplateManager::new(fs);

        if let Some(ref ws_path) = workspace_path {
            manager = manager.with_workspace_dir(std::path::Path::new(ws_path));
        }

        manager
            .get(name)
            .map(|t| t.raw_content.clone())
            .ok_or_else(|| JsValue::from_str(&format!("Template not found: {}", name)))
    })
}

/// Save a user template.
#[wasm_bindgen]
pub fn save_template(name: &str, content: &str, workspace_path: &str) -> Result<(), JsValue> {
    use diaryx_core::fs::FileSystem;

    with_fs_mut(|fs| {
        let templates_dir = PathBuf::from(workspace_path).join("_templates");

        // Ensure templates directory exists
        fs.create_dir_all(&templates_dir)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let template_path = templates_dir.join(format!("{}.md", name));
        fs.write_file(&template_path, content)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

/// Delete a user template.
#[wasm_bindgen]
pub fn delete_template(name: &str, workspace_path: &str) -> Result<(), JsValue> {
    use diaryx_core::fs::FileSystem;

    with_fs_mut(|fs| {
        let template_path = PathBuf::from(workspace_path)
            .join("_templates")
            .join(format!("{}.md", name));

        fs.delete_file(&template_path)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

// ============================================================================
// Utility Functions
// These functions operate on raw content strings for JavaScript compatibility.
// Core's `DiaryxApp::parse_file` works on file paths, but these utilities are
// needed for web frontend scenarios where raw content is passed from JavaScript.
// ============================================================================

/// Parse YAML frontmatter from raw markdown content.
///
/// This function operates on raw content strings for JavaScript compatibility.
/// For file-based parsing, use `get_entry` or `get_frontmatter` instead.
///
/// Returns an empty object if no valid frontmatter is found.
#[wasm_bindgen]
pub fn parse_frontmatter(content: &str) -> Result<JsValue, JsValue> {
    if !content.starts_with("---\n") {
        return Ok(serde_wasm_bindgen::to_value(&serde_json::Map::<
            String,
            serde_json::Value,
        >::new())?);
    }

    let rest = &content[4..];
    let end_idx = rest.find("\n---");

    let yaml_str = match end_idx {
        Some(idx) => &rest[..idx],
        None => {
            return Ok(serde_wasm_bindgen::to_value(&serde_json::Map::<
                String,
                serde_json::Value,
            >::new())?);
        }
    };

    match serde_yaml::from_str::<serde_json::Value>(yaml_str) {
        Ok(value) => {
            if let serde_json::Value::Object(map) = value {
                Ok(serde_wasm_bindgen::to_value(&map)?)
            } else {
                Ok(serde_wasm_bindgen::to_value(&serde_json::Map::<
                    String,
                    serde_json::Value,
                >::new())?)
            }
        }
        Err(_) => Ok(serde_wasm_bindgen::to_value(&serde_json::Map::<
            String,
            serde_json::Value,
        >::new())?),
    }
}

/// Serialize a JavaScript object to YAML frontmatter format.
#[wasm_bindgen]
pub fn serialize_frontmatter(frontmatter: JsValue) -> Result<String, JsValue> {
    let map: serde_json::Map<String, serde_json::Value> =
        serde_wasm_bindgen::from_value(frontmatter)?;

    let yaml = serde_yaml::to_string(&map).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let yaml = yaml.trim_end();
    Ok(format!("---\n{}\n---", yaml))
}

/// Extract the body content from raw markdown (everything after frontmatter).
///
/// This function operates on raw content strings for JavaScript compatibility.
/// For file-based content retrieval, use `get_entry` which returns both
/// frontmatter and body content parsed via `DiaryxApp::get_content`.
#[wasm_bindgen]
pub fn extract_body(content: &str) -> String {
    if !content.starts_with("---\n") {
        return content.to_string();
    }

    let rest = &content[4..];
    if let Some(end_idx) = rest.find("\n---") {
        let after_frontmatter = &rest[end_idx + 4..];
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
}
