//!
//! Tauri IPC command handlers
//!
//! These commands are callable from the frontend via Tauri's invoke system.
//!
//! All workspace operations go through the unified `execute()` command,
//! which routes to the appropriate handler in diaryx_core.
//!
//! Platform-specific commands (backup, cloud sync, import) are handled
//! separately as they require Tauri plugins or system APIs.

use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use diaryx_core::{
    Command,
    config::Config,
    crdt::{CrdtStorage, SqliteStorage},
    diaryx::Diaryx,
    error::SerializableError,
    fs::{FileSystem, InMemoryFileSystem, RealFileSystem, SyncToAsyncFs},
    workspace::Workspace,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};

// ============================================================================
// Types
// ============================================================================

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
    /// Whether CRDT storage was successfully initialized
    pub crdt_initialized: bool,
    /// Error message if CRDT initialization failed
    pub crdt_error: Option<String>,
}

/// Google Auth configuration
#[derive(Debug, Serialize)]
pub struct GoogleAuthConfig {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

/// Global state for CRDT-enabled Diaryx instances.
///
/// This stores the SQLite storage backend shared across all execute() calls,
/// allowing CRDT state to persist across commands.
///
/// The `diaryx` field caches a fully-initialized Diaryx instance, avoiding
/// expensive CRDT state reload from SQLite on every command. This is critical
/// for performance - without caching, each command would take seconds to load.
pub struct CrdtState {
    /// Path to the active workspace
    pub workspace_path: Mutex<Option<PathBuf>>,
    /// CRDT storage backend (shared across calls)
    /// Note: CrdtStorage trait already requires Send + Sync
    pub storage: Mutex<Option<Arc<dyn CrdtStorage>>>,
    /// Cached Diaryx instance with CRDT support.
    /// Wrapped in Arc to allow sharing the same instance across command invocations.
    pub diaryx: Mutex<Option<Arc<Diaryx<SyncToAsyncFs<RealFileSystem>>>>>,
}

impl CrdtState {
    pub fn new() -> Self {
        Self {
            workspace_path: Mutex::new(None),
            storage: Mutex::new(None),
            diaryx: Mutex::new(None),
        }
    }
}

impl Default for CrdtState {
    fn default() -> Self {
        Self::new()
    }
}

/// State for guest mode - holds an in-memory filesystem when active.
///
/// When a Tauri user joins a share session as a guest, this state holds
/// the in-memory filesystem that all file operations are routed through.
/// This prevents guest session files from affecting the user's local workspace.
pub struct GuestModeState {
    /// Whether guest mode is currently active
    pub active: Mutex<bool>,
    /// The in-memory filesystem used during guest mode
    pub filesystem: Mutex<Option<InMemoryFileSystem>>,
    /// The join code of the current guest session
    pub join_code: Mutex<Option<String>>,
}

impl GuestModeState {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(false),
            filesystem: Mutex::new(None),
            join_code: Mutex::new(None),
        }
    }
}

impl Default for GuestModeState {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to safely acquire a mutex lock without panicking.
///
/// Returns a SerializableError if the mutex is poisoned, instead of panicking.
fn acquire_lock<T>(mutex: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, SerializableError> {
    mutex.lock().map_err(|e| SerializableError {
        kind: "LockError".to_string(),
        message: format!("Failed to acquire lock: mutex is poisoned - {}", e),
        path: None,
    })
}

// ============================================================================
// Unified Command API
// ============================================================================

/// Execute a command using the unified command pattern.
///
/// This is the primary API for all diaryx operations, replacing the many
/// individual commands with a single entry point.
///
/// ## Example from TypeScript:
/// ```typescript
/// const command = { type: 'GetEntry', params: { path: 'workspace/notes.md' } };
/// const response = await invoke('execute', { commandJson: JSON.stringify(command) });
/// const result = JSON.parse(response);
/// ```
#[tauri::command]
pub async fn execute<R: Runtime>(
    app: AppHandle<R>,
    command_json: String,
) -> Result<String, SerializableError> {
    log::trace!("[execute] Received command");
    log::trace!("[execute] Command JSON: {}", command_json);

    // Parse the command from JSON
    let cmd: Command = serde_json::from_str(&command_json).map_err(|e| {
        log::error!("[execute] Failed to parse command: {}", e);
        SerializableError {
            kind: "ParseError".to_string(),
            message: format!("Failed to parse command JSON: {}", e),
            path: None,
        }
    })?;

    log::trace!(
        "[execute] Parsed command type: {:?}",
        std::mem::discriminant(&cmd)
    );

    // Check if we're in guest mode and get the appropriate filesystem
    // We need to extract data from mutex guards before any async points
    let guest_state = app.state::<GuestModeState>();
    let is_guest = *acquire_lock(&guest_state.active)?;

    // Execute command using appropriate filesystem
    let response = if is_guest {
        // Guest mode: use in-memory filesystem (no CRDT storage for guests)
        // Clone the filesystem and release the guard before await
        let mem_fs = {
            let fs_guard = acquire_lock(&guest_state.filesystem)?;
            fs_guard
                .as_ref()
                .cloned()
                .ok_or_else(|| SerializableError {
                    kind: "GuestModeError".to_string(),
                    message: "Guest mode active but no filesystem initialized".to_string(),
                    path: None,
                })?
        };
        log::debug!("[execute] Using in-memory filesystem (guest mode)");
        let diaryx = Diaryx::new(SyncToAsyncFs::new(mem_fs));
        diaryx.execute(cmd).await.map_err(|e| {
            log::error!("[execute] Command execution failed: {:?}", e);
            e.to_serializable()
        })?
    } else {
        // Normal mode: use real filesystem with optional CRDT support
        // Try to use cached Diaryx instance for performance
        let crdt_state = app.state::<CrdtState>();

        // First, try to get cached diaryx (fast path)
        let cached_diaryx = {
            let diaryx_guard = acquire_lock(&crdt_state.diaryx)?;
            diaryx_guard.as_ref().map(Arc::clone)
        };

        let diaryx = if let Some(cached) = cached_diaryx {
            log::trace!("[execute] Using cached Diaryx instance");
            cached
        } else {
            // No cached instance - need to create one (slow path, only happens once)
            log::debug!("[execute] No cached Diaryx, creating new instance");
            let (storage, workspace_path) = {
                let storage_guard = acquire_lock(&crdt_state.storage)?;
                let ws_guard = acquire_lock(&crdt_state.workspace_path)?;
                (storage_guard.as_ref().map(Arc::clone), ws_guard.clone())
            };

            let new_diaryx = if let Some(storage) = storage {
                match Diaryx::with_crdt_load(SyncToAsyncFs::new(RealFileSystem), storage) {
                    Ok(d) => {
                        log::debug!("[execute] Created Diaryx with CRDT support");
                        // Set workspace root for sync handler to write files to correct location
                        if let Some(ref ws_path) = workspace_path {
                            log::debug!("[execute] Setting workspace root: {:?}", ws_path);
                            d.set_workspace_root(ws_path.clone());
                        }
                        Arc::new(d)
                    }
                    Err(e) => {
                        log::warn!(
                            "[execute] Failed to load CRDT state: {:?}, using without CRDT",
                            e
                        );
                        Arc::new(Diaryx::new(SyncToAsyncFs::new(RealFileSystem)))
                    }
                }
            } else {
                log::debug!("[execute] No CRDT storage configured, using basic Diaryx");
                Arc::new(Diaryx::new(SyncToAsyncFs::new(RealFileSystem)))
            };

            // Cache the new instance for future commands
            {
                let mut diaryx_guard = acquire_lock(&crdt_state.diaryx)?;
                *diaryx_guard = Some(Arc::clone(&new_diaryx));
                log::debug!("[execute] Cached Diaryx instance for future commands");
            }

            new_diaryx
        };

        diaryx.execute(cmd).await.map_err(|e| {
            log::error!("[execute] Command execution failed: {:?}", e);
            e.to_serializable()
        })?
    };

    // Serialize the response to JSON
    let response_json = serde_json::to_string(&response).map_err(|e| {
        log::error!("[execute] Failed to serialize response: {}", e);
        SerializableError {
            kind: "SerializeError".to_string(),
            message: format!("Failed to serialize response: {}", e),
            path: None,
        }
    })?;

    log::trace!("[execute] Command executed successfully");
    Ok(response_json)
}

// ============================================================================
// Platform Path Resolution
// ============================================================================

/// Get platform-appropriate paths for the app
/// On iOS/Android, uses Tauri's app_data_dir which is within the app sandbox
/// On desktop, uses the standard dirs crate locations
fn get_platform_paths<R: Runtime>(app: &AppHandle<R>) -> Result<AppPaths, SerializableError> {
    let path_resolver = app.path();

    // Check if we're on mobile (iOS or Android)
    let is_mobile = cfg!(target_os = "ios") || cfg!(target_os = "android");

    if is_mobile {
        // On mobile, use document_dir for user files so they appear in Files app
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
            crdt_initialized: false,
            crdt_error: None,
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
            crdt_initialized: false,
            crdt_error: None,
        })
    }
}

/// Get the app paths for the current platform
#[tauri::command]
pub fn get_app_paths<R: Runtime>(app: AppHandle<R>) -> Result<AppPaths, SerializableError> {
    get_platform_paths(&app)
}

