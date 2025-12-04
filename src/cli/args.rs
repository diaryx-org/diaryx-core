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
        #[command(subcommand)]
        operation: PropertyCommands,
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

    /// Open an entry in your editor
    Open {
        /// Path or date to open (supports dates, fuzzy matching, globs, directories)
        /// Examples: "today", "README", "*.md", ".", "2024-01-15"
        path: String,
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
pub enum PropertyCommands {
    /// Get a property value
    #[command(alias = "g")]
    Get {
        /// Path to the entry file (supports directories, globs, dates, fuzzy matching)
        path: String,

        /// Property key to get
        key: String,

        /// Skip confirmation prompts for multi-file operations
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Set a property value
    #[command(alias = "s")]
    Set {
        /// Path to the entry file (supports directories, globs, dates, fuzzy matching)
        path: String,

        /// Property key to set
        key: String,

        /// Value to set (as YAML)
        value: String,

        /// Skip confirmation prompts for multi-file operations
        #[arg(short = 'y', long)]
        yes: bool,

        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Remove a property
    #[command(alias = "rm")]
    Remove {
        /// Path to the entry file (supports directories, globs, dates, fuzzy matching)
        path: String,

        /// Property key to remove
        key: String,

        /// Skip confirmation prompts for multi-file operations
        #[arg(short = 'y', long)]
        yes: bool,

        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Rename a property key
    #[command(alias = "mv")]
    Rename {
        /// Path to the entry file (supports directories, globs, dates, fuzzy matching)
        path: String,

        /// Current property key
        old_key: String,

        /// New property key
        new_key: String,

        /// Skip confirmation prompts for multi-file operations
        #[arg(short = 'y', long)]
        yes: bool,

        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// List all properties in a file
    #[command(alias = "ls")]
    List {
        /// Path to the entry file (supports directories, globs, dates, fuzzy matching)
        path: String,

        /// Skip confirmation prompts for multi-file operations
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Append a value to a list property
    Append {
        /// Path to the entry file (supports directories, globs, dates, fuzzy matching)
        path: String,

        /// Property key (must be a list)
        key: String,

        /// Value to append
        value: String,

        /// Skip confirmation prompts for multi-file operations
        #[arg(short = 'y', long)]
        yes: bool,

        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Prepend a value to a list property
    Prepend {
        /// Path to the entry file (supports directories, globs, dates, fuzzy matching)
        path: String,

        /// Property key (must be a list)
        key: String,

        /// Value to prepend
        value: String,

        /// Skip confirmation prompts for multi-file operations
        #[arg(short = 'y', long)]
        yes: bool,

        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Remove a value from a list by index
    Pop {
        /// Path to the entry file (supports directories, globs, dates, fuzzy matching)
        path: String,

        /// Property key (must be a list)
        key: String,

        /// Index to remove (negative indices count from end, default: -1)
        #[arg(default_value = "-1")]
        index: i32,

        /// Skip confirmation prompts for multi-file operations
        #[arg(short = 'y', long)]
        yes: bool,

        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Set a value at a specific index in a list
    SetAt {
        /// Path to the entry file (supports directories, globs, dates, fuzzy matching)
        path: String,

        /// Property key (must be a list)
        key: String,

        /// Index to set
        index: usize,

        /// Value to set
        value: String,

        /// Skip confirmation prompts for multi-file operations
        #[arg(short = 'y', long)]
        yes: bool,

        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Remove a specific value from a list
    RemoveValue {
        /// Path to the entry file (supports directories, globs, dates, fuzzy matching)
        path: String,

        /// Property key (must be a list)
        key: String,

        /// Value to remove
        value: String,

        /// Skip confirmation prompts for multi-file operations
        #[arg(short = 'y', long)]
        yes: bool,

        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Show list items with their indices
    Show {
        /// Path to the entry file (supports directories, globs, dates, fuzzy matching)
        path: String,

        /// Property key (must be a list)
        key: String,

        /// Skip confirmation prompts for multi-file operations
        #[arg(short = 'y', long)]
        yes: bool,
    },
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
