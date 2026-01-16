//! Google Drive cloud backup target implementation.
//!
//! This module provides both:
//! - ZIP-based backup (existing `BackupTarget` implementation)
//! - File-level sync (new `CloudSyncProvider` implementation)

use chrono::{DateTime, Utc};
use diaryx_core::backup::{BackupResult, BackupTarget, CloudBackupConfig, FailurePolicy};
use diaryx_core::fs::{AsyncFileSystem, BoxFuture, FileSystem, RealFileSystem};
use diaryx_core::sync::RemoteFileInfo;
use diaryx_core::sync::engine::CloudSyncProvider;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

/// Google Drive cloud backup target.
pub struct GoogleDriveTarget {
    config: CloudBackupConfig,
    access_token: String,
    folder_id: Option<String>,
}

impl GoogleDriveTarget {
    /// Create a new Google Drive backup target.
    pub fn new(
        config: CloudBackupConfig,
        access_token: String,
        folder_id: Option<String>,
    ) -> Result<Self, String> {
        Ok(Self {
            config,
            access_token,
            folder_id,
        })
    }

    fn backup_filename(&self) -> String {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        format!("diaryx_backup_{}.zip", timestamp)
    }

    /// Sanitize a filename for use in multipart Content-Disposition header.
    /// Removes or replaces characters that could break multipart encoding.
    fn sanitize_filename_for_multipart(filename: &str) -> String {
        // Replace problematic characters that break multipart encoding:
        // - Quotes (") break the Content-Disposition header parsing
        // - Backslashes (\) are escape characters
        // - Carriage returns and newlines break headers
        filename
            .replace('"', "'") // Replace double quotes with single quotes
            .replace('\\', "_") // Replace backslashes with underscores
            .replace(['\r', '\n'], "") // Remove newlines
    }

    fn create_zip_archive_with_progress<F>(
        &self,
        fs: &dyn FileSystem,
        workspace_path: &Path,
        mut on_progress: F,
    ) -> Result<Vec<u8>, String>
    where
        F: FnMut(usize, usize, u8),
    {
        let mut buffer = Vec::new();
        {
            let mut zip = ZipWriter::new(std::io::Cursor::new(&mut buffer));
            let options = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .compression_level(Some(6));

            let entries = fs
                .list_all_files_recursive(workspace_path)
                .map_err(|e| format!("Failed to list files: {}", e))?;

            // Filter to markdown files and attachments, excluding hidden files/folders
            let files: Vec<_> = entries
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
            for (i, file_path) in files.iter().enumerate() {
                let relative_path = file_path
                    .strip_prefix(workspace_path)
                    .unwrap_or(file_path)
                    .to_string_lossy()
                    .to_string();

                let percent = 10 + ((i * 70) / total_files.max(1)) as u8;
                if i % 100 == 0 {
                    on_progress(i, total_files, percent);
                }

                zip.start_file(&relative_path, options)
                    .map_err(|e| format!("Failed to start file in zip: {}", e))?;

                let content = if let Ok(binary) = fs.read_binary(file_path) {
                    binary
                } else if let Ok(text) = fs.read_to_string(file_path) {
                    text.into_bytes()
                } else {
                    log::warn!(
                        "[Google Drive] Skipping file: {}. Could not read.",
                        file_path.display()
                    );
                    continue;
                };

                zip.write_all(&content)
                    .map_err(|e| format!("Failed to write to zip: {}", e))?;
            }

            zip.finish()
                .map_err(|e| format!("Failed to finish zip: {}", e))?;
        }

        Ok(buffer)
    }

    async fn upload_to_drive(&self, filename: &str, data: Vec<u8>) -> Result<String, String> {
        let client = reqwest::Client::new();

        let mut metadata = serde_json::json!({
            "name": filename,
            "mimeType": "application/zip"
        });

        if let Some(ref folder_id) = self.folder_id {
            metadata["parents"] = serde_json::json!([folder_id]);
        }

        let metadata_part = reqwest::multipart::Part::text(metadata.to_string())
            .mime_str("application/json")
            .map_err(|e| format!("Failed to create metadata part: {}", e))?;

        let file_part = reqwest::multipart::Part::bytes(data)
            .file_name(filename.to_string())
            .mime_str("application/zip")
            .map_err(|e| format!("Failed to create file part: {}", e))?;

        let form = reqwest::multipart::Form::new()
            .part("metadata", metadata_part)
            .part("file", file_part);

        let response = client
            .post("https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart")
            .bearer_auth(&self.access_token)
            .multipart(form)
            .send()
            .await
            .map_err(|e| format!("Failed to upload to Drive: {}", e))?;

        if response.status().is_success() {
            let result: serde_json::Value = response
                .json()
                .await
                .map_err(|e| format!("Failed to parse response: {}", e))?;
            let file_id = result["id"].as_str().unwrap_or("unknown").to_string();
            log::info!("[Google Drive] Upload complete! File ID: {}", file_id);
            Ok(file_id)
        } else {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            Err(format!("Drive API error {}: {}", status, error_text))
        }
    }

