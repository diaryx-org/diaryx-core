use crate::fs::FileSystem;
use serde_yaml::Value;
use std::collections::HashMap;

pub struct DiaryxApp<FS: FileSystem> {
    fs: FS,
}

#[derive(Debug)]
pub enum FrontmatterError {
    Io(std::io::Error),
    Yaml(serde_yaml::Error),
    NoFrontmatter,
    InvalidFrontmatter,
}

impl From<std::io::Error> for FrontmatterError {
    fn from(err: std::io::Error) -> Self {
        FrontmatterError::Io(err)
    }
}

impl From<serde_yaml::Error> for FrontmatterError {
    fn from(err: serde_yaml::Error) -> Self {
        FrontmatterError::Yaml(err)
    }
}

impl std::fmt::Display for FrontmatterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrontmatterError::Io(e) => write!(f, "IO error: {}", e),
            FrontmatterError::Yaml(e) => write!(f, "YAML error: {}", e),
            FrontmatterError::NoFrontmatter => write!(f, "No frontmatter found"),
            FrontmatterError::InvalidFrontmatter => write!(f, "Invalid frontmatter structure"),
        }
    }
}

impl std::error::Error for FrontmatterError {}

impl<FS: FileSystem> DiaryxApp<FS> {
    pub fn new(fs: FS) -> Self {
        Self { fs }
    }

    pub fn create_entry(&self, path: &str) -> std::io::Result<()> {
        let content = format!("---\ntitle: {}\n---\n\n# {}\n\n", path, path);
        self.fs.create_new(std::path::Path::new(path), &content)?;
        Ok(())
    }

    /// Parses a markdown file and extracts frontmatter and body
    fn parse_file(&self, path: &str) -> Result<(HashMap<String, Value>, String), FrontmatterError> {
        let content = self.fs.read_to_string(std::path::Path::new(path))?;

        // Check if content starts with frontmatter delimiter
        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            return Err(FrontmatterError::NoFrontmatter);
        }

        // Find the closing delimiter
        let rest = &content[4..]; // Skip first "---\n"
        let end_idx = rest.find("\n---\n")
            .or_else(|| rest.find("\n---\r\n"))
            .ok_or(FrontmatterError::NoFrontmatter)?;

        let frontmatter_str = &rest[..end_idx];
        let body = &rest[end_idx + 5..]; // Skip "\n---\n"

        // Parse YAML frontmatter
        let frontmatter: HashMap<String, Value> = serde_yaml::from_str(frontmatter_str)?;

        Ok((frontmatter, body.to_string()))
    }

    /// Reconstructs a markdown file with updated frontmatter
    fn reconstruct_file(&self, path: &str, frontmatter: &HashMap<String, Value>, body: &str) -> Result<(), FrontmatterError> {
        let yaml_str = serde_yaml::to_string(frontmatter)?;
        let content = format!("---\n{}---\n{}", yaml_str, body);
        self.fs.write_file(std::path::Path::new(path), &content)?;
        Ok(())
    }

    /// Adds or updates a frontmatter property
    pub fn set_frontmatter_property(&self, path: &str, key: &str, value: Value) -> Result<(), FrontmatterError> {
        let (mut frontmatter, body) = self.parse_file(path)?;
        frontmatter.insert(key.to_string(), value);
        self.reconstruct_file(path, &frontmatter, &body)
    }

    /// Removes a frontmatter property
    pub fn remove_frontmatter_property(&self, path: &str, key: &str) -> Result<(), FrontmatterError> {
        let (mut frontmatter, body) = self.parse_file(path)?;
        frontmatter.remove(key);
        self.reconstruct_file(path, &frontmatter, &body)
    }

    /// Gets a frontmatter property value
    pub fn get_frontmatter_property(&self, path: &str, key: &str) -> Result<Option<Value>, FrontmatterError> {
        let (frontmatter, _) = self.parse_file(path)?;
        Ok(frontmatter.get(key).cloned())
    }

    /// Gets all frontmatter properties
    pub fn get_all_frontmatter(&self, path: &str) -> Result<HashMap<String, Value>, FrontmatterError> {
        let (frontmatter, _) = self.parse_file(path)?;
        Ok(frontmatter)
    }
}
