//!
//! Tauri IPC command handlers
//!
//! These commands are callable from the frontend via Tauri's invoke system.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use diaryx_core::{
    config::Config,
    entry::DiaryxApp,
    error::SerializableError,
    fs::{FileSystem, RealFileSystem},
    search::{SearchQuery, SearchResults, Searcher},
    workspace::{TreeNode, Workspace},
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tauri::{AppHandle, Manager, Runtime};

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

/// App paths for different platforms
#[derive(Debug, Serialize)]
pub struct AppPaths {
    /// Directory for app data (config, etc.)
    pub data_dir: PathBuf,
    /// Directory for user documents/workspaces
    pub document_dir: PathBuf,
    /// Default workspace path
    pub default_workspace: PathBuf,
    /// Config file path
    pub config_path: PathBuf,
    /// Whether this is a mobile platform (iOS/Android)
    pub is_mobile: bool,
}

/// Get platform-appropriate paths for the app
/// On iOS/Android, uses Tauri's app_data_dir which is within the app sandbox
/// On desktop, uses the standard dirs crate locations
fn get_platform_paths<R: Runtime>(app: &AppHandle<R>) -> Result<AppPaths, SerializableError> {
    let path_resolver = app.path();

    // Check if we're on mobile (iOS or Android)
    let is_mobile = cfg!(target_os = "ios") || cfg!(target_os = "android");

    if is_mobile {
        // On mobile, use document_dir for user files so they appear in Files app
        // (UIFileSharingEnabled and LSSupportsOpeningDocumentsInPlace must be set in Info.plist)
        let document_dir = path_resolver
            .document_dir()
            .map_err(|e| SerializableError {
                kind: "PathError".to_string(),
                message: format!("Failed to get document directory: {}", e),
                path: None,
            })?;

        // Use app_data_dir for internal config (not exposed to Files app)
        let data_dir = path_resolver
            .app_data_dir()
            .map_err(|e| SerializableError {
                kind: "PathError".to_string(),
                message: format!("Failed to get app data directory: {}", e),
                path: None,
            })?;

        // Workspace goes in Documents so users can access via Files app
        let default_workspace = document_dir.join("Diaryx");
        // Config stays in Application Support (internal)
        let config_path = data_dir.join("config.toml");

        Ok(AppPaths {
            data_dir,
            document_dir,
            default_workspace,
            config_path,
            is_mobile: true,
        })
    } else {
        // On desktop, use standard locations
        let data_dir = path_resolver
            .app_data_dir()
            .map_err(|e| SerializableError {
                kind: "PathError".to_string(),
                message: format!("Failed to get app data directory: {}", e),
                path: None,
            })?;

        let document_dir = path_resolver
            .document_dir()
            .map_err(|e| SerializableError {
                kind: "PathError".to_string(),
                message: format!("Failed to get document directory: {}", e),
                path: None,
            })?;

        // Use the standard config location
        let config_path = path_resolver
            .app_config_dir()
            .map_err(|e| SerializableError {
                kind: "PathError".to_string(),
                message: format!("Failed to get config directory: {}", e),
                path: None,
            })?
            .join("config.toml");

        // Default workspace in home directory for desktop
        let default_workspace = path_resolver
            .home_dir()
            .unwrap_or_else(|_| document_dir.clone())
            .join("diaryx");

        Ok(AppPaths {
            data_dir,
            document_dir,
            default_workspace,
            config_path,
            is_mobile: false,
        })
    }
}

/// Get the app paths for the current platform
#[tauri::command]
pub fn get_app_paths<R: Runtime>(app: AppHandle<R>) -> Result<AppPaths, SerializableError> {
    get_platform_paths(&app)
}

