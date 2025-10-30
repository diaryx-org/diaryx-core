use serde::{Deserialize, Deserializer, Serialize};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime};
use std::collections::HashMap;

/// A flexible DateTime that can be either naive (no timezone) or timezone-aware
#[derive(Debug, Clone)]
pub enum FlexibleDateTime {
  NaiveDateTime(NaiveDateTime),
  NaiveDate(NaiveDate),
  DateTime(DateTime<FixedOffset>),
}

impl<'de> Deserialize<'de> for FlexibleDateTime {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let s = String::deserialize(deserializer)?;

    // Try parsing as timezone-aware DateTime first
    if let Ok(dt) = DateTime::parse_from_rfc3339(&s) {
      return Ok(FlexibleDateTime::DateTime(dt));
    }

    // Try parsing as ISO 8601 with timezone
    if let Ok(dt) = DateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S%z") {
      return Ok(FlexibleDateTime::DateTime(dt));
    }

    // Try parsing as naive DateTime (with time)
    if let Ok(dt) = NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S") {
      return Ok(FlexibleDateTime::NaiveDateTime(dt));
    }

    if let Ok(dt) = NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S") {
      return Ok(FlexibleDateTime::NaiveDateTime(dt));
    }

    // Try parsing as just a date (YYYY-MM-DD)
    if let Ok(date) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
      return Ok(FlexibleDateTime::NaiveDate(date));
    }

    Err(serde::de::Error::custom(format!(
      "Unable to parse date/time: {}",
      s
    )))
  }
}

impl Serialize for FlexibleDateTime {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::Serializer,
  {
    match self {
      FlexibleDateTime::NaiveDateTime(dt) => serializer.serialize_str(&dt.to_string()),
      FlexibleDateTime::NaiveDate(date) => serializer.serialize_str(&date.to_string()),
      FlexibleDateTime::DateTime(dt) => serializer.serialize_str(&dt.to_rfc3339()),
    }
  }
}

/// For those who use journaling to track their health, Diaryx has optional support for a wide array of health metrics: mood, activity, sleep, vitals, and nutrition.
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthProperties {
  pub mood: Option<String>,
  pub activity: Option<String>,
  pub sleep: Option<String>,
}

/// Represents the frontmatter of a Diaryx formatted document.
#[derive(Debug, Serialize, Deserialize)]
pub struct Frontmatter {
  // Required
  pub title: String,
  pub author: Vec<String>,
  pub audience: Vec<String>,

  // Recommended
  pub created: Option<FlexibleDateTime>,
  pub updated: Option<FlexibleDateTime>,
  pub format: Option<String>,
  pub reachable: Option<String>,

  // Optional
  pub contents: Option<String>,
  pub part_of: Option<String>,
  pub version: Option<String>,
  pub copying: Option<String>,
  pub tags: Option<Vec<String>>,
  pub aliases: Option<Vec<String>>,

  pub health: Option<HealthProperties>,

  pub coordinates: Option<String>,
  pub location: Option<String>,
  pub position: Option<String>,
  pub weather: Option<Vec<String>>,
  pub created_on_hardware: Option<Vec<String>>,
  pub created_on_software: Option<Vec<String>>,


  #[serde(flatten)]
  pub extra: HashMap<String, serde_yaml::Value>,
}

#[derive(Debug)]
pub struct Document {
  pub frontmatter: Option<Frontmatter>,
  pub content: String,
}

impl Document {
  /// Parse a markdown document with YAML frontmatter
  pub fn parse(markdown: &str) -> Result<Self, Box<dyn std::error::Error>> {
      // Check if document starts with frontmatter delimiter
      if !markdown.starts_with("---") {
          return Ok(Document {
              frontmatter: None,
              content: markdown.to_string(),
          });
      }

      // Find the closing delimiter
      let rest = &markdown[3..]; // Skip first "---"
      if let Some(end_pos) = rest.find("\n---\n") {
          let frontmatter_str = &rest[..end_pos];
          let content = &rest[end_pos + 5..]; // Skip "\n---\n"

          let frontmatter: Frontmatter = serde_yaml::from_str(frontmatter_str)?;

          Ok(Document {
              frontmatter: Some(frontmatter),
              content: content.to_string(),
          })
      } else {
          // No closing delimiter found, treat whole thing as content
          Ok(Document {
              frontmatter: None,
              content: markdown.to_string(),
          })
      }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_parse_document_with_naive_date() {
    let markdown = r#"---
title: Test Document
author:
  - Alice
audience:
  - developers
created: 2025-10-23
---

# Hello

Test content"#;

    let doc = Document::parse(markdown).unwrap();
    assert!(doc.frontmatter.is_some());

    let frontmatter = doc.frontmatter.unwrap();
    assert_eq!(frontmatter.title, "Test Document");
    assert_eq!(frontmatter.author, vec!["Alice"]);
    assert!(matches!(frontmatter.created, Some(FlexibleDateTime::NaiveDate(_))));
    assert_eq!(doc.content.trim(), "# Hello\n\nTest content");
  }

  #[test]
  fn test_parse_document_with_timezone_aware_date() {
    let markdown = r#"---
title: Test Document
author:
  - Bob
audience:
  - users
created: 2025-10-23T14:30:00-05:00
---

# Content"#;

    let doc = Document::parse(markdown).unwrap();
    let frontmatter = doc.frontmatter.unwrap();
    assert!(matches!(frontmatter.created, Some(FlexibleDateTime::DateTime(_))));
  }

  #[test]
  fn test_parse_document_without_frontmatter() {
    let markdown = "# Just a heading\n\nNo frontmatter here.";

    let doc = Document::parse(markdown).unwrap();
    assert!(doc.frontmatter.is_none());
    assert_eq!(doc.content, markdown);
  }

  #[test]
  fn test_extra_fields_in_frontmatter() {
    let markdown = r#"---
title: Test
author:
  - Charlie
audience:
  - all
created: 2025-10-23
custom_field: custom_value
another_field: 123
---

Content"#;

    let doc = Document::parse(markdown).unwrap();
    let frontmatter = doc.frontmatter.unwrap();

    assert!(frontmatter.extra.contains_key("custom_field"));
    assert!(frontmatter.extra.contains_key("another_field"));
  }
}
