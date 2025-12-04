use crate::config::Config;
use crate::date::{date_to_path, parse_date};
use crate::error::{DiaryxError, Result};
use crate::fs::FileSystem;
use chrono::NaiveDate;
use indexmap::IndexMap;
use serde_yaml::Value;
use std::path::PathBuf;

pub struct DiaryxApp<FS: FileSystem> {
    fs: FS,
}

impl<FS: FileSystem> DiaryxApp<FS> {
    pub fn new(fs: FS) -> Self {
        Self { fs }
    }

    pub fn create_entry(&self, path: &str) -> Result<()> {
        let content = format!("---\ntitle: {}\n---\n\n# {}\n\n", path, path);
        self.fs.create_new(std::path::Path::new(path), &content)?;
        Ok(())
    }

    /// Parses a markdown file and extracts frontmatter and body
    fn parse_file(&self, path: &str) -> Result<(IndexMap<String, Value>, String)> {
        let path_buf = PathBuf::from(path);
        let content = self
            .fs
            .read_to_string(std::path::Path::new(path))
            .map_err(|e| DiaryxError::FileRead {
                path: path_buf.clone(),
                source: e,
            })?;

        // Check if content starts with frontmatter delimiter
        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            return Err(DiaryxError::NoFrontmatter(path_buf));
        }

        // Find the closing delimiter
        let rest = &content[4..]; // Skip first "---\n"
        let end_idx = rest
            .find("\n---\n")
            .or_else(|| rest.find("\n---\r\n"))
            .ok_or_else(|| DiaryxError::NoFrontmatter(path_buf.clone()))?;

        let frontmatter_str = &rest[..end_idx];
        let body = &rest[end_idx + 5..]; // Skip "\n---\n"

        // Parse YAML frontmatter into IndexMap to preserve order
        let frontmatter: IndexMap<String, Value> = serde_yaml::from_str(frontmatter_str)?;