/// Initialize the app - creates necessary directories and default workspace if needed
#[tauri::command]
pub fn initialize_app<R: Runtime>(app: AppHandle<R>) -> Result<AppPaths, SerializableError> {
    log::info!("[initialize_app] Starting initialization...");

    let paths = get_platform_paths(&app).map_err(|e| {
        log::error!("[initialize_app] Failed to get platform paths: {:?}", e);
        e
    })?;

    log::info!("[initialize_app] Platform paths resolved:");
    log::info!("  data_dir: {:?}", paths.data_dir);
    log::info!("  document_dir: {:?}", paths.document_dir);
    log::info!("  default_workspace: {:?}", paths.default_workspace);
    log::info!("  config_path: {:?}", paths.config_path);
    log::info!("  is_mobile: {}", paths.is_mobile);

    // Create data directory if it doesn't exist
    if !paths.data_dir.exists() {
        log::info!("[initialize_app] Creating data directory...");
        std::fs::create_dir_all(&paths.data_dir).map_err(|e| {
            log::error!("[initialize_app] Failed to create data directory: {}", e);
            SerializableError {
                kind: "IoError".to_string(),
                message: format!("Failed to create data directory: {}", e),
                path: Some(paths.data_dir.clone()),
            }
        })?;
    }

    // Create default workspace if it doesn't exist
    if !paths.default_workspace.exists() {
        log::info!("[initialize_app] Creating workspace directory...");
        std::fs::create_dir_all(&paths.default_workspace).map_err(|e| {
            log::error!(
                "[initialize_app] Failed to create workspace directory: {}",
                e
            );
            SerializableError {
                kind: "IoError".to_string(),
                message: format!("Failed to create workspace directory: {}", e),
                path: Some(paths.default_workspace.clone()),
            }
        })?;
    }

    // Check if workspace is already initialized (has a root index file)
    log::info!("[initialize_app] Checking if workspace is initialized...");
    let ws = Workspace::new(RealFileSystem);
    let workspace_initialized = match ws.find_root_index_in_dir(&paths.default_workspace) {
        Ok(Some(path)) => {
            log::info!("[initialize_app] Found existing root index at: {:?}", path);
            true
        }
        Ok(None) => {
            log::info!("[initialize_app] No root index found, workspace needs initialization");
            false
        }
        Err(e) => {
            log::warn!(
                "[initialize_app] Error checking for root index: {:?}, assuming not initialized",
                e
            );
            false
        }
    };

    if !workspace_initialized {
        log::info!("[initialize_app] Initializing workspace...");
        ws.init_workspace(&paths.default_workspace, Some("My Workspace"), None)
            .map_err(|e| {
                log::error!("[initialize_app] Failed to initialize workspace: {:?}", e);
                e.to_serializable()
            })?;
        log::info!("[initialize_app] Workspace initialized successfully");
    }

    // Create or update config file
    // On mobile, we use a placeholder path since iOS changes container UUIDs between reinstalls
    // The actual path resolution happens at runtime in get_workspace_tree and other commands
    log::info!("[initialize_app] Loading/creating config...");
    let config = if paths.is_mobile {
        // On mobile, always use a placeholder - actual paths resolved at runtime
        log::info!("[initialize_app] Mobile: using placeholder workspace path in config");
        Config::new(PathBuf::from("workspace"))
    } else if paths.config_path.exists() {
        log::info!(
            "[initialize_app] Loading existing config from {:?}",
            paths.config_path
        );
        Config::load_from(&RealFileSystem, &paths.config_path).unwrap_or_else(|e| {
            log::warn!(
                "[initialize_app] Failed to load config, creating new: {:?}",
                e
            );
            Config::new(paths.default_workspace.clone())
        })
    } else {
        log::info!("[initialize_app] Creating new config");
        Config::new(paths.default_workspace.clone())
    };

    // Save config (ensures parent directories exist)
    log::info!("[initialize_app] Saving config to {:?}", paths.config_path);
    config
        .save_to(&RealFileSystem, &paths.config_path)
        .map_err(|e| {
            log::error!("[initialize_app] Failed to save config: {:?}", e);
            e.to_serializable()
        })?;

    log::info!("[initialize_app] Initialization complete!");
    Ok(paths)
}

