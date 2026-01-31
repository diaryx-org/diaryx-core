//! S3 cloud backup target implementation.
//!
//! This module provides both:
//! - ZIP-based backup (existing `BackupTarget` implementation)
//! - File-level sync (new `CloudSyncProvider` implementation)

use aws_config::BehaviorVersion;
use aws_sdk_s3::Client;
use aws_smithy_types::byte_stream::ByteStream;
use chrono::{DateTime, Utc};
use diaryx_core::backup::{
    BackupResult, BackupTarget, CloudBackupConfig, CloudProvider, FailurePolicy,
};
use diaryx_core::cloud::RemoteFileInfo;
use diaryx_core::cloud::engine::CloudSyncProvider;
use diaryx_core::fs::{AsyncFileSystem, BoxFuture, FileSystem, RealFileSystem};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

/// S3 cloud backup target.
///
/// Note: This struct does NOT store a tokio runtime to avoid panics when
/// dropped in an async context. A temporary runtime is created for blocking
/// operations that need it.
pub struct S3Target {
    config: CloudBackupConfig,
    client: Client,
    // Note: We don't store the runtime to avoid "Cannot drop a runtime in a context
    // where blocking is not allowed" panics. A temporary runtime is created when needed.
}

impl S3Target {
    /// Create a new S3 target with credentials.
    ///
    /// This is an async function and must be called from within an async context.
    pub async fn new(
        config: CloudBackupConfig,
        access_key: String,
        secret_key: String,
    ) -> Result<Self, String> {
        // Validate config is S3
        let (_bucket, region, endpoint) = match &config.provider {
            CloudProvider::S3 {
                bucket,
                region,
                endpoint,
                ..
            } => (bucket.clone(), region.clone(), endpoint.clone()),
            _ => return Err("Config must be S3 provider".to_string()),
        };

        // Build S3 client asynchronously
        let config_builder = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_sdk_s3::config::Region::new(region))
            .credentials_provider(aws_sdk_s3::config::Credentials::new(
                &access_key,
                &secret_key,
                None,
                None,
                "diaryx",
            ));

        let sdk_config = config_builder.load().await;

        let mut s3_config_builder = aws_sdk_s3::config::Builder::from(&sdk_config);

        // Use custom endpoint if provided (for MinIO, etc.)
        if let Some(ep) = endpoint {
            s3_config_builder = s3_config_builder.endpoint_url(&ep).force_path_style(true);
        }

        let client = Client::from_conf(s3_config_builder.build());

