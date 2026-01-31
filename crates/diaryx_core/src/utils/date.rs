//! Date parsing and path generation utilities.
//!
//! This module provides natural language date parsing via chrono-english,
//! and path generation for daily entries following the `YYYY/MM/YYYY-MM-DD.md` format.
//!
//! # Supported Date Formats
//!
//! - **Relative**: `"today"`, `"yesterday"`, `"tomorrow"`, `"3 days ago"`, `"in 5 days"`
//! - **Named days**: `"last friday"`, `"next monday"`, `"this wednesday"`
//! - **Periods**: `"last week"`, `"last month"`
//! - **ISO 8601**: `"2024-01-15"`, `"2024-12-31"`
//!
//! # Key Functions
//!
//! - [`parse_date()`]: Parse natural language or ISO dates into `NaiveDate`
//! - [`date_to_path()`]: Generate `YYYY/MM/YYYY-MM-DD.md` path from date
//! - [`path_to_date()`]: Extract date from a daily entry path
//! - [`is_daily_entry()`]: Check if a path matches daily entry format
//! - [`get_adjacent_daily_entry_path()`]: Navigate between daily entries
//!
//! # Example
//!
//! ```ignore
//! use diaryx_core::date::{parse_date, date_to_path};
//! use std::path::Path;
//!
//! let date = parse_date("yesterday")?;
//! let path = date_to_path(Path::new("/diary"), &date);
//! // e.g., "/diary/2024/01/2024-01-14.md"
//! ```

use chrono::{Duration, Local, NaiveDate};
use chrono_english::{Dialect, parse_date_string};
use std::path::{Path, PathBuf};

use crate::error::{DiaryxError, Result};

/// Parse a date string into a NaiveDate
/// Supports natural language dates via chrono-english:
/// - "today", "yesterday", "tomorrow"
/// - "3 days ago", "in 5 days"
/// - "last friday", "next monday", "this wednesday"
/// - "last week", "last month"
/// - "YYYY-MM-DD" format
pub fn parse_date(date_str: &str) -> Result<NaiveDate> {
    let now = Local::now();

    // First try parsing as YYYY-MM-DD for exact dates
    if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        return Ok(date);
    }

    // Use chrono-english for natural language parsing
    parse_date_string(date_str, now, Dialect::Us)
        .map(|dt| dt.date_naive())
        .map_err(|_| DiaryxError::InvalidDateFormat(date_str.to_string()))
}

/// Generate the file path for a given date
/// Format: {base_dir}/YYYY/MM/YYYY-MM-DD.md
pub fn date_to_path(base_dir: &Path, date: &NaiveDate) -> PathBuf {
    let year = date.format("%Y").to_string();
    let month = date.format("%m").to_string();
    let filename = format!("{}.md", date.format("%Y-%m-%d"));

    base_dir.join(&year).join(&month).join(filename)
}

/// Extract date from a path if it matches the expected format
/// Returns None if path doesn't match YYYY/MM/YYYY-MM-DD.md
pub fn path_to_date(path: &Path) -> Option<NaiveDate> {
    let filename = path.file_stem()?.to_str()?;
    NaiveDate::parse_from_str(filename, "%Y-%m-%d").ok()
}

/// Check if a path represents a daily entry.
/// Returns true if the path matches the pattern: .../YYYY/MM/YYYY-MM-DD.md
pub fn is_daily_entry(path: &Path) -> bool {
    // Check filename matches YYYY-MM-DD.md
    let filename = match path.file_stem().and_then(|s| s.to_str()) {
        Some(name) => name,
        None => return false,
    };

    // Try to parse as date
    let date = match NaiveDate::parse_from_str(filename, "%Y-%m-%d") {
        Ok(d) => d,
        Err(_) => return false,
    };

    // Check parent directory is a month (01-12)
    let parent = match path.parent() {
        Some(p) => p,
        None => return false,
    };
    let month_dir = match parent.file_name().and_then(|s| s.to_str()) {
        Some(name) => name,
        None => return false,
    };

    // Verify month directory matches the date's month
    let expected_month = date.format("%m").to_string();
    if month_dir != expected_month {
        return false;
    }

    // Check grandparent directory is a year
    let grandparent = match parent.parent() {
        Some(p) => p,
        None => return false,
    };
    let year_dir = match grandparent.file_name().and_then(|s| s.to_str()) {
        Some(name) => name,
        None => return false,
    };

    // Verify year directory matches the date's year
    let expected_year = date.format("%Y").to_string();
    year_dir == expected_year
}

/// Get the date of an adjacent day from a daily entry path.
/// Returns None if the path is not a daily entry.
pub fn get_adjacent_date(path: &Path, offset_days: i64) -> Option<NaiveDate> {
    if !is_daily_entry(path) {
        return None;
    }

    let date = path_to_date(path)?;
    date.checked_add_signed(Duration::days(offset_days))
}