/// Get Google Auth configuration from environment variables
///
/// Checks compile-time env vars first (for release builds), then runtime env vars.
#[tauri::command]
pub fn get_google_auth_config() -> GoogleAuthConfig {
    // First try compile-time environment variables (embedded in binary)
    let client_id = option_env!("GOOGLE_CLIENT_ID").map(String::from);
    let client_secret = option_env!("GOOGLE_CLIENT_SECRET").map(String::from);

    // If not found at compile time, try runtime environment variables
    let client_id = client_id.or_else(|| {
        let _ = dotenvy::dotenv(); // Load .env file if it exists
        std::env::var("GOOGLE_CLIENT_ID").ok()
    });
    let client_secret = client_secret.or_else(|| std::env::var("GOOGLE_CLIENT_SECRET").ok());

    GoogleAuthConfig {
        client_id,
        client_secret,
    }
}

/// Pick a folder using native dialog and set it as workspace
#[tauri::command]
pub async fn pick_workspace_folder<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<AppPaths>, SerializableError> {
    // Folder picking is not supported on iOS
    #[cfg(target_os = "ios")]
    {
        return Err(SerializableError {
            kind: "UnsupportedPlatform".to_string(),
            message: "Folder picking is not supported on iOS".to_string(),
            path: None,
        });
    }

    #[cfg(not(target_os = "ios"))]
    {
        use tauri_plugin_dialog::DialogExt;

        let paths = get_platform_paths(&app)?;

        // Use folder picker
        let folder_path = app
            .dialog()
            .file()
            .set_title("Select Workspace Folder")
            .blocking_pick_folder();

        let selected_path = match folder_path {
            Some(path) => path.into_path().map_err(|e| SerializableError {
                kind: "PathError".to_string(),
                message: format!("Failed to get folder path: {:?}", e),
                path: None,
            })?,
            None => {
                // User cancelled
                return Ok(None);
            }
        };

        log::info!(
            "[pick_workspace_folder] User selected folder: {:?}",
            selected_path
        );

        let fs = SyncToAsyncFs::new(RealFileSystem);

        // Load existing config or create new one
        let mut config = if paths.config_path.exists() {
            Config::load_from(&fs, &paths.config_path)
                .await
                .unwrap_or_else(|_| Config::new(paths.default_workspace.clone()))
        } else {
            Config::new(paths.default_workspace.clone())
        };

        // Update workspace path
        config.default_workspace = selected_path.clone();

        // Save config
        config
            .save_to(&fs, &paths.config_path)
            .await
            .map_err(|e| e.to_serializable())?;

        // Initialize workspace if it doesn't exist
        let ws = Workspace::new(SyncToAsyncFs::new(RealFileSystem));
        let workspace_initialized = match ws.find_root_index_in_dir(&selected_path).await {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(_) => false,
        };

        if !workspace_initialized {
            log::info!(
                "[pick_workspace_folder] Initializing workspace at {:?}",
                selected_path
            );
            ws.init_workspace(&selected_path, Some("My Workspace"), None)
                .await
                .map_err(|e| e.to_serializable())?;
        }

        // Return updated paths (CRDT not initialized for new workspace yet)
        Ok(Some(AppPaths {
            data_dir: paths.data_dir,
            document_dir: paths.document_dir,
            default_workspace: selected_path,
            config_path: paths.config_path,
            is_mobile: paths.is_mobile,
            crdt_initialized: false,
            crdt_error: None,
        }))
    }
}

