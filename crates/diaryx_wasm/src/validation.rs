//! Validation operations for WASM.

use std::path::PathBuf;

use diaryx_core::validate::{ValidationFixer, Validator};
use diaryx_core::workspace::Workspace;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::error::IntoJsResult;
use crate::state::{block_on, with_async_fs};

// ============================================================================
// Types
// ============================================================================

/// Validation error returned to JavaScript
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum JsValidationError {
    BrokenPartOf { file: String, target: String },
    BrokenContentsRef { index: String, target: String },
    BrokenAttachment { file: String, attachment: String },
}

impl From<diaryx_core::validate::ValidationError> for JsValidationError {
    fn from(err: diaryx_core::validate::ValidationError) -> Self {
        use diaryx_core::validate::ValidationError;
        match err {
            ValidationError::BrokenPartOf { file, target } => JsValidationError::BrokenPartOf {
                file: file.to_string_lossy().to_string(),
                target,
            },
            ValidationError::BrokenContentsRef { index, target } => {
                JsValidationError::BrokenContentsRef {
                    index: index.to_string_lossy().to_string(),
                    target,
                }
            }
            ValidationError::BrokenAttachment { file, attachment } => {
                JsValidationError::BrokenAttachment {
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
pub enum JsValidationWarning {
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

impl From<diaryx_core::validate::ValidationWarning> for JsValidationWarning {
    fn from(warn: diaryx_core::validate::ValidationWarning) -> Self {
        use diaryx_core::validate::ValidationWarning;
        match warn {
            ValidationWarning::OrphanFile { file } => JsValidationWarning::OrphanFile {
                file: file.to_string_lossy().to_string(),
            },
            ValidationWarning::CircularReference { files } => {
                JsValidationWarning::CircularReference {
                    files: files
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect(),
                }
            }
            ValidationWarning::UnlinkedEntry { path, is_dir } => {
                JsValidationWarning::UnlinkedEntry {
                    path: path.to_string_lossy().to_string(),
                    is_dir,
                }
            }
            ValidationWarning::UnlistedFile { index, file } => JsValidationWarning::UnlistedFile {
                index: index.to_string_lossy().to_string(),
                file: file.to_string_lossy().to_string(),
            },
            ValidationWarning::NonPortablePath {
                file,
                property,
                value,
                suggested,
            } => JsValidationWarning::NonPortablePath {
                file: file.to_string_lossy().to_string(),
                property,
                value,
                suggested,
            },
            ValidationWarning::MultipleIndexes { directory, indexes } => {
                JsValidationWarning::MultipleIndexes {
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
            } => JsValidationWarning::OrphanBinaryFile {
                file: file.to_string_lossy().to_string(),
                suggested_index: suggested_index.map(|p| p.to_string_lossy().to_string()),
            },
            ValidationWarning::MissingPartOf {
                file,
                suggested_index,
            } => JsValidationWarning::MissingPartOf {
                file: file.to_string_lossy().to_string(),
                suggested_index: suggested_index.map(|p| p.to_string_lossy().to_string()),
            },
        }
    }
}

/// Validation result returned to JavaScript
#[derive(Debug, Serialize, Deserialize)]
pub struct JsValidationResult {
    pub errors: Vec<JsValidationError>,
    pub warnings: Vec<JsValidationWarning>,
    pub files_checked: usize,
}

/// Fix result returned to JavaScript
#[derive(Debug, Serialize, Deserialize)]
pub struct JsFixResult {
    pub success: bool,
    pub message: String,
}

impl From<diaryx_core::validate::FixResult> for JsFixResult {
    fn from(result: diaryx_core::validate::FixResult) -> Self {
        JsFixResult {
            success: result.success,
            message: result.message,
        }
    }
}

/// Summary of fix operations
#[derive(Debug, Serialize, Deserialize)]
pub struct JsFixSummary {
    pub error_fixes: Vec<JsFixResult>,
    pub warning_fixes: Vec<JsFixResult>,
    pub total_fixed: usize,
    pub total_failed: usize,
}

// ============================================================================
// DiaryxValidation Class
// ============================================================================

/// Validation operations for checking workspace integrity.
#[wasm_bindgen]
pub struct DiaryxValidation;

#[wasm_bindgen]
impl DiaryxValidation {
    /// Create a new DiaryxValidation instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Validate workspace links.
    #[wasm_bindgen]
    pub fn validate(&self, workspace_path: &str) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let validator = Validator::new(fs.clone());
            let root_path = PathBuf::from(workspace_path);

            let ws = Workspace::new(fs);
            let root_index = block_on(ws.find_root_index_in_dir(&root_path))
                .js_err()?
                .or_else(|| {
                    block_on(ws.find_any_index_in_dir(&root_path))
                        .ok()
                        .flatten()
                })
                .ok_or_else(|| {
                    JsValue::from_str(&format!("No workspace found at '{}'", workspace_path))
                })?;

            let result = block_on(validator.validate_workspace(&root_index)).js_err()?;

            let js_result = JsValidationResult {
                errors: result
                    .errors
                    .into_iter()
                    .map(JsValidationError::from)
                    .collect(),
                warnings: result
                    .warnings
                    .into_iter()
                    .map(JsValidationWarning::from)
                    .collect(),
                files_checked: result.files_checked,
            };

            serde_wasm_bindgen::to_value(&js_result).js_err()
        })
    }

    /// Validate a single file's links.
    #[wasm_bindgen]
    pub fn validate_file(&self, file_path: &str) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let validator = Validator::new(fs);
            let path = PathBuf::from(file_path);

            let result = block_on(validator.validate_file(&path)).js_err()?;

            let js_result = JsValidationResult {
                errors: result
                    .errors
                    .into_iter()
                    .map(JsValidationError::from)
                    .collect(),
                warnings: result
                    .warnings
                    .into_iter()
                    .map(JsValidationWarning::from)
                    .collect(),
                files_checked: result.files_checked,
            };

            serde_wasm_bindgen::to_value(&js_result).js_err()
        })
    }

    /// Fix a broken part_of reference by removing it.
    #[wasm_bindgen]
    pub fn fix_broken_part_of(&self, file_path: &str) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let fixer = ValidationFixer::new(fs);
            let path = PathBuf::from(file_path);
            let result = block_on(fixer.fix_broken_part_of(&path));
            serde_wasm_bindgen::to_value(&JsFixResult::from(result)).js_err()
        })
    }

    /// Fix a broken contents reference by removing it.
    #[wasm_bindgen]
    pub fn fix_broken_contents_ref(
        &self,
        index_path: &str,
        target: &str,
    ) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let fixer = ValidationFixer::new(fs);
            let path = PathBuf::from(index_path);
            let result = block_on(fixer.fix_broken_contents_ref(&path, target));
            serde_wasm_bindgen::to_value(&JsFixResult::from(result)).js_err()
        })
    }

    /// Fix a broken attachment reference by removing it.
    #[wasm_bindgen]
    pub fn fix_broken_attachment(
        &self,
        file_path: &str,
        attachment: &str,
    ) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let fixer = ValidationFixer::new(fs);
            let path = PathBuf::from(file_path);
            let result = block_on(fixer.fix_broken_attachment(&path, attachment));
            serde_wasm_bindgen::to_value(&JsFixResult::from(result)).js_err()
        })
    }

    /// Fix a non-portable path by normalizing it.
    #[wasm_bindgen]
    pub fn fix_non_portable_path(
        &self,
        file_path: &str,
        property: &str,
        old_value: &str,
        new_value: &str,
    ) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let fixer = ValidationFixer::new(fs);
            let path = PathBuf::from(file_path);
            let result =
                block_on(fixer.fix_non_portable_path(&path, property, old_value, new_value));
            serde_wasm_bindgen::to_value(&JsFixResult::from(result)).js_err()
        })
    }

    /// Add an unlisted file to an index's contents.
    #[wasm_bindgen]
    pub fn fix_unlisted_file(&self, index_path: &str, file_path: &str) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let fixer = ValidationFixer::new(fs);
            let index = PathBuf::from(index_path);
            let file = PathBuf::from(file_path);
            let result = block_on(fixer.fix_unlisted_file(&index, &file));
            serde_wasm_bindgen::to_value(&JsFixResult::from(result)).js_err()
        })
    }

    /// Add an orphan binary file to an index's attachments.
    #[wasm_bindgen]
    pub fn fix_orphan_binary_file(
        &self,
        index_path: &str,
        file_path: &str,
    ) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let fixer = ValidationFixer::new(fs);
            let index = PathBuf::from(index_path);
            let file = PathBuf::from(file_path);
            let result = block_on(fixer.fix_orphan_binary_file(&index, &file));
            serde_wasm_bindgen::to_value(&JsFixResult::from(result)).js_err()
        })
    }

    /// Fix a missing part_of by setting it to point to the given index.
    #[wasm_bindgen]
    pub fn fix_missing_part_of(
        &self,
        file_path: &str,
        index_path: &str,
    ) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let fixer = ValidationFixer::new(fs);
            let file = PathBuf::from(file_path);
            let index = PathBuf::from(index_path);
            let result = block_on(fixer.fix_missing_part_of(&file, &index));
            serde_wasm_bindgen::to_value(&JsFixResult::from(result)).js_err()
        })
    }

    /// Fix all errors and fixable warnings in a validation result.
    ///
    /// Takes a validation result (from validate or validate_file) and attempts
    /// to fix all issues.
    #[wasm_bindgen]
    pub fn fix_all(&self, validation_result: JsValue) -> Result<JsValue, JsValue> {
        with_async_fs(|fs| {
            let fixer = ValidationFixer::new(fs);

            // Parse the JS validation result
            let js_result: JsValidationResult =
                serde_wasm_bindgen::from_value(validation_result).js_err()?;

            // Convert back to core types for fixing
            let mut error_fixes = Vec::new();
            let mut warning_fixes = Vec::new();

            // Fix errors
            for err in &js_result.errors {
                let result = match err {
                    JsValidationError::BrokenPartOf { file, target: _ } => {
                        block_on(fixer.fix_broken_part_of(&PathBuf::from(file)))
                    }
                    JsValidationError::BrokenContentsRef { index, target } => {
                        block_on(fixer.fix_broken_contents_ref(&PathBuf::from(index), target))
                    }
                    JsValidationError::BrokenAttachment { file, attachment } => {
                        block_on(fixer.fix_broken_attachment(&PathBuf::from(file), attachment))
                    }
                };
                error_fixes.push(JsFixResult::from(result));
            }

            // Fix warnings
            for warn in &js_result.warnings {
                let result = match warn {
                    JsValidationWarning::UnlistedFile { index, file } => Some(block_on(
                        fixer.fix_unlisted_file(&PathBuf::from(index), &PathBuf::from(file)),
                    )),
                    JsValidationWarning::NonPortablePath {
                        file,
                        property,
                        value,
                        suggested,
                    } => Some(block_on(fixer.fix_non_portable_path(
                        &PathBuf::from(file),
                        property,
                        value,
                        suggested,
                    ))),
                    JsValidationWarning::OrphanBinaryFile {
                        file,
                        suggested_index,
                    } => {
                        suggested_index.as_ref().map(|index| {
                            block_on(fixer.fix_orphan_binary_file(
                                &PathBuf::from(index),
                                &PathBuf::from(file),
                            ))
                        })
                    }
                    JsValidationWarning::MissingPartOf {
                        file,
                        suggested_index,
                    } => suggested_index.as_ref().map(|index| {
                        block_on(
                            fixer.fix_missing_part_of(&PathBuf::from(file), &PathBuf::from(index)),
                        )
                    }),
                    // These cannot be auto-fixed
                    JsValidationWarning::OrphanFile { .. }
                    | JsValidationWarning::UnlinkedEntry { .. }
                    | JsValidationWarning::CircularReference { .. }
                    | JsValidationWarning::MultipleIndexes { .. } => None,
                };

                if let Some(r) = result {
                    warning_fixes.push(JsFixResult::from(r));
                }
            }

            let total_fixed = error_fixes.iter().filter(|r| r.success).count()
                + warning_fixes.iter().filter(|r| r.success).count();
            let total_failed = error_fixes.iter().filter(|r| !r.success).count()
                + warning_fixes.iter().filter(|r| !r.success).count();

            let summary = JsFixSummary {
                error_fixes,
                warning_fixes,
                total_fixed,
                total_failed,
            };

            serde_wasm_bindgen::to_value(&summary).js_err()
        })
    }
}

impl Default for DiaryxValidation {
    fn default() -> Self {
        Self::new()
    }
}
