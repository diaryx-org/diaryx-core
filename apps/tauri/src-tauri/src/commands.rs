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
    validate::{ValidationResult, Validator},
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

/// Get the filesystem tree structure (for "Show All Files" mode)
/// Unlike get_workspace_tree, this scans actual filesystem rather than following contents/part_of
#[tauri::command]
pub fn get_filesystem_tree<R: Runtime>(
    app: AppHandle<R>,
    workspace_path: Option<String>,
    show_hidden: Option<bool>,
) -> Result<TreeNode, SerializableError> {
    log::info!(
        "[get_filesystem_tree] Called with workspace_path: {:?}, show_hidden: {:?}",
        workspace_path,
        show_hidden
    );

    let paths = get_platform_paths(&app)?;

    // Determine workspace root
    let root_path = match workspace_path {
        Some(ref p) => PathBuf::from(p),
        None => {
            // Load config to get default workspace
            let config = Config::load_from(&RealFileSystem, &paths.config_path)
                .map_err(|e| e.to_serializable())?;
            PathBuf::from(&config.default_workspace)
        }
    };

    log::info!("[get_filesystem_tree] Using root path: {:?}", root_path);

    // Build filesystem tree
    let ws = Workspace::new(RealFileSystem);
    let tree = ws
        .build_filesystem_tree(&root_path, show_hidden.unwrap_or(false))
        .map_err(|e| {
            log::error!("[get_filesystem_tree] Error building tree: {:?}", e);
            e.to_serializable()
        })?;

    log::info!(
        "[get_filesystem_tree] Tree built successfully: name={}",
        tree.name
    );
    Ok(tree)
}

/// Validate workspace links and find unlinked entries
#[tauri::command]
pub fn validate_workspace<R: Runtime>(
    app: AppHandle<R>,
    workspace_path: Option<String>,
) -> Result<ValidationResult, SerializableError> {
    log::info!(
        "[validate_workspace] Called with workspace_path: {:?}",
        workspace_path
    );

    let paths = get_platform_paths(&app)?;

    // Determine workspace root
    let root_path = match workspace_path {
        Some(ref p) => PathBuf::from(p),
        None => {
            // Load config to get default workspace
            let config = Config::load_from(&RealFileSystem, &paths.config_path)
                .map_err(|e| e.to_serializable())?;
            PathBuf::from(&config.default_workspace)
        }
    };

    log::info!("[validate_workspace] Using root path: {:?}", root_path);

    // Find the index file in the workspace directory
    let ws = Workspace::new(RealFileSystem);
    let index_path = ws
        .find_any_index_in_dir(&root_path)
        .map_err(|e| e.to_serializable())?
        .ok_or_else(|| {
            diaryx_core::error::DiaryxError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("No index file found in workspace: {:?}", root_path),
            ))
            .to_serializable()
        })?;

    // Run validation
    let validator = Validator::new(RealFileSystem);
    let result = validator
        .validate_workspace(&index_path)
        .map_err(|e| e.to_serializable())?;

    log::info!(
        "[validate_workspace] Validation complete: {} errors, {} warnings, {} files checked",
        result.errors.len(),
        result.warnings.len(),
        result.files_checked
    );

    Ok(result)
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

