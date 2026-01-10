//! Async validation operations for WASM with native Promise support.
//!
//! This module provides async validation operations that work directly with
//! `JsAsyncFileSystem`, returning native JavaScript Promises. This enables
//! proper async/await patterns in the web frontend without the need for
//! synchronous wrappers or `block_on`.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { JsAsyncFileSystem, DiaryxAsyncValidation } from './wasm/diaryx_wasm.js';
//!
//! // Create filesystem with your storage backend callbacks
//! const fs = new JsAsyncFileSystem({ /* callbacks */ });
//!
//! // Create async validation instance
//! const validation = new DiaryxAsyncValidation(fs);
//!
//! // All methods return native Promises
//! const result = await validation.validate('workspace');
//! if (result.errors.length > 0) {
//!     const fixes = await validation.fixAll(result);
//!     console.log(`Fixed ${fixes.total_fixed} issues`);
//! }
//! ```

use std::path::PathBuf;

use diaryx_core::validate::{ValidationFixer, Validator};
use diaryx_core::workspace::Workspace;
use js_sys::Promise;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::error::IntoJsResult;
use crate::js_async_fs::JsAsyncFileSystem;

// ============================================================================
// Types (reuse from validation.rs)
// ============================================================================

/// Validation error returned to JavaScript
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum JsAsyncValidationError {
    BrokenPartOf { file: String, target: String },
    BrokenContentsRef { index: String, target: String },
    BrokenAttachment { file: String, attachment: String },
}

impl From<diaryx_core::validate::ValidationError> for JsAsyncValidationError {
    fn from(err: diaryx_core::validate::ValidationError) -> Self {
        use diaryx_core::validate::ValidationError;
        match err {
            ValidationError::BrokenPartOf { file, target } => {
                JsAsyncValidationError::BrokenPartOf {
                    file: file.to_string_lossy().to_string(),
                    target,
                }
            }
            ValidationError::BrokenContentsRef { index, target } => {
                JsAsyncValidationError::BrokenContentsRef {
                    index: index.to_string_lossy().to_string(),
                    target,
                }
            }
            ValidationError::BrokenAttachment { file, attachment } => {
                JsAsyncValidationError::BrokenAttachment {
                    file: file.to_string_lossy().to_string(),
                    attachment,
                }
            }
        }
    }
}

/// Validation warning returned to JavaScript
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum JsAsyncValidationWarning {
    OrphanFile {
        file: String,
    },
    CircularReference {
        files: Vec<String>,
    },
    UnlinkedEntry {
        path: String,
        is_dir: bool,
    },
    UnlistedFile {
        index: String,
        file: String,
    },
    NonPortablePath {
        file: String,
        property: String,
        value: String,
        suggested: String,
    },
    MultipleIndexes {
        directory: String,
        indexes: Vec<String>,
    },
    OrphanBinaryFile {
        file: String,
        suggested_index: Option<String>,
    },
    MissingPartOf {
        file: String,
        suggested_index: Option<String>,
    },
}

impl From<diaryx_core::validate::ValidationWarning> for JsAsyncValidationWarning {
    fn from(warn: diaryx_core::validate::ValidationWarning) -> Self {
        use diaryx_core::validate::ValidationWarning;
        match warn {
            ValidationWarning::OrphanFile { file } => JsAsyncValidationWarning::OrphanFile {
                file: file.to_string_lossy().to_string(),
            },
            ValidationWarning::CircularReference { files } => {
                JsAsyncValidationWarning::CircularReference {
                    files: files
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect(),
                }
            }
            ValidationWarning::UnlinkedEntry { path, is_dir } => {
                JsAsyncValidationWarning::UnlinkedEntry {
                    path: path.to_string_lossy().to_string(),
                    is_dir,
                }
            }
            ValidationWarning::UnlistedFile { index, file } => {
                JsAsyncValidationWarning::UnlistedFile {
                    index: index.to_string_lossy().to_string(),
                    file: file.to_string_lossy().to_string(),
                }
            }
            ValidationWarning::NonPortablePath {
                file,
                property,
                value,
                suggested,
            } => JsAsyncValidationWarning::NonPortablePath {
                file: file.to_string_lossy().to_string(),
                property,
                value,
                suggested,
            },
            ValidationWarning::MultipleIndexes { directory, indexes } => {
                JsAsyncValidationWarning::MultipleIndexes {
                    directory: directory.to_string_lossy().to_string(),
                    indexes: indexes
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect(),
                }
            }
            ValidationWarning::OrphanBinaryFile {
                file,
                suggested_index,
            } => JsAsyncValidationWarning::OrphanBinaryFile {
                file: file.to_string_lossy().to_string(),
                suggested_index: suggested_index.map(|p| p.to_string_lossy().to_string()),
            },
            ValidationWarning::MissingPartOf {
                file,
                suggested_index,
            } => JsAsyncValidationWarning::MissingPartOf {
                file: file.to_string_lossy().to_string(),
                suggested_index: suggested_index.map(|p| p.to_string_lossy().to_string()),
            },
        }
    }
}

