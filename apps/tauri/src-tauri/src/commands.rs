//! Tauri IPC command handlers
//!
//! These commands are callable from the frontend via Tauri's invoke system.

use std::collections::HashSet;
use std::path::PathBuf;

use diaryx_core::{
    config::Config,
    entry::DiaryxApp,
    error::SerializableError,
    fs::RealFileSystem,
    search::{SearchQuery, SearchResults, Searcher},
    workspace::{TreeNode, Workspace},
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// Entry data returned to frontend
#[derive(Debug, Serialize)]
pub struct EntryData {
    pub path: PathBuf,
    pub title: Option<String>,
    pub frontmatter: serde_json::Map<String, JsonValue>,
    pub content: String,
}

/// Request to save an entry
#[derive(Debug, Deserialize)]
pub struct SaveEntryRequest {
    pub path: String,
    pub content: String,
}

/// Get the current configuration
#[tauri::command]
pub fn get_config() -> Result<Config, SerializableError> {
    Config::load().map_err(|e| e.to_serializable())
}

/// Get the workspace tree structure
#[tauri::command]
pub fn get_workspace_tree(
    workspace_path: Option<String>,
    depth: Option<usize>,
) -> Result<TreeNode, SerializableError> {
    let ws = Workspace::new(RealFileSystem);

    // Resolve workspace path
    let root_path = match workspace_path {
        Some(p) => PathBuf::from(p),
        None => {
            let config = Config::load().map_err(|e| e.to_serializable())?;
            config.default_workspace.clone()
        }
    };

    // Find the root index
    let root_index = ws
        .find_root_index_in_dir(&root_path)
        .map_err(|e| e.to_serializable())?
        .ok_or_else(|| SerializableError {
            kind: "WorkspaceNotFound".to_string(),
            message: format!("No workspace found at '{}'", root_path.display()),
            path: Some(root_path.clone()),
        })?;

    let max_depth = depth;
    let mut visited = HashSet::new();
    ws.build_tree_with_depth(&root_index, max_depth, &mut visited)
        .map_err(|e| e.to_serializable())
}

/// Get an entry's content and metadata
#[tauri::command]
pub fn get_entry(path: String) -> Result<EntryData, SerializableError> {
    let app = DiaryxApp::new(RealFileSystem);
    let path_buf = PathBuf::from(&path);

    // Get frontmatter
    let frontmatter = app
        .get_all_frontmatter(&path)
        .map_err(|e| e.to_serializable())?;

    // Convert to JSON-compatible map
    let mut json_frontmatter = serde_json::Map::new();
    for (key, value) in frontmatter {
        if let Ok(json_val) = serde_json::to_value(&value) {
            json_frontmatter.insert(key, json_val);
        }
    }

    // Extract title
    let title = json_frontmatter
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Get content
    let content = app.get_content(&path).map_err(|e| e.to_serializable())?;

    Ok(EntryData {
        path: path_buf,
        title,
        frontmatter: json_frontmatter,
        content,
    })
}

/// Save an entry's content
#[tauri::command]
pub fn save_entry(request: SaveEntryRequest) -> Result<(), SerializableError> {
    let app = DiaryxApp::new(RealFileSystem);
    app.set_content(&request.path, &request.content)
        .map_err(|e| e.to_serializable())
}

/// Search the workspace
#[tauri::command]
pub fn search_workspace(
    pattern: String,
    workspace_path: Option<String>,
    search_frontmatter: Option<bool>,
    property: Option<String>,
    case_sensitive: Option<bool>,
) -> Result<SearchResults, SerializableError> {
    let searcher = Searcher::new(RealFileSystem);
    let ws = Workspace::new(RealFileSystem);

    // Resolve workspace path
    let root_path = match workspace_path {
        Some(p) => PathBuf::from(p),
        None => {
            let config = Config::load().map_err(|e| e.to_serializable())?;
            config.default_workspace.clone()
        }
    };

    // Find root index
    let root_index = ws
        .find_root_index_in_dir(&root_path)
        .map_err(|e| e.to_serializable())?
        .ok_or_else(|| SerializableError {
            kind: "WorkspaceNotFound".to_string(),
            message: format!("No workspace found at '{}'", root_path.display()),
            path: Some(root_path.clone()),
        })?;

    // Build query
    let query = if let Some(prop) = property {
        SearchQuery::property(&pattern, prop)
    } else if search_frontmatter.unwrap_or(false) {
        SearchQuery::frontmatter(&pattern)
    } else {
        SearchQuery::content(&pattern)
    };

    let query = query.case_sensitive(case_sensitive.unwrap_or(false));

    searcher
        .search_workspace(&root_index, &query)
        .map_err(|e| e.to_serializable())
}

/// Create a new entry
#[tauri::command]
pub fn create_entry(
    path: String,
    title: Option<String>,
    part_of: Option<String>,
) -> Result<PathBuf, SerializableError> {
    let app = DiaryxApp::new(RealFileSystem);
    let path_buf = PathBuf::from(&path);

    // Create entry with basic frontmatter
    let entry_title = title.unwrap_or_else(|| {
        path_buf
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string()
    });

    app.create_entry(&path).map_err(|e| e.to_serializable())?;

    // Set title
    app.set_frontmatter_property(&path, "title", serde_yaml::Value::String(entry_title))
        .map_err(|e| e.to_serializable())?;

    // Set part_of if provided
    if let Some(parent) = part_of {
        app.set_frontmatter_property(&path, "part_of", serde_yaml::Value::String(parent))
            .map_err(|e| e.to_serializable())?;
    }

    Ok(path_buf)
}

/// Get frontmatter for an entry
#[tauri::command]
pub fn get_frontmatter(
    path: String,
) -> Result<serde_json::Map<String, JsonValue>, SerializableError> {
    let app = DiaryxApp::new(RealFileSystem);

    let frontmatter = app
        .get_all_frontmatter(&path)
        .map_err(|e| e.to_serializable())?;

    let mut json_frontmatter = serde_json::Map::new();
    for (key, value) in frontmatter {
        if let Ok(json_val) = serde_json::to_value(&value) {
            json_frontmatter.insert(key, json_val);
        }
    }

    Ok(json_frontmatter)
}

/// Set a frontmatter property
#[tauri::command]
pub fn set_frontmatter_property(
    path: String,
    key: String,
    value: JsonValue,
) -> Result<(), SerializableError> {
    let app = DiaryxApp::new(RealFileSystem);

    // Convert JSON value to YAML value
    let yaml_value: serde_yaml::Value =
        serde_json::from_value(value).map_err(|e| SerializableError {
            kind: "SerializationError".to_string(),
            message: format!("Failed to convert value: {}", e),
            path: None,
        })?;

    app.set_frontmatter_property(&path, &key, yaml_value)
        .map_err(|e| e.to_serializable())
}