/// Remove a frontmatter property
#[tauri::command]
pub fn remove_frontmatter_property(path: String, key: String) -> Result<(), SerializableError> {
    let app = DiaryxApp::new(RealFileSystem);
    app.remove_frontmatter_property(&path, &key)
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
    use diaryx_core::workspace::Workspace;

    let from = PathBuf::from(&request.from_path);
    let to = PathBuf::from(&request.to_path);

    if from == to {
        return Ok(to);
    }

    // Use shared Workspace::move_entry which correctly finds parent index files
    // using find_any_index_in_dir (handles both index.md and {dirname}.md naming)
    let ws = Workspace::new(RealFileSystem);
    ws.move_entry(&from, &to)
        .map_err(|e| e.to_serializable())?;

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

/// Attach an existing entry to a parent, moving it into the parent's directory.
///
/// Uses Workspace::attach_and_move_entry_to_parent from diaryx_core which:
/// - Converts parent to index if it's a leaf file (creates directory)
/// - Moves entry into the parent's directory if not already there
/// - Creates bidirectional links (contents and part_of)
///
/// Returns the new path to the entry after any moves.
#[tauri::command]
pub fn attach_entry_to_parent(
    request: AttachEntryToParentRequest,
) -> Result<PathBuf, SerializableError> {
    let ws = Workspace::new(RealFileSystem);

    let entry = PathBuf::from(&request.entry_path);
    let parent = PathBuf::from(&request.parent_index_path);

    ws.attach_and_move_entry_to_parent(&entry, &parent)
        .map_err(|e| e.to_serializable())
}

// ============================================================================
// Attachment Commands
// ============================================================================

/// Get attachments list from an entry's frontmatter
#[tauri::command]
pub fn get_attachments(entry_path: String) -> Result<Vec<String>, SerializableError> {
    let app = DiaryxApp::new(RealFileSystem);

    let frontmatter = app
        .get_all_frontmatter(&entry_path)
        .map_err(|e| e.to_serializable())?;

    let attachments: Vec<String> = frontmatter
        .get("attachments")
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

    Ok(attachments)
}

/// Upload an attachment file (base64 encoded) to the entry's _attachments folder
#[tauri::command]
pub fn upload_attachment(
    entry_path: String,
    filename: String,
    data_base64: String,
) -> Result<String, SerializableError> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    let entry = PathBuf::from(&entry_path);
    let entry_dir = entry.parent().unwrap_or_else(|| Path::new("."));
    let attachments_dir = entry_dir.join("_attachments");

    // Create _attachments directory if needed
    if !attachments_dir.exists() {
        std::fs::create_dir_all(&attachments_dir).map_err(|e| SerializableError {
            kind: "IoError".to_string(),
            message: format!("Failed to create attachments directory: {}", e),
            path: Some(attachments_dir.clone()),
        })?;
    }

    // Decode base64 data
    let data = STANDARD
        .decode(&data_base64)
        .map_err(|e| SerializableError {
            kind: "DecodeError".to_string(),
            message: format!("Failed to decode base64 data: {}", e),
            path: None,
        })?;

    // Write file
    let dest_path = attachments_dir.join(&filename);
    std::fs::write(&dest_path, &data).map_err(|e| SerializableError {
        kind: "IoError".to_string(),
        message: format!("Failed to write attachment: {}", e),
        path: Some(dest_path.clone()),
    })?;

    // Add to frontmatter attachments
    let attachment_rel_path = format!("_attachments/{}", filename);
    let app = DiaryxApp::new(RealFileSystem);
    app.add_attachment(&entry_path, &attachment_rel_path)
        .map_err(|e| e.to_serializable())?;

    Ok(attachment_rel_path)
}

/// Delete an attachment file and remove from entry's frontmatter
#[tauri::command]
pub fn delete_attachment(
    entry_path: String,
    attachment_path: String,
) -> Result<(), SerializableError> {
    let entry = PathBuf::from(&entry_path);
    let entry_dir = entry.parent().unwrap_or_else(|| Path::new("."));
    let full_path = entry_dir.join(&attachment_path);

    // Delete the file if it exists
    if full_path.exists() {
        std::fs::remove_file(&full_path).map_err(|e| SerializableError {
            kind: "IoError".to_string(),
            message: format!("Failed to delete attachment file: {}", e),
            path: Some(full_path.clone()),
        })?;
    }

    // Remove from frontmatter
    let app = DiaryxApp::new(RealFileSystem);
    app.remove_attachment(&entry_path, &attachment_path)
        .map_err(|e| e.to_serializable())?;

    Ok(())
}

/// Read binary attachment data
#[tauri::command]
pub fn get_attachment_data(
    entry_path: String,
    attachment_path: String,
) -> Result<Vec<u8>, SerializableError> {
    let entry = PathBuf::from(&entry_path);
    let entry_dir = entry.parent().unwrap_or_else(|| Path::new("."));
    let full_path = entry_dir.join(&attachment_path);

    std::fs::read(&full_path).map_err(|e| SerializableError {
        kind: "IoError".to_string(),
        message: format!("Failed to read attachment: {}", e),
        path: Some(full_path),
    })
}

/// Storage info returned to frontend
#[derive(Debug, Serialize)]
pub struct StorageInfo {
    pub used: u64,
    pub total: u64,
    pub percentage: f64,
}

