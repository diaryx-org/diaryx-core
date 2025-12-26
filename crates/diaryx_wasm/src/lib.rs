//! WebAssembly bindings for Diaryx core functionality.
//!
//! This crate provides a complete backend implementation for the web frontend,
//! using an in-memory filesystem that can be persisted to IndexedDB.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

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

/// Export all binary files from the in-memory filesystem.
/// Returns an array of objects with path and data for persistence to IndexedDB.
#[wasm_bindgen]
pub fn export_binary_files() -> Result<JsValue, JsValue> {
    let entries = with_fs(|fs| fs.export_binary_entries());
    // Convert to a serializable format
    let serializable: Vec<BinaryEntry> = entries
        .into_iter()
        .map(|(path, data)| BinaryEntry { path, data })
        .collect();
    Ok(serde_wasm_bindgen::to_value(&serializable)?)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct BinaryEntry {
    path: String,
    data: Vec<u8>,
}

/// Load binary files into the in-memory filesystem.
/// Takes an array of objects with path and data.
#[wasm_bindgen]
pub fn load_binary_files(entries: JsValue) -> Result<(), JsValue> {
    let binary_entries: Vec<BinaryEntry> = serde_wasm_bindgen::from_value(entries)?;
    let entries: Vec<(String, Vec<u8>)> = binary_entries
        .into_iter()
        .map(|e| (e.path, e.data))
        .collect();

    with_fs(|fs| {
        fs.load_binary_entries(entries);
    });

    Ok(())
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

/// Attach an existing entry to a parent, moving it into the parent's directory.
///
/// Uses core `Workspace::attach_and_move_entry_to_parent` which:
/// - Converts parent to index if it's a leaf file (creates directory)
/// - Moves entry into the parent's directory if not already there
/// - Adds the entry to the parent index's `contents`
/// - Sets the entry's `part_of` to point back to the parent index
///
/// Returns the new path to the entry after any moves.
#[wasm_bindgen]
pub fn attach_entry_to_parent(entry_path: &str, parent_path: &str) -> Result<String, JsValue> {
    with_fs_mut(|fs| {
        let ws = Workspace::new(fs);
        let entry = PathBuf::from(entry_path);
        let parent = PathBuf::from(parent_path);

        ws.attach_and_move_entry_to_parent(&entry, &parent)
            .map(|p| p.to_string_lossy().to_string())
            .map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

/// Delete an entry while updating workspace references.
///
/// Uses core `Workspace::delete_entry` which:
/// - Fails if the entry has children (non-empty contents)
/// - Removes the entry from parent's `contents`
/// - Deletes the file
#[wasm_bindgen]
pub fn delete_entry(path: &str) -> Result<(), JsValue> {
    with_fs_mut(|fs| {
        let ws = Workspace::new(fs);
        let path_buf = PathBuf::from(path);

        ws.delete_entry(&path_buf)
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

/// Convert a leaf file to an index file with a directory.
///
/// Example: `journal/my-note.md` → `journal/my-note/index.md`
///
/// Uses core `Workspace::convert_to_index` which:
/// - Creates a directory with the same name as the file (without .md)
/// - Moves the file into the directory as `index.md`
/// - Adds `contents: []` frontmatter
/// - Adjusts `part_of` path (adds `../` prefix)
#[wasm_bindgen]
pub fn convert_to_index(path: &str) -> Result<String, JsValue> {
    with_fs_mut(|fs| {
        let ws = Workspace::new(fs);
        let path_buf = PathBuf::from(path);

        let new_path = ws
            .convert_to_index(&path_buf)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        Ok(new_path.to_string_lossy().to_string())
    })
}

/// Convert an empty index file back to a leaf file.
///
/// Example: `journal/my-note/index.md` → `journal/my-note.md`
///
/// Uses core `Workspace::convert_to_leaf` which:
/// - Fails if `contents` is not empty
/// - Moves `dir/index.md` → `parent/dir.md`
/// - Removes `contents` property
/// - Adjusts `part_of` path (removes `../` prefix)
#[wasm_bindgen]
pub fn convert_to_leaf(path: &str) -> Result<String, JsValue> {
    with_fs_mut(|fs| {
        let ws = Workspace::new(fs);
        let path_buf = PathBuf::from(path);

        let new_path = ws
            .convert_to_leaf(&path_buf)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        Ok(new_path.to_string_lossy().to_string())
    })
}

/// Create a new child entry under a parent.
///
/// Uses core `Workspace::create_child_entry` which:
/// - Converts parent to index if it's a leaf file (creates directory)
/// - Generates a unique filename
/// - Creates the entry with title, created, and updated frontmatter
/// - Attaches the new entry to the parent
///
/// Returns the path to the newly created entry.
#[wasm_bindgen]
pub fn create_child_entry(parent_path: &str) -> Result<String, JsValue> {
    with_fs_mut(|fs| {
        let ws = Workspace::new(fs);
        let parent = PathBuf::from(parent_path);

        ws.create_child_entry(&parent, None)
            .map(|p| p.to_string_lossy().to_string())
            .map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

/// Convert a title to a kebab-case filename.
///
/// Uses core `slugify_title` which:
/// - Converts to lowercase
/// - Replaces non-alphanumeric characters with dashes
/// - Removes consecutive dashes
/// - Returns "untitled.md" for empty titles
#[wasm_bindgen]
pub fn slugify_title(title: &str) -> String {
    diaryx_core::entry::slugify_title(title)
}

/// Rename an entry file by giving it a new filename.
///
/// Uses core `Workspace::rename_entry` which:
/// - For leaf files: renames the file and updates parent `contents`
/// - For index files: renames the containing directory and updates grandparent `contents`
///
/// Returns the new path to the renamed file.
#[wasm_bindgen]
pub fn rename_entry(path: &str, new_filename: &str) -> Result<String, JsValue> {
    with_fs_mut(|fs| {
        let ws = Workspace::new(fs);
        let path_buf = PathBuf::from(path);

        let new_path = ws
            .rename_entry(&path_buf, new_filename)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        Ok(new_path.to_string_lossy().to_string())
    })
}

/// Ensure today's daily entry exists, creating it if necessary.
///
/// Returns the path to today's daily entry.
/// Creates entries in the "Daily" folder within the workspace root,
/// and automatically connects daily_index.md to the workspace root via part_of/contents.
#[wasm_bindgen]
pub fn ensure_daily_entry() -> Result<String, JsValue> {
    use chrono::Local;
    use diaryx_core::config::Config;
    use std::path::PathBuf;

    with_fs_mut(|fs| {
        let app = DiaryxApp::new(fs);
        let today = Local::now().date_naive();

        // Use config with "Daily" folder so entries go to workspace/Daily/
        // This also triggers automatic part_of/contents linking to root
        let config = Config::with_options(
            PathBuf::from("workspace"),
            Some("Daily".to_string()), // daily_entry_folder
            None,                      // editor
            None,                      // default_template
            None,                      // daily_template
        );

        let path = app
            .ensure_dated_entry(&today, &config)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        Ok(path.to_string_lossy().to_string())
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
// Attachment Operations
// ============================================================================

/// Add an attachment path to an entry's attachments list.
#[wasm_bindgen]
pub fn add_attachment(entry_path: &str, attachment_path: &str) -> Result<(), JsValue> {
    with_fs(|fs| {
        let app = DiaryxApp::new(fs);
        app.add_attachment(entry_path, attachment_path)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

/// Remove an attachment path from an entry's attachments list.
#[wasm_bindgen]
pub fn remove_attachment(entry_path: &str, attachment_path: &str) -> Result<(), JsValue> {
    with_fs(|fs| {
        let app = DiaryxApp::new(fs);
        app.remove_attachment(entry_path, attachment_path)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

/// Get the list of attachments declared in an entry.
#[wasm_bindgen]
pub fn get_attachments(entry_path: &str) -> Result<JsValue, JsValue> {
    with_fs(|fs| {
        let app = DiaryxApp::new(fs);
        let attachments = app
            .get_attachments(entry_path)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        serde_wasm_bindgen::to_value(&attachments).map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

/// Upload an attachment file (base64 encoded data).
/// Stores the file and adds it to the entry's attachments list.
#[wasm_bindgen]
pub fn upload_attachment(
    entry_path: &str,
    filename: &str,
    data_base64: &str,
) -> Result<String, JsValue> {
    with_fs_mut(|fs| {
        // Decode base64 data
        let data = base64_decode(data_base64)
            .map_err(|e| JsValue::from_str(&format!("Base64 decode error: {}", e)))?;

        // Determine attachment path relative to entry
        let entry_dir = Path::new(entry_path).parent().unwrap_or(Path::new("."));
        let attachments_dir = entry_dir.join("_attachments");
        let attachment_path = attachments_dir.join(filename);

        // Create directory if needed
        fs.create_dir_all(&attachments_dir)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Write the binary file
        fs.write_binary(&attachment_path, &data)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Calculate relative path for frontmatter
        let relative_path = format!("_attachments/{}", filename);

        // Add to entry's attachments list
        let app = DiaryxApp::new(fs);
        app.add_attachment(entry_path, &relative_path)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        Ok(relative_path)
    })
}

/// Delete an attachment file and remove it from the entry's attachments list.
#[wasm_bindgen]
pub fn delete_attachment(entry_path: &str, attachment_path: &str) -> Result<(), JsValue> {
    with_fs_mut(|fs| {
        // Remove from frontmatter
        let app = DiaryxApp::new(fs);
        app.remove_attachment(entry_path, attachment_path)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Delete the file
        let entry_dir = Path::new(entry_path).parent().unwrap_or(Path::new("."));
        let full_path = entry_dir.join(attachment_path);
        if fs.exists(&full_path) {
            fs.delete_file(&full_path)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
        }

        Ok(())
    })
}

/// Read attachment binary data for blob URL creation.
/// Returns the raw bytes as a Uint8Array.
#[wasm_bindgen]
pub fn read_attachment_data(
    entry_path: &str,
    attachment_path: &str,
) -> Result<js_sys::Uint8Array, JsValue> {
    with_fs(|fs| {
        let entry_dir = Path::new(entry_path).parent().unwrap_or(Path::new("."));
        let full_path = entry_dir.join(attachment_path);

        let data = fs
            .read_binary(&full_path)
            .map_err(|e| JsValue::from_str(&format!("Failed to read attachment: {}", e)))?;

        Ok(js_sys::Uint8Array::from(data.as_slice()))
    })
}

/// Get storage usage information.
/// Returns { used: number, limit: number } in bytes.
/// TODO: enforce limits at browser/IndexedDB backend level
#[wasm_bindgen]
pub fn get_storage_usage() -> Result<JsValue, JsValue> {
    with_fs(|fs| {
        // Calculate total size of all files
        let mut total_size: u64 = 0;

        fn count_size<FS: FileSystem>(fs: &FS, dir: &Path, total: &mut u64) {
            if let Ok(entries) = fs.list_files(dir) {
                for path in entries {
                    if fs.is_dir(&path) {
                        count_size(fs, &path, total);
                    } else if let Ok(data) = fs.read_binary(&path) {
                        *total += data.len() as u64;
                    }
                }
            }
        }

        count_size(fs, Path::new("/"), &mut total_size);

        #[derive(serde::Serialize)]
        struct StorageInfo {
            used: u64,
            limit: u64,
            attachment_limit: u64,
        }

        let info = StorageInfo {
            used: total_size,
            limit: 100 * 1024 * 1024,          // 100MB
            attachment_limit: 5 * 1024 * 1024, // 5MB
        };

        serde_wasm_bindgen::to_value(&info).map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

/// Simple base64 decoder
fn base64_decode(input: &str) -> std::result::Result<Vec<u8>, String> {
    // Remove data URL prefix if present
    let data = if let Some(pos) = input.find(",") {
        &input[pos + 1..]
    } else {
        input
    };

    // Standard base64 decoding
    const DECODE_TABLE: [i8; 256] = {
        let mut table = [-1i8; 256];
        let mut i = 0u8;
        while i < 26 {
            table[(b'A' + i) as usize] = i as i8;
            table[(b'a' + i) as usize] = (i + 26) as i8;
            i += 1;
        }
        let mut i = 0u8;
        while i < 10 {
            table[(b'0' + i) as usize] = (i + 52) as i8;
            i += 1;
        }
        table[b'+' as usize] = 62;
        table[b'/' as usize] = 63;
        table[b'=' as usize] = 0;
        table
    };

    let bytes: Vec<u8> = data.bytes().filter(|&b| b != b'\n' && b != b'\r').collect();
    let mut output = Vec::with_capacity(bytes.len() * 3 / 4);

    for chunk in bytes.chunks(4) {
        if chunk.len() < 4 {
            break;
        }

        let a = DECODE_TABLE[chunk[0] as usize];
        let b = DECODE_TABLE[chunk[1] as usize];
        let c = DECODE_TABLE[chunk[2] as usize];
        let d = DECODE_TABLE[chunk[3] as usize];

        if a < 0 || b < 0 {
            return Err("Invalid base64 character".to_string());
        }

        output.push(((a as u8) << 2) | ((b as u8) >> 4));
        if chunk[2] != b'=' {
            output.push(((b as u8) << 4) | ((c as u8) >> 2));
        }
        if chunk[3] != b'=' {
            output.push(((c as u8) << 6) | (d as u8));
        }
    }

    Ok(output)
}

// ============================================================================
// Export Operations
// ============================================================================

/// Get all available audience tags from the workspace.
/// Scans all files and collects unique audience values.
#[wasm_bindgen]
pub fn get_available_audiences(root_path: &str) -> Result<JsValue, JsValue> {
    use std::collections::HashSet;

    with_fs(|fs| {
        let ws = Workspace::new(fs);
        let mut audiences: HashSet<String> = HashSet::new();

        // Parse the root index and traverse
        fn collect_audiences<FS: FileSystem>(
            ws: &Workspace<FS>,
            path: &Path,
            audiences: &mut HashSet<String>,
            visited: &mut HashSet<PathBuf>,
        ) {
            if visited.contains(path) {
                return;
            }
            visited.insert(path.to_path_buf());

            if let Ok(index) = ws.parse_index(path) {
                // Collect audience tags from this file
                if let Some(file_audiences) = &index.frontmatter.audience {
                    for a in file_audiences {
                        if a.to_lowercase() != "private" {
                            audiences.insert(a.clone());
                        }
                    }
                }

                // Recurse into children
                if index.frontmatter.is_index() {
                    for child_rel in index.frontmatter.contents_list() {
                        let child_path = index.resolve_path(child_rel);
                        if ws.fs_ref().exists(&child_path) {
                            collect_audiences(ws, &child_path, audiences, visited);
                        }
                    }
                }
            }
        }

        let mut visited = HashSet::new();
        collect_audiences(&ws, Path::new(root_path), &mut audiences, &mut visited);

        let mut result: Vec<String> = audiences.into_iter().collect();
        result.sort();

        serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

/// Plan an export operation, showing which files would be included/excluded.
/// Returns { included: [{path, relativePath}], excluded: [{path, reason}], audience }
/// If audience is "*", all files are included without filtering.
#[wasm_bindgen]
pub fn plan_export(root_path: &str, audience: &str) -> Result<JsValue, JsValue> {
    use std::collections::HashSet;

    with_fs(|fs| {
        #[derive(serde::Serialize)]
        struct ExportPlanJs {
            included: Vec<IncludedFileJs>,
            excluded: Vec<ExcludedFileJs>,
            audience: String,
        }

        #[derive(serde::Serialize)]
        struct IncludedFileJs {
            path: String,
            relative_path: String,
        }

        #[derive(serde::Serialize)]
        struct ExcludedFileJs {
            path: String,
            reason: String,
        }

        // Special case: "*" means export all without audience filtering
        if audience == "*" {
            let ws = Workspace::new(fs);
            let mut included = Vec::new();
            let root = Path::new(root_path);
            let root_dir = root.parent().unwrap_or(root);

            fn collect_all<FS: FileSystem>(
                ws: &Workspace<FS>,
                path: &Path,
                root_dir: &Path,
                included: &mut Vec<IncludedFileJs>,
                visited: &mut HashSet<PathBuf>,
            ) {
                if visited.contains(path) {
                    return;
                }
                visited.insert(path.to_path_buf());

                if let Ok(index) = ws.parse_index(path) {
                    let relative_path =
                        pathdiff::diff_paths(path, root_dir).unwrap_or_else(|| path.to_path_buf());

                    included.push(IncludedFileJs {
                        path: path.to_string_lossy().to_string(),
                        relative_path: relative_path.to_string_lossy().to_string(),
                    });

                    // Recurse into children
                    if index.frontmatter.is_index() {
                        for child_rel in index.frontmatter.contents_list() {
                            let child_path = index.resolve_path(child_rel);
                            if ws.fs_ref().exists(&child_path) {
                                collect_all(ws, &child_path, root_dir, included, visited);
                            }
                        }
                    }
                }
            }

            let mut visited = HashSet::new();
            collect_all(&ws, root, root_dir, &mut included, &mut visited);

            let result = ExportPlanJs {
                included,
                excluded: vec![],
                audience: "*".to_string(),
            };

            return serde_wasm_bindgen::to_value(&result)
                .map_err(|e| JsValue::from_str(&e.to_string()));
        }

        // Normal audience-filtered export
        use diaryx_core::export::Exporter;
        let exporter = Exporter::new(fs);

        // Use a virtual destination since we're just planning
        let plan = exporter
            .plan_export(Path::new(root_path), audience, Path::new("/export"))
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let result = ExportPlanJs {
            included: plan
                .included
                .iter()
                .map(|f| IncludedFileJs {
                    path: f.source_path.to_string_lossy().to_string(),
                    relative_path: f.relative_path.to_string_lossy().to_string(),
                })
                .collect(),
            excluded: plan
                .excluded
                .iter()
                .map(|f| ExcludedFileJs {
                    path: f.path.to_string_lossy().to_string(),
                    reason: f.reason.to_string(),
                })
                .collect(),
            audience: plan.audience,
        };

        serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

/// Export files to memory (returns array of {path, content} for zip creation in JS).
/// Files are processed to remove filtered contents and audience properties.
/// If audience is "*", all files are exported without filtering.
#[wasm_bindgen]
pub fn export_to_memory(root_path: &str, audience: &str) -> Result<JsValue, JsValue> {
    use std::collections::HashSet;

    with_fs(|fs| {
        #[derive(serde::Serialize)]
        struct ExportedFile {
            path: String,
            content: String,
        }

        // Special case: "*" means export all without audience filtering
        if audience == "*" {
            let ws = Workspace::new(fs);
            let mut files: Vec<ExportedFile> = Vec::new();
            let root = Path::new(root_path);
            let root_dir = root.parent().unwrap_or(root);

            fn collect_all<FS: FileSystem>(
                ws: &Workspace<FS>,
                path: &Path,
                root_dir: &Path,
                files: &mut Vec<ExportedFile>,
                visited: &mut HashSet<PathBuf>,
            ) {
                if visited.contains(path) {
                    return;
                }
                visited.insert(path.to_path_buf());

                if let Ok(index) = ws.parse_index(path) {
                    let relative_path =
                        pathdiff::diff_paths(path, root_dir).unwrap_or_else(|| path.to_path_buf());

                    // Read and include file content
                    if let Ok(content) = ws.fs_ref().read_to_string(path) {
                        // Remove audience property from content
                        let processed = remove_audience_from_content(&content);
                        files.push(ExportedFile {
                            path: relative_path.to_string_lossy().to_string(),
                            content: processed,
                        });
                    }

                    // Recurse into children
                    if index.frontmatter.is_index() {
                        for child_rel in index.frontmatter.contents_list() {
                            let child_path = index.resolve_path(child_rel);
                            if ws.fs_ref().exists(&child_path) {
                                collect_all(ws, &child_path, root_dir, files, visited);
                            }
                        }
                    }
                }
            }

            let mut visited = HashSet::new();
            collect_all(&ws, root, root_dir, &mut files, &mut visited);

            return serde_wasm_bindgen::to_value(&files)
                .map_err(|e| JsValue::from_str(&e.to_string()));
        }

        // Normal audience-filtered export
        use diaryx_core::export::Exporter;
        let exporter = Exporter::new(fs);

        let plan = exporter
            .plan_export(Path::new(root_path), audience, Path::new("/export"))
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let _ws = Workspace::new(fs);
        let mut files: Vec<ExportedFile> = Vec::new();

        for export_file in &plan.included {
            // Read original content
            let content = fs
                .read_to_string(&export_file.source_path)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Process content: remove audience property and filter contents
            let processed = if !export_file.filtered_contents.is_empty() {
                filter_contents_and_audience(&content, &export_file.filtered_contents)
            } else {
                remove_audience_from_content(&content)
            };

            files.push(ExportedFile {
                path: export_file.relative_path.to_string_lossy().to_string(),
                content: processed,
            });
        }

        serde_wasm_bindgen::to_value(&files).map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

/// Export files to memory as HTML (converts markdown to HTML).
/// Returns array of {path, content} with .html extensions for zip creation in JS.
#[wasm_bindgen]
pub fn export_to_html(root_path: &str, audience: &str) -> Result<JsValue, JsValue> {
    use comrak::{Options, markdown_to_html};
    use std::collections::HashSet;

    with_fs(|fs| {
        #[derive(serde::Serialize)]
        struct ExportedFile {
            path: String,
            content: String,
        }

        fn convert_md_to_html(markdown: &str) -> String {
            let mut options = Options::default();
            options.extension.strikethrough = true;
            options.extension.table = true;
            options.extension.autolink = true;
            options.extension.tasklist = true;
            options.render.r#unsafe = true;

            let html_body = markdown_to_html(markdown, &options);

            // Wrap in a minimal HTML document
            format!(
                r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; max-width: 800px; margin: 0 auto; padding: 2rem; line-height: 1.6; }}
        pre {{ background: #f4f4f4; padding: 1rem; overflow-x: auto; }}
        code {{ background: #f4f4f4; padding: 0.2rem 0.4rem; }}
        img {{ max-width: 100%; }}
        table {{ border-collapse: collapse; width: 100%; }}
        th, td {{ border: 1px solid #ddd; padding: 0.5rem; text-align: left; }}
    </style>
</head>
<body>
{}
</body>
</html>"#,
                html_body
            )
        }

        // Special case: "*" means export all without audience filtering
        if audience == "*" {
            let ws = Workspace::new(fs);
            let mut files: Vec<ExportedFile> = Vec::new();
            let root = Path::new(root_path);
            let root_dir = root.parent().unwrap_or(root);

            fn collect_all<FS: FileSystem>(
                ws: &Workspace<FS>,
                path: &Path,
                root_dir: &Path,
                files: &mut Vec<ExportedFile>,
                visited: &mut HashSet<PathBuf>,
                convert_fn: &dyn Fn(&str) -> String,
            ) {
                if visited.contains(path) {
                    return;
                }
                visited.insert(path.to_path_buf());

                if let Ok(index) = ws.parse_index(path) {
                    let relative_path =
                        pathdiff::diff_paths(path, root_dir).unwrap_or_else(|| path.to_path_buf());

                    if let Ok(content) = ws.fs_ref().read_to_string(path) {
                        // Extract body (without frontmatter)
                        let body = extract_body(&content);
                        let html = convert_fn(&body);

                        // Change extension to .html
                        let html_path = relative_path.to_string_lossy().replace(".md", ".html");

                        files.push(ExportedFile {
                            path: html_path,
                            content: html,
                        });
                    }

                    if index.frontmatter.is_index() {
                        for child_rel in index.frontmatter.contents_list() {
                            let child_path = index.resolve_path(child_rel);
                            if ws.fs_ref().exists(&child_path) {
                                collect_all(ws, &child_path, root_dir, files, visited, convert_fn);
                            }
                        }
                    }
                }
            }

            let mut visited = HashSet::new();
            collect_all(
                &ws,
                root,
                root_dir,
                &mut files,
                &mut visited,
                &convert_md_to_html,
            );

            return serde_wasm_bindgen::to_value(&files)
                .map_err(|e| JsValue::from_str(&e.to_string()));
        }

        // Normal audience-filtered export
        use diaryx_core::export::Exporter;
        let exporter = Exporter::new(fs);

        let plan = exporter
            .plan_export(Path::new(root_path), audience, Path::new("/export"))
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let mut files: Vec<ExportedFile> = Vec::new();

        for export_file in &plan.included {
            let content = fs
                .read_to_string(&export_file.source_path)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Extract body and convert to HTML
            let body = extract_body(&content);
            let html = convert_md_to_html(&body);

            // Change extension to .html
            let html_path = export_file
                .relative_path
                .to_string_lossy()
                .replace(".md", ".html");

            files.push(ExportedFile {
                path: html_path,
                content: html,
            });
        }

        serde_wasm_bindgen::to_value(&files).map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

/// Export binary attachment files for entries in the export.
/// Returns array of {path, data: number[]} for zip creation in JS.
/// Collects all files in _attachments folders for exported entries.
#[wasm_bindgen]
pub fn export_binary_attachments(root_path: &str, _audience: &str) -> Result<JsValue, JsValue> {
    use std::collections::HashSet;

    with_fs(|fs| {
        #[derive(serde::Serialize)]
        struct BinaryExportFile {
            path: String,
            data: Vec<u8>,
        }

        let ws = Workspace::new(fs);
        let root = Path::new(root_path);
        let root_dir = root.parent().unwrap_or(root);
        let mut binary_files: Vec<BinaryExportFile> = Vec::new();
        let mut visited_entries: HashSet<PathBuf> = HashSet::new();
        let mut visited_attachment_dirs: HashSet<PathBuf> = HashSet::new();

        // Collect _attachments folders from all entries in the export
        fn collect_attachments<FS: FileSystem>(
            ws: &Workspace<FS>,
            entry_path: &Path,
            root_dir: &Path,
            binary_files: &mut Vec<BinaryExportFile>,
            visited_entries: &mut HashSet<PathBuf>,
            visited_attachment_dirs: &mut HashSet<PathBuf>,
        ) {
            if visited_entries.contains(entry_path) {
                return;
            }
            visited_entries.insert(entry_path.to_path_buf());

            if let Ok(index) = ws.parse_index(entry_path) {
                // Check for _attachments folder next to this entry
                let entry_dir = entry_path.parent().unwrap_or(Path::new("."));
                let attachments_dir = entry_dir.join("_attachments");

                if ws.fs_ref().is_dir(&attachments_dir)
                    && !visited_attachment_dirs.contains(&attachments_dir)
                {
                    visited_attachment_dirs.insert(attachments_dir.clone());

                    // List all files in _attachments directory
                    if let Ok(files) = ws.fs_ref().list_files(&attachments_dir) {
                        for file_path in files {
                            if !ws.fs_ref().is_dir(&file_path) {
                                // Read binary content
                                if let Ok(data) = ws.fs_ref().read_binary(&file_path) {
                                    let relative_path = pathdiff::diff_paths(&file_path, root_dir)
                                        .unwrap_or_else(|| file_path.clone());

                                    binary_files.push(BinaryExportFile {
                                        path: relative_path.to_string_lossy().to_string(),
                                        data,
                                    });
                                }
                            }
                        }
                    }
                }

                // Recurse into children
                if index.frontmatter.is_index() {
                    for child_rel in index.frontmatter.contents_list() {
                        let child_path = index.resolve_path(child_rel);
                        if ws.fs_ref().exists(&child_path) {
                            collect_attachments(
                                ws,
                                &child_path,
                                root_dir,
                                binary_files,
                                visited_entries,
                                visited_attachment_dirs,
                            );
                        }
                    }
                }
            }
        }

        collect_attachments(
            &ws,
            root,
            root_dir,
            &mut binary_files,
            &mut visited_entries,
            &mut visited_attachment_dirs,
        );

        serde_wasm_bindgen::to_value(&binary_files).map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

/// Helper to remove audience property from content
fn remove_audience_from_content(content: &str) -> String {
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        return content.to_string();
    }

    let rest = &content[4..];
    let Some(end_idx) = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n")) else {
        return content.to_string();
    };

    let frontmatter_str = &rest[..end_idx];
    let body = &rest[end_idx + 5..];

    // Parse and remove audience
    if let Ok(mut frontmatter) = serde_yaml::from_str::<serde_yaml::Value>(frontmatter_str) {
        if let Some(map) = frontmatter.as_mapping_mut() {
            if map
                .remove(serde_yaml::Value::String("audience".to_string()))
                .is_some()
            {
                if let Ok(new_fm) = serde_yaml::to_string(&frontmatter) {
                    return format!("---\n{}---\n{}", new_fm, body);
                }
            }
        }
    }

    content.to_string()
}

/// Helper to filter contents array and remove audience
fn filter_contents_and_audience(content: &str, filtered: &[String]) -> String {
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        return content.to_string();
    }

    let rest = &content[4..];
    let Some(end_idx) = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n")) else {
        return content.to_string();
    };

    let frontmatter_str = &rest[..end_idx];
    let body = &rest[end_idx + 5..];

    if let Ok(mut frontmatter) = serde_yaml::from_str::<serde_yaml::Value>(frontmatter_str) {
        if let Some(map) = frontmatter.as_mapping_mut() {
            // Remove audience
            map.remove(serde_yaml::Value::String("audience".to_string()));

            // Filter contents
            if let Some(contents) = map.get_mut(&serde_yaml::Value::String("contents".to_string()))
            {
                if let Some(arr) = contents.as_sequence_mut() {
                    arr.retain(|item| {
                        if let Some(s) = item.as_str() {
                            !filtered.iter().any(|f| f == s)
                        } else {
                            true
                        }
                    });
                }
            }
        }

        if let Ok(new_fm) = serde_yaml::to_string(&frontmatter) {
            return format!("---\n{}---\n{}", new_fm, body);
        }
    }

    content.to_string()
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