/// Initialize the app - creates necessary directories and default workspace if needed
#[tauri::command]
pub async fn initialize_app<R: Runtime>(app: AppHandle<R>) -> Result<AppPaths, SerializableError> {
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

    // Load or create config file FIRST to get the actual workspace path
    log::info!("[initialize_app] Loading/creating config...");
    let config = if paths.is_mobile {
        // On mobile, use the platform-specific Documents/Diaryx path
        log::info!(
            "[initialize_app] Mobile: using platform workspace path: {:?}",
            paths.default_workspace
        );
        Config::new(paths.default_workspace.clone())
    } else if paths.config_path.exists() {
        log::info!(
            "[initialize_app] Loading existing config from {:?}",
            paths.config_path
        );
        Config::load_from(&SyncToAsyncFs::new(RealFileSystem), &paths.config_path)
            .await
            .unwrap_or_else(|e| {
                log::warn!(
                    "[initialize_app] Failed to load config, creating new: {:?}",
                    e
                );
                Config::new(paths.default_workspace.clone())
            })
    } else {
        log::info!("[initialize_app] Creating new config with default workspace");
        let new_config = Config::new(paths.default_workspace.clone());
        // Save the new config
        new_config
            .save_to(&SyncToAsyncFs::new(RealFileSystem), &paths.config_path)
            .await
            .map_err(|e| {
                log::error!("[initialize_app] Failed to save config: {:?}", e);
                e.to_serializable()
            })?;
        new_config
    };

    // Use the workspace path from config (may differ from platform default)
    let actual_workspace = config.default_workspace.clone();
    log::info!(
        "[initialize_app] Using workspace from config: {:?}",
        actual_workspace
    );

    // Make sure the workspace directory exists
    if !actual_workspace.exists() {
        log::info!(
            "[initialize_app] Creating workspace directory: {:?}",
            actual_workspace
        );
        std::fs::create_dir_all(&actual_workspace).map_err(|e| {
            log::error!(
                "[initialize_app] Failed to create workspace directory: {}",
                e
            );
            SerializableError {
                kind: "IoError".to_string(),
                message: format!("Failed to create workspace directory: {}", e),
                path: Some(actual_workspace.clone()),
            }
        })?;
    }

    // Check if workspace needs initialization (has a root index file)
    log::info!("[initialize_app] Checking if workspace is initialized...");
    let ws = Workspace::new(SyncToAsyncFs::new(RealFileSystem));
    let workspace_has_root = match ws.find_root_index_in_dir(&actual_workspace).await {
        Ok(Some(path)) => {
            log::info!("[initialize_app] Found root index at: {:?}", path);
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

    if !workspace_has_root {
        log::info!(
            "[initialize_app] Initializing workspace at {:?}",
            actual_workspace
        );
        ws.init_workspace(&actual_workspace, Some("My Workspace"), None)
            .await
            .map_err(|e| {
                log::error!("[initialize_app] Failed to initialize workspace: {:?}", e);
                e.to_serializable()
            })?;
        log::info!("[initialize_app] Workspace initialized successfully");
    }

    log::info!("[initialize_app] Initialization complete!");

    // Initialize CRDT storage for the workspace
    let (crdt_initialized, crdt_error) = {
        let crdt_state = app.state::<CrdtState>();
        let crdt_dir = actual_workspace.join(".diaryx");

        match std::fs::create_dir_all(&crdt_dir) {
            Err(e) => {
                let error_msg = format!("Failed to create CRDT directory {:?}: {}", crdt_dir, e);
                log::warn!("[initialize_app] {}", error_msg);
                (false, Some(error_msg))
            }
            Ok(_) => {
                let db_path = crdt_dir.join("crdt.db");
                match SqliteStorage::open(&db_path) {
                    Ok(storage) => {
                        match (
                            acquire_lock(&crdt_state.workspace_path),
                            acquire_lock(&crdt_state.storage),
                        ) {
                            (Ok(mut ws_lock), Ok(mut storage_lock)) => {
                                *ws_lock = Some(actual_workspace.clone());
                                *storage_lock = Some(Arc::new(storage));
                                log::info!(
                                    "[initialize_app] CRDT storage initialized at {:?}",
                                    db_path
                                );
                                (true, None)
                            }
                            (Err(e), _) | (_, Err(e)) => {
                                let error_msg =
                                    format!("Failed to acquire CRDT state lock: {:?}", e);
                                log::error!("[initialize_app] {}", error_msg);
                                (false, Some(error_msg))
                            }
                        }
                    }
                    Err(e) => {
                        let error_msg = format!(
                            "Failed to initialize CRDT storage at {:?}: {:?}",
                            db_path, e
                        );
                        log::warn!("[initialize_app] {}", error_msg);
                        (false, Some(error_msg))
                    }
                }
            }
        }
    };

    // Return paths with the actual workspace from config and CRDT status
    Ok(AppPaths {
        data_dir: paths.data_dir,
        document_dir: paths.document_dir,
        default_workspace: actual_workspace,
        config_path: paths.config_path,
        is_mobile: paths.is_mobile,
        crdt_initialized,
        crdt_error,
    })
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

/// Progress event for backup operations (emitted via Tauri events)
#[derive(Debug, Clone, Serialize)]
pub struct BackupProgressEvent {
    /// Current stage: "preparing", "zipping", "uploading", "complete", "error"
    pub stage: String,
    /// Progress percentage (0-100)
    pub percent: u8,
    /// Optional message for additional context
    pub message: Option<String>,
}

/// Progress event for sync operations (emitted via Tauri events)
#[derive(Debug, Clone, Serialize)]
pub struct SyncProgressEvent {
    /// Current stage: "detecting_local", "detecting_remote", "uploading", "downloading", "deleting", "complete", "error"
    pub stage: String,
    /// Current item being processed
    pub current: usize,
    /// Total items in this stage
    pub total: usize,
    /// Progress percentage (0-100)
    pub percent: u8,
    /// Optional message for additional context
    pub message: Option<String>,
}

/// Backup workspace to all configured targets
#[tauri::command]
pub async fn backup_workspace<R: Runtime>(
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
    let results = manager
        .backup_all(&SyncToAsyncFs::new(RealFileSystem), &workspace)
        .await;

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
pub async fn restore_workspace<R: Runtime>(
    app: AppHandle<R>,
    workspace_path: Option<String>,
    _target_name: Option<String>,
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
    match manager
        .restore_from_primary(&SyncToAsyncFs::new(RealFileSystem), &workspace)
        .await
    {
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

    Ok(manager
        .target_names()
        .into_iter()
        .map(String::from)
        .collect())
}

// ============================================================================
// Cloud Backup Commands (S3)
// ============================================================================

/// S3 configuration request
#[derive(Debug, Deserialize)]
pub struct S3ConfigRequest {
    pub name: String,
    pub bucket: String,
    pub region: String,
    pub prefix: Option<String>,
    pub endpoint: Option<String>,
    pub access_key: String,
    pub secret_key: String,
}

/// Test S3 connection
#[tauri::command]
pub fn test_s3_connection(config: S3ConfigRequest) -> Result<bool, SerializableError> {
    use crate::cloud::S3Target;
    use diaryx_core::backup::{BackupTarget, CloudBackupConfig, CloudProvider};

    let cloud_config = CloudBackupConfig {
        id: "test".to_string(),
        name: config.name,
        provider: CloudProvider::S3 {
            bucket: config.bucket,
            region: config.region,
            prefix: config.prefix,
            endpoint: config.endpoint,
        },
        enabled: true,
    };

    let target = S3Target::new_blocking(cloud_config, config.access_key, config.secret_key)
        .map_err(|e| SerializableError {
            kind: "S3ConfigError".to_string(),
            message: e,
            path: None,
        })?;

    Ok(target.is_available())
}

/// Backup workspace to S3
#[tauri::command]
pub async fn backup_to_s3<R: Runtime>(
    app: AppHandle<R>,
    workspace_path: Option<String>,
    config: S3ConfigRequest,
) -> Result<BackupStatus, SerializableError> {
    use crate::cloud::S3Target;
    use diaryx_core::backup::{CloudBackupConfig, CloudProvider};
    use tokio::sync::mpsc;

    let paths = get_platform_paths(&app)?;
    let workspace = workspace_path
        .map(PathBuf::from)
        .unwrap_or(paths.default_workspace);

    let config_name = config.name.clone();

    // Create a channel to send progress from the background thread
    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel::<BackupProgressEvent>();

    // Clone app for the event emission task
    let app_clone = app.clone();

    // Spawn a task to forward progress events to the frontend
    let event_task = tauri::async_runtime::spawn(async move {
        while let Some(event) = progress_rx.recv().await {
            let _ = app_clone.emit("backup_progress", &event);
        }
    });

    // Run backup in a blocking thread to not freeze the UI
    let result = tauri::async_runtime::spawn_blocking(move || {
        let fs = RealFileSystem;

        // Emit preparing stage
        let _ = progress_tx.send(BackupProgressEvent {
            stage: "preparing".to_string(),
            percent: 5,
            message: Some("Preparing backup...".to_string()),
        });

        let cloud_config = CloudBackupConfig {
            id: uuid::Uuid::new_v4().to_string(),
            name: config.name.clone(),
            provider: CloudProvider::S3 {
                bucket: config.bucket,
                region: config.region,
                prefix: config.prefix,
                endpoint: config.endpoint,
            },
            enabled: true,
        };

        let target = S3Target::new_blocking(cloud_config, config.access_key, config.secret_key)
            .map_err(|e| SerializableError {
                kind: "S3ConfigError".to_string(),
                message: e,
                path: None,
            })?;

        // Use backup_with_progress to get per-file progress callbacks
        let result =
            target.backup_with_progress(&fs, &workspace, |stage, current, total, percent| {
                let message = match stage {
                    "preparing" => Some("Preparing backup...".to_string()),
                    "zipping" => Some(format!("Zipping files: {}/{}", current, total)),
                    "uploading" => Some("Uploading to S3...".to_string()),
                    "complete" => Some("Backup complete!".to_string()),
                    "error" => Some("Backup failed".to_string()),
                    _ => None,
                };

                let _ = progress_tx.send(BackupProgressEvent {
                    stage: stage.to_string(),
                    percent,
                    message,
                });
            });

        Ok::<_, SerializableError>(result)
    })
    .await
    .map_err(|e| SerializableError {
        kind: "SpawnError".to_string(),
        message: e.to_string(),
        path: None,
    })??;

    // Wait for event task to finish
    let _ = event_task.await;

    Ok(BackupStatus {
        target_name: config_name,
        success: result.success,
        files_processed: result.files_processed,
        error: result.error,
    })
}

/// Restore workspace from S3
#[tauri::command]
pub async fn restore_from_s3<R: Runtime>(
    app: AppHandle<R>,
    workspace_path: Option<String>,
    config: S3ConfigRequest,
) -> Result<BackupStatus, SerializableError> {
    use crate::cloud::S3Target;
    use diaryx_core::backup::{BackupTarget, CloudBackupConfig, CloudProvider};

    let paths = get_platform_paths(&app)?;
    let workspace = workspace_path
        .map(PathBuf::from)
        .unwrap_or(paths.default_workspace);
    let fs = SyncToAsyncFs::new(RealFileSystem);

    let cloud_config = CloudBackupConfig {
        id: "restore".to_string(),
        name: config.name.clone(),
        provider: CloudProvider::S3 {
            bucket: config.bucket,
            region: config.region,
            prefix: config.prefix,
            endpoint: config.endpoint,
        },
        enabled: true,
    };

    let target = S3Target::new(cloud_config, config.access_key, config.secret_key)
        .await
        .map_err(|e| SerializableError {
            kind: "S3ConfigError".to_string(),
            message: e,
            path: None,
        })?;

    let result = target.restore(&fs, &workspace).await;

    Ok(BackupStatus {
        target_name: config.name,
        success: result.success,
        files_processed: result.files_processed,
        error: result.error,
    })
}

// ============================================================================
// Cloud Backup Commands (Google Drive)
// ============================================================================

/// Google Drive configuration request from frontend
#[derive(Debug, Deserialize)]
pub struct GoogleDriveConfigRequest {
    pub name: String,
    pub access_token: String,
    pub folder_id: Option<String>,
}

/// Backup workspace to Google Drive
#[tauri::command]
pub async fn backup_to_google_drive<R: Runtime>(
    app: AppHandle<R>,
    workspace_path: Option<String>,
    config: GoogleDriveConfigRequest,
) -> Result<BackupStatus, SerializableError> {
    use crate::cloud::GoogleDriveTarget;
    use diaryx_core::backup::{CloudBackupConfig, CloudProvider};
    use tokio::sync::mpsc;

    let paths = get_platform_paths(&app)?;
    let workspace = workspace_path
        .map(PathBuf::from)
        .unwrap_or(paths.default_workspace);

    let config_name = config.name.clone();
    let access_token = config.access_token.clone();
    let folder_id = config.folder_id.clone();

    // Create a channel to send progress from the background thread
    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel::<BackupProgressEvent>();

    // Clone app for the event emission task
    let app_clone = app.clone();

    // Spawn a task to forward progress events to the frontend
    let event_task = tauri::async_runtime::spawn(async move {
        while let Some(event) = progress_rx.recv().await {
            let _ = app_clone.emit("backup_progress", &event);
        }
    });

    // Run backup in a blocking thread
    let result = tauri::async_runtime::spawn_blocking(move || {
        let fs = RealFileSystem;

        // Emit preparing stage
        let _ = progress_tx.send(BackupProgressEvent {
            stage: "preparing".to_string(),
            percent: 5,
            message: Some("Preparing backup...".to_string()),
        });

        let cloud_config = CloudBackupConfig {
            id: uuid::Uuid::new_v4().to_string(),
            name: config.name.clone(),
            provider: CloudProvider::GoogleDrive {
                folder_id: folder_id.clone(),
            },
            enabled: true,
        };

        let target =
            GoogleDriveTarget::new(cloud_config, access_token, folder_id).map_err(|e| {
                SerializableError {
                    kind: "GoogleDriveConfigError".to_string(),
                    message: e,
                    path: None,
                }
            })?;

        // Use backup_with_progress for progress callbacks
        let result =
            target.backup_with_progress(&fs, &workspace, |stage, current, total, percent| {
                let message = match stage {
                    "preparing" => Some("Preparing backup...".to_string()),
                    "zipping" => Some(format!("Zipping files: {}/{}", current, total)),
                    "uploading" => Some("Uploading to Google Drive...".to_string()),
                    "complete" => Some("Backup complete!".to_string()),
                    "error" => Some("Backup failed".to_string()),
                    _ => None,
                };

                let _ = progress_tx.send(BackupProgressEvent {
                    stage: stage.to_string(),
                    percent,
                    message,
                });
            });

        Ok::<_, SerializableError>(result)
    })
    .await
    .map_err(|e| SerializableError {
        kind: "SpawnError".to_string(),
        message: e.to_string(),
        path: None,
    })??;

    // Wait for event task to finish
    let _ = event_task.await;

    Ok(BackupStatus {
        target_name: config_name,
        success: result.success,
        files_processed: result.files_processed,
        error: result.error,
    })
}

// ============================================================================
// Import Commands
// ============================================================================

/// Result of an import operation
#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub success: bool,
    pub files_imported: usize,
    pub files_skipped: usize,
    pub workspace_path: String,
    pub error: Option<String>,
    /// True if user cancelled the file picker
    pub cancelled: bool,
}

/// Import workspace from a backup zip file
#[tauri::command]
pub async fn import_from_zip(
    zip_path: String,
    workspace_path: Option<String>,
) -> Result<ImportResult, SerializableError> {
    use std::io::Read;

    let fs = RealFileSystem;

    // Get workspace path
    let workspace = match workspace_path {
        Some(p) => PathBuf::from(p),
        None => {
            let config = Config::default();
            if config.default_workspace.as_os_str().is_empty() {
                return Err(SerializableError {
                    kind: "ImportError".to_string(),
                    message: "No workspace specified and no default workspace configured"
                        .to_string(),
                    path: None,
                });
            }
            config.default_workspace
        }
    };

    log::info!("[Import] Importing from {} to {:?}", zip_path, workspace);

    // Open zip file
    let zip_file = std::fs::File::open(&zip_path).map_err(|e| SerializableError {
        kind: "ImportError".to_string(),
        message: format!("Failed to open zip file: {}", e),
        path: Some(PathBuf::from(&zip_path)),
    })?;

    let mut archive = zip::ZipArchive::new(zip_file).map_err(|e| SerializableError {
        kind: "ImportError".to_string(),
        message: format!("Failed to read zip archive: {}", e),
        path: Some(PathBuf::from(&zip_path)),
    })?;

    let total_files = archive.len();
    let mut files_imported = 0;
    let files_skipped = 0;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| SerializableError {
            kind: "ImportError".to_string(),
            message: format!("Failed to read zip entry: {}", e),
            path: None,
        })?;

        if file.is_dir() {
            continue;
        }

        let file_name = file.name().to_string();

        // Skip system files
        let should_skip = file_name
            .split('/')
            .any(|part| part.starts_with('.') || part == "Thumbs.db" || part == "desktop.ini");

        if should_skip {
            continue;
        }

        // Only import markdown files and attachments
        let is_markdown = file_name.ends_with(".md");
        let is_in_attachments =
            file_name.contains("/attachments/") || file_name.contains("/assets/");
        let is_common_attachment = {
            let lower = file_name.to_lowercase();
            lower.ends_with(".png")
                || lower.ends_with(".jpg")
                || lower.ends_with(".jpeg")
                || lower.ends_with(".gif")
                || lower.ends_with(".svg")
                || lower.ends_with(".pdf")
                || lower.ends_with(".webp")
                || lower.ends_with(".heic")
                || lower.ends_with(".heif")
        };

        if !is_markdown && !is_in_attachments && !is_common_attachment {
            continue;
        }

        let file_path = workspace.join(&file_name);

        // Create parent directories
        if let Some(parent) = file_path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent).map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to create directory: {}", e),
                path: Some(parent.to_path_buf()),
            })?;
        }

        // Read and write file
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to read file from zip: {}", e),
                path: Some(file_path.clone()),
            })?;

        fs.write_binary(&file_path, &contents)
            .map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to write file: {}", e),
                path: Some(file_path.clone()),
            })?;

        files_imported += 1;

        if files_imported % 100 == 0 {
            log::info!(
                "[Import] Progress: {}/{} files",
                files_imported,
                total_files
            );
        }
    }

    log::info!(
        "[Import] Complete: {} files imported, {} skipped",
        files_imported,
        files_skipped
    );

    Ok(ImportResult {
        success: true,
        files_imported,
        files_skipped,
        workspace_path: workspace.to_string_lossy().to_string(),
        error: None,
        cancelled: false,
    })
}

