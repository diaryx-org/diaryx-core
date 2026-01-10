//! Google Drive cloud backup target implementation.

use diaryx_core::backup::{BackupResult, BackupTarget, CloudBackupConfig, FailurePolicy};
use diaryx_core::fs::{AsyncFileSystem, BoxFuture, FileSystem, RealFileSystem};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

/// Google Drive cloud backup target.
pub struct GoogleDriveTarget {
    config: CloudBackupConfig,
    runtime: Arc<Runtime>,
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
        let runtime =
            Arc::new(Runtime::new().map_err(|e| format!("Failed to create tokio runtime: {}", e))?);

        Ok(Self {
            config,
            runtime,
            access_token,
            folder_id,
        })
    }

    fn backup_filename(&self) -> String {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        format!("diaryx_backup_{}.zip", timestamp)
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

            let files: Vec<_> = entries.into_iter().filter(|p| !fs.is_dir(p)).collect();

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

        let result = self.runtime.block_on(async {
            tokio::time::timeout(
                Duration::from_secs(300),
                self.upload_to_drive(&filename, zip_data),
            )
            .await
        });

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
