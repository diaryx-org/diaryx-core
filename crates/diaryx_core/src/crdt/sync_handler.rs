//! Sync handler for processing remote CRDT updates.
//!
//! This module provides `SyncHandler`, which handles the side effects of remote
//! CRDT updates including writing files to disk with merged metadata. It serves
//! as the single source of truth for sync logic, replacing TypeScript-side processing.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::body_doc_manager::BodyDocManager;
use super::types::FileMetadata;
use crate::error::Result;
use crate::fs::{AsyncFileSystem, FileSystemEvent};
use crate::metadata_writer;

/// Configuration for guest mode sync.
///
/// Guests need special path handling to isolate their storage from the host.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct GuestConfig {
    /// The join code for the share session.
    pub join_code: String,

    /// If true, prefix paths with guest/{join_code}/ for OPFS storage.
    /// If false (in-memory storage), paths are used as-is.
    pub uses_opfs: bool,
}

/// Handler for sync side effects.
///
/// The SyncHandler is responsible for processing remote CRDT updates and
/// performing the necessary disk writes. It handles:
/// - Writing updated file metadata and body to disk after remote updates
/// - Merging CRDT metadata with disk metadata (CRDT wins, disk as fallback)
/// - Guest path prefixing for isolated storage
/// - Emitting FileSystemEvents for UI updates
pub struct SyncHandler<FS: AsyncFileSystem> {
    fs: FS,
    /// Guest configuration, if operating in guest mode.
    guest_config: RwLock<Option<GuestConfig>>,
    /// Optional callback for emitting filesystem events.
    event_callback: Option<Box<dyn Fn(&FileSystemEvent) + Send + Sync>>,
}

impl<FS: AsyncFileSystem> SyncHandler<FS> {
    /// Create a new SyncHandler with the given filesystem.
    pub fn new(fs: FS) -> Self {
        Self {
            fs,
            guest_config: RwLock::new(None),
            event_callback: None,
        }
    }

    /// Set the event callback for emitting filesystem events.
    pub fn set_event_callback(&mut self, callback: Box<dyn Fn(&FileSystemEvent) + Send + Sync>) {
        self.event_callback = Some(callback);
    }

    /// Configure guest mode.
    ///
    /// In guest mode, storage paths are prefixed with `guest/{join_code}/`
    /// when using OPFS, or used as-is for in-memory storage.
    pub fn configure_guest(&self, config: Option<GuestConfig>) {
        let mut gc = self.guest_config.write().unwrap();
        *gc = config;
    }

    /// Check if we're in guest mode.
    pub fn is_guest(&self) -> bool {
        self.guest_config.read().unwrap().is_some()
    }

    /// Get the storage path for a canonical path.
    ///
    /// For guests using OPFS: prefixes with `guest/{join_code}/`
    /// For guests using in-memory storage or hosts: returns the path as-is
    pub fn get_storage_path(&self, canonical_path: &str) -> PathBuf {
        let gc = self.guest_config.read().unwrap();
        match &*gc {
            Some(config) if config.uses_opfs => {
                PathBuf::from(format!("guest/{}/{}", config.join_code, canonical_path))
            }
            _ => PathBuf::from(canonical_path),
        }
    }

    /// Get the canonical path from a storage path.
    ///
    /// Strips the `guest/{join_code}/` prefix if present for OPFS guests.
    pub fn get_canonical_path(&self, storage_path: &str) -> String {
        let gc = self.guest_config.read().unwrap();
        match &*gc {
            Some(config) if config.uses_opfs => {
                let prefix = format!("guest/{}/", config.join_code);
                if storage_path.starts_with(&prefix) {
                    storage_path[prefix.len()..].to_string()
                } else {
                    storage_path.to_string()
                }
            }
            _ => storage_path.to_string(),
        }
    }

    /// Emit a filesystem event to the registered callback.
    fn emit_event(&self, event: FileSystemEvent) {
        if let Some(ref cb) = self.event_callback {
            cb(&event);
        }
    }