/// Pick a zip file using native dialog and import it
#[tauri::command]
pub async fn pick_and_import_zip<R: Runtime>(
    app: AppHandle<R>,
    workspace_path: Option<String>,
) -> Result<ImportResult, SerializableError> {
    use std::io::Read;
    use tauri_plugin_dialog::DialogExt;

    let fs = RealFileSystem;

    // Get workspace path
    let workspace = match workspace_path {
        Some(p) => PathBuf::from(p),
        None => {
            let config = Config::default();
            if config.default_workspace.as_os_str().is_empty() {
                return Err(SerializableError {
                    kind: "ImportError".to_string(),
                    message: "No workspace specified".to_string(),
                    path: None,
                });
            }
            config.default_workspace
        }
    };

    // Use file picker
    let file_path = app
        .dialog()
        .file()
        .add_filter("Zip Archive", &["zip", "application/zip"])
        .set_title("Select Backup Zip to Import")
        .blocking_pick_file();

    let selected_path = match file_path {
        Some(path) => path.into_path().map_err(|e| SerializableError {
            kind: "ImportError".to_string(),
            message: format!("Failed to get file path: {:?}", e),
            path: None,
        })?,
        None => {
            // User cancelled
            return Ok(ImportResult {
                success: false,
                files_imported: 0,
                files_skipped: 0,
                workspace_path: workspace.to_string_lossy().to_string(),
                error: None,
                cancelled: true,
            });
        }
    };

    log::info!(
        "[Import] Importing from {:?} to {:?}",
        selected_path,
        workspace
    );

    // Open and process zip file
    let zip_file = std::fs::File::open(&selected_path).map_err(|e| SerializableError {
        kind: "ImportError".to_string(),
        message: format!("Failed to open zip file: {}", e),
        path: Some(selected_path.clone()),
    })?;

    let mut archive = zip::ZipArchive::new(zip_file).map_err(|e| SerializableError {
        kind: "ImportError".to_string(),
        message: format!("Failed to read zip archive: {}", e),
        path: Some(selected_path.clone()),
    })?;

    let total_files = archive.len();
    let mut files_imported = 0;
    let files_skipped = 0;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| SerializableError {
            kind: "ImportError".to_string(),
            message: format!("Failed to read zip entry: {}", e),
            path: None,
        })?;

        if file.is_dir() {
            continue;
        }

        let file_name = file.name().to_string();

        // Skip system files
        let should_skip = file_name
            .split('/')
            .any(|part| part.starts_with('.') || part == "Thumbs.db" || part == "desktop.ini");

        if should_skip {
            continue;
        }

        // Only import markdown and attachments
        let is_markdown = file_name.ends_with(".md");
        let is_in_attachments =
            file_name.contains("/attachments/") || file_name.contains("/assets/");
        let is_common_attachment = {
            let lower = file_name.to_lowercase();
            lower.ends_with(".png")
                || lower.ends_with(".jpg")
                || lower.ends_with(".jpeg")
                || lower.ends_with(".gif")
                || lower.ends_with(".svg")
                || lower.ends_with(".pdf")
                || lower.ends_with(".webp")
        };

        if !is_markdown && !is_in_attachments && !is_common_attachment {
            continue;
        }

        let file_path = workspace.join(&file_name);

        // Create parent directories
        if let Some(parent) = file_path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent).map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to create directory: {}", e),
                path: Some(parent.to_path_buf()),
            })?;
        }

        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to read file from zip: {}", e),
                path: Some(file_path.clone()),
            })?;

        fs.write_binary(&file_path, &contents)
            .map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to write file: {}", e),
                path: Some(file_path.clone()),
            })?;

        files_imported += 1;

        if files_imported % 100 == 0 {
            log::info!(
                "[Import] Progress: {}/{} files",
                files_imported,
                total_files
            );
        }
    }

    log::info!(
        "[Import] Complete: {} files imported, {} skipped",
        files_imported,
        files_skipped
    );

    Ok(ImportResult {
        success: true,
        files_imported,
        files_skipped,
        workspace_path: workspace.to_string_lossy().to_string(),
        error: None,
        cancelled: false,
    })
}