    pub fn backup_with_progress<F>(
        &self,
        fs: &dyn FileSystem,
        workspace_path: &Path,
        mut on_progress: F,
    ) -> BackupResult
    where
        F: FnMut(&str, usize, usize, u8),
    {
        on_progress("preparing", 0, 0, 5);

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
        log::info!("[Google Drive] Zip size: {:.2} MB", zip_size_mb);

        on_progress("zipping", 1, 1, 80);

        let filename = self.backup_filename();
        on_progress("uploading", 0, 1, 85);

        // Create a temporary runtime for the blocking upload
        // This runtime is created and dropped within this sync context, avoiding the panic
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                on_progress("error", 0, 0, 0);
                return BackupResult::failure(format!("Failed to create runtime: {}", e));
            }
        };

        let result = runtime.block_on(async {
            tokio::time::timeout(
                Duration::from_secs(300),
                self.upload_to_drive(&filename, zip_data),
            )
            .await
        });

        // Explicitly drop the runtime in this sync context
        drop(runtime);

        match result {
            Ok(Ok(_file_id)) => {
                on_progress("complete", 1, 1, 100);
                BackupResult::success(1)
            }
            Ok(Err(e)) => {
                on_progress("error", 0, 0, 0);
                BackupResult::failure(e)
            }
            Err(_) => {
                on_progress("error", 0, 0, 0);
                BackupResult::failure("Upload timed out after 5 minutes")
            }
        }
    }
}

impl BackupTarget for GoogleDriveTarget {
    fn is_available(&self) -> bool {
        !self.access_token.is_empty()
    }

    fn name(&self) -> &str {
        &self.config.name
    }

    fn frequency(&self) -> Duration {
        Duration::from_secs(3600)
    }

    fn failure_policy(&self) -> FailurePolicy {
        FailurePolicy::Retry(3)
    }

    fn backup<'a>(
        &'a self,
        _fs: &'a dyn AsyncFileSystem,
        workspace_path: &'a Path,
    ) -> BoxFuture<'a, BackupResult> {
        Box::pin(async move {
            self.backup_with_progress(&RealFileSystem, workspace_path, |_, _, _, _| {})
        })
    }

    fn restore<'a>(
        &'a self,
        _fs: &'a dyn AsyncFileSystem,
        _workspace_path: &'a Path,
    ) -> BoxFuture<'a, BackupResult> {
        Box::pin(async move { BackupResult::failure("Google Drive restore not yet implemented") })
    }
}

// ============================================================================
// CloudSyncProvider Implementation (for file-level bidirectional sync)
// ============================================================================

impl GoogleDriveTarget {
    /// Get or create the sync folder within the configured folder.
    /// Returns the folder ID for the sync folder.
    async fn get_or_create_sync_folder(&self) -> Result<String, String> {
        let parent_id = self.folder_id.clone();
        self.get_or_create_folder("diaryx-sync", parent_id.as_deref())
            .await
    }

