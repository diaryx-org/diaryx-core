//! WASM SQLite storage implementation using JavaScript bridge.
//!
//! This module provides a `CrdtStorage` implementation that delegates to
//! JavaScript functions via `js_sys`. The JavaScript side uses sql.js
//! with OPFS persistence for durable storage.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    Rust (WASM)                                  │
//! │  WasmSqliteStorage implements CrdtStorage trait                │
//! │  Calls JS functions via js_sys::Reflect                         │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    JavaScript Bridge                            │
//! │  window.__diaryx_crdt_storage = { loadDoc, saveDoc, ... }      │
//! │  Calls SqliteStorage methods                                    │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    SqliteStorage                                │
//! │  sql.js (SQLite in WASM) + OPFS persistence                    │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! 1. JavaScript must initialize storage before creating DiaryxBackend:
//!    ```javascript
//!    import { initializeSqliteStorage } from './lib/storage/sqliteStorageBridge.js';
//!    await initializeSqliteStorage();
//!    ```
//!
//! 2. JavaScript must set up the global bridge:
//!    ```javascript
//!    import * as bridge from './lib/storage/sqliteStorageBridge.js';
//!    window.__diaryx_crdt_storage = bridge;
//!    ```
//!
//! 3. Then Rust can create WasmSqliteStorage:
//!    ```rust
//!    let storage = WasmSqliteStorage::new()?;
//!    let diaryx = Diaryx::with_crdt_load(fs, Arc::new(storage))?;
//!    ```

use diaryx_core::crdt::{CrdtStorage, CrdtUpdate, StorageResult, UpdateOrigin};
use diaryx_core::error::DiaryxError;
use js_sys::{Array, Function, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;

/// WASM SQLite storage that delegates to JavaScript.
///
/// This implements the `CrdtStorage` trait by calling JavaScript functions
/// that operate on a sql.js SQLite database.
pub struct WasmSqliteStorage {
    /// Reference to the global storage bridge object
    bridge: JsValue,
}

impl std::fmt::Debug for WasmSqliteStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmSqliteStorage").finish_non_exhaustive()
    }
}

// SAFETY: WASM is single-threaded, so these are safe
unsafe impl Send for WasmSqliteStorage {}
unsafe impl Sync for WasmSqliteStorage {}

impl WasmSqliteStorage {
    /// Create a new WasmSqliteStorage instance.
    ///
    /// This expects the JavaScript bridge to be available at
    /// `globalThis.__diaryx_crdt_storage` (works in both Window and Worker contexts).
    /// If not available, returns an error.
    pub fn new() -> Result<Self, JsValue> {
        // Use globalThis which works in both Window and Worker contexts
        let global = js_sys::global();

        let bridge = Reflect::get(&global, &JsValue::from_str("__diaryx_crdt_storage"))
            .map_err(|_| JsValue::from_str("__diaryx_crdt_storage not found on globalThis"))?;

        if bridge.is_undefined() || bridge.is_null() {
            return Err(JsValue::from_str(
                "CRDT storage bridge not initialized. Call setupCrdtStorageBridge() first.",
            ));
        }

        // Verify that required functions exist
        let required_functions = [
            "crdt_load_doc",
            "crdt_save_doc",
            "crdt_delete_doc",
            "crdt_list_docs",
            "crdt_append_update",
            "crdt_get_updates_since",
            "crdt_get_all_updates",
            "crdt_get_latest_update_id",
            "crdt_compact",
        ];

        for func_name in required_functions {
            let func = Reflect::get(&bridge, &JsValue::from_str(func_name))
                .map_err(|_| JsValue::from_str(&format!("Missing function: {}", func_name)))?;
            if !func.is_function() {
                return Err(JsValue::from_str(&format!(
                    "{} is not a function",
                    func_name
                )));
            }
        }

        Ok(Self { bridge })
    }

    /// Call a JavaScript function on the bridge.
    fn call_fn(&self, name: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
        let func = Reflect::get(&self.bridge, &JsValue::from_str(name))?;
        let func: Function = func.dyn_into()?;

        let args_array = Array::new();
        for arg in args {
            args_array.push(arg);
        }

        Reflect::apply(&func, &self.bridge, &args_array)
    }

    /// Convert a Uint8Array to Vec<u8>
    fn uint8array_to_vec(arr: &Uint8Array) -> Vec<u8> {
        arr.to_vec()
    }