/// Import workspace from base64-encoded zip data
#[tauri::command]
pub async fn import_from_zip_data(
    zip_data: String,
    workspace_path: Option<String>,
) -> Result<ImportResult, SerializableError> {
    use base64::Engine;
    use std::io::{Cursor, Read};

    let fs = RealFileSystem;

    // Get workspace path
    let workspace = match workspace_path {
        Some(p) => PathBuf::from(p),
        None => {
            let config = Config::default();
            if config.default_workspace.as_os_str().is_empty() {
                return Err(SerializableError {
                    kind: "ImportError".to_string(),
                    message: "No workspace specified".to_string(),
                    path: None,
                });
            }
            config.default_workspace
        }
    };

    log::info!(
        "[Import] Importing from base64 data ({} chars) to {:?}",
        zip_data.len(),
        workspace
    );

    // Decode base64
    let zip_bytes = base64::engine::general_purpose::STANDARD
        .decode(&zip_data)
        .map_err(|e| SerializableError {
            kind: "ImportError".to_string(),
            message: format!("Failed to decode base64: {}", e),
            path: None,
        })?;

    log::info!("[Import] Decoded {} bytes of zip data", zip_bytes.len());

    // Create zip archive from bytes
    let cursor = Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| SerializableError {
        kind: "ImportError".to_string(),
        message: format!("Failed to read zip archive: {}", e),
        path: None,
    })?;

    let total_files = archive.len();
    let mut files_imported = 0;
    let files_skipped = 0;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| SerializableError {
            kind: "ImportError".to_string(),
            message: format!("Failed to read zip entry: {}", e),
            path: None,
        })?;

        if file.is_dir() {
            continue;
        }

        let file_name = file.name().to_string();

        // Skip system files
        let should_skip = file_name
            .split('/')
            .any(|part| part.starts_with('.') || part == "Thumbs.db" || part == "desktop.ini");

        if should_skip {
            continue;
        }

        // Only import markdown and attachments
        let is_markdown = file_name.ends_with(".md");
        let is_in_attachments =
            file_name.contains("/attachments/") || file_name.contains("/assets/");
        let is_common_attachment = {
            let lower = file_name.to_lowercase();
            lower.ends_with(".png")
                || lower.ends_with(".jpg")
                || lower.ends_with(".jpeg")
                || lower.ends_with(".gif")
                || lower.ends_with(".svg")
                || lower.ends_with(".pdf")
                || lower.ends_with(".webp")
                || lower.ends_with(".heic")
                || lower.ends_with(".heif")
        };

        if !is_markdown && !is_in_attachments && !is_common_attachment {
            continue;
        }

        let file_path = workspace.join(&file_name);

        // Create parent directories
        if let Some(parent) = file_path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent).map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to create directory: {}", e),
                path: Some(parent.to_path_buf()),
            })?;
        }

        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to read file from zip: {}", e),
                path: Some(file_path.clone()),
            })?;

        fs.write_binary(&file_path, &contents)
            .map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to write file: {}", e),
                path: Some(file_path.clone()),
            })?;

        files_imported += 1;

        if files_imported % 100 == 0 {
            log::info!(
                "[Import] Progress: {}/{} files",
                files_imported,
                total_files
            );
        }
    }

    log::info!(
        "[Import] Complete: {} files imported, {} skipped",
        files_imported,
        files_skipped
    );

    Ok(ImportResult {
        success: true,
        files_imported,
        files_skipped,
        workspace_path: workspace.to_string_lossy().to_string(),
        error: None,
        cancelled: false,
    })
}

// ============================================================================
// Chunked Import Commands
// ============================================================================

/// Global storage for in-progress uploads
static UPLOAD_SESSIONS: std::sync::LazyLock<Mutex<HashMap<String, std::fs::File>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Start a chunked upload session
#[tauri::command]
pub async fn start_import_upload() -> Result<String, SerializableError> {
    use uuid::Uuid;

    let session_id = Uuid::new_v4().to_string();
    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join(format!("diaryx_import_{}.zip", &session_id));

    log::info!(
        "[Import] Starting chunked upload session: {} -> {:?}",
        session_id,
        temp_path
    );

    let file = std::fs::File::create(&temp_path).map_err(|e| SerializableError {
        kind: "ImportError".to_string(),
        message: format!("Failed to create temp file: {}", e),
        path: Some(temp_path),
    })?;

    UPLOAD_SESSIONS
        .lock()
        .unwrap()
        .insert(session_id.clone(), file);

    Ok(session_id)
}

/// Append a chunk of base64-encoded data to an upload session
#[tauri::command]
pub async fn append_import_chunk(
    session_id: String,
    chunk: String,
) -> Result<usize, SerializableError> {
    use base64::Engine;
    use std::io::Write;

    // Decode base64 chunk
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&chunk)
        .map_err(|e| SerializableError {
            kind: "ImportError".to_string(),
            message: format!("Failed to decode chunk: {}", e),
            path: None,
        })?;

    let bytes_len = bytes.len();

    // Write to temp file
    let mut sessions = UPLOAD_SESSIONS.lock().unwrap();
    let file = sessions
        .get_mut(&session_id)
        .ok_or_else(|| SerializableError {
            kind: "ImportError".to_string(),
            message: format!("Upload session not found: {}", session_id),
            path: None,
        })?;

    file.write_all(&bytes).map_err(|e| SerializableError {
        kind: "ImportError".to_string(),
        message: format!("Failed to write chunk: {}", e),
        path: None,
    })?;

    Ok(bytes_len)
}

/// Finish a chunked upload and import the zip file
#[tauri::command]
pub async fn finish_import_upload<R: Runtime>(
    app: AppHandle<R>,
    session_id: String,
    workspace_path: Option<String>,
) -> Result<ImportResult, SerializableError> {
    use std::io::Read;

    let fs = RealFileSystem;

    // Get workspace path
    let workspace = match workspace_path {
        Some(p) => PathBuf::from(p),
        None => {
            let config = Config::default();
            if config.default_workspace.as_os_str().is_empty() {
                return Err(SerializableError {
                    kind: "ImportError".to_string(),
                    message: "No workspace specified".to_string(),
                    path: None,
                });
            }
            config.default_workspace
        }
    };

    // Close the file and remove from sessions
    let temp_path = {
        let mut sessions = UPLOAD_SESSIONS.lock().unwrap();
        sessions.remove(&session_id);
        std::env::temp_dir().join(format!("diaryx_import_{}.zip", &session_id))
    };

    log::info!(
        "[Import] Finishing chunked upload: {} -> {:?}",
        session_id,
        temp_path
    );

    // Open the completed temp file
    let zip_file = std::fs::File::open(&temp_path).map_err(|e| SerializableError {
        kind: "ImportError".to_string(),
        message: format!("Failed to open temp file: {}", e),
        path: Some(temp_path.clone()),
    })?;

    let mut archive = zip::ZipArchive::new(zip_file).map_err(|e| SerializableError {
        kind: "ImportError".to_string(),
        message: format!("Failed to read zip archive: {}", e),
        path: Some(temp_path.clone()),
    })?;

    let total_files = archive.len();
    let mut files_imported = 0;
    let files_skipped = 0;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| SerializableError {
            kind: "ImportError".to_string(),
            message: format!("Failed to read zip entry: {}", e),
            path: None,
        })?;

        if file.is_dir() {
            continue;
        }

        let file_name = file.name().to_string();

        // Skip system files
        let should_skip = file_name.split('/').any(|part| {
            part.starts_with('.')
                || part == ".DS_Store"
                || part == ".git"
                || part == "Thumbs.db"
                || part == "desktop.ini"
        });

        if should_skip {
            log::debug!("[Import] Skipping system file: {}", file_name);
            continue;
        }

        // Only import markdown and attachments
        let is_markdown = file_name.ends_with(".md");
        let is_in_attachments =
            file_name.contains("/attachments/") || file_name.contains("/assets/");
        let is_common_attachment = {
            let lower = file_name.to_lowercase();
            lower.ends_with(".png")
                || lower.ends_with(".jpg")
                || lower.ends_with(".jpeg")
                || lower.ends_with(".gif")
                || lower.ends_with(".svg")
                || lower.ends_with(".pdf")
                || lower.ends_with(".webp")
        };

        if !is_markdown && !is_in_attachments && !is_common_attachment {
            log::debug!("[Import] Skipping non-workspace file: {}", file_name);
            continue;
        }

        let file_path = workspace.join(&file_name);

        log::info!(
            "[Import] Processing zip entry: {} -> {:?}",
            file_name,
            file_path
        );

        // Create parent directories
        if let Some(parent) = file_path.parent()
            && !parent.as_os_str().is_empty()
        {
            let mut current = workspace.clone();
            for component in std::path::Path::new(&file_name)
                .parent()
                .unwrap_or(std::path::Path::new(""))
                .components()
            {
                current = current.join(component);

                if current.exists() && current.is_file() {
                    // Delete file blocking directory creation
                    std::fs::remove_file(&current).map_err(|e| SerializableError {
                        kind: "ImportError".to_string(),
                        message: format!("Failed to remove conflicting file: {}", e),
                        path: Some(current.clone()),
                    })?;
                    log::info!("[Import] Removed conflicting file: {:?}", current);
                }

                if !current.exists() {
                    std::fs::create_dir(&current).map_err(|e| SerializableError {
                        kind: "ImportError".to_string(),
                        message: format!("Failed to create directory: {}", e),
                        path: Some(current.clone()),
                    })?;
                }
            }
        }

        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to read file from zip: {}", e),
                path: Some(file_path.clone()),
            })?;

        fs.write_binary(&file_path, &contents)
            .map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to write file: {}", e),
                path: Some(file_path.clone()),
            })?;

        files_imported += 1;

        if files_imported % 100 == 0 {
            log::info!(
                "[Import] Progress: {}/{} files",
                files_imported,
                total_files
            );
        }
    }

    // Clean up temp file
    if let Err(e) = std::fs::remove_file(&temp_path) {
        log::warn!("[Import] Failed to clean up temp file: {}", e);
    }

    log::info!(
        "[Import] Complete: {} files imported, {} skipped",
        files_imported,
        files_skipped
    );

    // Populate CRDT
    log::info!("[Import] Populating CRDT state...");
    let crdt_state = app.state::<CrdtState>();
    let storage = {
        let storage_guard = acquire_lock(&crdt_state.storage)?;
        storage_guard.as_ref().map(Arc::clone)
    };

    if let Some(storage) = storage {
        let diaryx = Diaryx::with_crdt_load(SyncToAsyncFs::new(RealFileSystem), storage)
            .unwrap_or_else(|e| {
                log::warn!("[Import] Failed to load CRDT state: {:?}", e);
                Diaryx::new(SyncToAsyncFs::new(RealFileSystem))
            });

        // Initialize CRDT by scanning workspace
        let cmd = Command::InitializeWorkspaceCrdt {
            workspace_path: workspace.to_string_lossy().to_string(),
            audience: None,
        };

        match diaryx.execute(cmd).await {
            Ok(response) => {
                log::info!("[Import] CRDT populated successfully: {:?}", response);
            }
            Err(e) => {
                log::error!("[Import] Failed to populate CRDT: {:?}", e);
            }
        }
    } else {
        log::warn!("[Import] CRDT storage not available, skipping population");
    }

    Ok(ImportResult {
        success: true,
        files_imported,
        files_skipped,
        workspace_path: workspace.to_string_lossy().to_string(),
        error: None,
        cancelled: false,
    })
}