/// Create a new workspace at the specified path
#[tauri::command]
pub fn create_workspace<R: Runtime>(
    app: AppHandle<R>,
    path: Option<String>,
    name: Option<String>,
) -> Result<PathBuf, SerializableError> {
    let paths = get_platform_paths(&app)?;

    // Use provided path or default workspace path
    let workspace_path = path
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.default_workspace.clone());

    let workspace_name = name.as_deref().unwrap_or("My Workspace");

    // Create the directory if it doesn't exist
    if !workspace_path.exists() {
        std::fs::create_dir_all(&workspace_path).map_err(|e| SerializableError {
            kind: "IoError".to_string(),
            message: format!("Failed to create workspace directory: {}", e),
            path: Some(workspace_path.clone()),
        })?;
    }

    // Initialize the workspace
    let ws = Workspace::new(RealFileSystem);
    ws.init_workspace(&workspace_path, Some(workspace_name), None)
        .map_err(|e| e.to_serializable())?;

    Ok(workspace_path)
}

/// Get the current configuration (platform-aware)
#[tauri::command]
pub fn get_config<R: Runtime>(app: AppHandle<R>) -> Result<Config, SerializableError> {
    let paths = get_platform_paths(&app)?;

    // On mobile, always return config with current app data directory paths
    // to avoid stale paths from previous container UUIDs
    if paths.is_mobile {
        return Ok(Config::new(paths.default_workspace));
    }

    if paths.config_path.exists() {
        Config::load_from(&RealFileSystem, &paths.config_path).map_err(|e| e.to_serializable())
    } else {
        // Return default config with platform-appropriate paths
        Ok(Config::new(paths.default_workspace))
    }
}

/// Save the configuration (platform-aware)
#[tauri::command]
pub fn save_config<R: Runtime>(app: AppHandle<R>, config: Config) -> Result<(), SerializableError> {
    let paths = get_platform_paths(&app)?;
    config
        .save_to(&RealFileSystem, &paths.config_path)
        .map_err(|e| e.to_serializable())
}

