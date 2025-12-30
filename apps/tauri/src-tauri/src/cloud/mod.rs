//! Cloud backup targets for Tauri app.
//!
//! Implements cloud storage backends (S3, Google Drive, etc.) for the backup system.

mod s3;
mod google_drive;

pub use s3::S3Target;
pub use google_drive::GoogleDriveTarget;
