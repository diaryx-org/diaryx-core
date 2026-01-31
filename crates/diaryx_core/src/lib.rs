#![doc = include_str!(concat!(env!("OUT_DIR"), "/README.md"))]
#![warn(missing_docs)]

/// Command pattern API for unified command execution
pub mod command;
pub use command::{Command, Response};

/// Unified Diaryx API - the main entry point
pub mod diaryx;

/// Command handler - execute() implementation for Diaryx
mod command_handler;

/// Configuration options
pub mod config;

/// Backup system for persisting workspace data
pub mod backup;

/// Entry docs
pub mod entry;

/// Error (common error types)
pub mod error;

/// Export (for backup or filtering by audience property)
pub mod export;

/// Filesystem abstraction
pub mod fs;

/// Publish (exports as HTML)
pub mod publish;

/// Search (query frontmatter or search content)
pub mod search;

/// Frontmatter parsing and manipulation utilities
pub mod frontmatter;

/// Metadata-to-frontmatter conversion and file writing utilities
pub mod metadata_writer;

/// Templates for creating new entries
pub mod template;

/// Validate (check workspace link integrity)
pub mod validate;

/// Portable path link parsing and formatting for part_of/contents properties
pub mod link_parser;

/// Utility functions (date parsing, path calculations)
pub mod utils;

/// Workspace (specify a directory to work in)
pub mod workspace;

/// CRDT-based synchronization (requires `crdt` feature)
#[cfg(feature = "crdt")]
pub mod crdt;

/// Cloud storage module for bidirectional file synchronization
pub mod cloud;

// Re-export key cloud types for convenience
pub use cloud::conflict::ConflictResolution;
pub use cloud::engine::CloudSyncProvider;
pub use cloud::manifest::SyncManifest;
pub use cloud::{CloudSyncResult, RemoteFileInfo};

// Re-exports for backwards compatibility
pub use utils::date;
pub use utils::path as path_utils;

#[cfg(test)]
pub mod test_utils;