    /// Get or create a folder by name within a parent folder.
    async fn get_or_create_folder(
        &self,
        name: &str,
        parent_id: Option<&str>,
    ) -> Result<String, String> {
        let client = reqwest::Client::new();

        // Check if folder exists
        let parent_query = match parent_id {
            Some(id) => format!("'{}' in parents and ", id),
            None => String::new(),
        };

        let query = format!(
            "{}name = '{}' and mimeType = 'application/vnd.google-apps.folder' and trashed = false",
            parent_query,
            name.replace("'", "\\'")
        );

        let response = client
            .get("https://www.googleapis.com/drive/v3/files")
            .bearer_auth(&self.access_token)
            .query(&[("q", &query), ("fields", &"files(id,name)".to_string())])
            .send()
            .await
            .map_err(|e| format!("Failed to search for folder: {}", e))?;

        if !response.status().is_success() {
            let error = response.text().await.unwrap_or_default();
            return Err(format!("Drive API error: {}", error));
        }

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        if let Some(files) = result["files"].as_array()
            && let Some(folder) = files.first()
                && let Some(id) = folder["id"].as_str() {
                    return Ok(id.to_string());
                }

        // Create folder
        let mut metadata = serde_json::json!({
            "name": name,
            "mimeType": "application/vnd.google-apps.folder"
        });

        if let Some(pid) = parent_id {
            metadata["parents"] = serde_json::json!([pid]);
        }

        let response = client
            .post("https://www.googleapis.com/drive/v3/files")
            .bearer_auth(&self.access_token)
            .json(&metadata)
            .send()
            .await
            .map_err(|e| format!("Failed to create folder: {}", e))?;

        if !response.status().is_success() {
            let error = response.text().await.unwrap_or_default();
            return Err(format!("Failed to create folder: {}", error));
        }

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        result["id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "No folder ID in response".to_string())
    }

    /// Get or create nested folder path, returning the final folder ID.
    /// e.g., "2024/01" creates "2024" folder, then "01" inside it.
    async fn get_or_create_folder_path(
        &self,
        sync_folder_id: &str,
        path: &str,
    ) -> Result<String, String> {
        let path_obj = Path::new(path);
        let parent_path = path_obj.parent();

        // If no parent directory, return the sync folder
        let parent_path = match parent_path {
            Some(p) if !p.as_os_str().is_empty() => p,
            _ => return Ok(sync_folder_id.to_string()),
        };

        // Create each folder in the path
        let mut current_parent_id = sync_folder_id.to_string();
        for component in parent_path.components() {
            if let std::path::Component::Normal(name) = component
                && let Some(name_str) = name.to_str() {
                    current_parent_id = self
                        .get_or_create_folder(name_str, Some(&current_parent_id))
                        .await?;
                }
        }

        Ok(current_parent_id)
    }

    /// Find a file by exact path within the sync folder structure.
    async fn find_file_by_path(
        &self,
        sync_folder_id: &str,
        path: &str,
    ) -> Result<Option<String>, String> {
        let client = reqwest::Client::new();

        // Navigate to the correct parent folder
        let parent_folder_id = self.get_or_create_folder_path(sync_folder_id, path).await?;

        let file_name = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path);

        let query = format!(
            "'{}' in parents and name = '{}' and trashed = false",
            parent_folder_id,
            file_name.replace("'", "\\'")
        );

        let response = client
            .get("https://www.googleapis.com/drive/v3/files")
            .bearer_auth(&self.access_token)
            .query(&[
                ("q", &query),
                ("fields", &"files(id,name,modifiedTime)".to_string()),
            ])
            .send()
            .await
            .map_err(|e| format!("Failed to search for file: {}", e))?;

        if !response.status().is_success() {
            let error = response.text().await.unwrap_or_default();
            return Err(format!("Drive API error: {}", error));
        }

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        if let Some(files) = result["files"].as_array()
            && let Some(file) = files.first()
                && let Some(id) = file["id"].as_str() {
                    return Ok(Some(id.to_string()));
                }

        Ok(None)
    }