        Ok((frontmatter, body.to_string()))
    }

    /// Reconstructs a markdown file with updated frontmatter
    fn reconstruct_file(
        &self,
        path: &str,
        frontmatter: &IndexMap<String, Value>,
        body: &str,
    ) -> Result<()> {
        let yaml_str = serde_yaml::to_string(frontmatter)?;
        let content = format!("---\n{}---\n{}", yaml_str, body);
        self.fs
            .write_file(std::path::Path::new(path), &content)
            .map_err(|e| DiaryxError::FileWrite {
                path: PathBuf::from(path),
                source: e,
            })?;
        Ok(())
    }

    /// Adds or updates a frontmatter property
    pub fn set_frontmatter_property(&self, path: &str, key: &str, value: Value) -> Result<()> {
        let (mut frontmatter, body) = self.parse_file(path)?;
        frontmatter.insert(key.to_string(), value);
        self.reconstruct_file(path, &frontmatter, &body)
    }

    /// Removes a frontmatter property
    pub fn remove_frontmatter_property(&self, path: &str, key: &str) -> Result<()> {
        let (mut frontmatter, body) = self.parse_file(path)?;
        frontmatter.shift_remove(key);
        self.reconstruct_file(path, &frontmatter, &body)
    }

    /// Renames a frontmatter property key
    /// Returns Ok(true) if the key was found and renamed, Ok(false) if key was not found
    pub fn rename_frontmatter_property(
        &self,
        path: &str,
        old_key: &str,
        new_key: &str,
    ) -> Result<bool> {
        let (frontmatter, body) = self.parse_file(path)?;

        if !frontmatter.contains_key(old_key) {
            return Ok(false);
        }

        // Rebuild the map, replacing old_key with new_key at the same position
        let mut result: IndexMap<String, Value> = IndexMap::new();
        for (k, v) in frontmatter {
            if k == old_key {
                result.insert(new_key.to_string(), v);
            } else {
                result.insert(k, v);
            }
        }

        self.reconstruct_file(path, &result, &body)?;
        Ok(true)
    }

    /// Gets a frontmatter property value
    pub fn get_frontmatter_property(&self, path: &str, key: &str) -> Result<Option<Value>> {
        let (frontmatter, _) = self.parse_file(path)?;
        Ok(frontmatter.get(key).cloned())
    }

    /// Gets all frontmatter properties
    pub fn get_all_frontmatter(&self, path: &str) -> Result<IndexMap<String, Value>> {
        let (frontmatter, _) = self.parse_file(path)?;
        Ok(frontmatter)
    }

    /// Sort frontmatter keys according to a pattern
    /// Pattern is comma-separated keys, with "*" meaning "rest alphabetically"
    /// Example: "title,description,*" puts title first, description second, rest alphabetically
    pub fn sort_frontmatter(&self, path: &str, pattern: Option<&str>) -> Result<()> {
        let (frontmatter, body) = self.parse_file(path)?;

        let sorted = match pattern {
            Some(p) => self.sort_by_pattern(frontmatter, p),
            None => self.sort_alphabetically(frontmatter),
        };

        self.reconstruct_file(path, &sorted, &body)
    }

    fn sort_alphabetically(&self, frontmatter: IndexMap<String, Value>) -> IndexMap<String, Value> {
        let mut pairs: Vec<_> = frontmatter.into_iter().collect();
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        pairs.into_iter().collect()
    }

    fn sort_by_pattern(
        &self,
        frontmatter: IndexMap<String, Value>,
        pattern: &str,
    ) -> IndexMap<String, Value> {
        let priority_keys: Vec<&str> = pattern.split(',').map(|s| s.trim()).collect();

        let mut result = IndexMap::new();
        let mut remaining: IndexMap<String, Value> = frontmatter;

        for key in &priority_keys {
            if *key == "*" {
                // Insert remaining keys alphabetically
                let mut rest: Vec<_> = remaining.drain(..).collect();
                rest.sort_by(|a, b| a.0.cmp(&b.0));
                for (k, v) in rest {
                    result.insert(k, v);
                }
                break;
            } else if let Some(value) = remaining.shift_remove(*key) {
                result.insert(key.to_string(), value);
            }
        }

        // If no "*" was in pattern, append any remaining keys alphabetically
        if !remaining.is_empty() {
            let mut rest: Vec<_> = remaining.drain(..).collect();
            rest.sort_by(|a, b| a.0.cmp(&b.0));
            for (k, v) in rest {
                result.insert(k, v);
            }
        }

        result
    }

    /// Get the full path for a date-based entry
    pub fn get_dated_entry_path(&self, date_str: &str, config: &Config) -> Result<PathBuf> {
        let date = parse_date(date_str)?;
        Ok(date_to_path(&config.base_dir, &date))
    }

    /// Resolve a path string - either a date string ("today", "2024-01-15") or a literal path
    /// If it parses as a date, returns the dated entry path. Otherwise, treats it as a literal path.
    pub fn resolve_path(&self, path_str: &str, config: &Config) -> PathBuf {
        // Try to parse as a date first
        if let Ok(date) = parse_date(path_str) {
            date_to_path(&config.base_dir, &date)
        } else {
            // Treat as literal path
            PathBuf::from(path_str)
        }
    }

    /// Create a dated entry with proper frontmatter
    pub fn create_dated_entry(&self, date: &NaiveDate, config: &Config) -> Result<PathBuf> {
        let path = date_to_path(&config.base_dir, date);

        // Create parent directories if they don't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Create entry with date-based frontmatter
        let date_str = date.format("%Y-%m-%d").to_string();
        let title = date.format("%B %d, %Y").to_string(); // e.g., "January 15, 2024"

        let content = format!(
            "---\ndate: {}\ntitle: {}\n---\n\n# {}\n\n",
            date_str, title, title
        );

        self.fs.create_new(&path, &content)?;

        Ok(path)
    }

    /// Ensure a dated entry exists, creating it if necessary
    /// This will NEVER overwrite an existing file
    pub fn ensure_dated_entry(&self, date: &NaiveDate, config: &Config) -> Result<PathBuf> {
        let path = date_to_path(&config.base_dir, date);

        // Check if file already exists using FileSystem trait
        if self.fs.exists(&path) {
            return Ok(path);
        }

        // Create the entry (create_new will fail if file exists)
        self.create_dated_entry(date, config)
    }
}