/// Get storage usage for the workspace
#[tauri::command]
pub fn get_storage_usage<R: Runtime>(app: AppHandle<R>) -> Result<StorageInfo, SerializableError> {
    let paths = get_platform_paths(&app)?;

    // Calculate used space in workspace
    let used = calculate_dir_size(&paths.default_workspace);

    // For total, use a reasonable default (can't easily get disk space on mobile)
    // On desktop, this could be enhanced to get actual disk space
    let total = 10 * 1024 * 1024 * 1024; // 10 GB placeholder
    let percentage = if total > 0 {
        (used as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    Ok(StorageInfo {
        used,
        total,
        percentage,
    })
}

/// Recursively calculate directory size
fn calculate_dir_size(path: &Path) -> u64 {
    let mut total = 0u64;

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_file() {
                if let Ok(metadata) = entry_path.metadata() {
                    total += metadata.len();
                }
            } else if entry_path.is_dir() {
                total += calculate_dir_size(&entry_path);
            }
        }
    }

    total
}

// ============================================================================
// Entry Operation Commands
// ============================================================================

/// Convert a leaf entry to an index by adding empty `contents` property
#[tauri::command]
pub fn convert_to_index(path: String) -> Result<PathBuf, SerializableError> {
    let app = DiaryxApp::new(RealFileSystem);
    let path_buf = PathBuf::from(&path);

    // Check if already has contents
    let frontmatter = app
        .get_all_frontmatter(&path)
        .map_err(|e| e.to_serializable())?;

    if frontmatter.contains_key("contents") {
        // Already an index, return as-is
        return Ok(path_buf);
    }

    // Add empty contents array
    app.set_frontmatter_property(&path, "contents", serde_yaml::Value::Sequence(vec![]))
        .map_err(|e| e.to_serializable())?;

    Ok(path_buf)
}

/// Convert an index entry to a leaf by removing `contents` property
#[tauri::command]
pub fn convert_to_leaf(path: String) -> Result<PathBuf, SerializableError> {
    let app = DiaryxApp::new(RealFileSystem);
    let path_buf = PathBuf::from(&path);

    // Remove contents property if it exists
    app.remove_frontmatter_property(&path, "contents")
        .map_err(|e| e.to_serializable())?;

    Ok(path_buf)
}

/// Create a new child entry under a parent.
///
/// Uses Workspace::create_child_entry from diaryx_core which:
/// - Converts parent to index if it's a leaf file (creates directory)
/// - Generates a unique filename
/// - Creates the entry with title, created, and updated frontmatter
/// - Attaches the new entry to the parent
///
/// Returns the path to the newly created entry.
#[tauri::command]
pub fn create_child_entry(
    parent_path: String,
    title: Option<String>,
) -> Result<PathBuf, SerializableError> {
    let ws = Workspace::new(RealFileSystem);
    let parent = PathBuf::from(&parent_path);

    ws.create_child_entry(&parent, title.as_deref())
        .map_err(|e| e.to_serializable())
}

/// Rename an entry file while updating references
#[tauri::command]
pub fn rename_entry(path: String, new_filename: String) -> Result<PathBuf, SerializableError> {
    let from = PathBuf::from(&path);
    let parent_dir = from.parent().unwrap_or_else(|| Path::new("."));
    let to = parent_dir.join(&new_filename);

    if from == to {
        return Ok(to);
    }

    // Use move_entry logic for consistency
    let request = MoveEntryRequest {
        from_path: path,
        to_path: to.to_string_lossy().to_string(),
    };

    move_entry(request)
}

/// Ensure today's daily entry exists, creating if needed
#[tauri::command]
pub fn ensure_daily_entry<R: Runtime>(app: AppHandle<R>) -> Result<PathBuf, SerializableError> {
    use chrono::Local;

    let paths = get_platform_paths(&app)?;
    let config = if paths.config_path.exists() && !paths.is_mobile {
        Config::load_from(&RealFileSystem, &paths.config_path)
            .unwrap_or_else(|_| Config::new(paths.default_workspace.clone()))
    } else {
        Config::new(paths.default_workspace.clone())
    };

    // Get daily folder from config or use "Daily" default
    let daily_folder = config
        .daily_entry_folder
        .clone()
        .unwrap_or_else(|| "Daily".to_string());
    let daily_dir = paths.default_workspace.join(&daily_folder);

    // Create daily directory if needed
    if !daily_dir.exists() {
        std::fs::create_dir_all(&daily_dir).map_err(|e| SerializableError {
            kind: "IoError".to_string(),
            message: format!("Failed to create daily folder: {}", e),
            path: Some(daily_dir.clone()),
        })?;
    }

    // Generate today's filename
    let today = Local::now();
    let filename = format!("{}.md", today.format("%Y-%m-%d"));
    let entry_path = daily_dir.join(&filename);

    // If file exists, return it
    if entry_path.exists() {
        return Ok(entry_path);
    }

    // Create new daily entry
    let entry_path_str = entry_path.to_string_lossy().to_string();
    let diary_app = DiaryxApp::new(RealFileSystem);

    diary_app
        .create_entry(&entry_path_str)
        .map_err(|e| e.to_serializable())?;

    // Set title to today's date
    let title = today.format("%B %d, %Y").to_string(); // e.g., "December 24, 2024"
    diary_app
        .set_frontmatter_property(&entry_path_str, "title", serde_yaml::Value::String(title))
        .map_err(|e| e.to_serializable())?;

    Ok(entry_path)
}

