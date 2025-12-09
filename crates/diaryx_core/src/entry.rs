use crate::config::Config;
use crate::date::{date_to_path, parse_date};
use crate::error::{DiaryxError, Result};
use crate::fs::FileSystem;
use chrono::NaiveDate;
use indexmap::IndexMap;
use serde_yaml::Value;
use std::path::{Path, PathBuf};

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
    /// Returns an error if no frontmatter is found
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

    /// Parses a markdown file, creating empty frontmatter if none exists
    /// Creates the file if it doesn't exist
    /// Use this for operations that should create frontmatter when missing (like set)
    fn parse_file_or_create_frontmatter(
        &self,
        path: &str,
    ) -> Result<(IndexMap<String, Value>, String)> {
        let path_buf = PathBuf::from(path);

        // Try to read the file, if it doesn't exist, return empty frontmatter and body
        let content = match self.fs.read_to_string(std::path::Path::new(path)) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // File doesn't exist - return empty frontmatter and body
                // The file will be created when reconstruct_file is called
                return Ok((IndexMap::new(), String::new()));
            }
            Err(e) => {
                return Err(DiaryxError::FileRead {
                    path: path_buf,
                    source: e,
                });
            }
        };

        // Check if content starts with frontmatter delimiter
        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            // No frontmatter - return empty frontmatter and entire content as body
            return Ok((IndexMap::new(), content));
        }

        // Find the closing delimiter
        let rest = &content[4..]; // Skip first "---\n"
        let end_idx = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n"));

        match end_idx {
            Some(idx) => {
                let frontmatter_str = &rest[..idx];
                let body = &rest[idx + 5..]; // Skip "\n---\n"

                // Parse YAML frontmatter into IndexMap to preserve order
                let frontmatter: IndexMap<String, Value> = serde_yaml::from_str(frontmatter_str)?;

                Ok((frontmatter, body.to_string()))
            }
            None => {
                // Malformed frontmatter (no closing delimiter) - treat as no frontmatter
                Ok((IndexMap::new(), content))
            }
        }
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
    /// Creates frontmatter if none exists
    pub fn set_frontmatter_property(&self, path: &str, key: &str, value: Value) -> Result<()> {
        let (mut frontmatter, body) = self.parse_file_or_create_frontmatter(path)?;
        frontmatter.insert(key.to_string(), value);
        self.reconstruct_file(path, &frontmatter, &body)
    }

    /// Removes a frontmatter property
    /// Does nothing if no frontmatter exists or key is not found
    pub fn remove_frontmatter_property(&self, path: &str, key: &str) -> Result<()> {
        match self.parse_file(path) {
            Ok((mut frontmatter, body)) => {
                frontmatter.shift_remove(key);
                self.reconstruct_file(path, &frontmatter, &body)
            }
            Err(DiaryxError::NoFrontmatter(_)) => Ok(()), // No frontmatter, nothing to remove
            Err(e) => Err(e),
        }
    }

    /// Renames a frontmatter property key
    /// Returns Ok(true) if the key was found and renamed, Ok(false) if key was not found or no frontmatter
    pub fn rename_frontmatter_property(
        &self,
        path: &str,
        old_key: &str,
        new_key: &str,
    ) -> Result<bool> {
        let (frontmatter, body) = match self.parse_file(path) {
            Ok(result) => result,
            Err(DiaryxError::NoFrontmatter(_)) => return Ok(false), // No frontmatter, key not found
            Err(e) => return Err(e),
        };

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
    /// Returns Ok(None) if no frontmatter exists or key is not found
    pub fn get_frontmatter_property(&self, path: &str, key: &str) -> Result<Option<Value>> {
        match self.parse_file(path) {
            Ok((frontmatter, _)) => Ok(frontmatter.get(key).cloned()),
            Err(DiaryxError::NoFrontmatter(_)) => Ok(None), // No frontmatter, key not found
            Err(e) => Err(e),
        }
    }

    /// Gets all frontmatter properties
    /// Returns empty map if no frontmatter exists
    pub fn get_all_frontmatter(&self, path: &str) -> Result<IndexMap<String, Value>> {
        match self.parse_file(path) {
            Ok((frontmatter, _)) => Ok(frontmatter),
            Err(DiaryxError::NoFrontmatter(_)) => Ok(IndexMap::new()), // No frontmatter, return empty
            Err(e) => Err(e),
        }
    }

    // ==================== Content Methods ====================

    /// Get the content (body) of a file, excluding frontmatter
    pub fn get_content(&self, path: &str) -> Result<String> {
        let (_, body) = self.parse_file_or_create_frontmatter(path)?;
        Ok(body)
    }

    /// Set the content (body) of a file, preserving frontmatter
    /// Creates frontmatter if none exists
    pub fn set_content(&self, path: &str, content: &str) -> Result<()> {
        let (frontmatter, _) = self.parse_file_or_create_frontmatter(path)?;
        self.reconstruct_file(path, &frontmatter, content)
    }

    /// Clear the content (body) of a file, preserving frontmatter
    pub fn clear_content(&self, path: &str) -> Result<()> {
        self.set_content(path, "")
    }

    /// Append content to the end of a file's body
    pub fn append_content(&self, path: &str, content: &str) -> Result<()> {
        let (frontmatter, body) = self.parse_file_or_create_frontmatter(path)?;
        let new_body = if body.is_empty() {
            content.to_string()
        } else if body.ends_with('\n') {
            format!("{}{}", body, content)
        } else {
            format!("{}\n{}", body, content)
        };
        self.reconstruct_file(path, &frontmatter, &new_body)
    }

    /// Prepend content to the beginning of a file's body
    pub fn prepend_content(&self, path: &str, content: &str) -> Result<()> {
        let (frontmatter, body) = self.parse_file_or_create_frontmatter(path)?;
        let new_body = if body.is_empty() {
            content.to_string()
        } else if content.ends_with('\n') {
            format!("{}{}", content, body)
        } else {
            format!("{}\n{}", content, body)
        };
        self.reconstruct_file(path, &frontmatter, &new_body)
    }

    // ==================== Frontmatter Sorting ====================

    /// Sort frontmatter keys according to a pattern
    /// Pattern is comma-separated keys, with "*" meaning "rest alphabetically"
    /// Example: "title,description,*" puts title first, description second, rest alphabetically
    /// Does nothing if no frontmatter exists (won't add empty frontmatter)
    pub fn sort_frontmatter(&self, path: &str, pattern: Option<&str>) -> Result<()> {
        let (frontmatter, body) = match self.parse_file(path) {
            Ok(result) => result,
            Err(DiaryxError::NoFrontmatter(_)) => return Ok(()), // No frontmatter, nothing to sort
            Err(e) => return Err(e),
        };

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
        Ok(date_to_path(&config.daily_entry_dir(), &date))
    }

    /// Resolve a path string - either a date string ("today", "2024-01-15") or a literal path
    /// If it parses as a date, returns the dated entry path. Otherwise, treats it as a literal path.
    pub fn resolve_path(&self, path_str: &str, config: &Config) -> PathBuf {
        // Try to parse as a date first
        if let Ok(date) = parse_date(path_str) {
            date_to_path(&config.daily_entry_dir(), &date)
        } else {
            // Treat as literal path
            PathBuf::from(path_str)
        }
    }

    /// Create a dated entry with proper frontmatter and index hierarchy
    pub fn create_dated_entry(&self, date: &NaiveDate, config: &Config) -> Result<PathBuf> {
        let daily_dir = config.daily_entry_dir();
        let path = date_to_path(&daily_dir, date);

        // Create parent directories if they don't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Ensure the index hierarchy exists
        self.ensure_daily_index_hierarchy(date, config)?;

        // Create entry with date-based frontmatter
        let date_str = date.format("%Y-%m-%d").to_string();
        let title = date.format("%B %d, %Y").to_string(); // e.g., "January 15, 2024"
        let month_index_name = Self::month_index_filename(date);

        let content = format!(
            "---\ndate: {}\ntitle: {}\npart_of: {}\n---\n\n# {}\n\n",
            date_str, title, month_index_name, title
        );

        self.fs.create_new(&path, &content)?;

        // Add entry to month index contents
        let month_index_path = path.parent().unwrap().join(&month_index_name);
        self.add_to_index_contents(&month_index_path, &format!("{}.md", date_str))?;

        Ok(path)
    }

    /// Ensure a dated entry exists, creating it if necessary
    /// This will NEVER overwrite an existing file
    pub fn ensure_dated_entry(&self, date: &NaiveDate, config: &Config) -> Result<PathBuf> {
        let path = date_to_path(&config.daily_entry_dir(), date);

        // Check if file already exists using FileSystem trait
        if self.fs.exists(&path) {
            return Ok(path);
        }

        // Create the entry (create_new will fail if file exists)
        self.create_dated_entry(date, config)
    }

    /// Ensure the daily index hierarchy exists for a given date
    /// Creates: daily_index.md -> YYYY_index.md -> YYYY_month.md
    /// Also connects daily_index.md to workspace root if daily_entry_folder is configured
    fn ensure_daily_index_hierarchy(&self, date: &NaiveDate, config: &Config) -> Result<()> {
        let daily_dir = config.daily_entry_dir();
        let year = date.format("%Y").to_string();
        let month = date.format("%m").to_string();

        // Paths for each level
        let daily_index_path = daily_dir.join("daily_index.md");
        let year_dir = daily_dir.join(&year);
        let year_index_path = year_dir.join(Self::year_index_filename(date));
        let month_dir = year_dir.join(&month);
        let month_index_path = month_dir.join(Self::month_index_filename(date));

        // Create directories
        std::fs::create_dir_all(&month_dir)?;

        // 1. Ensure daily_index.md exists
        let daily_index_created = !self.fs.exists(&daily_index_path);
        if daily_index_created {
            // Determine if we should link to workspace root
            let part_of = if config.daily_entry_folder.is_some() {
                // Daily folder is configured, try to link to workspace root
                self.find_workspace_root_relative(&daily_dir)
            } else {
                None
            };
            self.create_daily_index(&daily_index_path, part_of.as_deref())?;

            // If we have a workspace root, add daily_index to its contents
            if let Some(ref root_rel) = part_of {
                let workspace_root = daily_dir.join(root_rel);
                if self.fs.exists(&workspace_root) {
                    // Calculate relative path from workspace root to daily_index
                    if let Some(daily_folder) = &config.daily_entry_folder {
                        let daily_index_rel = format!("{}/daily_index.md", daily_folder);
                        self.add_to_index_contents(&workspace_root, &daily_index_rel)?;
                    }
                }
            }
        }

        // 2. Ensure year index exists
        if !self.fs.exists(&year_index_path) {
            self.create_year_index(&year_index_path, date)?;
            // Add year index to daily index contents
            let year_index_rel = format!("{}/{}", year, Self::year_index_filename(date));
            self.add_to_index_contents(&daily_index_path, &year_index_rel)?;
        }

        // 3. Ensure month index exists
        if !self.fs.exists(&month_index_path) {
            self.create_month_index(&month_index_path, date)?;
            // Add month index to year index contents
            let month_index_rel = format!("{}/{}", month, Self::month_index_filename(date));
            self.add_to_index_contents(&year_index_path, &month_index_rel)?;
        }

        Ok(())
    }

    /// Find the workspace root index relative to a directory
    fn find_workspace_root_relative(&self, from_dir: &Path) -> Option<String> {
        // Look for README.md in parent directory (workspace root)
        let parent = from_dir.parent()?;
        let readme_path = parent.join("README.md");

        if self.fs.exists(&readme_path) {
            // Check if it's actually an index (has contents property)
            let readme_str = readme_path.to_string_lossy();
            if let Ok(Some(_)) = self.get_frontmatter_property(&readme_str, "contents") {
                return Some("../README.md".to_string());
            }
            // Even without contents, if it exists we can link to it
            return Some("../README.md".to_string());
        }

        None
    }

    /// Create the root daily index file
    fn create_daily_index(&self, path: &Path, part_of: Option<&str>) -> Result<()> {
        let part_of_line = match part_of {
            Some(p) => format!("part_of: {}\n", p),
            None => String::new(),
        };

        let content = format!(
            "---\n\
            title: Daily Entries\n\
            {}contents: []\n\
            ---\n\n\
            # Daily Entries\n\n\
            This index contains all daily journal entries organized by year and month.\n",
            part_of_line
        );

        self.fs.write_file(path, &content)?;
        Ok(())
    }

    /// Create a year index file
    fn create_year_index(&self, path: &Path, date: &NaiveDate) -> Result<()> {
        let year = date.format("%Y").to_string();
        let content = format!(
            "---\n\
            title: {year}\n\
            part_of: ../daily_index.md\n\
            contents: []\n\
            ---\n\n\
            # {year}\n\n\
            Daily entries for {year}.\n"
        );

        self.fs.write_file(path, &content)?;
        Ok(())
    }

    /// Create a month index file
    fn create_month_index(&self, path: &Path, date: &NaiveDate) -> Result<()> {
        let year = date.format("%Y").to_string();
        let month_name = date.format("%B").to_string(); // e.g., "January"
        let title = format!("{} {}", month_name, year);
        let year_index_name = Self::year_index_filename(date);

        let content = format!(
            "---\n\
            title: {title}\n\
            part_of: ../{year_index_name}\n\
            contents: []\n\
            ---\n\n\
            # {title}\n\n\
            Daily entries for {title}.\n"
        );

        self.fs.write_file(path, &content)?;
        Ok(())
    }

    /// Add an entry to an index's contents list
    fn add_to_index_contents(&self, index_path: &Path, entry: &str) -> Result<()> {
        let index_str = index_path.to_string_lossy();

        match self.get_frontmatter_property(&index_str, "contents") {
            Ok(Some(Value::Sequence(mut items))) => {
                let entry_value = Value::String(entry.to_string());
                if !items.contains(&entry_value) {
                    items.push(entry_value);
                    // Sort contents for consistent ordering
                    items.sort_by(|a, b| {
                        let a_str = a.as_str().unwrap_or("");
                        let b_str = b.as_str().unwrap_or("");
                        a_str.cmp(b_str)
                    });
                    self.set_frontmatter_property(&index_str, "contents", Value::Sequence(items))?;
                }
            }
            Ok(None) => {
                // Create contents with just this entry
                let items = vec![Value::String(entry.to_string())];
                self.set_frontmatter_property(&index_str, "contents", Value::Sequence(items))?;
            }
            _ => {
                // Contents exists but isn't a sequence, or error reading - skip
            }
        }

        Ok(())
    }

    /// Generate the year index filename (e.g., "2025_index.md")
    fn year_index_filename(date: &NaiveDate) -> String {
        format!("{}_index.md", date.format("%Y"))
    }

    /// Generate the month index filename (e.g., "2025_january.md")
    fn month_index_filename(date: &NaiveDate) -> String {
        format!(
            "{}_{}.md",
            date.format("%Y"),
            date.format("%B").to_string().to_lowercase()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    /// A mock filesystem for testing
    #[derive(Clone, Default)]
    struct MockFileSystem {
        files: Arc<Mutex<HashMap<PathBuf, String>>>,
    }

    impl MockFileSystem {
        fn new() -> Self {
            Self {
                files: Arc::new(Mutex::new(HashMap::new())),
            }
        }

        fn with_file(self, path: &str, content: &str) -> Self {
            self.files
                .lock()
                .unwrap()
                .insert(PathBuf::from(path), content.to_string());
            self
        }

        fn get_content(&self, path: &str) -> Option<String> {
            self.files
                .lock()
                .unwrap()
                .get(&PathBuf::from(path))
                .cloned()
        }
    }

    impl crate::fs::FileSystem for MockFileSystem {
        fn read_to_string(&self, path: &Path) -> io::Result<String> {
            self.files
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "File not found"))
        }

        fn write_file(&self, path: &Path, content: &str) -> io::Result<()> {
            self.files
                .lock()
                .unwrap()
                .insert(path.to_path_buf(), content.to_string());
            Ok(())
        }

        fn exists(&self, path: &Path) -> bool {
            self.files.lock().unwrap().contains_key(path)
        }

        fn create_new(&self, path: &Path, content: &str) -> io::Result<()> {
            let mut files = self.files.lock().unwrap();
            if files.contains_key(path) {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, "File exists"));
            }
            files.insert(path.to_path_buf(), content.to_string());
            Ok(())
        }

        fn delete_file(&self, path: &Path) -> io::Result<()> {
            self.files.lock().unwrap().remove(path);
            Ok(())
        }

        fn list_md_files(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
            let files = self.files.lock().unwrap();
            let mut result = Vec::new();
            for path in files.keys() {
                if path.parent() == Some(dir) && path.extension().is_some_and(|ext| ext == "md") {
                    result.push(path.clone());
                }
            }
            Ok(result)
        }
    }

    #[test]
    fn test_get_content() {
        let fs = MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\nHello, world!");
        let app = DiaryxApp::new(fs);

        let content = app.get_content("test.md").unwrap();
        assert_eq!(content, "Hello, world!");
    }

    #[test]
    fn test_get_content_empty_body() {
        let fs = MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\n");
        let app = DiaryxApp::new(fs);

        let content = app.get_content("test.md").unwrap();
        assert_eq!(content, "");
    }

    #[test]
    fn test_get_content_no_frontmatter() {
        let fs = MockFileSystem::new().with_file("test.md", "Just plain content");
        let app = DiaryxApp::new(fs);

        let content = app.get_content("test.md").unwrap();
        assert_eq!(content, "Just plain content");
    }

    #[test]
    fn test_set_content() {
        let fs = MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\nOld content");
        let app = DiaryxApp::new(fs.clone());

        app.set_content("test.md", "New content").unwrap();

        let result = fs.get_content("test.md").unwrap();
        assert!(result.contains("title: Test"));
        assert!(result.contains("New content"));
        assert!(!result.contains("Old content"));
    }

    #[test]
    fn test_set_content_preserves_frontmatter() {
        let fs = MockFileSystem::new().with_file(
            "test.md",
            "---\ntitle: My Title\ndescription: A description\n---\nOld",
        );
        let app = DiaryxApp::new(fs.clone());

        app.set_content("test.md", "New body").unwrap();

        let result = fs.get_content("test.md").unwrap();
        assert!(result.contains("title: My Title"));
        assert!(result.contains("description: A description"));
        assert!(result.contains("New body"));
    }

    #[test]
    fn test_clear_content() {
        let fs =
            MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\nSome content here");
        let app = DiaryxApp::new(fs.clone());

        app.clear_content("test.md").unwrap();

        let result = fs.get_content("test.md").unwrap();
        assert!(result.contains("title: Test"));
        // Content should be empty after the frontmatter closing
        assert!(result.ends_with("---\n"));
    }

    #[test]
    fn test_append_content() {
        let fs = MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\nFirst line");
        let app = DiaryxApp::new(fs.clone());

        app.append_content("test.md", "Second line").unwrap();

        let result = fs.get_content("test.md").unwrap();
        assert!(result.contains("First line"));
        assert!(result.contains("Second line"));
        // Second line should come after first
        let first_pos = result.find("First line").unwrap();
        let second_pos = result.find("Second line").unwrap();
        assert!(second_pos > first_pos);
    }

    #[test]
    fn test_append_content_adds_newline() {
        let fs = MockFileSystem::new()
            .with_file("test.md", "---\ntitle: Test\n---\nNo trailing newline");
        let app = DiaryxApp::new(fs.clone());

        app.append_content("test.md", "Appended").unwrap();

        let content = app.get_content("test.md").unwrap();
        assert!(content.contains("No trailing newline\nAppended"));
    }

    #[test]
    fn test_append_content_to_empty_body() {
        let fs = MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\n");
        let app = DiaryxApp::new(fs.clone());

        app.append_content("test.md", "New content").unwrap();

        let content = app.get_content("test.md").unwrap();
        assert_eq!(content, "New content");
    }

    #[test]
    fn test_prepend_content() {
        let fs =
            MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\nExisting content");
        let app = DiaryxApp::new(fs.clone());

        app.prepend_content("test.md", "# Header").unwrap();

        let result = fs.get_content("test.md").unwrap();
        assert!(result.contains("# Header"));
        assert!(result.contains("Existing content"));
        // Header should come before existing content
        let header_pos = result.find("# Header").unwrap();
        let existing_pos = result.find("Existing content").unwrap();
        assert!(header_pos < existing_pos);
    }

    #[test]
    fn test_prepend_content_adds_newline() {
        let fs = MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\nExisting");
        let app = DiaryxApp::new(fs.clone());

        app.prepend_content("test.md", "Prepended").unwrap();

        let content = app.get_content("test.md").unwrap();
        assert!(content.contains("Prepended\nExisting"));
    }

    #[test]
    fn test_prepend_content_to_empty_body() {
        let fs = MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\n");
        let app = DiaryxApp::new(fs.clone());

        app.prepend_content("test.md", "New content").unwrap();

        let content = app.get_content("test.md").unwrap();
        assert_eq!(content, "New content");
    }

    #[test]
    fn test_content_operations_on_nonexistent_file() {
        let fs = MockFileSystem::new();
        let app = DiaryxApp::new(fs.clone());

        // set_content should create the file
        app.set_content("new.md", "Content").unwrap();

        let result = fs.get_content("new.md").unwrap();
        assert!(result.contains("Content"));
    }

    #[test]
    fn test_multiple_content_operations() {
        let fs = MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\n");
        let app = DiaryxApp::new(fs.clone());

        app.append_content("test.md", "Line 1").unwrap();
        app.append_content("test.md", "Line 2").unwrap();
        app.prepend_content("test.md", "# Title").unwrap();

        let content = app.get_content("test.md").unwrap();
        assert!(content.starts_with("# Title"));
        assert!(content.contains("Line 1"));
        assert!(content.contains("Line 2"));
    }
}