    /// Handle remote metadata updates by writing files to disk.
    ///
    /// This method processes a list of updated files from a remote sync and:
    /// 1. For each file, merges CRDT metadata with disk metadata
    /// 2. Gets body content from the BodyDocManager
    /// 3. Writes the file to disk with merged frontmatter
    /// 4. Emits appropriate FileSystemEvents
    ///
    /// Files marked as deleted are removed from disk.
    ///
    /// # Arguments
    /// * `files` - List of (canonical_path, metadata) tuples from the CRDT
    /// * `body_manager` - Manager for per-file body CRDTs
    /// * `write_to_disk` - If true, perform disk writes; if false, only emit events
    pub async fn handle_remote_metadata_update(
        &self,
        files: Vec<(String, FileMetadata)>,
        body_manager: Option<&BodyDocManager>,
        write_to_disk: bool,
    ) -> Result<usize> {
        let mut synced_count = 0;

        for (canonical_path, crdt_metadata) in files {
            let storage_path = self.get_storage_path(&canonical_path);

            if crdt_metadata.deleted {
                // File was deleted - remove from filesystem
                if write_to_disk && self.fs.exists(&storage_path).await {
                    log::debug!("SyncHandler: Deleting file from disk: {:?}", storage_path);
                    if let Err(e) = self.fs.delete_file(&storage_path).await {
                        log::warn!(
                            "SyncHandler: Failed to delete file {:?}: {}",
                            storage_path,
                            e
                        );
                    } else {
                        self.emit_event(FileSystemEvent::file_deleted(storage_path.clone()));
                        synced_count += 1;
                    }
                }
            } else {
                // File exists - merge metadata and write to disk
                // Use get_or_create to ensure the body doc is loaded from storage if it exists,
                // or created fresh if it doesn't. This fixes the bug where body docs that exist
                // in storage but aren't loaded in memory would return empty string.
                let body = if let Some(manager) = body_manager {
                    let doc = manager.get_or_create(&canonical_path);
                    doc.get_body()
                } else {
                    String::new()
                };

                // Try to read existing disk content for metadata merging
                let final_metadata = if write_to_disk && self.fs.exists(&storage_path).await {
                    match self.read_disk_frontmatter(&storage_path).await {
                        Ok(disk_fm) => self.merge_metadata(&crdt_metadata, Some(&disk_fm)),
                        Err(_) => crdt_metadata.clone(),
                    }
                } else {
                    crdt_metadata.clone()
                };

                // Preserve disk body if CRDT body is empty and disk has content
                let final_body = if body.is_empty() && write_to_disk {
                    match self.read_disk_body(&storage_path).await {
                        Ok(disk_body) if !disk_body.is_empty() => {
                            log::debug!(
                                "SyncHandler: Preserving disk body for {} ({} chars)",
                                canonical_path,
                                disk_body.len()
                            );
                            disk_body
                        }
                        _ => body,
                    }
                } else {
                    body
                };

                // Write file to disk
                if write_to_disk {
                    let metadata_json = serde_json::to_value(&final_metadata)
                        .unwrap_or(serde_json::Value::Object(Default::default()));

                    if let Err(e) = metadata_writer::write_file_with_metadata(
                        &self.fs,
                        &storage_path,
                        &metadata_json,
                        &final_body,
                    )
                    .await
                    {
                        log::warn!(
                            "SyncHandler: Failed to write file {:?}: {}",
                            storage_path,
                            e
                        );
                        continue;
                    }

                    log::debug!("SyncHandler: Wrote file to disk: {:?}", storage_path);
                }

                // Emit appropriate event
                let metadata_json = serde_json::to_value(&final_metadata).ok();
                self.emit_event(FileSystemEvent::file_created_with_metadata(
                    storage_path,
                    metadata_json,
                    None,
                ));

                synced_count += 1;
            }
        }

        Ok(synced_count)
    }

    /// Handle a remote body update by writing the body to disk.
    ///
    /// # Arguments
    /// * `canonical_path` - The canonical path of the file
    /// * `body` - The new body content
    /// * `crdt_metadata` - Optional metadata to use for the frontmatter
    pub async fn handle_remote_body_update(
        &self,
        canonical_path: &str,
        body: &str,
        crdt_metadata: Option<&FileMetadata>,
    ) -> Result<()> {
        let storage_path = self.get_storage_path(canonical_path);

        // Get or construct metadata for frontmatter
        let metadata = if let Some(m) = crdt_metadata {
            m.clone()
        } else if self.fs.exists(&storage_path).await {
            // Try to read existing frontmatter
            match self.read_disk_frontmatter(&storage_path).await {
                Ok(disk_fm) => disk_fm,
                Err(_) => FileMetadata::default(),
            }
        } else {
            FileMetadata::default()
        };

        // Merge with disk metadata if CRDT metadata provided
        let final_metadata = if crdt_metadata.is_some() && self.fs.exists(&storage_path).await {
            match self.read_disk_frontmatter(&storage_path).await {
                Ok(disk_fm) => self.merge_metadata(&metadata, Some(&disk_fm)),
                Err(_) => metadata,
            }
        } else {
            metadata
        };

        let metadata_json = serde_json::to_value(&final_metadata)
            .unwrap_or(serde_json::Value::Object(Default::default()));

        metadata_writer::write_file_with_metadata(&self.fs, &storage_path, &metadata_json, body)
            .await?;

        // Emit contents changed event
        self.emit_event(FileSystemEvent::contents_changed(
            storage_path,
            body.to_string(),
        ));

        Ok(())
    }