/// Validation result returned to JavaScript
#[derive(Debug, Serialize, Deserialize)]
pub struct JsAsyncValidationResult {
    pub errors: Vec<JsAsyncValidationError>,
    pub warnings: Vec<JsAsyncValidationWarning>,
    pub files_checked: usize,
}

/// Fix result returned to JavaScript
#[derive(Debug, Serialize, Deserialize)]
pub struct JsAsyncFixResult {
    pub success: bool,
    pub message: String,
}

impl From<diaryx_core::validate::FixResult> for JsAsyncFixResult {
    fn from(result: diaryx_core::validate::FixResult) -> Self {
        JsAsyncFixResult {
            success: result.success,
            message: result.message,
        }
    }
}

/// Summary of fix operations
#[derive(Debug, Serialize, Deserialize)]
pub struct JsAsyncFixSummary {
    pub error_fixes: Vec<JsAsyncFixResult>,
    pub warning_fixes: Vec<JsAsyncFixResult>,
    pub total_fixed: usize,
    pub total_failed: usize,
}

// ============================================================================
// DiaryxAsyncValidation Class
// ============================================================================

/// Async validation operations with native Promise support.
///
/// Unlike `DiaryxValidation` which uses `block_on` internally, this class
/// returns true JavaScript Promises that can be properly awaited.
#[wasm_bindgen]
pub struct DiaryxAsyncValidation {
    fs: JsAsyncFileSystem,
}

#[wasm_bindgen]
impl DiaryxAsyncValidation {
    /// Create a new DiaryxAsyncValidation with the provided filesystem.
    #[wasm_bindgen(constructor)]
    pub fn new(fs: JsAsyncFileSystem) -> Self {
        Self { fs }
    }