// ============================================================================
// Export Commands
// ============================================================================

/// Get all available audience tags from the workspace
#[tauri::command]
pub fn get_available_audiences<R: Runtime>(
    app: AppHandle<R>,
    root_path: String,
) -> Result<Vec<String>, SerializableError> {
    let paths = get_platform_paths(&app)?;
    let ws = Workspace::new(RealFileSystem);

    // Use provided root_path or default workspace
    let root = if root_path.is_empty() {
        paths.default_workspace.clone()
    } else {
        PathBuf::from(&root_path)
    };

    // Determine if root is already a file (index) or a directory
    let root_index = if root.is_file() {
        // It's already an index file, use it directly
        root.clone()
    } else {
        // It's a directory, find the root index inside
        ws.find_root_index_in_dir(&root)
            .map_err(|e| e.to_serializable())?
            .ok_or_else(|| SerializableError {
                kind: "WorkspaceNotFound".to_string(),
                message: format!("No workspace found at '{}'", root.display()),
                path: Some(root.clone()),
            })?
    };

    let mut audiences: HashSet<String> = HashSet::new();

    fn collect_audiences(
        ws: &Workspace<RealFileSystem>,
        path: &Path,
        audiences: &mut HashSet<String>,
        visited: &mut HashSet<PathBuf>,
    ) {
        if visited.contains(path) {
            return;
        }
        visited.insert(path.to_path_buf());

        if let Ok(index) = ws.parse_index(path) {
            if let Some(file_audiences) = &index.frontmatter.audience {
                for a in file_audiences {
                    if a.to_lowercase() != "private" {
                        audiences.insert(a.clone());
                    }
                }
            }

            if index.frontmatter.is_index() {
                for child_rel in index.frontmatter.contents_list() {
                    let child_path = index.resolve_path(child_rel);
                    if RealFileSystem.exists(&child_path) {
                        collect_audiences(ws, &child_path, audiences, visited);
                    }
                }
            }
        }
    }

    let mut visited = HashSet::new();
    collect_audiences(&ws, &root_index, &mut audiences, &mut visited);

    let mut result: Vec<String> = audiences.into_iter().collect();
    result.sort();

    Ok(result)
}

/// Export plan structures for serialization
#[derive(Debug, Serialize)]
pub struct ExportPlanResult {
    pub included: Vec<IncludedFile>,
    pub excluded: Vec<ExcludedFile>,
    pub audience: String,
}

#[derive(Debug, Serialize)]
pub struct IncludedFile {
    pub path: String,
    pub relative_path: String,
}

#[derive(Debug, Serialize)]
pub struct ExcludedFile {
    pub path: String,
    pub reason: String,
}