// ============================================================================
// Export Commands
// ============================================================================

/// Export result
#[derive(Debug, Serialize)]
pub struct ExportResult {
    pub success: bool,
    pub files_exported: usize,
    pub output_path: Option<String>,
    pub error: Option<String>,
    pub cancelled: bool,
}

/// Export workspace to a zip file using native save dialog
#[tauri::command]
pub async fn export_to_zip<R: Runtime>(
    app: AppHandle<R>,
    workspace_path: Option<String>,
) -> Result<ExportResult, SerializableError> {
    use diaryx_core::fs::FileSystem;
    use std::io::Write;
    use tauri_plugin_dialog::DialogExt;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    let paths = get_platform_paths(&app)?;
    let workspace = workspace_path
        .map(PathBuf::from)
        .unwrap_or(paths.default_workspace);

    // Get workspace name for default filename
    let workspace_name = workspace
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace");
    let timestamp = chrono::Utc::now().format("%Y-%m-%d");
    let default_filename = format!("{}-{}.zip", workspace_name, timestamp);

    // Show native save dialog
    let save_path = app
        .dialog()
        .file()
        .add_filter("Zip Archive", &["zip"])
        .set_file_name(&default_filename)
        .set_title("Export Workspace to Zip")
        .blocking_save_file();

    let output_path = match save_path {
        Some(path) => path.into_path().map_err(|e| SerializableError {
            kind: "ExportError".to_string(),
            message: format!("Failed to get save path: {:?}", e),
            path: None,
        })?,
        None => {
            // User cancelled
            return Ok(ExportResult {
                success: false,
                files_exported: 0,
                output_path: None,
                error: None,
                cancelled: true,
            });
        }
    };

    log::info!(
        "[Export] Exporting workspace {:?} to {:?}",
        workspace,
        output_path
    );

    // Create zip file
    let file = std::fs::File::create(&output_path).map_err(|e| SerializableError {
        kind: "ExportError".to_string(),
        message: format!("Failed to create zip file: {}", e),
        path: Some(output_path.clone()),
    })?;

    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(6));

    let fs = RealFileSystem;

    // Get all files in workspace
    let all_files = fs
        .list_all_files_recursive(&workspace)
        .map_err(|e| SerializableError {
            kind: "ExportError".to_string(),
            message: format!("Failed to list files: {}", e),
            path: None,
        })?;

    let mut files_exported = 0;

    for file_path in all_files {
        // Skip hidden files and directories
        let relative_path =
            pathdiff::diff_paths(&file_path, &workspace).unwrap_or_else(|| file_path.clone());

        let should_skip = relative_path
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.'));

        if should_skip {
            continue;
        }

        let relative_str = relative_path.to_string_lossy().to_string();

        // Read file content
        let content = match fs.read_binary(&file_path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("[Export] Failed to read {:?}: {}", file_path, e);
                continue;
            }
        };

        // Add to zip
        if let Err(e) = zip.start_file(&relative_str, options) {
            log::warn!("[Export] Failed to start zip entry {}: {}", relative_str, e);
            continue;
        }

        if let Err(e) = zip.write_all(&content) {
            log::warn!("[Export] Failed to write zip entry {}: {}", relative_str, e);
            continue;
        }

        files_exported += 1;
    }

    zip.finish().map_err(|e| SerializableError {
        kind: "ExportError".to_string(),
        message: format!("Failed to finalize zip: {}", e),
        path: Some(output_path.clone()),
    })?;

    log::info!("[Export] Complete: {} files exported", files_exported);

    Ok(ExportResult {
        success: true,
        files_exported,
        output_path: Some(output_path.to_string_lossy().to_string()),
        error: None,
        cancelled: false,
    })
}

// ============================================================================
// Cloud Sync Commands
// ============================================================================

/// Result of a sync operation
#[derive(Debug, Serialize)]
pub struct SyncStatus {
    pub provider: String,
    pub success: bool,
    pub files_uploaded: usize,
    pub files_downloaded: usize,
    pub files_deleted: usize,
    pub conflicts: Vec<SyncConflict>,
    pub error: Option<String>,
}

/// Information about a sync conflict
#[derive(Debug, Serialize)]
pub struct SyncConflict {
    pub path: String,
    pub local_modified: Option<i64>,
    pub remote_modified: Option<String>,
}

/// Sync workspace with S3
#[tauri::command]
pub async fn sync_to_s3<R: Runtime>(
    app: AppHandle<R>,
    workspace_path: Option<String>,
    config: S3ConfigRequest,
) -> Result<SyncStatus, SerializableError> {
    use crate::cloud::S3Target;
    use diaryx_core::backup::{CloudBackupConfig, CloudProvider};
    use diaryx_core::sync::engine::SyncEngine;
    use diaryx_core::sync::{SyncProgress, SyncStage};
    use tokio::sync::mpsc;

    let paths = get_platform_paths(&app)?;
    let workspace = workspace_path
        .map(PathBuf::from)
        .unwrap_or(paths.default_workspace);

    let provider_id = format!("s3:{}", config.bucket);

    // Create cloud config
    let cloud_config = CloudBackupConfig {
        id: uuid::Uuid::new_v4().to_string(),
        name: config.name.clone(),
        provider: CloudProvider::S3 {
            bucket: config.bucket.clone(),
            region: config.region.clone(),
            prefix: config.prefix.clone(),
            endpoint: config.endpoint.clone(),
        },
        enabled: true,
    };

    // Create S3 target
    let target = S3Target::new(cloud_config, config.access_key, config.secret_key)
        .await
        .map_err(|e| SerializableError {
            kind: "SyncError".to_string(),
            message: e,
            path: None,
        })?;

    // Create manifest path
    let manifest_path = workspace.join(".diaryx").join("sync_manifest_s3.json");

    // Create sync engine
    let mut engine = SyncEngine::new(target, manifest_path);

    // Create async filesystem wrapper
    let fs = SyncToAsyncFs::new(RealFileSystem);

    // Load existing manifest
    if let Err(e) = engine.load_manifest(&fs).await {
        log::warn!("Failed to load manifest: {}", e);
    }

    // Create channel for progress events
    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel::<SyncProgressEvent>();

    // Clone app for the event emission task
    let app_clone = app.clone();

    // Spawn a task to forward progress events to the frontend
    let event_task = tauri::async_runtime::spawn(async move {
        while let Some(event) = progress_rx.recv().await {
            let _ = app_clone.emit("sync_progress", &event);
        }
    });

    // Run sync with progress callback
    let result = engine
        .sync_with_progress(&fs, &workspace, |progress: SyncProgress| {
            let stage_str = match progress.stage {
                SyncStage::DetectingLocal => "detecting_local",
                SyncStage::DetectingRemote => "detecting_remote",
                SyncStage::Uploading => "uploading",
                SyncStage::Downloading => "downloading",
                SyncStage::Deleting => "deleting",
                SyncStage::Complete => "complete",
                SyncStage::Error => "error",
            };
            let _ = progress_tx.send(SyncProgressEvent {
                stage: stage_str.to_string(),
                current: progress.current,
                total: progress.total,
                percent: progress.percent,
                message: progress.message,
            });
        })
        .await;

    // Drop sender to close channel
    drop(progress_tx);

    // Wait for event task to finish
    let _ = event_task.await;

    // Convert conflicts
    let conflicts: Vec<SyncConflict> = result
        .conflicts
        .iter()
        .map(|c| SyncConflict {
            path: c.path.clone(),
            local_modified: c.local_modified_at,
            remote_modified: c.remote_modified_at.map(|dt| dt.to_rfc3339()),
        })
        .collect();

    Ok(SyncStatus {
        provider: provider_id,
        success: result.success,
        files_uploaded: result.files_uploaded,
        files_downloaded: result.files_downloaded,
        files_deleted: result.files_deleted,
        conflicts,
        error: result.error,
    })
}