    /// Validate workspace links.
    ///
    /// @param workspace_path - Path to the workspace directory
    /// @returns Promise resolving to validation result with errors and warnings
    #[wasm_bindgen]
    pub fn validate(&self, workspace_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let validator = Validator::new(&fs);
            let root_path = PathBuf::from(&workspace_path);

            let ws = Workspace::new(&fs);

            // Find root index
            let root_index = ws
                .find_root_index_in_dir(&root_path)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let root_index = match root_index {
                Some(idx) => idx,
                None => ws
                    .find_any_index_in_dir(&root_path)
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?
                    .ok_or_else(|| {
                        JsValue::from_str(&format!("No workspace found at '{}'", workspace_path))
                    })?,
            };

            let result = validator
                .validate_workspace(&root_index)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_result = JsAsyncValidationResult {
                errors: result
                    .errors
                    .into_iter()
                    .map(JsAsyncValidationError::from)
                    .collect(),
                warnings: result
                    .warnings
                    .into_iter()
                    .map(JsAsyncValidationWarning::from)
                    .collect(),
                files_checked: result.files_checked,
            };

            serde_wasm_bindgen::to_value(&js_result).js_err()
        })
    }

    /// Validate a single file's links.
    ///
    /// @param file_path - Path to the file to validate
    /// @returns Promise resolving to validation result
    #[wasm_bindgen(js_name = "validateFile")]
    pub fn validate_file(&self, file_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let validator = Validator::new(&fs);
            let path = PathBuf::from(&file_path);

            let result = validator
                .validate_file(&path)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let js_result = JsAsyncValidationResult {
                errors: result
                    .errors
                    .into_iter()
                    .map(JsAsyncValidationError::from)
                    .collect(),
                warnings: result
                    .warnings
                    .into_iter()
                    .map(JsAsyncValidationWarning::from)
                    .collect(),
                files_checked: result.files_checked,
            };

            serde_wasm_bindgen::to_value(&js_result).js_err()
        })
    }

    /// Fix a broken part_of reference by removing it.
    ///
    /// @param file_path - Path to the file with broken part_of
    /// @returns Promise resolving to fix result
    #[wasm_bindgen(js_name = "fixBrokenPartOf")]
    pub fn fix_broken_part_of(&self, file_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = ValidationFixer::new(&fs);
            let path = PathBuf::from(&file_path);
            let result = fixer.fix_broken_part_of(&path).await;
            serde_wasm_bindgen::to_value(&JsAsyncFixResult::from(result)).js_err()
        })
    }

    /// Fix a broken contents reference by removing it.
    ///
    /// @param index_path - Path to the index file
    /// @param target - The broken target reference to remove
    /// @returns Promise resolving to fix result
    #[wasm_bindgen(js_name = "fixBrokenContentsRef")]
    pub fn fix_broken_contents_ref(&self, index_path: String, target: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = ValidationFixer::new(&fs);
            let path = PathBuf::from(&index_path);
            let result = fixer.fix_broken_contents_ref(&path, &target).await;
            serde_wasm_bindgen::to_value(&JsAsyncFixResult::from(result)).js_err()
        })
    }

    /// Fix a broken attachment reference by removing it.
    ///
    /// @param file_path - Path to the file with broken attachment
    /// @param attachment - The broken attachment reference to remove
    /// @returns Promise resolving to fix result
    #[wasm_bindgen(js_name = "fixBrokenAttachment")]
    pub fn fix_broken_attachment(&self, file_path: String, attachment: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = ValidationFixer::new(&fs);
            let path = PathBuf::from(&file_path);
            let result = fixer.fix_broken_attachment(&path, &attachment).await;
            serde_wasm_bindgen::to_value(&JsAsyncFixResult::from(result)).js_err()
        })
    }

    /// Fix a non-portable path by normalizing it.
    ///
    /// @param file_path - Path to the file
    /// @param property - The frontmatter property containing the path
    /// @param old_value - The current non-portable value
    /// @param new_value - The normalized portable value
    /// @returns Promise resolving to fix result
    #[wasm_bindgen(js_name = "fixNonPortablePath")]
    pub fn fix_non_portable_path(
        &self,
        file_path: String,
        property: String,
        old_value: String,
        new_value: String,
    ) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = ValidationFixer::new(&fs);
            let path = PathBuf::from(&file_path);
            let result = fixer
                .fix_non_portable_path(&path, &property, &old_value, &new_value)
                .await;
            serde_wasm_bindgen::to_value(&JsAsyncFixResult::from(result)).js_err()
        })
    }

    /// Add an unlisted file to an index's contents.
    ///
    /// @param index_path - Path to the index file
    /// @param file_path - Path to the file to add to contents
    /// @returns Promise resolving to fix result
    #[wasm_bindgen(js_name = "fixUnlistedFile")]
    pub fn fix_unlisted_file(&self, index_path: String, file_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = ValidationFixer::new(&fs);
            let index = PathBuf::from(&index_path);
            let file = PathBuf::from(&file_path);
            let result = fixer.fix_unlisted_file(&index, &file).await;
            serde_wasm_bindgen::to_value(&JsAsyncFixResult::from(result)).js_err()
        })
    }

    /// Add an orphan binary file to an index's attachments.
    ///
    /// @param index_path - Path to the index file
    /// @param file_path - Path to the binary file to add
    /// @returns Promise resolving to fix result
    #[wasm_bindgen(js_name = "fixOrphanBinaryFile")]
    pub fn fix_orphan_binary_file(&self, index_path: String, file_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = ValidationFixer::new(&fs);
            let index = PathBuf::from(&index_path);
            let file = PathBuf::from(&file_path);
            let result = fixer.fix_orphan_binary_file(&index, &file).await;
            serde_wasm_bindgen::to_value(&JsAsyncFixResult::from(result)).js_err()
        })
    }

    /// Fix a missing part_of by setting it to point to the given index.
    ///
    /// @param file_path - Path to the file missing part_of
    /// @param index_path - Path to the index that should be the parent
    /// @returns Promise resolving to fix result
    #[wasm_bindgen(js_name = "fixMissingPartOf")]
    pub fn fix_missing_part_of(&self, file_path: String, index_path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = ValidationFixer::new(&fs);
            let file = PathBuf::from(&file_path);
            let index = PathBuf::from(&index_path);
            let result = fixer.fix_missing_part_of(&file, &index).await;
            serde_wasm_bindgen::to_value(&JsAsyncFixResult::from(result)).js_err()
        })
    }

    /// Fix all errors and fixable warnings in a validation result.
    ///
    /// @param validation_result - The validation result from validate() or validateFile()
    /// @returns Promise resolving to fix summary
    #[wasm_bindgen(js_name = "fixAll")]
    pub fn fix_all(&self, validation_result: JsValue) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let fixer = ValidationFixer::new(&fs);

            // Parse the JS validation result
            let js_result: JsAsyncValidationResult =
                serde_wasm_bindgen::from_value(validation_result)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

            let mut error_fixes = Vec::new();
            let mut warning_fixes = Vec::new();

            // Fix errors
            for err in &js_result.errors {
                let result = match err {
                    JsAsyncValidationError::BrokenPartOf { file, target: _ } => {
                        fixer.fix_broken_part_of(&PathBuf::from(file)).await
                    }
                    JsAsyncValidationError::BrokenContentsRef { index, target } => {
                        fixer
                            .fix_broken_contents_ref(&PathBuf::from(index), target)
                            .await
                    }
                    JsAsyncValidationError::BrokenAttachment { file, attachment } => {
                        fixer
                            .fix_broken_attachment(&PathBuf::from(file), attachment)
                            .await
                    }
                };
                error_fixes.push(JsAsyncFixResult::from(result));
            }

            // Fix warnings
            for warn in &js_result.warnings {
                let result = match warn {
                    JsAsyncValidationWarning::UnlistedFile { index, file } => Some(
                        fixer
                            .fix_unlisted_file(&PathBuf::from(index), &PathBuf::from(file))
                            .await,
                    ),
                    JsAsyncValidationWarning::NonPortablePath {
                        file,
                        property,
                        value,
                        suggested,
                    } => Some(
                        fixer
                            .fix_non_portable_path(&PathBuf::from(file), property, value, suggested)
                            .await,
                    ),
                    JsAsyncValidationWarning::OrphanBinaryFile {
                        file,
                        suggested_index,
                    } => {
                        if let Some(index) = suggested_index {
                            Some(
                                fixer
                                    .fix_orphan_binary_file(
                                        &PathBuf::from(index),
                                        &PathBuf::from(file),
                                    )
                                    .await,
                            )
                        } else {
                            None
                        }
                    }
                    JsAsyncValidationWarning::MissingPartOf {
                        file,
                        suggested_index,
                    } => {
                        if let Some(index) = suggested_index {
                            Some(
                                fixer
                                    .fix_missing_part_of(
                                        &PathBuf::from(file),
                                        &PathBuf::from(index),
                                    )
                                    .await,
                            )
                        } else {
                            None
                        }
                    }
                    // These cannot be auto-fixed
                    JsAsyncValidationWarning::OrphanFile { .. }
                    | JsAsyncValidationWarning::UnlinkedEntry { .. }
                    | JsAsyncValidationWarning::CircularReference { .. }
                    | JsAsyncValidationWarning::MultipleIndexes { .. } => None,
                };

                if let Some(r) = result {
                    warning_fixes.push(JsAsyncFixResult::from(r));
                }
            }

            let total_fixed = error_fixes.iter().filter(|r| r.success).count()
                + warning_fixes.iter().filter(|r| r.success).count();
            let total_failed = error_fixes.iter().filter(|r| !r.success).count()
                + warning_fixes.iter().filter(|r| !r.success).count();

            let summary = JsAsyncFixSummary {
                error_fixes,
                warning_fixes,
                total_fixed,
                total_failed,
            };

            serde_wasm_bindgen::to_value(&summary).js_err()
        })
    }
}

