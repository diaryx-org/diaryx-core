use chrono::{Local, NaiveDate};
use chrono_english::{parse_date_string, Dialect};
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

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
}