    /// Merge CRDT metadata with disk metadata.
    ///
    /// CRDT values take precedence. Disk values are used as fallback only when
    /// CRDT values are `None` (not set). An explicitly set empty array `Some([])`
    /// is NOT replaced with disk values, as this represents an intentional deletion.
    pub fn merge_metadata(&self, crdt: &FileMetadata, disk: Option<&FileMetadata>) -> FileMetadata {
        let disk = match disk {
            Some(d) => d,
            None => return crdt.clone(),
        };

        FileMetadata {
            title: crdt.title.clone().or_else(|| disk.title.clone()),
            part_of: crdt.part_of.clone().or_else(|| disk.part_of.clone()),
            // Only fall back to disk if crdt.contents is None (not set).
            // Some([]) means explicitly cleared and should not be overwritten.
            contents: match &crdt.contents {
                None => disk.contents.clone(),
                Some(_) => crdt.contents.clone(),
            },
            attachments: if crdt.attachments.is_empty() {
                disk.attachments.clone()
            } else {
                crdt.attachments.clone()
            },
            deleted: crdt.deleted,
            // Only fall back to disk if crdt.audience is None (not set).
            // Some([]) means explicitly cleared and should not be overwritten.
            audience: match &crdt.audience {
                None => disk.audience.clone(),
                Some(_) => crdt.audience.clone(),
            },
            description: crdt
                .description
                .clone()
                .or_else(|| disk.description.clone()),
            extra: if crdt.extra.is_empty() {
                disk.extra.clone()
            } else {
                crdt.extra.clone()
            },
            modified_at: crdt.modified_at,
        }
    }

    /// Read frontmatter from a disk file and convert to FileMetadata.
    async fn read_disk_frontmatter(&self, path: &Path) -> Result<FileMetadata> {
        let content = self.fs.read_to_string(path).await?;
        let parsed = crate::frontmatter::parse_or_empty(&content)?;

        // Convert IndexMap<String, Value> to FileMetadata
        let fm = &parsed.frontmatter;

        Ok(FileMetadata {
            title: fm.get("title").and_then(|v| v.as_str()).map(String::from),
            part_of: fm.get("part_of").and_then(|v| v.as_str()).map(String::from),
            contents: fm.get("contents").and_then(|v| {
                v.as_sequence().map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
            }),
            attachments: fm
                .get("attachments")
                .and_then(|v| {
                    v.as_sequence().map(|seq| {
                        seq.iter()
                            .filter_map(|v| {
                                // Handle both string and object formats
                                if let Some(s) = v.as_str() {
                                    Some(super::types::BinaryRef {
                                        path: s.to_string(),
                                        source: "local".to_string(),
                                        hash: String::new(),
                                        mime_type: String::new(),
                                        size: 0,
                                        uploaded_at: None,
                                        deleted: false,
                                    })
                                } else {
                                    None
                                }
                            })
                            .collect()
                    })
                })
                .unwrap_or_default(),
            deleted: fm.get("deleted").and_then(|v| v.as_bool()).unwrap_or(false),
            audience: fm.get("audience").and_then(|v| {
                v.as_sequence().map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
            }),
            description: fm
                .get("description")
                .and_then(|v| v.as_str())
                .map(String::from),
            extra: std::collections::HashMap::new(), // TODO: Parse extra fields
            modified_at: chrono::Utc::now().timestamp_millis(),
        })
    }

    /// Read body content from a disk file.
    async fn read_disk_body(&self, path: &Path) -> Result<String> {
        let content = self.fs.read_to_string(path).await?;
        let parsed = crate::frontmatter::parse_or_empty(&content)?;
        Ok(parsed.body)
    }
}