/// Plan an export operation
#[tauri::command]
pub fn plan_export<R: Runtime>(
    app: AppHandle<R>,
    root_path: String,
    audience: String,
) -> Result<ExportPlanResult, SerializableError> {
    use diaryx_core::export::Exporter;

    let paths = get_platform_paths(&app)?;
    let ws = Workspace::new(RealFileSystem);

    let root = if root_path.is_empty() {
        paths.default_workspace.clone()
    } else {
        PathBuf::from(&root_path)
    };

    // Determine if root is already a file (index) or a directory
    let root_index = if root.is_file() {
        root.clone()
    } else {
        ws.find_root_index_in_dir(&root)
            .map_err(|e| e.to_serializable())?
            .ok_or_else(|| SerializableError {
                kind: "WorkspaceNotFound".to_string(),
                message: format!("No workspace found at '{}'", root.display()),
                path: Some(root.clone()),
            })?
    };

    let root_dir = root_index.parent().unwrap_or(&root_index);

    // Special case: "*" means export all without audience filtering
    if audience == "*" {
        let mut included = Vec::new();

        fn collect_all(
            ws: &Workspace<RealFileSystem>,
            path: &Path,
            root_dir: &Path,
            included: &mut Vec<IncludedFile>,
            visited: &mut HashSet<PathBuf>,
        ) {
            if visited.contains(path) {
                return;
            }
            visited.insert(path.to_path_buf());

            if let Ok(index) = ws.parse_index(path) {
                let relative_path =
                    pathdiff::diff_paths(path, root_dir).unwrap_or_else(|| path.to_path_buf());

                included.push(IncludedFile {
                    path: path.to_string_lossy().to_string(),
                    relative_path: relative_path.to_string_lossy().to_string(),
                });

                if index.frontmatter.is_index() {
                    for child_rel in index.frontmatter.contents_list() {
                        let child_path = index.resolve_path(child_rel);
                        if RealFileSystem.exists(&child_path) {
                            collect_all(ws, &child_path, root_dir, included, visited);
                        }
                    }
                }
            }
        }

        let mut visited = HashSet::new();
        collect_all(&ws, &root_index, root_dir, &mut included, &mut visited);

        return Ok(ExportPlanResult {
            included,
            excluded: vec![],
            audience: "*".to_string(),
        });
    }

    // Normal audience-filtered export
    let exporter = Exporter::new(RealFileSystem);
    let plan = exporter
        .plan_export(&root_index, &audience, Path::new("/export"))
        .map_err(|e| e.to_serializable())?;

    Ok(ExportPlanResult {
        included: plan
            .included
            .iter()
            .map(|f| IncludedFile {
                path: f.source_path.to_string_lossy().to_string(),
                relative_path: f.relative_path.to_string_lossy().to_string(),
            })
            .collect(),
        excluded: plan
            .excluded
            .iter()
            .map(|f| ExcludedFile {
                path: f.path.to_string_lossy().to_string(),
                reason: f.reason.to_string(),
            })
            .collect(),
        audience,
    })
}

/// Exported file with content
#[derive(Debug, Serialize)]
pub struct ExportedFileResult {
    pub path: String,
    pub content: String,
}

/// Export files to memory (as markdown strings)
#[tauri::command]
pub fn export_to_memory<R: Runtime>(
    app: AppHandle<R>,
    root_path: String,
    audience: String,
) -> Result<Vec<ExportedFileResult>, SerializableError> {
    let plan = plan_export(app, root_path, audience)?;

    let mut files = Vec::new();
    for included in plan.included {
        let path = PathBuf::from(&included.path);
        if let Ok(content) = std::fs::read_to_string(&path) {
            files.push(ExportedFileResult {
                path: included.relative_path,
                content,
            });
        }
    }

    Ok(files)
}

/// Export files as HTML
#[tauri::command]
pub fn export_to_html<R: Runtime>(
    app: AppHandle<R>,
    root_path: String,
    audience: String,
) -> Result<Vec<ExportedFileResult>, SerializableError> {
    let plan = plan_export(app, root_path, audience)?;

    let mut files = Vec::new();
    for included in plan.included {
        let path = PathBuf::from(&included.path);
        if let Ok(markdown) = std::fs::read_to_string(&path) {
            // Convert markdown to HTML using pulldown_cmark if available
            // For now, just return markdown as-is (HTML conversion can be added)
            files.push(ExportedFileResult {
                path: included.relative_path.replace(".md", ".html"),
                content: markdown, // TODO: Add markdown-to-HTML conversion
            });
        }
    }

    Ok(files)
}

/// Binary attachment data for export
#[derive(Debug, Serialize)]
pub struct BinaryExportResult {
    pub path: String,
    pub data: Vec<u8>,
}