/// Get the workspace tree structure
#[tauri::command]
pub fn get_workspace_tree<R: Runtime>(
    app: AppHandle<R>,
    workspace_path: Option<String>,
    depth: Option<usize>,
) -> Result<TreeNode, SerializableError> {
    log::info!(
        "[get_workspace_tree] Called with workspace_path: {:?}, depth: {:?}",
        workspace_path,
        depth
    );

    let ws = Workspace::new(RealFileSystem);
    let paths = get_platform_paths(&app)?;
    log::info!(
        "[get_workspace_tree] Platform paths: default_workspace={:?}",
        paths.default_workspace
    );

    // Resolve workspace path
    // On mobile (iOS/Android), ALWAYS use the current app data directory
    // because the container UUID changes between app reinstalls and the
    // config file may contain stale absolute paths
    let root_path = match workspace_path {
        Some(p) if !paths.is_mobile => {
            log::info!("[get_workspace_tree] Using provided workspace path: {}", p);
            PathBuf::from(p)
        }
        _ if paths.is_mobile => {
            // On mobile, always use current default_workspace from platform paths
            log::info!(
                "[get_workspace_tree] Mobile: using current app data directory: {:?}",
                paths.default_workspace
            );
            paths.default_workspace.clone()
        }
        _ => {
            // Desktop: Try to load config, fall back to default workspace
            let resolved = if paths.config_path.exists() {
                Config::load_from(&RealFileSystem, &paths.config_path)
                    .map(|c| c.default_workspace)
                    .unwrap_or(paths.default_workspace.clone())
            } else {
                paths.default_workspace.clone()
            };
            log::info!(
                "[get_workspace_tree] Desktop: resolved workspace path from config: {:?}",
                resolved
            );
            resolved
        }
    };

    log::info!(
        "[get_workspace_tree] Looking for root index in: {:?}",
        root_path
    );

    // Find the root index
    let root_index = match ws.find_root_index_in_dir(&root_path) {
        Ok(Some(path)) => {
            log::info!("[get_workspace_tree] Found root index at: {:?}", path);
            path
        }
        Ok(None) => {
            log::error!(
                "[get_workspace_tree] No root index found in {:?}",
                root_path
            );
            return Err(SerializableError {
                kind: "WorkspaceNotFound".to_string(),
                message: format!(
                    "No workspace found at '{}'. Try initializing the app first.",
                    root_path.display()
                ),
                path: Some(root_path.clone()),
            });
        }
        Err(e) => {
            log::error!("[get_workspace_tree] Error finding root index: {:?}", e);
            return Err(e.to_serializable());
        }
    };

    let max_depth = depth;
    let mut visited = HashSet::new();
    log::info!("[get_workspace_tree] Building tree from root index...");
    let tree = ws
        .build_tree_with_depth(&root_index, max_depth, &mut visited)
        .map_err(|e| {
            log::error!("[get_workspace_tree] Error building tree: {:?}", e);
            e.to_serializable()
        })?;

    log::info!(
        "[get_workspace_tree] Tree built successfully: name={}",
        tree.name
    );
    Ok(tree)
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

/// Save an entry's content and update the 'updated' timestamp
#[tauri::command]
pub fn save_entry(request: SaveEntryRequest) -> Result<(), SerializableError> {
    let app = DiaryxApp::new(RealFileSystem);
    app.save_content(&request.path, &request.content)
        .map_err(|e| e.to_serializable())
}

/// Search the workspace
#[tauri::command]
pub fn search_workspace<R: Runtime>(
    app: AppHandle<R>,
    pattern: String,
    workspace_path: Option<String>,
    search_frontmatter: Option<bool>,
    property: Option<String>,
    case_sensitive: Option<bool>,
) -> Result<SearchResults, SerializableError> {
    let searcher = Searcher::new(RealFileSystem);
    let ws = Workspace::new(RealFileSystem);
    let paths = get_platform_paths(&app)?;

    // Resolve workspace path
    // On mobile, always use current app data directory (see get_workspace_tree for explanation)
    let root_path = match workspace_path {
        Some(p) if !paths.is_mobile => PathBuf::from(p),
        _ if paths.is_mobile => paths.default_workspace.clone(),
        _ => {
            if paths.config_path.exists() {
                Config::load_from(&RealFileSystem, &paths.config_path)
                    .map(|c| c.default_workspace)
                    .unwrap_or(paths.default_workspace.clone())
            } else {
                paths.default_workspace.clone()
            }
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

/// Delete an entry
#[tauri::command]
pub fn delete_entry(path: String) -> Result<(), SerializableError> {
    RealFileSystem
        .delete_file(Path::new(&path))
        .map_err(|e| SerializableError {
            kind: "FileDeleteError".to_string(),
            message: format!("Failed to delete entry '{}': {}", path, e),
            path: Some(PathBuf::from(path)),
        })
}

#[derive(serde::Deserialize)]
pub struct MoveEntryRequest {
    pub from_path: String,
    pub to_path: String,
}

#[derive(serde::Deserialize)]
pub struct AttachEntryToParentRequest {
    pub entry_path: String,
    pub parent_index_path: String,
}

/// Move/rename an entry, keeping parent index `contents` and entry `part_of` consistent.
#[tauri::command]
pub fn move_entry(request: MoveEntryRequest) -> Result<PathBuf, SerializableError> {
    let app = DiaryxApp::new(RealFileSystem);

    let from = PathBuf::from(&request.from_path);
    let to = PathBuf::from(&request.to_path);

    if from == to {
        return Ok(to);
    }

    // Compute old/new parent indexes and file names before moving
    let old_parent = from.parent().ok_or_else(|| SerializableError {
        kind: "InvalidPath".to_string(),
        message: "No parent directory for source path".to_string(),
        path: Some(from.clone()),
    })?;
    let old_index = old_parent.join("index.md");
    let old_file_name =
        from.file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| SerializableError {
                kind: "InvalidPath".to_string(),
                message: "Invalid source file name".to_string(),
                path: Some(from.clone()),
            })?;

    let new_parent = to.parent().ok_or_else(|| SerializableError {
        kind: "InvalidPath".to_string(),
        message: "No parent directory for destination path".to_string(),
        path: Some(to.clone()),
    })?;
    let new_index = new_parent.join("index.md");
    let new_file_name =
        to.file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| SerializableError {
                kind: "InvalidPath".to_string(),
                message: "Invalid destination file name".to_string(),
                path: Some(to.clone()),
            })?;

    // Move file
    RealFileSystem
        .move_file(&from, &to)
        .map_err(|e| SerializableError {
            kind: "FileMoveError".to_string(),
            message: format!(
                "Failed to move entry '{}' -> '{}': {}",
                request.from_path, request.to_path, e
            ),
            path: Some(from.clone()),
        })?;

    // Update old parent index contents (if it exists)
    if RealFileSystem.exists(&old_index) {
        remove_from_index_contents(&app, &old_index, old_file_name)?;
    }

    // Update new parent index contents + part_of (if it exists)
    if RealFileSystem.exists(&new_index) {
        add_to_index_contents(&app, &new_index, new_file_name)?;

        let rel_part_of = relative_path_from_entry_to_target(&to, &new_index);
        let to_str = request.to_path.clone();
        app.set_frontmatter_property(&to_str, "part_of", serde_yaml::Value::String(rel_part_of))
            .map_err(|e| e.to_serializable())?;
    }

    Ok(to)
}

fn add_to_index_contents(
    app: &DiaryxApp<RealFileSystem>,
    index_path: &Path,
    entry: &str,
) -> Result<(), SerializableError> {
    let index_str = index_path.to_string_lossy().to_string();

    let frontmatter = app
        .get_all_frontmatter(&index_str)
        .map_err(|e| e.to_serializable())?;

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

    if !contents.contains(&entry.to_string()) {
        contents.push(entry.to_string());
        contents.sort();

        let yaml_contents: Vec<serde_yaml::Value> = contents
            .into_iter()
            .map(serde_yaml::Value::String)
            .collect();

        app.set_frontmatter_property(
            &index_str,
            "contents",
            serde_yaml::Value::Sequence(yaml_contents),
        )
        .map_err(|e| e.to_serializable())?;
    }

    Ok(())
}

fn remove_from_index_contents(
    app: &DiaryxApp<RealFileSystem>,
    index_path: &Path,
    entry: &str,
) -> Result<(), SerializableError> {
    let index_str = index_path.to_string_lossy().to_string();

    let frontmatter = app
        .get_all_frontmatter(&index_str)
        .map_err(|e| e.to_serializable())?;

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

    let before_len = contents.len();
    contents.retain(|c| c != entry);

    if contents.len() != before_len {
        contents.sort();

        let yaml_contents: Vec<serde_yaml::Value> = contents
            .into_iter()
            .map(serde_yaml::Value::String)
            .collect();

        app.set_frontmatter_property(
            &index_str,
            "contents",
            serde_yaml::Value::Sequence(yaml_contents),
        )
        .map_err(|e| e.to_serializable())?;
    }

    Ok(())
}

/// Compute a relative path from the entry file location to a target file.
/// Example: entry at `a/b/note.md`, target at `a/index.md` => `../index.md`
fn relative_path_from_entry_to_target(entry_path: &Path, target_path: &Path) -> String {
    let entry_dir = entry_path.parent().unwrap_or_else(|| Path::new(""));

    let entry_components: Vec<_> = entry_dir.components().collect();
    let target_components: Vec<_> = target_path.components().collect();

    let mut common = 0usize;
    while common < entry_components.len()
        && common < target_components.len()
        && entry_components[common] == target_components[common]
    {
        common += 1;
    }

    let mut parts: Vec<String> = Vec::new();
    for _ in common..entry_components.len() {
        parts.push("..".to_string());
    }

    for comp in target_components.iter().skip(common) {
        parts.push(comp.as_os_str().to_string_lossy().to_string());
    }

    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

/// Add `entry` to `index_path` frontmatter `contents` sequence (if not already present).
fn add_to_index_contents_tauri(
    app: &DiaryxApp<RealFileSystem>,
    index_path: &Path,
    entry: &str,
) -> Result<(), SerializableError> {
    let index_str = index_path.to_string_lossy().to_string();

    let frontmatter = app
        .get_all_frontmatter(&index_str)
        .map_err(|e| e.to_serializable())?;

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

    if !contents.contains(&entry.to_string()) {
        contents.push(entry.to_string());
        contents.sort();

        let yaml_contents: Vec<serde_yaml::Value> = contents
            .into_iter()
            .map(serde_yaml::Value::String)
            .collect();

        app.set_frontmatter_property(
            &index_str,
            "contents",
            serde_yaml::Value::Sequence(yaml_contents),
        )
        .map_err(|e| e.to_serializable())?;
    }

    Ok(())
}

/// Attach an existing entry to a parent index ("workspace add" behavior).
/// - Adds the entry to the parent's `contents` using a path relative to the parent index directory
/// - Sets the entry's `part_of` to point back to the parent index (relative to the entry)
#[tauri::command]
pub fn attach_entry_to_parent(
    request: AttachEntryToParentRequest,
) -> Result<(), SerializableError> {
    let app = DiaryxApp::new(RealFileSystem);

    let entry = PathBuf::from(&request.entry_path);
    let parent_index = PathBuf::from(&request.parent_index_path);

    if !RealFileSystem.exists(&entry) {
        return Err(SerializableError {
            kind: "FileNotFound".to_string(),
            message: format!("Entry does not exist: {}", request.entry_path),
            path: Some(entry),
        });
    }

    if !RealFileSystem.exists(&parent_index) {
        return Err(SerializableError {
            kind: "FileNotFound".to_string(),
            message: format!("Parent index does not exist: {}", request.parent_index_path),
            path: Some(parent_index),
        });
    }

    // Add child link to parent's contents (relative to the parent index directory)
    let parent_dir = parent_index.parent().unwrap_or_else(|| Path::new(""));
    let child_rel = relative_path_from_dir_to_target(parent_dir, &entry);
    add_to_index_contents_tauri(&app, &parent_index, &child_rel)?;

    // Set child's part_of (relative to the entry directory)
    let parent_rel = relative_path_from_entry_to_target(&entry, &parent_index);
    app.set_frontmatter_property(
        &request.entry_path,
        "part_of",
        serde_yaml::Value::String(parent_rel),
    )
    .map_err(|e| e.to_serializable())?;

    Ok(())
}

/// Compute a relative path from a base directory to a target file.
/// Example: base_dir `workspace`, target `workspace/Daily/daily_index.md` => `Daily/daily_index.md`
fn relative_path_from_dir_to_target(base_dir: &Path, target_path: &Path) -> String {
    let base_components: Vec<_> = base_dir.components().collect();
    let target_components: Vec<_> = target_path.components().collect();

    let mut common = 0usize;
    while common < base_components.len()
        && common < target_components.len()
        && base_components[common] == target_components[common]
    {
        common += 1;
    }

    let mut parts: Vec<String> = Vec::new();
    for _ in common..base_components.len() {
        parts.push("..".to_string());
    }

    for comp in target_components.iter().skip(common) {
        parts.push(comp.as_os_str().to_string_lossy().to_string());
    }

    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}