/// Sync workspace with Google Drive
#[tauri::command]
pub async fn sync_to_google_drive<R: Runtime>(
    app: AppHandle<R>,
    workspace_path: Option<String>,
    config: GoogleDriveConfigRequest,
) -> Result<SyncStatus, SerializableError> {
    use crate::cloud::GoogleDriveTarget;
    use diaryx_core::backup::{CloudBackupConfig, CloudProvider};
    use diaryx_core::sync::engine::SyncEngine;
    use diaryx_core::sync::{SyncProgress, SyncStage};
    use tokio::sync::mpsc;

    let paths = get_platform_paths(&app)?;
    let workspace = workspace_path
        .map(PathBuf::from)
        .unwrap_or(paths.default_workspace);

    let provider_id = format!("gdrive:{}", config.folder_id.as_deref().unwrap_or("root"));

    // Create cloud config
    let cloud_config = CloudBackupConfig {
        id: uuid::Uuid::new_v4().to_string(),
        name: config.name.clone(),
        provider: CloudProvider::GoogleDrive {
            folder_id: config.folder_id.clone(),
        },
        enabled: true,
    };

    // Create Google Drive target
    let target = GoogleDriveTarget::new(cloud_config, config.access_token, config.folder_id)
        .map_err(|e| SerializableError {
            kind: "SyncError".to_string(),
            message: e,
            path: None,
        })?;

    // Create manifest path
    let manifest_path = workspace.join(".diaryx").join("sync_manifest_gdrive.json");

    // Create sync engine
    let mut engine = SyncEngine::new(target, manifest_path);

    // Create async filesystem wrapper
    let fs = SyncToAsyncFs::new(RealFileSystem);

    // Load existing manifest
    if let Err(e) = engine.load_manifest(&fs).await {
        log::warn!("Failed to load manifest: {}", e);
    }

    // Create channel for progress events
    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel::<SyncProgressEvent>();

    // Clone app for the event emission task
    let app_clone = app.clone();

    // Spawn a task to forward progress events to the frontend
    let event_task = tauri::async_runtime::spawn(async move {
        while let Some(event) = progress_rx.recv().await {
            let _ = app_clone.emit("sync_progress", &event);
        }
    });

    // Run sync with progress callback
    let result = engine
        .sync_with_progress(&fs, &workspace, |progress: SyncProgress| {
            let stage_str = match progress.stage {
                SyncStage::DetectingLocal => "detecting_local",
                SyncStage::DetectingRemote => "detecting_remote",
                SyncStage::Uploading => "uploading",
                SyncStage::Downloading => "downloading",
                SyncStage::Deleting => "deleting",
                SyncStage::Complete => "complete",
                SyncStage::Error => "error",
            };
            let _ = progress_tx.send(SyncProgressEvent {
                stage: stage_str.to_string(),
                current: progress.current,
                total: progress.total,
                percent: progress.percent,
                message: progress.message,
            });
        })
        .await;

    // Drop sender to close channel
    drop(progress_tx);

    // Wait for event task to finish
    let _ = event_task.await;

    // Convert conflicts
    let conflicts: Vec<SyncConflict> = result
        .conflicts
        .iter()
        .map(|c| SyncConflict {
            path: c.path.clone(),
            local_modified: c.local_modified_at,
            remote_modified: c.remote_modified_at.map(|dt| dt.to_rfc3339()),
        })
        .collect();

    Ok(SyncStatus {
        provider: provider_id,
        success: result.success,
        files_uploaded: result.files_uploaded,
        files_downloaded: result.files_downloaded,
        files_deleted: result.files_deleted,
        conflicts,
        error: result.error,
    })
}

/// Get sync status (last sync time, pending changes)
#[tauri::command]
pub async fn get_sync_status<R: Runtime>(
    app: AppHandle<R>,
    provider: String,
    workspace_path: Option<String>,
) -> Result<SyncStatusInfo, SerializableError> {
    use diaryx_core::sync::SyncManifest;

    let paths = get_platform_paths(&app)?;
    let workspace = workspace_path
        .map(PathBuf::from)
        .unwrap_or(paths.default_workspace);

    let manifest_filename = match provider.as_str() {
        "s3" => "sync_manifest_s3.json",
        "google_drive" | "gdrive" => "sync_manifest_gdrive.json",
        _ => {
            return Err(SerializableError {
                kind: "SyncError".to_string(),
                message: format!("Unknown provider: {}", provider),
                path: None,
            });
        }
    };

    let manifest_path = workspace.join(".diaryx").join(manifest_filename);
    let fs = SyncToAsyncFs::new(RealFileSystem);

    let manifest = (SyncManifest::load_from_file(&fs, &manifest_path).await).ok();

    Ok(SyncStatusInfo {
        provider,
        last_sync: manifest
            .as_ref()
            .and_then(|m| m.last_sync.map(|dt| dt.to_rfc3339())),
        files_synced: manifest.as_ref().map(|m| m.files.len()).unwrap_or(0),
        is_configured: manifest.is_some(),
    })
}

/// Sync status information
#[derive(Debug, Serialize)]
pub struct SyncStatusInfo {
    pub provider: String,
    pub last_sync: Option<String>,
    pub files_synced: usize,
    pub is_configured: bool,
}

/// Resolve a sync conflict
#[tauri::command]
pub async fn resolve_sync_conflict<R: Runtime>(
    app: AppHandle<R>,
    provider: String,
    path: String,
    resolution: String,
    _workspace_path: Option<String>,
    _config: Option<serde_json::Value>,
) -> Result<bool, SerializableError> {
    use diaryx_core::sync::conflict::ConflictResolution;

    let _paths = get_platform_paths(&app)?;

    let resolution = ConflictResolution::from_str(&resolution).map_err(|_| SerializableError {
        kind: "SyncError".to_string(),
        message: format!(
            "Invalid resolution: {}. Use 'local', 'remote', 'both', or 'skip'",
            resolution
        ),
        path: None,
    })?;

    // For now, just return success - the actual resolution would need to be implemented
    // with the full sync engine context
    log::info!(
        "Conflict resolution requested for {} in {}: {:?}",
        path,
        provider,
        resolution
    );

    Ok(true)
}

// ============================================================================
// Guest Mode Commands
// ============================================================================

/// Start guest mode for a share session.
///
/// Creates an in-memory filesystem for all operations. This allows the user
/// to join a share session without affecting their local workspace files.
/// All synced files will be stored in memory only.
#[tauri::command]
pub async fn start_guest_mode<R: Runtime>(
    app: AppHandle<R>,
    join_code: String,
) -> Result<(), SerializableError> {
    let guest_state = app.state::<GuestModeState>();
    let mut active = acquire_lock(&guest_state.active)?;
    let mut filesystem = acquire_lock(&guest_state.filesystem)?;
    let mut code = acquire_lock(&guest_state.join_code)?;

    if *active {
        return Err(SerializableError {
            kind: "GuestModeError".to_string(),
            message: "Already in guest mode".to_string(),
            path: None,
        });
    }

    *active = true;
    *filesystem = Some(InMemoryFileSystem::new());
    *code = Some(join_code.clone());

    log::info!("[guest_mode] Started guest mode for session: {}", join_code);
    Ok(())
}

/// End guest mode and clear in-memory data.
///
/// This clears the in-memory filesystem and returns the app to normal mode.
/// All files from the guest session will be discarded.
#[tauri::command]
pub async fn end_guest_mode<R: Runtime>(app: AppHandle<R>) -> Result<(), SerializableError> {
    let guest_state = app.state::<GuestModeState>();
    let mut active = acquire_lock(&guest_state.active)?;
    let mut filesystem = acquire_lock(&guest_state.filesystem)?;
    let mut code = acquire_lock(&guest_state.join_code)?;

    let was_active = *active;
    let join_code = code.clone();

    *filesystem = None;
    *active = false;
    *code = None;

    if was_active {
        log::info!(
            "[guest_mode] Ended guest mode for session: {}",
            join_code.unwrap_or_else(|| "unknown".to_string())
        );
    } else {
        log::debug!("[guest_mode] end_guest_mode called but guest mode was not active");
    }

    Ok(())
}

/// Check if guest mode is currently active.
#[tauri::command]
pub fn is_guest_mode<R: Runtime>(app: AppHandle<R>) -> Result<bool, SerializableError> {
    let guest_state = app.state::<GuestModeState>();
    let active = acquire_lock(&guest_state.active)?;
    Ok(*active)
}

// ============================================================================
// WebSocket Sync Commands
// ============================================================================

use crate::websocket_sync::{OutgoingSyncMessage, SyncConfig, SyncTransport};

/// State for WebSocket sync connections.
/// Uses tokio::sync::Mutex to allow holding across await points.
pub struct WebSocketSyncState {
    pub transport: tokio::sync::Mutex<Option<SyncTransport>>,
}

impl WebSocketSyncState {
    pub fn new() -> Self {
        Self {
            transport: tokio::sync::Mutex::new(None),
        }
    }
}

impl Default for WebSocketSyncState {
    fn default() -> Self {
        Self::new()
    }
}

