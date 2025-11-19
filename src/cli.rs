#![cfg(feature = "cli")]

use clap::{Parser, Subcommand};
use diaryx_core::app::DiaryxApp;
use diaryx_core::fs::RealFileSystem;
use serde_yaml::Value;

#[derive(Parser)]
#[command(name = "diaryx")]
#[command(about = "A tool to manage markdown files with YAML frontmatter", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new diary entry with default frontmatter
    Create {
        /// Path to the new entry file
        path: String,
    },

    /// Set a frontmatter property (adds or updates)
    Set {
        /// Path to the entry file
        path: String,

        /// Property key to set
        key: String,

        /// Property value (as YAML - e.g., "hello", "42", "[1,2,3]", "{a: 1}")
        value: String,
    },

    /// Get a frontmatter property value
    Get {
        /// Path to the entry file
        path: String,

        /// Property key to get
        key: String,
    },

    /// Remove a frontmatter property
    Remove {
        /// Path to the entry file
        path: String,

        /// Property key to remove
        key: String,
    },

    /// List all frontmatter properties
    List {
        /// Path to the entry file
        path: String,
    },
}

pub fn run_cli() {
    let cli = Cli::parse();

    // Setup dependencies
    let fs = RealFileSystem;
    let app = DiaryxApp::new(fs);

    // Execute commands
    match cli.command {
        Commands::Create { path } => {
            match app.create_entry(&path) {
                Ok(_) => println!("✓ Created entry: {}", path),
                Err(e) => eprintln!("✗ Error creating entry: {}", e),
            }
        }

        Commands::Set { path, key, value } => {
            // Parse the value as YAML
            match serde_yaml::from_str::<Value>(&value) {
                Ok(yaml_value) => {
                    match app.set_frontmatter_property(&path, &key, yaml_value) {
                        Ok(_) => println!("✓ Set '{}' in {}", key, path),
                        Err(e) => eprintln!("✗ Error setting property: {}", e),
                    }
                }
                Err(e) => eprintln!("✗ Invalid YAML value: {}", e),
            }
        }

        Commands::Get { path, key } => {
            match app.get_frontmatter_property(&path, &key) {
                Ok(Some(value)) => {
                    println!("{}: {}", key, serde_yaml::to_string(&value).unwrap_or_default().trim());
                }
                Ok(None) => {
                    eprintln!("Property '{}' not found in {}", key, path);
                }
                Err(e) => eprintln!("✗ Error getting property: {}", e),
            }
        }

        Commands::Remove { path, key } => {
            match app.remove_frontmatter_property(&path, &key) {
                Ok(_) => println!("✓ Removed '{}' from {}", key, path),
                Err(e) => eprintln!("✗ Error removing property: {}", e),
            }
        }

        Commands::List { path } => {
            match app.get_all_frontmatter(&path) {
                Ok(frontmatter) => {
                    if frontmatter.is_empty() {
                        println!("No frontmatter properties in {}", path);
                    } else {
                        println!("Frontmatter in {}:", path);
                        for (key, value) in frontmatter {
                            println!("  {}: {}", key, serde_yaml::to_string(&value).unwrap_or_default().trim());
                        }
                    }
                }
                Err(e) => eprintln!("✗ Error listing frontmatter: {}", e),
            }
        }
    }
}