    /// Convert a Vec<u8> to Uint8Array
    fn vec_to_uint8array(vec: &[u8]) -> Uint8Array {
        let arr = Uint8Array::new_with_length(vec.len() as u32);
        arr.copy_from(vec);
        arr
    }
}

impl CrdtStorage for WasmSqliteStorage {
    fn load_doc(&self, name: &str) -> StorageResult<Option<Vec<u8>>> {
        let result = self
            .call_fn("crdt_load_doc", &[JsValue::from_str(name)])
            .map_err(|e| DiaryxError::Crdt(format!("load_doc failed: {:?}", e)))?;

        if result.is_null() || result.is_undefined() {
            Ok(None)
        } else {
            let arr: Uint8Array = result
                .dyn_into()
                .map_err(|_| DiaryxError::Crdt("Expected Uint8Array from load_doc".to_string()))?;
            Ok(Some(Self::uint8array_to_vec(&arr)))
        }
    }

    fn save_doc(&self, name: &str, state: &[u8]) -> StorageResult<()> {
        // We need to compute state_vector from the state
        // For simplicity, we'll pass an empty state_vector and let JS handle it
        // Actually, we need to compute it properly using yrs
        use yrs::{
            Doc, ReadTxn, Transact, Update, updates::decoder::Decode, updates::encoder::Encode,
        };

        let state_vector = {
            let doc = Doc::new();
            {
                let mut txn = doc.transact_mut();
                if let Ok(update) = Update::decode_v1(state) {
                    let _ = txn.apply_update(update);
                }
            }
            let txn = doc.transact();
            txn.state_vector().encode_v1()
        };

        let state_arr = Self::vec_to_uint8array(state);
        let sv_arr = Self::vec_to_uint8array(&state_vector);

        self.call_fn(
            "crdt_save_doc",
            &[JsValue::from_str(name), state_arr.into(), sv_arr.into()],
        )
        .map_err(|e| DiaryxError::Crdt(format!("save_doc failed: {:?}", e)))?;

        Ok(())
    }

    fn delete_doc(&self, name: &str) -> StorageResult<()> {
        self.call_fn("crdt_delete_doc", &[JsValue::from_str(name)])
            .map_err(|e| DiaryxError::Crdt(format!("delete_doc failed: {:?}", e)))?;
        Ok(())
    }

    fn list_docs(&self) -> StorageResult<Vec<String>> {
        let result = self
            .call_fn("crdt_list_docs", &[])
            .map_err(|e| DiaryxError::Crdt(format!("list_docs failed: {:?}", e)))?;

        let arr: Array = result
            .dyn_into()
            .map_err(|_| DiaryxError::Crdt("Expected Array from list_docs".to_string()))?;

        let mut docs = Vec::with_capacity(arr.length() as usize);
        for i in 0..arr.length() {
            if let Some(s) = arr.get(i).as_string() {
                docs.push(s);
            }
        }
        Ok(docs)
    }

    fn append_update_with_device(
        &self,
        name: &str,
        update: &[u8],
        origin: UpdateOrigin,
        device_id: Option<&str>,
        device_name: Option<&str>,
    ) -> StorageResult<i64> {
        let update_arr = Self::vec_to_uint8array(update);
        let origin_str = origin.to_string();

        let result = self
            .call_fn(
                "crdt_append_update",
                &[
                    JsValue::from_str(name),
                    update_arr.into(),
                    JsValue::from_str(&origin_str),
                    device_id.map(JsValue::from_str).unwrap_or(JsValue::NULL),
                    device_name.map(JsValue::from_str).unwrap_or(JsValue::NULL),
                ],
            )
            .map_err(|e| DiaryxError::Crdt(format!("append_update failed: {:?}", e)))?;

        let id = result
            .as_f64()
            .ok_or_else(|| DiaryxError::Crdt("Expected number from append_update".to_string()))?
            as i64;

        Ok(id)
    }

    fn get_updates_since(&self, name: &str, since_id: i64) -> StorageResult<Vec<CrdtUpdate>> {
        let result = self
            .call_fn(
                "crdt_get_updates_since",
                &[JsValue::from_str(name), JsValue::from_f64(since_id as f64)],
            )
            .map_err(|e| DiaryxError::Crdt(format!("get_updates_since failed: {:?}", e)))?;

        self.parse_updates_array(result, name)
    }

