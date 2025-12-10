//! Content manipulation commands for diaryx CLI

use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;

use diaryx_core::config::Config;
use diaryx_core::entry::DiaryxApp;
use diaryx_core::fs::RealFileSystem;

use crate::cli::args::ContentCommands;
use crate::cli::util::{load_config, prompt_confirm, resolve_paths, ConfirmResult};

/// Handle all content subcommands
pub fn handle_content_command(app: &DiaryxApp<RealFileSystem>, operation: ContentCommands) {
    let config = match load_config() {
        Some(c) => c,
        None => return,
    };

    match operation {
        ContentCommands::Get { path } => {
            handle_get(app, &config, &path);
        }

        ContentCommands::Set {
            path,
            content,
            file,
            stdin,
            dry_run,
        } => {
            handle_set(app, &config, &path, content, file, stdin, dry_run);
        }

        ContentCommands::Clear { path, yes, dry_run } => {
            handle_clear(app, &config, &path, yes, dry_run);
        }

        ContentCommands::Append {
            path,
            content,
            file,
            stdin,
            dry_run,
        } => {
            handle_append(app, &config, &path, content, file, stdin, dry_run);
        }

        ContentCommands::Prepend {
            path,
            content,
            file,
            stdin,
            dry_run,
        } => {
            handle_prepend(app, &config, &path, content, file, stdin, dry_run);
        }
    }
}

/// Get content from file, stdin, or argument
fn get_input_content(
    content: Option<String>,
    file: Option<PathBuf>,
    stdin: bool,
) -> Result<String, String> {
    if stdin {
        if content.is_some() || file.is_some() {
            return Err("Cannot use --stdin with content argument or --file".to_string());
        }
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|e| format!("Failed to read from stdin: {}", e))?;
        return Ok(buffer);
    }

    match (content, file) {
        (Some(c), None) => Ok(c),
        (None, Some(f)) => fs::read_to_string(&f)
            .map_err(|e| format!("Failed to read file '{}': {}", f.display(), e)),
        (None, None) => {
            Err("No content provided. Use a content argument, --file, or --stdin.".to_string())
        }
        (Some(_), Some(_)) => Err("Cannot specify both content argument and --file".to_string()),
    }
}

/// Handle the 'content get' command
fn handle_get(app: &DiaryxApp<RealFileSystem>, config: &Config, path: &str) {
    let resolved = resolve_paths(path, config, app);

    if resolved.is_empty() {
        eprintln!("✗ No files found matching: {}", path);
        return;
    }

    for file_path in resolved {
        let path_str = file_path.to_string_lossy();
        match app.get_content(&path_str) {
            Ok(content) => {
                print!("{}", content);
                // Flush to ensure output is written immediately (important for piping)
                let _ = io::stdout().flush();
            }
            Err(e) => {
                eprintln!("✗ Error reading '{}': {}", file_path.display(), e);
            }
        }
    }
}

/// Handle the 'content set' command
fn handle_set(
    app: &DiaryxApp<RealFileSystem>,
    config: &Config,
    path: &str,
    content: Option<String>,
    file: Option<PathBuf>,
    stdin: bool,
    dry_run: bool,
) {
    let input = match get_input_content(content, file, stdin) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("✗ {}", e);
            return;
        }
    };

    let resolved = resolve_paths(path, config, app);

    if resolved.is_empty() {
        eprintln!("✗ No files found matching: {}", path);
        return;
    }

    if resolved.len() > 1 {
        eprintln!(
            "✗ 'content set' only supports single files, found {} matches",
            resolved.len()
        );
        eprintln!("  Be more specific with your path");
        return;
    }

    let file_path = &resolved[0];
    let path_str = file_path.to_string_lossy();

    if dry_run {
        println!("Would set content of '{}':", file_path.display());
        println!("---");
        println!("{}", input);
        println!("---");
        return;
    }

    match app.set_content(&path_str, &input) {
        Ok(()) => {
            println!("✓ Set content of '{}'", file_path.display());
        }
        Err(e) => {
            eprintln!("✗ Error setting content: {}", e);
        }
    }
}