impl Default for DiaryxAsyncValidation {
    fn default() -> Self {
        Self {
            fs: JsAsyncFileSystem::new(JsValue::NULL),
        }
    }
}

// ============================================================================
// TypeScript Type Definitions
// ============================================================================

#[wasm_bindgen(typescript_custom_section)]
const TS_APPEND_CONTENT: &'static str = r#"
/**
 * Validation error types.
 */
export type AsyncValidationError =
    | { type: 'BrokenPartOf'; file: string; target: string }
    | { type: 'BrokenContentsRef'; index: string; target: string }
    | { type: 'BrokenAttachment'; file: string; attachment: string };

/**
 * Validation warning types.
 */
export type AsyncValidationWarning =
    | { type: 'OrphanFile'; file: string }
    | { type: 'CircularReference'; files: string[] }
    | { type: 'UnlinkedEntry'; path: string; is_dir: boolean }
    | { type: 'UnlistedFile'; index: string; file: string }
    | { type: 'NonPortablePath'; file: string; property: string; value: string; suggested: string }
    | { type: 'MultipleIndexes'; directory: string; indexes: string[] }
    | { type: 'OrphanBinaryFile'; file: string; suggested_index: string | null }
    | { type: 'MissingPartOf'; file: string; suggested_index: string | null };

/**
 * Validation result containing errors and warnings.
 */
export interface AsyncValidationResult {
    errors: AsyncValidationError[];
    warnings: AsyncValidationWarning[];
    files_checked: number;
}

/**
 * Result of a single fix operation.
 */
export interface AsyncFixResult {
    success: boolean;
    message: string;
}

/**
 * Summary of all fix operations.
 */
export interface AsyncFixSummary {
    error_fixes: AsyncFixResult[];
    warning_fixes: AsyncFixResult[];
    total_fixed: number;
    total_failed: number;
}
"#;