impl<FS: AsyncFileSystem> std::fmt::Debug for SyncHandler<FS> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let gc = self.guest_config.read().unwrap();
        f.debug_struct("SyncHandler")
            .field("guest_config", &*gc)
            .field("has_event_callback", &self.event_callback.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crdt::types::BinaryRef;
    use crate::fs::SyncToAsyncFs;

    // Use SyncToAsyncFs wrapper which provides AsyncFileSystem for any SyncFileSystem
    type TestFs = SyncToAsyncFs<crate::fs::RealFileSystem>;

    fn create_test_handler() -> SyncHandler<TestFs> {
        SyncHandler::new(SyncToAsyncFs::new(crate::fs::RealFileSystem))
    }

    #[test]
    fn test_get_storage_path_host() {
        let handler = create_test_handler();

        // Host mode - no prefix
        let path = handler.get_storage_path("notes/hello.md");
        assert_eq!(path, PathBuf::from("notes/hello.md"));
    }

    #[test]
    fn test_get_storage_path_guest_opfs() {
        let handler = create_test_handler();

        handler.configure_guest(Some(GuestConfig {
            join_code: "ABC123".to_string(),
            uses_opfs: true,
        }));

        let path = handler.get_storage_path("notes/hello.md");
        assert_eq!(path, PathBuf::from("guest/ABC123/notes/hello.md"));
    }

    #[test]
    fn test_get_storage_path_guest_memory() {
        let handler = create_test_handler();

        handler.configure_guest(Some(GuestConfig {
            join_code: "ABC123".to_string(),
            uses_opfs: false, // In-memory, no prefix
        }));

        let path = handler.get_storage_path("notes/hello.md");
        assert_eq!(path, PathBuf::from("notes/hello.md"));
    }

    #[test]
    fn test_get_canonical_path_guest_opfs() {
        let handler = create_test_handler();

        handler.configure_guest(Some(GuestConfig {
            join_code: "ABC123".to_string(),
            uses_opfs: true,
        }));

        let canonical = handler.get_canonical_path("guest/ABC123/notes/hello.md");
        assert_eq!(canonical, "notes/hello.md");

        // Path without prefix should be returned as-is
        let canonical = handler.get_canonical_path("notes/hello.md");
        assert_eq!(canonical, "notes/hello.md");
    }

    #[test]
    fn test_merge_metadata_crdt_wins() {
        let handler = create_test_handler();

        let crdt = FileMetadata {
            title: Some("CRDT Title".to_string()),
            description: Some("CRDT Desc".to_string()),
            ..Default::default()
        };

        let disk = FileMetadata {
            title: Some("Disk Title".to_string()),
            description: Some("Disk Desc".to_string()),
            part_of: Some("parent.md".to_string()),
            ..Default::default()
        };

        let merged = handler.merge_metadata(&crdt, Some(&disk));

        // CRDT values should win
        assert_eq!(merged.title, Some("CRDT Title".to_string()));
        assert_eq!(merged.description, Some("CRDT Desc".to_string()));
        // Disk fallback for missing CRDT values
        assert_eq!(merged.part_of, Some("parent.md".to_string()));
    }

    #[test]
    fn test_merge_metadata_disk_fallback_for_nulls() {
        let handler = create_test_handler();

        let crdt = FileMetadata {
            title: None,
            description: None,
            contents: None,
            ..Default::default()
        };

        let disk = FileMetadata {
            title: Some("Disk Title".to_string()),
            description: Some("Disk Desc".to_string()),
            contents: Some(vec!["child.md".to_string()]),
            ..Default::default()
        };

        let merged = handler.merge_metadata(&crdt, Some(&disk));

        // Disk values should be used as fallback
        assert_eq!(merged.title, Some("Disk Title".to_string()));
        assert_eq!(merged.description, Some("Disk Desc".to_string()));
        assert_eq!(merged.contents, Some(vec!["child.md".to_string()]));
    }

    #[test]
    fn test_merge_metadata_explicit_empty_array_not_overwritten() {
        let handler = create_test_handler();

        let crdt = FileMetadata {
            contents: Some(vec![]), // Explicitly cleared array
            attachments: vec![],    // Empty attachments (falls back to disk)
            ..Default::default()
        };

        let disk = FileMetadata {
            contents: Some(vec!["child.md".to_string()]),
            attachments: vec![BinaryRef {
                path: "image.png".to_string(),
                source: "local".to_string(),
                hash: "abc".to_string(),
                mime_type: "image/png".to_string(),
                size: 1024,
                uploaded_at: None,
                deleted: false,
            }],
            ..Default::default()
        };

        let merged = handler.merge_metadata(&crdt, Some(&disk));

        // Some([]) is an explicit clearing - should NOT fall back to disk
        // This enables proper sync of deletions from remote peers
        assert_eq!(merged.contents, Some(vec![]));
        // Empty Vec attachments still falls back to disk (no explicit clearing mechanism)
        assert_eq!(merged.attachments.len(), 1);
    }
}
