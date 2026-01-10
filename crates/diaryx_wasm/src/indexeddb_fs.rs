//! IndexedDB implementation of AsyncFileSystem.
//!
//! Uses the `indexed_db` crate to provide persistent file storage in browsers
//! that don't fully support OPFS (e.g., Safari in main thread context).
//!
//! ## Storage Schema
//!
//! - Database name: "diaryx"
//! - Object stores:
//!   - "files": Text files with path as key
//!   - "binary_files": Binary files with path as key

use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use diaryx_core::fs::{AsyncFileSystem, BoxFuture};
use indexed_db::{Database, Factory};
use js_sys::{JsString, Uint8Array};
use wasm_bindgen::prelude::*;

// ============================================================================
// Constants
// ============================================================================

const DB_NAME: &str = "diaryx";
const DB_VERSION: u32 = 1;
const STORE_FILES: &str = "files";
const STORE_BINARY_FILES: &str = "binary_files";

// ============================================================================
// IndexedDbFileSystem Implementation
// ============================================================================

/// AsyncFileSystem implementation backed by IndexedDB.
///
/// Used as a fallback for browsers that don't support OPFS or when
/// running outside a Web Worker context (where OPFS sync access isn't available).
#[wasm_bindgen]
pub struct IndexedDbFileSystem {
    db: Rc<Database<Error>>,
}

impl Clone for IndexedDbFileSystem {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
        }
    }
}

fn idb_to_io_error(e: indexed_db::Error<Error>) -> Error {
    match e {
        indexed_db::Error::User(e) => e,
        other => Error::new(ErrorKind::Other, format!("{:?}", other)),
    }
}

#[wasm_bindgen]
impl IndexedDbFileSystem {
    /// Create a new IndexedDbFileSystem.
    ///
    /// Opens or creates the IndexedDB database with the required object stores.
    #[wasm_bindgen]
    pub async fn create() -> std::result::Result<IndexedDbFileSystem, JsValue> {
        let factory = Factory::<Error>::get()
            .map_err(|e| JsValue::from_str(&format!("Failed to get IndexedDB factory: {:?}", e)))?;

        let db = factory
            .open(DB_NAME, DB_VERSION, |evt| async move {
                let db = evt.database();

                // Create files store if it doesn't exist
                if !db.object_store_names().contains(&STORE_FILES.to_string()) {
                    db.build_object_store(STORE_FILES).create()?;
                }

                // Create binary files store if it doesn't exist
                if !db
                    .object_store_names()
                    .contains(&STORE_BINARY_FILES.to_string())
                {
                    db.build_object_store(STORE_BINARY_FILES).create()?;
                }

                Ok(())
            })
            .await
            .map_err(|e| JsValue::from_str(&format!("Failed to open IndexedDB: {:?}", e)))?;

        Ok(Self { db: Rc::new(db) })
    }
}

// ============================================================================
// AsyncFileSystem Implementation
// ============================================================================

impl AsyncFileSystem for IndexedDbFileSystem {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        let path_str = path.to_string_lossy().to_string();
        let db = self.db.clone();

        Box::pin(async move {
            let result = db
                .transaction(&[STORE_FILES])
                .run(move |t| async move {
                    let store = t.object_store(STORE_FILES)?;
                    let key = JsString::from(path_str.as_str());
                    let value = store.get(&key).await?;
                    Ok(value)
                })
                .await
                .map_err(idb_to_io_error)?;

            match result {
                Some(value) => {
                    let js_str: JsString = value
                        .dyn_into()
                        .map_err(|_| Error::new(ErrorKind::InvalidData, "Value is not a string"))?;
                    Ok(String::from(&js_str))
                }
                None => Err(Error::new(
                    ErrorKind::NotFound,
                    format!("File not found: {}", path.display()),
                )),
            }
        })
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        let path_str = path.to_string_lossy().to_string();
        let content = content.to_string();
        let db = self.db.clone();