        Ok(Self { config, client })
    }

    /// Create a new S3 target with credentials (blocking version).
    ///
    /// Use this when calling from a blocking context (e.g., inside `spawn_blocking`).
    /// For async contexts, use `new()` instead.
    pub fn new_blocking(
        config: CloudBackupConfig,
        access_key: String,
        secret_key: String,
    ) -> Result<Self, String> {
        // Validate config is S3
        let (_bucket, region, endpoint) = match &config.provider {
            CloudProvider::S3 {
                bucket,
                region,
                endpoint,
                ..
            } => (bucket.clone(), region.clone(), endpoint.clone()),
            _ => return Err("Config must be S3 provider".to_string()),
        };

        // Create a temporary runtime for initialization
        let init_runtime =
            Runtime::new().map_err(|e| format!("Failed to create runtime: {}", e))?;

        // Build S3 client
        let client = init_runtime.block_on(async {
            let config_builder = aws_config::defaults(BehaviorVersion::latest())
                .region(aws_sdk_s3::config::Region::new(region))
                .credentials_provider(aws_sdk_s3::config::Credentials::new(
                    &access_key,
                    &secret_key,
                    None,
                    None,
                    "diaryx",
                ));

            let sdk_config = config_builder.load().await;

            let mut s3_config_builder = aws_sdk_s3::config::Builder::from(&sdk_config);

            // Use custom endpoint if provided (for MinIO, etc.)
            if let Some(ep) = endpoint {
                s3_config_builder = s3_config_builder.endpoint_url(&ep).force_path_style(true);
            }

            Client::from_conf(s3_config_builder.build())
        });

        // Drop the init runtime explicitly before returning
        drop(init_runtime);

        Ok(Self { config, client })
    }

    /// Create a temporary runtime for blocking operations.
    /// This avoids storing a runtime in the struct which would panic on drop in async context.
    fn create_runtime() -> Result<Runtime, String> {
        Runtime::new().map_err(|e| format!("Failed to create runtime: {}", e))
    }

    /// Get the S3 key for the backup file.
    fn backup_key(&self) -> String {
        let prefix = match &self.config.provider {
            CloudProvider::S3 { prefix, .. } => prefix.clone().unwrap_or_default(),
            _ => String::new(),
        };

        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        if prefix.is_empty() {
            format!("diaryx_backup_{}.zip", timestamp)
        } else {
            format!(
                "{}/diaryx_backup_{}.zip",
                prefix.trim_end_matches('/'),
                timestamp
            )
        }
    }

    /// Get S3 bucket name.
    pub fn bucket(&self) -> &str {
        match &self.config.provider {
            CloudProvider::S3 { bucket, .. } => bucket,
            _ => "",
        }
    }

    /// Get S3 key prefix for sync operations.
    pub fn sync_prefix(&self) -> String {
        match &self.config.provider {
            CloudProvider::S3 { prefix, .. } => prefix
                .clone()
                .unwrap_or_default()
                .trim_end_matches('/')
                .to_string(),
            _ => String::new(),
        }
    }

    /// Build the full S3 key for a file path.
    fn build_key(&self, path: &str) -> String {
        let prefix = self.sync_prefix();
        // URL-encode special characters in the path to avoid issues with S3-compatible storage
        let encoded_path = Self::encode_key_path(path);
        if prefix.is_empty() {
            encoded_path
        } else {
            format!("{}/{}", prefix, encoded_path)
        }
    }

    /// Extract relative path from S3 key.
    fn extract_path(&self, key: &str) -> Option<String> {
        let prefix = self.sync_prefix();
        let encoded_path = if prefix.is_empty() {
            key.to_string()
        } else {
            key.strip_prefix(&format!("{}/", prefix))?.to_string()
        };
        // Decode URL-encoded characters back to original
        Some(Self::decode_key_path(&encoded_path))
    }

    /// URL-encode special characters in a path for S3 key usage.
    /// This handles characters that may cause issues with S3-compatible storage (like R2).
    fn encode_key_path(path: &str) -> String {
        // Percent-encode special characters that cause issues:
        // - Quotes (") can break some S3-compatible implementations
        // We preserve forward slashes as path separators
        path.split('/')
            .map(|segment| {
                // Encode problematic characters in each path segment
                segment
                    .replace('%', "%25") // Encode percent first to avoid double-encoding
                    .replace('"', "%22")
                    .replace('\\', "%5C")
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Decode a URL-encoded S3 key path back to original form.
    fn decode_key_path(encoded: &str) -> String {
        encoded
            .replace("%22", "\"")
            .replace("%5C", "\\")
            .replace("%25", "%")
    }

    /// Create a zip archive of the workspace with progress callback.
    /// The callback receives (current_file, total_files, percent) for each file added.
    fn create_zip_archive_with_progress<F>(
        &self,
        fs: &dyn FileSystem,
        workspace_path: &Path,
        mut on_progress: F,
    ) -> Result<Vec<u8>, String>
    where
        F: FnMut(usize, usize, u8),
    {
        log::info!(
            "[S3 Backup] Creating zip from workspace: {:?}",
            workspace_path
        );

        let mut buffer = Vec::new();
        {
            let mut zip = ZipWriter::new(std::io::Cursor::new(&mut buffer));
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

            // Get all files recursively
            let files = fs
                .list_all_files_recursive(workspace_path)
                .map_err(|e| format!("Failed to list files: {}", e))?;

            // Filter to markdown files and attachments, excluding hidden files/folders
            let files: Vec<_> = files
                .into_iter()
                .filter(|p| {
                    if fs.is_dir(p) {
                        return false;
                    }
                    let rel_path = p
                        .strip_prefix(workspace_path)
                        .unwrap_or(p)
                        .to_string_lossy();
                    // Skip hidden files/folders
                    if rel_path.split('/').any(|c| c.starts_with('.')) {
                        return false;
                    }
                    // Include markdown files and files in _attachments folders
                    let is_markdown = rel_path.ends_with(".md");
                    let is_attachment = rel_path.split('/').any(|c| c == "_attachments");
                    is_markdown || is_attachment
                })
                .collect();

            let total_files = files.len();
            log::info!("[S3 Backup] Found {} files to backup", total_files);

            let mut files_added = 0;

            for (i, file_path) in files.iter().enumerate() {
                let relative_path = file_path.strip_prefix(workspace_path).unwrap_or(file_path);

                let path_str = relative_path.to_string_lossy().to_string();

                // Calculate percent (10-80 range for zipping, leaving room for upload)
                let percent = 10 + ((i * 70) / total_files.max(1)) as u8;

                // Emit progress every 100 files to avoid overwhelming
                if i % 100 == 0 {
                    on_progress(i, total_files, percent);
                }

                // Progress logging every 500 files
                if i % 500 == 0 && i > 0 {
                    log::info!(
                        "[S3 Backup] Progress: {}/{} files processed",
                        i,
                        total_files
                    );
                }

                // Try to read as text first, then binary
                if let Ok(content) = fs.read_to_string(file_path) {
                    zip.start_file(&path_str, options)
                        .map_err(|e| format!("Failed to start file {}: {}", path_str, e))?;
                    zip.write_all(content.as_bytes())
                        .map_err(|e| format!("Failed to write file {}: {}", path_str, e))?;
                    files_added += 1;
                } else if let Ok(content) = fs.read_binary(file_path) {
                    zip.start_file(&path_str, options)
                        .map_err(|e| format!("Failed to start file {}: {}", path_str, e))?;
                    zip.write_all(&content)
                        .map_err(|e| format!("Failed to write file {}: {}", path_str, e))?;
                    files_added += 1;
                } else {
                    log::warn!("[S3 Backup] Skipping file {}: could not read", path_str);
                }
            }

            // Final progress for zip complete
            on_progress(total_files, total_files, 80);
            log::info!("[S3 Backup] Zip complete: {} files added", files_added);
            zip.finish()
                .map_err(|e| format!("Failed to finish zip: {}", e))?;
        }

        Ok(buffer)
    }

    /// Backup workspace to S3 with progress callback.
    /// Callback receives: (stage: &str, current: usize, total: usize, percent: u8)
    pub fn backup_with_progress<F>(
        &self,
        fs: &dyn FileSystem,
        workspace_path: &Path,
        mut on_progress: F,
    ) -> BackupResult
    where
        F: FnMut(&str, usize, usize, u8),
    {
        // Emit preparing stage
        on_progress("preparing", 0, 0, 5);

        // Create zip archive with progress
        let zip_data = match self.create_zip_archive_with_progress(
            fs,
            workspace_path,
            |current, total, percent| {
                on_progress("zipping", current, total, percent);
            },
        ) {
            Ok(data) => data,
            Err(e) => return BackupResult::failure(e),
        };

        let zip_size_mb = zip_data.len() as f64 / (1024.0 * 1024.0);
        log::info!("[S3 Backup] Zip size: {:.2} MB", zip_size_mb);

        if zip_size_mb > 500.0 {
            log::warn!(
                "[S3 Backup] Zip is very large ({:.2} MB), upload may take a while",
                zip_size_mb
            );
        }

        let key = self.backup_key();
        let bucket = self.bucket().to_string();
        let client = self.client.clone();
        let zip_len = zip_data.len();

        // Emit upload stage start
        on_progress("uploading", 0, zip_len, 85);
        log::info!("[S3 Backup] Starting upload to s3://{}/{}", bucket, key);

        // Create a progress channel for upload progress
        let (progress_tx, mut progress_rx) = mpsc::unbounded_channel::<u8>();

        // Create a retryable body that emits progress as data is consumed
        let zip_data_clone = zip_data.clone();
        let body = aws_smithy_types::body::SdkBody::retryable(move || {
            use aws_smithy_types::body::SdkBody;

            let data = zip_data_clone.clone();
            let tx = progress_tx.clone();
            let _total = data.len();

            // For simplicity, we emit progress based on the fact the body was created
            // Real chunked progress would require implementing http_body::Body
            let _ = tx.send(90); // Midway through upload

            SdkBody::from(data)
        });

        let byte_stream = ByteStream::new(body);

        let runtime = match Self::create_runtime() {
            Ok(rt) => rt,
            Err(e) => {
                on_progress("error", 0, 0, 0);
                return BackupResult::failure(format!("Failed to create runtime: {}", e));
            }
        };

        let upload_result = runtime.block_on(async {
            // Process progress updates in parallel
            let progress_handler = tokio::spawn(async move {
                while let Some(_percent) = progress_rx.recv().await {
                    // Progress updates are consumed but not emitted here
                    // Real progress would be emitted to on_progress
                }
            });

            let upload_future = client
                .put_object()
                .bucket(&bucket)
                .key(&key)
                .content_length(zip_len as i64)
                .body(byte_stream)
                .send();

            let result =
                tokio::time::timeout(std::time::Duration::from_secs(300), upload_future).await;

            // Stop progress handler
            drop(progress_handler);

            result
        });

        match upload_result {
            Ok(Ok(_)) => {
                log::info!("[S3 Backup] Upload complete!");
                on_progress("complete", 1, 1, 100);
                BackupResult::success(1)
            }
            Ok(Err(e)) => {
                log::error!("[S3 Backup] Upload failed: {}", e);
                on_progress("error", 0, 0, 0);
                BackupResult::failure(format!("S3 upload failed: {}", e))
            }
            Err(_) => {
                log::error!("[S3 Backup] Upload timed out after 5 minutes");
                on_progress("error", 0, 0, 0);
                BackupResult::failure("S3 upload timed out after 5 minutes")
            }
        }
    }
}

impl BackupTarget for S3Target {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn frequency(&self) -> Duration {
        Duration::from_secs(3600) // 1 hour
    }

    fn failure_policy(&self) -> FailurePolicy {
        FailurePolicy::Retry(3)
    }

    fn backup<'a>(
        &'a self,
        _fs: &'a dyn AsyncFileSystem,
        workspace_path: &'a Path,
    ) -> BoxFuture<'a, BackupResult> {
        // Note: This implementation uses RealFileSystem internally since the S3
        // backup uses blocking I/O for zip creation.
        Box::pin(async move {
            self.backup_with_progress(&RealFileSystem, workspace_path, |_, _, _, _| {})
        })
    }

    fn restore<'a>(
        &'a self,
        _fs: &'a dyn AsyncFileSystem,
        workspace_path: &'a Path,
    ) -> BoxFuture<'a, BackupResult> {
        Box::pin(async move {
            // Find latest backup
            let bucket = self.bucket().to_string();
            let prefix = match &self.config.provider {
                CloudProvider::S3 { prefix, .. } => prefix.clone().unwrap_or_default(),
                _ => String::new(),
            };

            let client = self.client.clone();

            let runtime = match Self::create_runtime() {
                Ok(rt) => rt,
                Err(e) => return BackupResult::failure(format!("Failed to create runtime: {}", e)),
            };

            let result = runtime.block_on(async {
                // List objects to find latest
                let list_result = client
                    .list_objects_v2()
                    .bucket(&bucket)
                    .prefix(&prefix)
                    .send()
                    .await
                    .map_err(|e| format!("Failed to list objects: {}", e))?;

                let objects = list_result.contents();
                let latest = objects
                    .iter()
                    .filter(|o| o.key().map(|k| k.ends_with(".zip")).unwrap_or(false))
                    .max_by_key(|o| o.last_modified());

                let key = match latest {
                    Some(obj) => obj.key().ok_or("No key")?.to_string(),
                    None => return Err("No backups found".to_string()),
                };

                // Download the backup
                let get_result = client
                    .get_object()
                    .bucket(&bucket)
                    .key(&key)
                    .send()
                    .await
                    .map_err(|e| format!("Failed to download: {}", e))?;

                let body = get_result
                    .body
                    .collect()
                    .await
                    .map_err(|e| format!("Failed to read body: {}", e))?;

                Ok::<_, String>(body.into_bytes().to_vec())
            });

            let zip_data = match result {
                Ok(data) => data,
                Err(e) => return BackupResult::failure(format!("S3 download failed: {}", e)),
            };

            // Extract zip to workspace
            let cursor = std::io::Cursor::new(zip_data);
            let mut archive = match zip::ZipArchive::new(cursor) {
                Ok(a) => a,
                Err(e) => return BackupResult::failure(format!("Failed to open zip: {}", e)),
            };

            let mut files_restored = 0;
            for i in 0..archive.len() {
                let mut file = match archive.by_index(i) {
                    Ok(f) => f,
                    Err(e) => {
                        return BackupResult::failure(format!("Failed to read zip entry: {}", e));
                    }
                };

                if file.is_dir() {
                    continue;
                }

                let file_path = workspace_path.join(file.name());

                // Create parent directories using std::fs since we're in a sync context
                if let Some(parent) = file_path.parent()
                    && let Err(e) = std::fs::create_dir_all(parent)
                {
                    return BackupResult::failure(format!("Failed to create dir: {}", e));
                }

                // Read file contents
                let mut contents = Vec::new();
                if let Err(e) = file.read_to_end(&mut contents) {
                    return BackupResult::failure(format!("Failed to read file: {}", e));
                }

                // Write to filesystem using std::fs
                if let Err(e) = std::fs::write(&file_path, &contents) {
                    return BackupResult::failure(format!("Failed to write file: {}", e));
                }

                files_restored += 1;
            }

            BackupResult::success(files_restored)
        })
    }

    fn is_available(&self) -> bool {
        let bucket = self.bucket().to_string();
        let client = self.client.clone();

        let runtime = match Self::create_runtime() {
            Ok(rt) => rt,
            Err(_) => return false,
        };

        runtime.block_on(async { client.head_bucket().bucket(&bucket).send().await.is_ok() })
    }

    fn get_last_sync(&self) -> Option<std::time::SystemTime> {
        None // TODO: Track in metadata
    }
}

