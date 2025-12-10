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

        /// Template to use (default: "note")
        #[arg(short, long)]
        template: Option<String>,

        /// Title for the entry (defaults to filename)
        #[arg(long)]
        title: Option<String>,
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
    Today {
        /// Template to use (default: config's daily_template or "daily")
        #[arg(short, long)]
        template: Option<String>,
    },

    /// Open yesterday's entry in your editor
    Yesterday {
        /// Template to use (default: config's daily_template or "daily")
        #[arg(short, long)]
        template: Option<String>,
    },

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

    /// Uninstall diaryx by removing the binary
    /// Does not remove any files created by diaryx (config, workspace, entries, etc.)
    Uninstall {
        /// Confirm uninstallation without prompting
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Export workspace filtered by audience
    /// Creates a copy of the workspace with only files visible to the specified audience
    Export {
        /// Target audience to export for (e.g., "family", "public", "work")
        #[arg(short, long)]
        audience: String,

        /// Destination directory for the export
        destination: PathBuf,

        /// Overwrite existing destination
        #[arg(short, long)]
        force: bool,

        /// Keep the audience property in exported files
        #[arg(long)]
        keep_audience: bool,

        /// Show detailed information about what's being exported/excluded
        #[arg(short, long)]
        verbose: bool,

        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Manipulate file content (body text after frontmatter)
    #[command(alias = "c")]
    Content {
        #[command(subcommand)]
        operation: ContentCommands,
    },

    /// Search workspace files by content or frontmatter
    #[command(alias = "s")]
    Search {
        /// Search pattern (text to find)
        pattern: String,

        /// Search in frontmatter instead of content
        #[arg(short, long)]
        frontmatter: bool,

        /// Search a specific frontmatter property (implies --frontmatter)
        #[arg(short, long)]
        property: Option<String>,

        /// Case-sensitive search
        #[arg(short = 'S', long)]
        case_sensitive: bool,

        /// Maximum number of results to show
        #[arg(short, long)]
        limit: Option<usize>,

        /// Lines of context around matches (default: 0)
        #[arg(short, long, default_value = "0")]
        context: usize,

        /// Only show match counts per file
        #[arg(long)]
        count: bool,
    },

    /// Manage templates for creating entries
    #[command(alias = "tmpl")]
    Template {
        #[command(subcommand)]
        command: TemplateCommands,
    },

    /// Publish workspace as HTML for sharing
    #[command(alias = "pub")]
    Publish {
        /// Destination path (directory for multi-file, file for single-file)
        destination: PathBuf,

        /// Target audience to publish for (filters files by audience property)
        #[arg(short, long)]
        audience: Option<String>,

        /// Output as a single HTML file instead of multiple files
        #[arg(long)]
        single_file: bool,

        /// Site title (defaults to workspace title)
        #[arg(short, long)]
        title: Option<String>,

        /// Overwrite existing destination
        #[arg(short, long)]
        force: bool,

        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand, Clone)]
pub enum ContentCommands {
    /// Get the content (body) of a file
    #[command(alias = "g")]
    Get {
        /// Path to the entry file (supports fuzzy matching, dates, globs)
        path: String,
    },

    /// Set/replace the content (body) of a file
    #[command(alias = "s")]
    Set {
        /// Path to the entry file (supports fuzzy matching, dates)
        path: String,

        /// Content to set (use --file or --stdin to read from elsewhere)
        content: Option<String>,

        /// Read content from a file
        #[arg(short, long, value_name = "FILE", conflicts_with = "stdin")]
        file: Option<PathBuf>,

        /// Read content from stdin
        #[arg(long, conflicts_with = "file")]
        stdin: bool,

        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Clear the content (body) of a file, keeping frontmatter
    Clear {
        /// Path to the entry file (supports fuzzy matching, dates)
        path: String,

        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,

        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Append content to the end of a file's body
    #[command(alias = "a")]
    Append {
        /// Path to the entry file (supports fuzzy matching, dates)
        path: String,

        /// Content to append (use --file or --stdin to read from elsewhere)
        content: Option<String>,

        /// Read content from a file
        #[arg(short, long, value_name = "FILE", conflicts_with = "stdin")]
        file: Option<PathBuf>,

        /// Read content from stdin
        #[arg(long, conflicts_with = "file")]
        stdin: bool,

        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Prepend content to the beginning of a file's body
    Prepend {
        /// Path to the entry file (supports fuzzy matching, dates)
        path: String,

        /// Content to prepend (use --file or --stdin to read from elsewhere)
        content: Option<String>,

        /// Read content from a file
        #[arg(short, long, value_name = "FILE", conflicts_with = "stdin")]
        file: Option<PathBuf>,

        /// Read content from stdin
        #[arg(long, conflicts_with = "file")]
        stdin: bool,

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
    Info {
        /// Path to show tree for (use "." for current directory's index)
        path: Option<String>,

        /// Maximum depth to display (default: 3, use 0 for unlimited)
        #[arg(short, long, default_value = "3")]
        depth: usize,
    },

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
        /// With --recursive, this should be a directory path
        parent_or_child: String,

        /// Child file(s) to add (optional if parent_or_child is the child, supports globs)
        child: Option<String>,

        /// Create a new index file to hold the added files
        /// Example: --new-index docs_index creates docs_index.md as parent
        #[arg(long, value_name = "NAME")]
        new_index: Option<String>,

        /// Recursively add all files in subdirectories, creating indexes for each
        /// Each directory gets a <dirname>_index.md file
        #[arg(short, long)]
        recursive: bool,

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

        /// Template to use (default: "note")
        #[arg(long)]
        template: Option<String>,

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

#[derive(Subcommand)]
pub enum TemplateCommands {
    /// List all available templates
    #[command(alias = "ls")]
    List {
        /// Show full paths instead of just names
        #[arg(short, long)]
        paths: bool,
    },

    /// Show a template's contents
    #[command(alias = "cat")]
    Show {
        /// Name of the template to show
        name: String,
    },

    /// Create a new custom template
    New {
        /// Name for the new template
        name: String,

        /// Create from an existing template
        #[arg(short, long)]
        from: Option<String>,

        /// Open in editor after creating
        #[arg(short, long)]
        edit: bool,
    },

    /// Edit an existing template
    Edit {
        /// Name of the template to edit
        name: String,
    },

    /// Delete a custom template
    #[command(alias = "rm")]
    Delete {
        /// Name of the template to delete
        name: String,

        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Show the directories where templates are stored
    Path,

    /// List available template variables
    Variables,
}
