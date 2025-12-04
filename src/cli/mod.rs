//! CLI module - command-line interface for diaryx

mod args;
mod entry;
mod property;
mod sort;
mod util;
mod workspace;

use clap::Parser;
use std::path::PathBuf;

use diaryx_core::config::Config;
use diaryx_core::entry::DiaryxApp;
use diaryx_core::fs::RealFileSystem;
use diaryx_core::workspace::Workspace;

pub use args::Cli;
use args::Commands;

/// Main entry point for the CLI
pub fn run_cli() {
    let cli = Cli::parse();

    // Setup dependencies
    let fs = RealFileSystem;
    let app = DiaryxApp::new(fs);
    let ws = Workspace::new(RealFileSystem);

    // Execute commands
    match cli.command {
        Commands::Init {
            base_dir,
            title,
            description,
        } => {
            handle_init(base_dir, title, description, &ws);
        }

        Commands::Today => {
            entry::handle_today(&app);
        }

        Commands::Yesterday => {
            entry::handle_yesterday(&app);
        }

        Commands::Open { path } => {
            entry::handle_open(&app, &path);
        }

        Commands::Config => {
            entry::handle_config();
        }

        Commands::Create { path } => {
            entry::handle_create(&app, &path);
        }

        Commands::Property { operation } => {
            property::handle_property_command(&app, operation);
        }

        Commands::Sort {
            path,
            pattern,
            abc: _,
            default,
            index,
            yes,
            dry_run,
        } => {
            sort::handle_sort_command(&app, path, pattern, default, index, yes, dry_run);
        }

        Commands::Workspace { command } => {
            workspace::handle_workspace_command(command, cli.workspace, &ws);
        }
    }
}

/// Handle the init command
fn handle_init(
    base_dir: Option<PathBuf>,
    title: Option<String>,
    description: Option<String>,
    ws: &Workspace<RealFileSystem>,
) {
    let dir = base_dir.unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("diaryx")
    });

    // Initialize config
    match Config::init(dir.clone()) {
        Ok(_) => {
            println!("✓ Initialized diaryx configuration");
            println!("  Base directory: {}", dir.display());
            if let Some(config_path) = Config::config_path() {
                println!("  Config file: {}", config_path.display());
            }
        }
        Err(e) => {
            eprintln!("✗ Error initializing config: {}", e);
            return;
        }
    }

    // Initialize workspace (create README.md)
    match ws.init_workspace(&dir, title.as_deref(), description.as_deref()) {
        Ok(readme_path) => {
            println!("✓ Initialized workspace");
            println!("  Index file: {}", readme_path.display());
        }
        Err(e) => {
            // Don't fail if workspace already exists
            if !matches!(
                e,
                diaryx_core::error::DiaryxError::WorkspaceAlreadyExists(_)
            ) {
                eprintln!("✗ Error initializing workspace: {}", e);
            } else {
                println!("  Workspace already initialized");
            }
        }
    }
}