        Box::pin(async move {
            db.transaction(&[STORE_FILES])
                .rw()
                .run(move |t| async move {
                    let store = t.object_store(STORE_FILES)?;
                    let key = JsString::from(path_str.as_str());
                    let value = JsString::from(content.as_str());
                    store.put_kv(&key, &value).await?;
                    Ok(())
                })
                .await
                .map_err(idb_to_io_error)
        })
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        let path_str = path.to_string_lossy().to_string();
        let content = content.to_string();
        let db = self.db.clone();

        Box::pin(async move {
            db.transaction(&[STORE_FILES])
                .rw()
                .run(move |t| async move {
                    let store = t.object_store(STORE_FILES)?;
                    let key = JsString::from(path_str.as_str());

                    // Check if file exists
                    if store.get(&key).await?.is_some() {
                        return Err(indexed_db::Error::User(Error::new(
                            ErrorKind::AlreadyExists,
                            format!("File already exists: {}", path_str),
                        )));
                    }

                    let value = JsString::from(content.as_str());
                    store.put_kv(&key, &value).await?;
                    Ok(())
                })
                .await
                .map_err(idb_to_io_error)
        })
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        let path_str = path.to_string_lossy().to_string();
        let db = self.db.clone();

        Box::pin(async move {
            db.transaction(&[STORE_FILES])
                .rw()
                .run(move |t| async move {
                    let store = t.object_store(STORE_FILES)?;
                    let key = JsString::from(path_str.as_str());
                    store.delete(&key).await?;
                    Ok(())
                })
                .await
                .map_err(idb_to_io_error)
        })
    }

    fn list_md_files<'a>(&'a self, dir_path: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        let dir_str = dir_path.to_string_lossy().to_string();
        let db = self.db.clone();

        Box::pin(async move {
            let all_keys = db
                .transaction(&[STORE_FILES])
                .run(move |t| async move {
                    let store = t.object_store(STORE_FILES)?;
                    let mut cursor = store.cursor().open().await?;
                    let mut keys = Vec::new();

                    while let Some(key) = cursor.key() {
                        if let Some(s) = key.dyn_ref::<JsString>() {
                            keys.push(String::from(s));
                        }
                        cursor.advance(1).await?;
                    }

                    Ok(keys)
                })
                .await
                .map_err(idb_to_io_error)?;

            // Filter to files in the directory
            let prefix = if dir_str.is_empty() || dir_str == "." {
                String::new()
            } else {
                format!("{}/", dir_str)
            };

            let md_files: Vec<PathBuf> = all_keys
                .into_iter()
                .filter(|key| {
                    if key.ends_with(".md") {
                        if prefix.is_empty() {
                            // Root directory - file should have no directory
                            !key.contains('/')
                        } else {
                            // Check if file is directly in the directory
                            if let Some(rest) = key.strip_prefix(&prefix) {
                                !rest.contains('/')
                            } else {
                                false
                            }
                        }
                    } else {
                        false
                    }
                })
                .map(PathBuf::from)
                .collect();

            Ok(md_files)
        })
    }

    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        let path_str = path.to_string_lossy().to_string();
        let db = self.db.clone();

        Box::pin(async move {
            let result = db
                .transaction(&[STORE_FILES, STORE_BINARY_FILES])
                .run(move |t| async move {
                    // Check text files
                    let store = t.object_store(STORE_FILES)?;
                    let key = JsString::from(path_str.as_str());
                    if store.get(&key).await?.is_some() {
                        return Ok(true);
                    }

                    // Check binary files
                    let store = t.object_store(STORE_BINARY_FILES)?;
                    if store.get(&key).await?.is_some() {
                        return Ok(true);
                    }

                    Ok(false)
                })
                .await;

            result.unwrap_or(false)
        })
    }

    fn create_dir_all<'a>(&'a self, _path: &'a Path) -> BoxFuture<'a, Result<()>> {
        // IndexedDB is flat key-value store, directories are implicit
        Box::pin(async move { Ok(()) })
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        // Check if any files exist with this path as a prefix
        let dir_str = path.to_string_lossy().to_string();
        let db = self.db.clone();

        Box::pin(async move {
            let result = db
                .transaction(&[STORE_FILES])
                .run(move |t| async move {
                    let prefix = format!("{}/", dir_str);
                    let store = t.object_store(STORE_FILES)?;
                    let mut cursor = store.cursor().open().await?;

                    while let Some(key) = cursor.key() {
                        if let Some(s) = key.dyn_ref::<JsString>() {
                            if String::from(s).starts_with(&prefix) {
                                return Ok(true);
                            }
                        }
                        cursor.advance(1).await?;
                    }

                    Ok(false)
                })
                .await;

            result.unwrap_or(false)
        })
    }

    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let content = self.read_to_string(from).await?;
            self.write_file(to, &content).await?;
            self.delete_file(from).await?;
            Ok(())
        })
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        let path_str = path.to_string_lossy().to_string();
        let db = self.db.clone();

        Box::pin(async move {
            let result = db
                .transaction(&[STORE_BINARY_FILES])
                .run(move |t| async move {
                    let store = t.object_store(STORE_BINARY_FILES)?;
                    let key = JsString::from(path_str.as_str());
                    let value = store.get(&key).await?;
                    Ok(value)
                })
                .await
                .map_err(idb_to_io_error)?;

            match result {
                Some(value) => {
                    let array: Uint8Array = value.dyn_into().map_err(|_| {
                        Error::new(ErrorKind::InvalidData, "Value is not a Uint8Array")
                    })?;
                    Ok(array.to_vec())
                }
                None => Err(Error::new(
                    ErrorKind::NotFound,
                    format!("File not found: {}", path.display()),
                )),
            }
        })
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        let path_str = path.to_string_lossy().to_string();
        let content = content.to_vec();
        let db = self.db.clone();

        Box::pin(async move {
            db.transaction(&[STORE_BINARY_FILES])
                .rw()
                .run(move |t| async move {
                    let store = t.object_store(STORE_BINARY_FILES)?;
                    let key = JsString::from(path_str.as_str());
                    let array = Uint8Array::from(content.as_slice());
                    store.put_kv(&key, &array).await?;
                    Ok(())
                })
                .await
                .map_err(idb_to_io_error)
        })
    }

    fn list_files<'a>(&'a self, dir_path: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        let dir_str = dir_path.to_string_lossy().to_string();
        let db = self.db.clone();

        Box::pin(async move {
            let all_keys = db
                .transaction(&[STORE_FILES, STORE_BINARY_FILES])
                .run(move |t| async move {
                    let mut keys = Vec::new();
                    let prefix = if dir_str.is_empty() || dir_str == "." {
                        String::new()
                    } else {
                        format!("{}/", dir_str)
                    };

                    // Get text file keys
                    let store = t.object_store(STORE_FILES)?;
                    let mut cursor = store.cursor().open().await?;
                    while let Some(key) = cursor.key() {
                        if let Some(js_str) = key.dyn_ref::<JsString>() {
                            let s = String::from(js_str);
                            if prefix.is_empty() {
                                if !s.contains('/') {
                                    keys.push(s);
                                }
                            } else if let Some(rest) = s.strip_prefix(&prefix) {
                                if !rest.contains('/') {
                                    keys.push(s);
                                }
                            }
                        }
                        cursor.advance(1).await?;
                    }

                    // Get binary file keys
                    let store = t.object_store(STORE_BINARY_FILES)?;
                    let mut cursor = store.cursor().open().await?;
                    while let Some(key) = cursor.key() {
                        if let Some(js_str) = key.dyn_ref::<JsString>() {
                            let s = String::from(js_str);
                            if prefix.is_empty() {
                                if !s.contains('/') {
                                    keys.push(s);
                                }
                            } else if let Some(rest) = s.strip_prefix(&prefix) {
                                if !rest.contains('/') {
                                    keys.push(s);
                                }
                            }
                        }
                        cursor.advance(1).await?;
                    }

                    Ok(keys)
                })
                .await
                .map_err(idb_to_io_error)?;

            Ok(all_keys.into_iter().map(PathBuf::from).collect())
        })
    }
}