    fn get_all_updates(&self, name: &str) -> StorageResult<Vec<CrdtUpdate>> {
        let result = self
            .call_fn("crdt_get_all_updates", &[JsValue::from_str(name)])
            .map_err(|e| DiaryxError::Crdt(format!("get_all_updates failed: {:?}", e)))?;

        self.parse_updates_array(result, name)
    }

    fn get_state_at(&self, name: &str, update_id: i64) -> StorageResult<Option<Vec<u8>>> {
        // Reconstruct state by loading base doc and applying updates up to update_id
        use yrs::{Doc, ReadTxn, Transact, Update, updates::decoder::Decode};

        let base_state = self.load_doc(name)?;
        let all_updates = self.get_all_updates(name)?;

        // Filter updates up to the given ID
        let updates_to_apply: Vec<_> = all_updates
            .into_iter()
            .filter(|u| u.update_id <= update_id)
            .collect();

        if base_state.is_none() && updates_to_apply.is_empty() {
            return Ok(None);
        }

        let doc = Doc::new();
        {
            let mut txn = doc.transact_mut();

            // Apply base state
            if let Some(state) = base_state {
                if let Ok(update) = Update::decode_v1(&state) {
                    let _ = txn.apply_update(update);
                }
            }

            // Apply updates
            for u in updates_to_apply {
                if let Ok(update) = Update::decode_v1(&u.data) {
                    let _ = txn.apply_update(update);
                }
            }
        }

        let txn = doc.transact();
        Ok(Some(txn.encode_state_as_update_v1(&Default::default())))
    }

    fn compact(&self, name: &str, keep_updates: usize) -> StorageResult<()> {
        self.call_fn(
            "crdt_compact",
            &[
                JsValue::from_str(name),
                JsValue::from_f64(keep_updates as f64),
            ],
        )
        .map_err(|e| DiaryxError::Crdt(format!("compact failed: {:?}", e)))?;
        Ok(())
    }

    fn get_latest_update_id(&self, name: &str) -> StorageResult<i64> {
        let result = self
            .call_fn("crdt_get_latest_update_id", &[JsValue::from_str(name)])
            .map_err(|e| DiaryxError::Crdt(format!("get_latest_update_id failed: {:?}", e)))?;

        let id = result.as_f64().unwrap_or(0.0) as i64;
        Ok(id)
    }

    fn rename_doc(&self, old_name: &str, new_name: &str) -> StorageResult<()> {
        self.call_fn(
            "crdt_rename_doc",
            &[JsValue::from_str(old_name), JsValue::from_str(new_name)],
        )
        .map_err(|e| DiaryxError::Crdt(format!("rename_doc failed: {:?}", e)))?;
        Ok(())
    }
}

impl WasmSqliteStorage {
    /// Parse an array of JS update objects into CrdtUpdate structs.
    fn parse_updates_array(
        &self,
        result: JsValue,
        doc_name: &str,
    ) -> StorageResult<Vec<CrdtUpdate>> {
        let arr: Array = result
            .dyn_into()
            .map_err(|_| DiaryxError::Crdt("Expected Array from updates query".to_string()))?;

        let mut updates = Vec::with_capacity(arr.length() as usize);
        for i in 0..arr.length() {
            let obj = arr.get(i);
            if obj.is_undefined() {
                continue;
            }

            let update_id = Reflect::get(&obj, &JsValue::from_str("updateId"))
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as i64;

            let data = Reflect::get(&obj, &JsValue::from_str("data"))
                .ok()
                .and_then(|v| v.dyn_into::<Uint8Array>().ok())
                .map(|arr| Self::uint8array_to_vec(&arr))
                .unwrap_or_default();

            let timestamp = Reflect::get(&obj, &JsValue::from_str("timestamp"))
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as i64;

            let origin_str = Reflect::get(&obj, &JsValue::from_str("origin"))
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_else(|| "local".to_string());

            let origin = origin_str.parse().unwrap_or(UpdateOrigin::Local);

            let device_id = Reflect::get(&obj, &JsValue::from_str("deviceId"))
                .ok()
                .and_then(|v| v.as_string());

            let device_name = Reflect::get(&obj, &JsValue::from_str("deviceName"))
                .ok()
                .and_then(|v| v.as_string());

            updates.push(CrdtUpdate {
                update_id,
                doc_name: doc_name.to_string(),
                data,
                timestamp,
                origin,
                device_id,
                device_name,
            });
        }

        Ok(updates)
    }
}

#[cfg(test)]
mod tests {
    // Tests require a browser environment with the JS bridge set up
}
