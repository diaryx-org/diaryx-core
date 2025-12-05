//! Command-line argument structures and enums

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "diaryx")]
#[command(version)]
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
        /// Default workspace directory (default: ~/diaryx)
        #[arg(short = 'd', long, alias = "base-dir")]
        default_workspace: Option<PathBuf>,

        /// Subfolder for daily entries (e.g., "Daily" or "Journal/Daily")
        #[arg(long)]
        daily_folder: Option<String>,

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
    #[command(aliases = ["space", "w"])]
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommands,
    },

    /// Normalize filename(s) to match their title property
    /// Converts title to snake_case slug and renames file
    #[command(alias = "norm")]
    NormalizeFilename {
        /// Path to file(s) (supports directories, globs, dates, fuzzy matching, title:)
        path: String,

        /// Set this title before normalizing (also updates the title property)
        #[arg(short, long)]
        title: Option<String>,

        /// Skip confirmation prompts for multi-file operations
        #[arg(short = 'y', long)]
        yes: bool,

        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
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

    /// Move/rename a file while updating the workspace hierarchy
    /// Updates contents and part_of references automatically
    #[command(alias = "move")]
    Mv {
        /// Source file path (supports fuzzy matching)
        source: String,

        /// Destination file path
        dest: String,

        /// Create a new index file as the parent for the moved file
        /// Example: --new-index archive_index creates archive_index.md in dest directory
        #[arg(long, value_name = "NAME")]
        new_index: Option<String>,

        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Find orphan files (markdown files not in the workspace hierarchy)
    Orphans {
        /// Directory to search for orphans (default: current directory)
        #[arg(short, long)]
        dir: Option<PathBuf>,

        /// Search recursively in subdirectories
        #[arg(short, long)]
        recursive: bool,
    },

    /// Add an existing file as a child of a parent index
    /// Updates both the parent's `contents` and the child's `part_of`
    /// If only one argument is provided, uses the local index as parent
    /// Supports globs and multiple files (will skip the parent automatically)
    Add {
        /// Parent index file, or child file if only one argument (supports fuzzy matching, globs)
        parent_or_child: String,

        /// Child file(s) to add (optional if parent_or_child is the child, supports globs)
        child: Option<String>,

        /// Create a new index file to hold the added files
        /// Example: --new-index docs_index creates docs_index.md as parent
        #[arg(long, value_name = "NAME")]
        new_index: Option<String>,

        /// Skip confirmation prompts for multi-file operations
        #[arg(short = 'y', long)]
        yes: bool,

        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Create a new child file under a parent index
    /// If only one argument is provided, uses the local index as parent
    Create {
        /// Parent index file, or name if only one argument (supports fuzzy matching)
        parent_or_name: String,

        /// Name for the new child file (optional if parent_or_name is the name)
        name: Option<String>,

        /// Title for the new file (defaults to name)
        #[arg(short, long)]
        title: Option<String>,

        /// Description for the new file
        #[arg(short = 'D', long)]
        description: Option<String>,

        /// Make the new file an index (add empty `contents` property)
        #[arg(short, long)]
        index: bool,

        /// Open the new file in editor after creating
        #[arg(short, long)]
        edit: bool,
    },

    /// Remove a child from a parent's hierarchy
    /// Updates both the parent's `contents` and the child's `part_of`, but does not delete the file
    /// If only one argument is provided, uses the local index as parent
    #[command(alias = "rm")]
    Remove {
        /// Parent index file, or child file if only one argument (supports fuzzy matching)
        parent_or_child: String,

        /// Child file to remove (optional if parent_or_child is the child)
        child: Option<String>,

        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },
}