    /// Recursively list all files in a folder, returning full paths.
    fn list_files_recursive<'a>(
        &'a self,
        folder_id: &'a str,
        prefix: &'a str,
    ) -> BoxFuture<'a, Result<Vec<RemoteFileInfo>, String>> {
        Box::pin(async move {
            let client = reqwest::Client::new();
            let mut all_files = Vec::new();

            // List items in this folder
            let query = format!("'{}' in parents and trashed = false", folder_id);

            let response = client
                .get("https://www.googleapis.com/drive/v3/files")
                .bearer_auth(&self.access_token)
                .query(&[
                    ("q", query),
                    (
                        "fields",
                        "files(id,name,size,modifiedTime,md5Checksum,mimeType)".to_string(),
                    ),
                    ("pageSize", "1000".to_string()),
                ])
                .send()
                .await
                .map_err(|e| format!("Failed to list files: {}", e))?;

            if !response.status().is_success() {
                let error = response.text().await.unwrap_or_default();
                return Err(format!("Drive API error: {}", error));
            }

            let result: serde_json::Value = response
                .json()
                .await
                .map_err(|e| format!("Failed to parse response: {}", e))?;

            if let Some(files) = result["files"].as_array() {
                for file in files {
                    let name = file["name"].as_str().unwrap_or("").to_string();

                    // Skip hidden files and folders (starting with '.')
                    if name.starts_with('.') {
                        continue;
                    }

                    let mime_type = file["mimeType"].as_str().unwrap_or("");
                    let full_path = if prefix.is_empty() {
                        name.clone()
                    } else {
                        format!("{}/{}", prefix, name)
                    };

                    if mime_type == "application/vnd.google-apps.folder" {
                        // Recursively list subfolder
                        if let Some(id) = file["id"].as_str() {
                            let subfolder_files = self.list_files_recursive(id, &full_path).await?;
                            all_files.extend(subfolder_files);
                        }
                    } else {
                        // Sync markdown files and files in _attachments folders
                        let is_markdown = name.ends_with(".md");
                        let is_attachment = full_path.split('/').any(|c| c == "_attachments");

                        if is_markdown || is_attachment {
                            let modified_at = file["modifiedTime"]
                                .as_str()
                                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                                .map(|dt| dt.with_timezone(&Utc))
                                .unwrap_or_else(Utc::now);

                            all_files.push(RemoteFileInfo {
                                path: full_path,
                                size: file["size"]
                                    .as_str()
                                    .and_then(|s| s.parse().ok())
                                    .unwrap_or(0),
                                modified_at,
                                etag: file["id"].as_str().map(|s| s.to_string()),
                                content_hash: file["md5Checksum"].as_str().map(|s| s.to_string()),
                            });
                        }
                    }
                }
            }

            Ok(all_files)
        })
    }
}

impl CloudSyncProvider for GoogleDriveTarget {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn provider_id(&self) -> String {
        format!("gdrive:{}", self.folder_id.as_deref().unwrap_or("root"))
    }