/// Handle the 'content clear' command
fn handle_clear(
    app: &DiaryxApp<RealFileSystem>,
    config: &Config,
    path: &str,
    yes: bool,
    dry_run: bool,
) {
    let resolved = resolve_paths(path, config, app);

    if resolved.is_empty() {
        eprintln!("✗ No files found matching: {}", path);
        return;
    }

    if resolved.len() > 1 {
        eprintln!(
            "✗ 'content clear' only supports single files, found {} matches",
            resolved.len()
        );
        eprintln!("  Be more specific with your path");
        return;
    }

    let file_path = &resolved[0];
    let path_str = file_path.to_string_lossy();

    if dry_run {
        println!("Would clear content of '{}'", file_path.display());
        return;
    }

    // Confirm unless -y flag
    if !yes {
        println!("Clear all content from '{}'?", file_path.display());
        println!("(Frontmatter will be preserved)");
        match prompt_confirm("Proceed?") {
            ConfirmResult::Yes | ConfirmResult::All => {}
            _ => {
                println!("Cancelled");
                return;
            }
        }
    }

    match app.clear_content(&path_str) {
        Ok(()) => {
            println!("✓ Cleared content of '{}'", file_path.display());
        }
        Err(e) => {
            eprintln!("✗ Error clearing content: {}", e);
        }
    }
}

/// Handle the 'content append' command
fn handle_append(
    app: &DiaryxApp<RealFileSystem>,
    config: &Config,
    path: &str,
    content: Option<String>,
    file: Option<PathBuf>,
    stdin: bool,
    dry_run: bool,
) {
    let input = match get_input_content(content, file, stdin) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("✗ {}", e);
            return;
        }
    };

    let resolved = resolve_paths(path, config, app);

    if resolved.is_empty() {
        eprintln!("✗ No files found matching: {}", path);
        return;
    }

    if resolved.len() > 1 {
        eprintln!(
            "✗ 'content append' only supports single files, found {} matches",
            resolved.len()
        );
        eprintln!("  Be more specific with your path");
        return;
    }

    let file_path = &resolved[0];
    let path_str = file_path.to_string_lossy();

    if dry_run {
        println!("Would append to '{}':", file_path.display());
        println!("---");
        println!("{}", input);
        println!("---");
        return;
    }

    match app.append_content(&path_str, &input) {
        Ok(()) => {
            println!("✓ Appended content to '{}'", file_path.display());
        }
        Err(e) => {
            eprintln!("✗ Error appending content: {}", e);
        }
    }
}

/// Handle the 'content prepend' command
fn handle_prepend(
    app: &DiaryxApp<RealFileSystem>,
    config: &Config,
    path: &str,
    content: Option<String>,
    file: Option<PathBuf>,
    stdin: bool,
    dry_run: bool,
) {
    let input = match get_input_content(content, file, stdin) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("✗ {}", e);
            return;
        }
    };

    let resolved = resolve_paths(path, config, app);

    if resolved.is_empty() {
        eprintln!("✗ No files found matching: {}", path);
        return;
    }

    if resolved.len() > 1 {
        eprintln!(
            "✗ 'content prepend' only supports single files, found {} matches",
            resolved.len()
        );
        eprintln!("  Be more specific with your path");
        return;
    }

    let file_path = &resolved[0];
    let path_str = file_path.to_string_lossy();

    if dry_run {
        println!("Would prepend to '{}':", file_path.display());
        println!("---");
        println!("{}", input);
        println!("---");
        return;
    }

    match app.prepend_content(&path_str, &input) {
        Ok(()) => {
            println!("✓ Prepended content to '{}'", file_path.display());
        }
        Err(e) => {
            eprintln!("✗ Error prepending content: {}", e);
        }
    }
}