/// Export binary attachments
#[tauri::command]
pub fn export_binary_attachments<R: Runtime>(
    app: AppHandle<R>,
    root_path: String,
    _audience: String,
) -> Result<Vec<BinaryExportResult>, SerializableError> {
    let paths = get_platform_paths(&app)?;
    let ws = Workspace::new(RealFileSystem);

    let root = if root_path.is_empty() {
        paths.default_workspace.clone()
    } else {
        PathBuf::from(&root_path)
    };

    // Determine if root is already a file (index) or a directory
    let root_index = if root.is_file() {
        root.clone()
    } else {
        ws.find_root_index_in_dir(&root)
            .map_err(|e| e.to_serializable())?
            .ok_or_else(|| SerializableError {
                kind: "WorkspaceNotFound".to_string(),
                message: format!("No workspace found at '{}'", root.display()),
                path: Some(root.clone()),
            })?
    };

    let root_dir = root_index.parent().unwrap_or(&root_index);
    let mut attachments = Vec::new();

    fn collect_attachments(
        ws: &Workspace<RealFileSystem>,
        path: &Path,
        root_dir: &Path,
        attachments: &mut Vec<BinaryExportResult>,
        visited: &mut HashSet<PathBuf>,
    ) {
        if visited.contains(path) {
            return;
        }
        visited.insert(path.to_path_buf());

        if let Ok(index) = ws.parse_index(path) {
            // Check for _attachments folder
            if let Some(entry_dir) = path.parent() {
                let attachments_dir = entry_dir.join("_attachments");
                if attachments_dir.is_dir()
                    && let Ok(entries) = std::fs::read_dir(&attachments_dir)
                {
                    for entry in entries.flatten() {
                        let entry_path = entry.path();
                        if entry_path.is_file()
                            && let Ok(data) = std::fs::read(&entry_path)
                        {
                            let relative_path = pathdiff::diff_paths(&entry_path, root_dir)
                                .unwrap_or_else(|| entry_path.clone());
                            attachments.push(BinaryExportResult {
                                path: relative_path.to_string_lossy().to_string(),
                                data,
                            });
                        }
                    }
                }
            }

            // Recurse into children
            if index.frontmatter.is_index() {
                for child_rel in index.frontmatter.contents_list() {
                    let child_path = index.resolve_path(child_rel);
                    if RealFileSystem.exists(&child_path) {
                        collect_attachments(ws, &child_path, root_dir, attachments, visited);
                    }
                }
            }
        }
    }

    let mut visited = HashSet::new();
    collect_attachments(&ws, &root_index, root_dir, &mut attachments, &mut visited);

    Ok(attachments)
}

// ============================================================================
// Backup Commands
// ============================================================================

/// Status of a backup operation
#[derive(Debug, Serialize)]
pub struct BackupStatus {
    pub target_name: String,
    pub success: bool,
    pub files_processed: usize,
    pub error: Option<String>,
}

/// Backup workspace to all configured targets
#[tauri::command]
pub fn backup_workspace<R: Runtime>(
    app: AppHandle<R>,
    workspace_path: Option<String>,
) -> Result<Vec<BackupStatus>, SerializableError> {
    use diaryx_core::backup::{BackupManager, LocalDriveTarget};

    let paths = get_platform_paths(&app)?;
    let workspace = workspace_path
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.default_workspace.clone());

    // Create backup manager with default local target
    let backup_dir = paths.data_dir.join("backups");
    let target = LocalDriveTarget::new("Local Backup", backup_dir);

    let mut manager = BackupManager::new();
    manager.add_target(Box::new(target));

    // Run backup
    let results = manager.backup_all(&RealFileSystem, &workspace);

    Ok(results
        .into_iter()
        .zip(manager.target_names())
        .map(|(result, name)| BackupStatus {
            target_name: name.to_string(),
            success: result.success,
            files_processed: result.files_processed,
            error: result.error,
        })
        .collect())
}

/// Restore workspace from a backup target
#[tauri::command]
pub fn restore_workspace<R: Runtime>(
    app: AppHandle<R>,
    workspace_path: Option<String>,
    _target_name: Option<String>, // For future use with multiple targets
) -> Result<BackupStatus, SerializableError> {
    use diaryx_core::backup::{BackupManager, LocalDriveTarget};

    let paths = get_platform_paths(&app)?;
    let workspace = workspace_path
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.default_workspace.clone());

    // Create backup manager with default local target
    let backup_dir = paths.data_dir.join("backups");
    let target = LocalDriveTarget::new("Local Backup", backup_dir);

    let mut manager = BackupManager::new();
    manager.add_target(Box::new(target));

    // Restore from primary
    match manager.restore_from_primary(&RealFileSystem, &workspace) {
        Some(result) => Ok(BackupStatus {
            target_name: manager.primary_name().unwrap_or("Unknown").to_string(),
            success: result.success,
            files_processed: result.files_processed,
            error: result.error,
        }),
        None => Err(SerializableError {
            kind: "NoBackupTarget".to_string(),
            message: "No backup target configured".to_string(),
            path: None,
        }),
    }
}

/// List available backup targets
#[tauri::command]
pub fn list_backup_targets<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<String>, SerializableError> {
    use diaryx_core::backup::{BackupManager, LocalDriveTarget};

    let paths = get_platform_paths(&app)?;
    let backup_dir = paths.data_dir.join("backups");
    let target = LocalDriveTarget::new("Local Backup", backup_dir);

    let mut manager = BackupManager::new();
    manager.add_target(Box::new(target));

    Ok(manager.target_names().into_iter().map(String::from).collect())
}