    fn list_remote_files(&self) -> BoxFuture<'_, Result<Vec<RemoteFileInfo>, String>> {
        Box::pin(async move {
            let sync_folder_id = self.get_or_create_sync_folder().await?;
            // Recursively list all files with their full paths
            self.list_files_recursive(&sync_folder_id, "").await
        })
    }

    fn download_file(
        &self,
        path: &str,
    ) -> BoxFuture<'_, Result<(Vec<u8>, RemoteFileInfo), String>> {
        let path = path.to_string();
        Box::pin(async move {
            let sync_folder_id = self.get_or_create_sync_folder().await?;

            let file_id = self
                .find_file_by_path(&sync_folder_id, &path)
                .await?
                .ok_or_else(|| format!("File not found: {}", path))?;

            let client = reqwest::Client::new();

            // Get file metadata first
            let meta_response = client
                .get(format!(
                    "https://www.googleapis.com/drive/v3/files/{}",
                    file_id
                ))
                .bearer_auth(&self.access_token)
                .query(&[("fields", "id,name,size,modifiedTime,md5Checksum")])
                .send()
                .await
                .map_err(|e| format!("Failed to get file metadata: {}", e))?;

            let meta: serde_json::Value = meta_response
                .json()
                .await
                .map_err(|e| format!("Failed to parse metadata: {}", e))?;

            let modified_at = meta["modifiedTime"]
                .as_str()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(Utc::now);

            // Download content
            let response = client
                .get(format!(
                    "https://www.googleapis.com/drive/v3/files/{}",
                    file_id
                ))
                .bearer_auth(&self.access_token)
                .query(&[("alt", "media")])
                .send()
                .await
                .map_err(|e| format!("Failed to download file: {}", e))?;

            if !response.status().is_success() {
                let error = response.text().await.unwrap_or_default();
                return Err(format!("Download failed: {}", error));
            }

            let bytes = response
                .bytes()
                .await
                .map_err(|e| format!("Failed to read response: {}", e))?
                .to_vec();

            let info = RemoteFileInfo {
                path,
                size: bytes.len() as u64,
                modified_at,
                etag: Some(file_id),
                content_hash: meta["md5Checksum"].as_str().map(|s| s.to_string()),
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
            let sync_folder_id = self.get_or_create_sync_folder().await?;
            let client = reqwest::Client::new();

            // Check if file already exists
            let existing_id = self.find_file_by_path(&sync_folder_id, &path).await?;

            let file_name = Path::new(&path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&path);

            // Sanitize filename for multipart header (quotes break encoding)
            let safe_file_name = Self::sanitize_filename_for_multipart(file_name);

            let (file_id, modified_at) = if let Some(id) = existing_id {
                // Update existing file
                let metadata_part = reqwest::multipart::Part::text("{}")
                    .mime_str("application/json")
                    .map_err(|e| format!("Failed to create metadata part: {}", e))?;

                let file_part = reqwest::multipart::Part::bytes(content.clone())
                    .file_name(safe_file_name.clone())
                    .mime_str("text/markdown; charset=utf-8")
                    .map_err(|e| format!("Failed to create file part: {}", e))?;

                let form = reqwest::multipart::Form::new()
                    .part("metadata", metadata_part)
                    .part("file", file_part);

                let response = client
                    .patch(format!(
                        "https://www.googleapis.com/upload/drive/v3/files/{}?uploadType=multipart",
                        id
                    ))
                    .bearer_auth(&self.access_token)
                    .multipart(form)
                    .send()
                    .await
                    .map_err(|e| format!("Failed to update file: {}", e))?;

                if !response.status().is_success() {
                    let error = response.text().await.unwrap_or_default();
                    return Err(format!("Update failed: {}", error));
                }

                let result: serde_json::Value = response
                    .json()
                    .await
                    .map_err(|e| format!("Failed to parse response: {}", e))?;

                let modified = result["modifiedTime"]
                    .as_str()
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(Utc::now);

                (id, modified)
            } else {
                // Create parent folders if needed
                let parent_folder_id = self
                    .get_or_create_folder_path(&sync_folder_id, &path)
                    .await?;

                // Create new file in the correct folder
                let metadata = serde_json::json!({
                    "name": file_name,
                    "parents": [parent_folder_id]
                });

                let metadata_part = reqwest::multipart::Part::text(metadata.to_string())
                    .mime_str("application/json")
                    .map_err(|e| format!("Failed to create metadata part: {}", e))?;

                let file_part = reqwest::multipart::Part::bytes(content.clone())
                    .file_name(safe_file_name.clone())
                    .mime_str("text/markdown; charset=utf-8")
                    .map_err(|e| format!("Failed to create file part: {}", e))?;

                let form = reqwest::multipart::Form::new()
                    .part("metadata", metadata_part)
                    .part("file", file_part);

                let response = client
                    .post("https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart")
                    .bearer_auth(&self.access_token)
                    .multipart(form)
                    .send()
                    .await
                    .map_err(|e| format!("Failed to upload file: {}", e))?;

                if !response.status().is_success() {
                    let error = response.text().await.unwrap_or_default();
                    return Err(format!("Upload failed: {}", error));
                }

                let result: serde_json::Value = response
                    .json()
                    .await
                    .map_err(|e| format!("Failed to parse response: {}", e))?;

                let id = result["id"]
                    .as_str()
                    .ok_or("No file ID in response")?
                    .to_string();

                (id, Utc::now())
            };

            Ok(RemoteFileInfo {
                path,
                size: content.len() as u64,
                modified_at,
                etag: Some(file_id),
                content_hash: None,
            })
        })
    }

    fn delete_remote_file(&self, path: &str) -> BoxFuture<'_, Result<(), String>> {
        let path = path.to_string();
        Box::pin(async move {
            let sync_folder_id = self.get_or_create_sync_folder().await?;

            let file_id = match self.find_file_by_path(&sync_folder_id, &path).await? {
                Some(id) => id,
                None => return Ok(()), // File doesn't exist, nothing to delete
            };

            let client = reqwest::Client::new();

            let response = client
                .delete(format!(
                    "https://www.googleapis.com/drive/v3/files/{}",
                    file_id
                ))
                .bearer_auth(&self.access_token)
                .send()
                .await
                .map_err(|e| format!("Failed to delete file: {}", e))?;

            if !response.status().is_success()
                && response.status() != reqwest::StatusCode::NOT_FOUND
            {
                let error = response.text().await.unwrap_or_default();
                return Err(format!("Delete failed: {}", error));
            }

            Ok(())
        })
    }

    fn is_available(&self) -> bool {
        BackupTarget::is_available(self)
    }
}
