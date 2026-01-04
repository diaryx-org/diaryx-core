//! Utility functions for date parsing and path calculations.
//!
//! This module consolidates various utility functions used across the crate.

/// Date parsing and path generation utilities.
pub mod date;
/// Path calculation utilities for relative paths.
pub mod path;

// Re-export commonly used items for convenience
pub use date::{date_to_path, parse_date, path_to_date};
pub use path::{relative_path_from_dir_to_target, relative_path_from_file_to_target};