/// Get the path to an adjacent daily entry.
/// The path will be in the same base directory structure as the original.
/// Returns None if the path is not a daily entry.
pub fn get_adjacent_daily_entry_path(path: &Path, offset_days: i64) -> Option<PathBuf> {
    if !is_daily_entry(path) {
        return None;
    }

    let date = path_to_date(path)?;
    let new_date = date.checked_add_signed(Duration::days(offset_days))?;

    // Navigate up to the base directory (above YYYY/MM/)
    // path: base_dir/YYYY/MM/YYYY-MM-DD.md
    // We need to get base_dir
    let month_dir = path.parent()?;
    let year_dir = month_dir.parent()?;
    let base_dir = year_dir.parent()?;

    Some(date_to_path(base_dir, &new_date))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_date_iso_format() {
        let date = parse_date("2024-01-15").unwrap();
        assert_eq!(date, NaiveDate::from_ymd_opt(2024, 1, 15).unwrap());
    }

    #[test]
    fn test_parse_date_today() {
        let date = parse_date("today").unwrap();
        assert_eq!(date, Local::now().date_naive());
    }

    #[test]
    fn test_parse_date_yesterday() {
        let date = parse_date("yesterday").unwrap();
        assert_eq!(date, Local::now().date_naive() - Duration::days(1));
    }

    #[test]
    fn test_parse_date_tomorrow() {
        let date = parse_date("tomorrow").unwrap();
        assert_eq!(date, Local::now().date_naive() + Duration::days(1));
    }

    #[test]
    fn test_parse_date_days_ago() {
        let date = parse_date("3 days ago").unwrap();
        assert_eq!(date, Local::now().date_naive() - Duration::days(3));
    }

    #[test]
    fn test_parse_date_invalid() {
        assert!(parse_date("not a date").is_err());
    }

    #[test]
    fn test_date_to_path() {
        let base = PathBuf::from("/home/user/diary");
        let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        let path = date_to_path(&base, &date);

        assert_eq!(
            path,
            PathBuf::from("/home/user/diary/2024/01/2024-01-15.md")
        );
    }

    #[test]
    fn test_path_to_date() {
        let path = PathBuf::from("/home/user/diary/2024/01/2024-01-15.md");
        let date = path_to_date(&path).unwrap();

        assert_eq!(date, NaiveDate::from_ymd_opt(2024, 1, 15).unwrap());

        // Test invalid path
        let invalid_path = PathBuf::from("/home/user/diary/random.md");
        assert!(path_to_date(&invalid_path).is_none());
    }

    #[test]
    fn test_is_daily_entry() {
        // Valid daily entry path
        let valid_path = PathBuf::from("/home/user/diary/2024/01/2024-01-15.md");
        assert!(is_daily_entry(&valid_path));

        // Invalid: wrong month directory
        let wrong_month = PathBuf::from("/home/user/diary/2024/02/2024-01-15.md");
        assert!(!is_daily_entry(&wrong_month));

        // Invalid: wrong year directory
        let wrong_year = PathBuf::from("/home/user/diary/2023/01/2024-01-15.md");
        assert!(!is_daily_entry(&wrong_year));

        // Invalid: not a date filename
        let not_date = PathBuf::from("/home/user/diary/2024/01/notes.md");
        assert!(!is_daily_entry(&not_date));

        // Invalid: regular file
        let regular = PathBuf::from("/home/user/notes/todo.md");
        assert!(!is_daily_entry(&regular));
    }

    #[test]
    fn test_get_adjacent_date() {
        let path = PathBuf::from("/home/user/diary/2024/01/2024-01-15.md");

        // Next day
        let next = get_adjacent_date(&path, 1).unwrap();
        assert_eq!(next, NaiveDate::from_ymd_opt(2024, 1, 16).unwrap());

        // Previous day
        let prev = get_adjacent_date(&path, -1).unwrap();
        assert_eq!(prev, NaiveDate::from_ymd_opt(2024, 1, 14).unwrap());

        // Non-daily entry returns None
        let regular = PathBuf::from("/home/user/notes/todo.md");
        assert!(get_adjacent_date(&regular, 1).is_none());
    }

    #[test]
    fn test_get_adjacent_daily_entry_path() {
        let path = PathBuf::from("/home/user/diary/2024/01/2024-01-15.md");

        // Next day
        let next = get_adjacent_daily_entry_path(&path, 1).unwrap();
        assert_eq!(
            next,
            PathBuf::from("/home/user/diary/2024/01/2024-01-16.md")
        );

        // Previous day
        let prev = get_adjacent_daily_entry_path(&path, -1).unwrap();
        assert_eq!(
            prev,
            PathBuf::from("/home/user/diary/2024/01/2024-01-14.md")
        );

        // Cross month boundary (Jan 1 -> Dec 31 of previous year)
        let jan_first = PathBuf::from("/home/user/diary/2024/01/2024-01-01.md");
        let dec_last = get_adjacent_daily_entry_path(&jan_first, -1).unwrap();
        assert_eq!(
            dec_last,
            PathBuf::from("/home/user/diary/2023/12/2023-12-31.md")
        );

        // Non-daily entry returns None
        let regular = PathBuf::from("/home/user/notes/todo.md");
        assert!(get_adjacent_daily_entry_path(&regular, 1).is_none());
    }
}
