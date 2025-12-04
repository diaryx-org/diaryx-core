//! Command-line argument structures and enums

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "diaryx")]
#[command(about = "A tool to manage markdown files with YAML frontmatter", long_about = None)]
pub struct Cli {
    /// Override workspace location
    #[arg(short, long, global = true)]
    pub workspace: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create a new diary entry with default frontmatter
    Create {
        /// Path to the new entry file
        path: String,
    },

    /// Manipulate frontmatter properties
    #[command(alias = "p")]
    Property {
        /// Path to the entry file (supports dates like "today", "yesterday", "last friday", or glob patterns like "*.md")
        path: String,

        /// Property key (if omitted, lists all properties)
        key: Option<String>,

        #[command(subcommand)]
        operation: Option<PropertyOperation>,

        /// Operate on the property as a list
        #[arg(short, long)]
        list: bool,

        /// Skip confirmation prompts for multi-file operations
        #[arg(short = 'y', long, global = true)]
        yes: bool,

        /// Show what would be done without making changes
        #[arg(long, global = true)]
        dry_run: bool,
    },

    /// Initialize diaryx configuration and workspace
    Init {
        /// Base directory for diary entries (default: ~/diaryx)
        #[arg(short, long)]
        base_dir: Option<PathBuf>,

        /// Title for the workspace
        #[arg(short, long)]
        title: Option<String>,

        /// Description for the workspace
        #[arg(short = 'D', long)]
        description: Option<String>,
    },

    /// Open today's entry in your editor
    Today,

    /// Open yesterday's entry in your editor
    Yesterday,

    /// Open an entry for a specific date
    Open {
        /// Date to open (e.g., "2024-01-15", "today", "yesterday", "last friday")
        date: String,
    },

    /// Show current configuration
    Config,

    /// Sort frontmatter keys
    Sort {
        /// Path to the entry file (supports dates or glob patterns like "*.md")
        path: String,

        /// Custom sort pattern: comma-separated keys with "*" for rest alphabetically
        /// Example: "title,description,*"
        #[arg(short, long)]
        pattern: Option<String>,

        /// Sort alphabetically (default)
        #[arg(long, group = "preset")]
        abc: bool,

        /// Sort with common metadata first: title, description, author, date, tags, *
        #[arg(long, group = "preset")]
        default: bool,

        /// Sort with index/workspace fields first: title, description, part_of, contents, *
        #[arg(long, group = "preset")]
        index: bool,

        /// Skip confirmation prompts for multi-file operations
        #[arg(short = 'y', long)]
        yes: bool,

        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Workspace management commands
    #[command(alias = "space")]
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommands,
    },
}

#[derive(Subcommand, Clone)]
pub enum PropertyOperation {
    /// Get the property value (default if no operation specified)
    Get,

    /// Set the property value
    Set {
        /// Value to set (as YAML)
        value: String,
    },

    /// Remove the property
    Remove,

    /// Rename the property key
    Rename {
        /// New key name
        new_key: String,
    },

    /// List operations - append a value
    Append {
        /// Value to append
        value: String,
    },

    /// List operations - prepend a value
    Prepend {
        /// Value to prepend
        value: String,
    },

    /// List operations - pop a value by index
    Pop {
        /// Index to remove (default: last item)
        #[arg(default_value = "-1")]
        index: i32,
    },

    /// List operations - set value at index
    SetAt {
        /// Index to set
        index: usize,

        /// Value to set
        value: String,
    },

    /// List operations - remove by value
    RemoveValue {
        /// Value to remove
        value: String,
    },

    /// Show list with indices
    Show,
}

#[derive(Subcommand)]
pub enum WorkspaceCommands {
    /// Show workspace info as a tree
    Info,

    /// Initialize a new workspace in the current or specified directory
    Init {
        /// Directory to initialize (default: current directory)
        #[arg(short, long)]
        dir: Option<PathBuf>,

        /// Title for the workspace
        #[arg(short, long)]
        title: Option<String>,

        /// Description for the workspace
        #[arg(short = 'D', long)]
        description: Option<String>,
    },

    /// Show the current workspace root path
    Path,
}