// ============================================================================
// CloudSyncProvider Implementation (for file-level bidirectional sync)
// ============================================================================

impl CloudSyncProvider for S3Target {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn provider_id(&self) -> String {
        format!("s3:{}", self.bucket())
    }

    fn list_remote_files(&self) -> BoxFuture<'_, Result<Vec<RemoteFileInfo>, String>> {
        Box::pin(async move {
            let bucket = self.bucket().to_string();
            let prefix = self.sync_prefix();
            let client = self.client.clone();

            let mut files = Vec::new();
            let mut continuation_token: Option<String> = None;

            loop {
                let mut request = client.list_objects_v2().bucket(&bucket);

                if !prefix.is_empty() {
                    request = request.prefix(&prefix);
                }

                if let Some(token) = &continuation_token {
                    request = request.continuation_token(token);
                }

                let response = request
                    .send()
                    .await
                    .map_err(|e| format!("Failed to list objects: {}", e))?;

                for object in response.contents() {
                    let key = match object.key() {
                        Some(k) => k,
                        None => continue,
                    };

                    // Only include markdown files, skip backup ZIPs
                    if !key.ends_with(".md") {
                        continue;
                    }

                    // Extract relative path
                    let path = match self.extract_path(key) {
                        Some(p) => p,
                        None => continue,
                    };

                    let modified_at = object
                        .last_modified()
                        .and_then(|t| DateTime::from_timestamp(t.secs(), t.subsec_nanos()))
                        .unwrap_or_else(Utc::now);

                    files.push(RemoteFileInfo {
                        path,
                        size: object.size().unwrap_or(0) as u64,
                        modified_at,
                        etag: object.e_tag().map(|s| s.to_string()),
                        content_hash: None,
                    });
                }

                // Check if there are more results
                if response.is_truncated() == Some(true) {
                    continuation_token = response.next_continuation_token().map(|s| s.to_string());
                } else {
                    break;
                }
            }

            Ok(files)
        })
    }

    fn download_file(
        &self,
        path: &str,
    ) -> BoxFuture<'_, Result<(Vec<u8>, RemoteFileInfo), String>> {
        let path = path.to_string();
        Box::pin(async move {
            let bucket = self.bucket().to_string();
            let key = self.build_key(&path);
            let client = self.client.clone();

            let response = client
                .get_object()
                .bucket(&bucket)
                .key(&key)
                .send()
                .await
                .map_err(|e| format!("Failed to download {}: {}", path, e))?;

            // Extract metadata before consuming the body
            let modified_at = response
                .last_modified()
                .and_then(|t| DateTime::from_timestamp(t.secs(), t.subsec_nanos()))
                .unwrap_or_else(Utc::now);
            let etag = response.e_tag().map(|s| s.to_string());

            let body = response
                .body
                .collect()
                .await
                .map_err(|e| format!("Failed to read body: {}", e))?;

            let bytes = body.into_bytes().to_vec();

            let info = RemoteFileInfo {
                path: path.clone(),
                size: bytes.len() as u64,
                modified_at,
                etag,
                content_hash: None,
            };

            Ok((bytes, info))
        })
    }

    fn upload_file(
        &self,
        path: &str,
        content: &[u8],
    ) -> BoxFuture<'_, Result<RemoteFileInfo, String>> {
        let path = path.to_string();
        let content = content.to_vec();
        Box::pin(async move {
            let bucket = self.bucket().to_string();
            let key = self.build_key(&path);
            let client = self.client.clone();
            let content_len = content.len();

            let body = ByteStream::from(content);

            let response = client
                .put_object()
                .bucket(&bucket)
                .key(&key)
                .content_type("text/markdown; charset=utf-8")
                .content_length(content_len as i64)
                .body(body)
                .send()
                .await
                .map_err(|e| format!("Failed to upload {}: {}", path, e))?;

            let info = RemoteFileInfo {
                path: path.clone(),
                size: content_len as u64,
                modified_at: Utc::now(),
                etag: response.e_tag().map(|s| s.to_string()),
                content_hash: None,
            };

            Ok(info)
        })
    }

    fn delete_remote_file(&self, path: &str) -> BoxFuture<'_, Result<(), String>> {
        let path = path.to_string();
        Box::pin(async move {
            let bucket = self.bucket().to_string();
            let key = self.build_key(&path);
            let client = self.client.clone();

            client
                .delete_object()
                .bucket(&bucket)
                .key(&key)
                .send()
                .await
                .map_err(|e| format!("Failed to delete {}: {}", path, e))?;

            Ok(())
        })
    }

    fn is_available(&self) -> bool {
        BackupTarget::is_available(self)
    }
}
