//! Frontmatter operations for WASM.

use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::error::IntoJsResult;
use crate::state::{block_on, with_fs, with_fs_mut};
use diaryx_core::entry::DiaryxApp;
use diaryx_core::fs::SyncToAsyncFs;

// ============================================================================
// DiaryxFrontmatter Class
// ============================================================================

/// Frontmatter operations for managing YAML frontmatter.
#[wasm_bindgen]
pub struct DiaryxFrontmatter;

#[wasm_bindgen]
impl DiaryxFrontmatter {
    /// Create a new DiaryxFrontmatter instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Get all frontmatter for an entry.
    #[wasm_bindgen]
    pub fn get_all(&self, path: &str) -> Result<JsValue, JsValue> {
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);

        with_fs(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);

            let frontmatter = block_on(app.get_all_frontmatter(path)).js_err()?;

            let mut json_map = serde_json::Map::new();
            for (key, value) in frontmatter {
                if let Ok(json_val) = serde_json::to_value(&value) {
                    json_map.insert(key, json_val);
                }
            }

            json_map.serialize(&serializer).js_err()
        })
    }

    /// Set a frontmatter property.
    #[wasm_bindgen]
    pub fn set_property(&self, path: &str, key: &str, value: JsValue) -> Result<(), JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);

            let json_value: serde_json::Value = serde_wasm_bindgen::from_value(value).js_err()?;
            let yaml_value: serde_yaml::Value = serde_json::from_value(json_value).js_err()?;

            block_on(app.set_frontmatter_property(path, key, yaml_value)).js_err()
        })
    }

    /// Remove a frontmatter property.
    #[wasm_bindgen]
    pub fn remove_property(&self, path: &str, key: &str) -> Result<(), JsValue> {
        with_fs_mut(|fs| {
            let async_fs = SyncToAsyncFs::new(fs.clone());
            let app = DiaryxApp::new(async_fs);
            block_on(app.remove_frontmatter_property(path, key)).js_err()
        })
    }
}

impl Default for DiaryxFrontmatter {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Parse YAML frontmatter from raw markdown content.
#[wasm_bindgen]
pub fn parse_frontmatter(content: &str) -> Result<JsValue, JsValue> {
    if !content.starts_with("---\n") {
        return serde_wasm_bindgen::to_value(&serde_json::Map::<String, serde_json::Value>::new())
            .js_err();
    }

    let rest = &content[4..];
    let end_idx = rest.find("\n---");

    let yaml_str = match end_idx {
        Some(idx) => &rest[..idx],
        None => {
            return serde_wasm_bindgen::to_value(
                &serde_json::Map::<String, serde_json::Value>::new(),
            )
            .js_err();
        }
    };

    match serde_yaml::from_str::<serde_json::Value>(yaml_str) {
        Ok(serde_json::Value::Object(map)) => serde_wasm_bindgen::to_value(&map).js_err(),
        _ => serde_wasm_bindgen::to_value(&serde_json::Map::<String, serde_json::Value>::new())
            .js_err(),
    }
}

/// Serialize a JavaScript object to YAML frontmatter format.
#[wasm_bindgen]
pub fn serialize_frontmatter(frontmatter: JsValue) -> Result<String, JsValue> {
    let map: serde_json::Map<String, serde_json::Value> =
        serde_wasm_bindgen::from_value(frontmatter).js_err()?;

    let yaml = serde_yaml::to_string(&map).js_err()?;
    let yaml = yaml.trim_end();
    Ok(format!("---\n{}\n---", yaml))
}

/// Extract the body content from raw markdown.
#[wasm_bindgen]
pub fn extract_body(content: &str) -> String {
    if !content.starts_with("---\n") {
        return content.to_string();
    }

    let rest = &content[4..];
    if let Some(end_idx) = rest.find("\n---") {
        let after_frontmatter = &rest[end_idx + 4..];
        after_frontmatter.trim_start_matches('\n').to_string()
    } else {
        content.to_string()
    }
}
