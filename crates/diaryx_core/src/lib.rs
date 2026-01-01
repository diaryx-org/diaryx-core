#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

/// Configuration options
pub mod config;

/// Backup system for persisting workspace data
pub mod backup;

/// Date parsing
pub mod date;

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

pub mod template;
/// Validate (check workspace link integrity)
pub mod validate;

/// Path utilities for relative path calculations
pub mod path_utils;

/// Workspace (specify a directory to work in)
pub mod workspace;

/// Live sync (CRDT-based synchronization)
#[cfg(feature = "live-sync")]
pub mod sync_crdt;

#[cfg(test)]
pub mod test_utils;
