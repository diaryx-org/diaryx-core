//! Attachment operations for WASM.

use std::path::Path;

use diaryx_core::entry::DiaryxApp;
use diaryx_core::fs::{FileSystem, SyncToAsyncFs};
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::error::IntoJsResult;
use crate::state::{block_on, with_fs, with_fs_mut};

// ============================================================================
// Types
// ============================================================================

#[derive(Serialize)]
struct StorageInfo {
    used: u64,
    limit: u64,
    attachment_limit: u64,
}

// ============================================================================
// DiaryxAttachment Class
// ============================================================================

/// Attachment operations for managing file attachments.
#[wasm_bindgen]
pub struct DiaryxAttachment;

#[wasm_bindgen]
impl DiaryxAttachment {
    /// Create a new DiaryxAttachment instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Add an attachment path to an entry.
    #[wasm_bindgen]
    pub fn add(&self, entry_path: &str, attachment_path: &str) -> Result<(), JsValue> {
        with_fs(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);
            block_on(app.add_attachment(entry_path, attachment_path)).js_err()
        })
    }

    /// Remove an attachment path from an entry.
    #[wasm_bindgen]
    pub fn remove(&self, entry_path: &str, attachment_path: &str) -> Result<(), JsValue> {
        with_fs(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);
            block_on(app.remove_attachment(entry_path, attachment_path)).js_err()
        })
    }

    /// Get attachments for an entry.
    #[wasm_bindgen]
    pub fn list(&self, entry_path: &str) -> Result<JsValue, JsValue> {
        with_fs(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);
            let attachments = block_on(app.get_attachments(entry_path)).js_err()?;
            serde_wasm_bindgen::to_value(&attachments).js_err()
        })
    }

    /// Upload an attachment file (base64 encoded).
    #[wasm_bindgen]
    pub fn upload(
        &self,
        entry_path: &str,
        filename: &str,
        data_base64: &str,
    ) -> Result<String, JsValue> {
        with_fs_mut(|fs| {
            let data = base64_decode(data_base64)
                .map_err(|e| JsValue::from_str(&format!("Base64 decode error: {}", e)))?;

            let entry_dir = Path::new(entry_path).parent().unwrap_or(Path::new("."));
            let attachments_dir = entry_dir.join("_attachments");
            let attachment_path = attachments_dir.join(filename);

            fs.create_dir_all(&attachments_dir).js_err()?;
            fs.write_binary(&attachment_path, &data).js_err()?;

            let relative_path = format!("_attachments/{}", filename);

            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);
            block_on(app.add_attachment(entry_path, &relative_path)).js_err()?;

            Ok(relative_path)
        })
    }

    /// Delete an attachment file.
    #[wasm_bindgen]
    pub fn delete(&self, entry_path: &str, attachment_path: &str) -> Result<(), JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);
            block_on(app.remove_attachment(entry_path, attachment_path)).js_err()?;

            let entry_dir = Path::new(entry_path).parent().unwrap_or(Path::new("."));
            let full_path = entry_dir.join(attachment_path);
            if fs.exists(&full_path) {
                fs.delete_file(&full_path).js_err()?;
            }

            Ok(())
        })
    }

    /// Read attachment data as Uint8Array.
    #[wasm_bindgen]
    pub fn read_data(
        &self,
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
    #[wasm_bindgen]
    pub fn get_storage_usage(&self) -> Result<JsValue, JsValue> {
        with_fs(|fs| {
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

            let info = StorageInfo {
                used: total_size,
                limit: 100 * 1024 * 1024,          // 100MB
                attachment_limit: 5 * 1024 * 1024, // 5MB
            };

            serde_wasm_bindgen::to_value(&info).js_err()
        })
    }
}

impl Default for DiaryxAttachment {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Simple base64 decoder
fn base64_decode(input: &str) -> std::result::Result<Vec<u8>, String> {
    let data = if let Some(pos) = input.find(",") {
        &input[pos + 1..]
    } else {
        input
    };

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