/// Start WebSocket sync connection.
///
/// Connects to the specified sync server and begins real-time sync.
#[tauri::command]
pub async fn start_websocket_sync<R: Runtime>(
    app: AppHandle<R>,
    server_url: String,
    doc_name: String,
    auth_token: Option<String>,
) -> Result<(), SerializableError> {
    log::info!(
        "[start_websocket_sync] Starting sync to {} with doc {}",
        server_url,
        doc_name
    );

    let ws_state = app.state::<WebSocketSyncState>();
    let mut transport_guard = ws_state.transport.lock().await;

    // Disconnect existing connection if any
    if let Some(ref mut transport) = *transport_guard {
        transport.disconnect().await;
    }

    // Get CRDT storage and workspace path from app state
    let crdt_state = app.state::<CrdtState>();
    let storage = {
        let storage_guard = crdt_state.storage.lock().map_err(|e| SerializableError {
            kind: "SyncError".to_string(),
            message: format!("Failed to acquire storage lock: {}", e),
            path: None,
        })?;
        storage_guard.clone().ok_or_else(|| SerializableError {
            kind: "SyncError".to_string(),
            message: "CRDT storage not initialized".to_string(),
            path: None,
        })?
    };
    let workspace_path = {
        let path_guard = crdt_state
            .workspace_path
            .lock()
            .map_err(|e| SerializableError {
                kind: "SyncError".to_string(),
                message: format!("Failed to acquire workspace path lock: {}", e),
                path: None,
            })?;
        path_guard.clone().ok_or_else(|| SerializableError {
            kind: "SyncError".to_string(),
            message: "Workspace path not set".to_string(),
            path: None,
        })?
    };

    // Extract sync_manager from cached Diaryx instance (if available).
    // This ensures the WebSocket sync uses the same CRDT instances as command execution,
    // preventing state divergence between the two.
    let sync_manager = {
        let diaryx_guard = crdt_state.diaryx.lock().map_err(|e| SerializableError {
            kind: "SyncError".to_string(),
            message: format!("Failed to acquire diaryx lock: {}", e),
            path: None,
        })?;
        diaryx_guard
            .as_ref()
            .and_then(|d| d.sync_manager().cloned())
    };

    if sync_manager.is_some() {
        log::info!("[start_websocket_sync] Using sync_manager from cached Diaryx instance");
    } else {
        log::warn!(
            "[start_websocket_sync] No cached Diaryx or sync_manager available, will create new one"
        );
    }

    // Create new transport with CRDT storage
    let config = SyncConfig {
        server_url,
        doc_name,
        auth_token,
        write_to_disk: true,
        storage,
        workspace_root: workspace_path,
        sync_manager,
    };

    let mut transport = SyncTransport::new(config);
    transport.connect().await.map_err(|e| SerializableError {
        kind: "SyncError".to_string(),
        message: format!("Failed to connect: {}", e),
        path: None,
    })?;

    // Get the outgoing sender to connect local edits to the WebSocket
    let outgoing_tx = transport.get_outgoing_sender();

    *transport_guard = Some(transport);

    // Set up event callback on the cached Diaryx instance to send local edits
    // to the WebSocket via the outgoing channel
    if let Some(tx) = outgoing_tx {
        let cached_diaryx = {
            let diaryx_guard = crdt_state.diaryx.lock().map_err(|e| SerializableError {
                kind: "SyncError".to_string(),
                message: format!("Failed to acquire diaryx lock: {}", e),
                path: None,
            })?;
            diaryx_guard.as_ref().map(Arc::clone)
        };

        if let Some(diaryx) = cached_diaryx {
            log::info!("[start_websocket_sync] Setting up event callback for local edit sync");
            let tx_clone = tx.clone();
            diaryx.set_sync_event_callback(Arc::new(move |event| {
                if let diaryx_core::fs::FileSystemEvent::SendSyncMessage {
                    doc_name,
                    message,
                    is_body,
                } = event
                {
                    let outgoing = OutgoingSyncMessage {
                        doc_name: doc_name.clone(),
                        message: message.clone(),
                        is_body: *is_body,
                    };
                    if let Err(e) = tx_clone.send(outgoing) {
                        log::warn!(
                            "[start_websocket_sync] Failed to send outgoing message for {}: {}",
                            doc_name,
                            e
                        );
                    } else {
                        log::debug!(
                            "[start_websocket_sync] Queued outgoing {} message for {}, {} bytes",
                            if *is_body { "body" } else { "metadata" },
                            doc_name,
                            message.len()
                        );
                    }
                }
            }));
        } else {
            log::warn!("[start_websocket_sync] No cached Diaryx instance to set up event callback");
        }
    }

    Ok(())
}

/// Stop WebSocket sync connection.
#[tauri::command]
pub async fn stop_websocket_sync<R: Runtime>(app: AppHandle<R>) -> Result<(), SerializableError> {
    log::info!("[stop_websocket_sync] Stopping sync");

    let ws_state = app.state::<WebSocketSyncState>();
    let mut transport_guard = ws_state.transport.lock().await;

    if let Some(ref mut transport) = *transport_guard {
        transport.disconnect().await;
    }
    *transport_guard = None;

    Ok(())
}

/// Get WebSocket sync status.
#[tauri::command]
pub async fn get_websocket_sync_status<R: Runtime>(
    app: AppHandle<R>,
) -> Result<WebSocketSyncStatusResponse, SerializableError> {
    let ws_state = app.state::<WebSocketSyncState>();
    let transport_guard = ws_state.transport.lock().await;

    if let Some(ref transport) = *transport_guard {
        let status = transport.status().await;
        let connected = transport.is_running();
        Ok(WebSocketSyncStatusResponse {
            connected,
            status: Some(status),
        })
    } else {
        Ok(WebSocketSyncStatusResponse {
            connected: false,
            status: None,
        })
    }
}

/// WebSocket sync status response.
#[derive(Debug, Serialize)]
pub struct WebSocketSyncStatusResponse {
    pub connected: bool,
    pub status: Option<crate::websocket_sync::SyncStatus>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use diaryx_core::crdt::SqliteStorage;
    use std::time::Instant;

    /// Performance test to verify CRDT caching works.
    ///
    /// This test validates that:
    /// 1. First command execution (cold start) loads CRDT from storage
    /// 2. Subsequent commands reuse the cached Diaryx instance and are fast
    #[tokio::test]
    async fn test_crdt_caching_performance() {
        // Setup: create temp directory with SQLite storage and files
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("crdt.sqlite");
        let workspace_path = temp_dir.path().join("workspace");
        std::fs::create_dir_all(&workspace_path).unwrap();

        // Create SQLite storage
        let storage = Arc::new(SqliteStorage::open(&db_path).unwrap());

        // Create workspace with ~50 test files to simulate real usage
        let diaryx_setup = Diaryx::with_crdt(
            SyncToAsyncFs::new(RealFileSystem),
            Arc::clone(&storage) as Arc<dyn CrdtStorage>,
        );
        diaryx_setup.set_workspace_root(workspace_path.clone());

        // Create index file
        let index_path = workspace_path.join("index.md");
        std::fs::write(&index_path, "---\ntitle: Test Workspace\n---\n# Test\n").unwrap();

        // Create 50 test files to simulate a real workspace
        for i in 0..50 {
            let file_path = workspace_path.join(format!("note_{}.md", i));
            std::fs::write(
                &file_path,
                format!(
                    "---\ntitle: Note {}\n---\n# Note {}\n\nContent for note {}.\n",
                    i, i, i
                ),
            )
            .unwrap();
        }

        // Initialize workspace CRDT (populate with files)
        let cmd = Command::InitializeWorkspaceCrdt {
            workspace_path: workspace_path.to_string_lossy().to_string(),
            audience: None,
        };
        let _ = diaryx_setup.execute(cmd).await;
        drop(diaryx_setup);

        // Setup CrdtState like Tauri does
        let crdt_state = CrdtState {
            workspace_path: Mutex::new(Some(workspace_path.clone())),
            storage: Mutex::new(Some(Arc::clone(&storage) as Arc<dyn CrdtStorage>)),
            diaryx: Mutex::new(None), // Start with no cached instance
        };

        // Helper to execute a command using the CrdtState (simulating execute() logic)
        async fn execute_with_state(
            state: &CrdtState,
            cmd: Command,
        ) -> Result<diaryx_core::command::Response, diaryx_core::error::DiaryxError> {
            // Try cached diaryx first
            let cached = {
                let guard = state.diaryx.lock().unwrap();
                guard.as_ref().map(Arc::clone)
            };

            let diaryx = if let Some(cached) = cached {
                cached
            } else {
                // Load from storage (slow path)
                let storage = {
                    let guard = state.storage.lock().unwrap();
                    guard.as_ref().map(Arc::clone)
                };
                let ws_path = {
                    let guard = state.workspace_path.lock().unwrap();
                    guard.clone()
                };

                let new = if let Some(storage) = storage {
                    Arc::new(Diaryx::with_crdt_load(
                        SyncToAsyncFs::new(RealFileSystem),
                        storage,
                    )?)
                } else {
                    Arc::new(Diaryx::new(SyncToAsyncFs::new(RealFileSystem)))
                };

                if let Some(ref ws) = ws_path {
                    new.set_workspace_root(ws.clone());
                }

                // Cache it
                {
                    let mut guard = state.diaryx.lock().unwrap();
                    *guard = Some(Arc::clone(&new));
                }
                new
            };

            diaryx.execute(cmd).await
        }

        // Test 1: First command should work (may take time to load CRDT)
        let first_cmd = Command::GetEntry {
            path: index_path.to_string_lossy().to_string(),
        };
        let start = Instant::now();
        let result = execute_with_state(&crdt_state, first_cmd).await;
        let first_duration = start.elapsed();
        assert!(result.is_ok(), "First command should succeed");
        println!("First command (cold): {:?}", first_duration);

        // Test 2: Subsequent commands should be fast (cached)
        let mut total_warm_duration = std::time::Duration::ZERO;
        for i in 0..10 {
            let cmd = Command::GetEntry {
                path: workspace_path
                    .join(format!("note_{}.md", i % 50))
                    .to_string_lossy()
                    .to_string(),
            };
            let start = Instant::now();
            let result = execute_with_state(&crdt_state, cmd).await;
            total_warm_duration += start.elapsed();
            assert!(result.is_ok(), "Subsequent command {} should succeed", i);
        }
        let avg_warm_duration = total_warm_duration / 10;
        println!("Average warm command: {:?}", avg_warm_duration);

        // Assert: warm commands should be significantly faster than 100ms each
        // (Before fix: each command could take 10+ seconds due to CRDT reload)
        assert!(
            avg_warm_duration.as_millis() < 100,
            "Cached commands should complete in <100ms, got {:?}",
            avg_warm_duration
        );
    }
}
