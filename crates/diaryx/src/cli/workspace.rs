//! Workspace command handlers

use diaryx_core::config::Config;
use diaryx_core::entry::{DiaryxAppSync, prettify_filename, slugify};
use diaryx_core::fs::{FileSystem, RealFileSystem, SyncToAsyncFs};
use diaryx_core::template::TemplateContext;
use diaryx_core::validate::ValidationFixer;
use diaryx_core::workspace::Workspace;
use serde_yaml::Value;
use std::path::{Path, PathBuf};

use crate::cli::args::WorkspaceCommands;
use crate::cli::util::{calculate_relative_path, rename_file_with_refs, resolve_paths};
use crate::cli::{CliDiaryxAppSync, CliWorkspace, block_on};
use crate::editor::launch_editor;

pub fn handle_workspace_command(
    command: WorkspaceCommands,
    workspace_override: Option<PathBuf>,
    ws: &CliWorkspace,
    app: &CliDiaryxAppSync,
) {
    let config = Config::load().ok();
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    match command {
        WorkspaceCommands::Info { path, depth } => {
            handle_info(workspace_override, ws, &config, &current_dir, path, depth);
        }

        WorkspaceCommands::Init {
            dir,
            title,
            description,
        } => {
            handle_init(ws, dir, title, description, &current_dir);
        }

        WorkspaceCommands::Path => {
            handle_path(workspace_override, ws, &config, &current_dir);
        }

        WorkspaceCommands::Add {
            parent_or_child,
            child,
            new_index,
            recursive,
            yes,
            dry_run,
        } => {
            if let Some(ref cfg) = config {
                if recursive {
                    // Recursive add - create indexes for directory hierarchy
                    handle_add_recursive(
                        app,
                        cfg,
                        ws,
                        &current_dir,
                        &parent_or_child,
                        yes,
                        dry_run,
                    );
                } else if let Some(index_name) = new_index {
                    // Create new index and add files to it
                    handle_add_with_new_index(
                        app,
                        cfg,
                        ws,
                        &current_dir,
                        &parent_or_child,
                        child,
                        &index_name,
                        yes,
                        dry_run,
                    );
                } else {
                    let (parent, child_pattern) =
                        resolve_parent_child(ws, &current_dir, &parent_or_child, child);
                    if let (Some(p), Some(c)) = (parent, child_pattern) {
                        handle_add(app, cfg, &p, &c, yes, dry_run);
                    }
                }
            } else {
                eprintln!("✗ No config found. Run 'diaryx init' first");
            }
        }

        WorkspaceCommands::Create {
            parent_or_name,
            name,
            title,
            description,
            template,
            index,
            edit,
        } => {
            if let Some(ref cfg) = config {
                let (parent, name) = resolve_parent_name(ws, &current_dir, &parent_or_name, name);
                if let (Some(p), Some(n)) = (parent, name) {
                    handle_create(app, cfg, &p, &n, title, description, template, index, edit);
                }
            } else {
                eprintln!("✗ No config found. Run 'diaryx init' first");
            }
        }

        WorkspaceCommands::Remove {
            parent_or_child,
            child,
            dry_run,
        } => {
            if let Some(ref cfg) = config {
                let (parent, child) =
                    resolve_parent_child(ws, &current_dir, &parent_or_child, child);
                if let (Some(p), Some(c)) = (parent, child) {
                    handle_remove(app, cfg, &p, &c, dry_run);
                }
            } else {
                eprintln!("✗ No config found. Run 'diaryx init' first");
            }
        }

        WorkspaceCommands::Mv {
            source,
            dest,
            new_index,
            dry_run,
        } => {
            if let Some(ref cfg) = config {
                handle_mv(app, cfg, ws, &source, &dest, new_index, dry_run);
            } else {
                eprintln!("✗ No config found. Run 'diaryx init' first");
            }
        }

        WorkspaceCommands::Validate {
            path,
            fix,
            recursive,
            verbose,
        } => {
            handle_validate(
                workspace_override,
                ws,
                &config,
                &current_dir,
                path,
                fix,
                recursive,
                verbose,
            );
        }
    }
}

/// Handle the 'workspace validate' command
/// Validates workspace link integrity (part_of and contents references)
fn handle_validate(
    workspace_override: Option<PathBuf>,
    ws: &CliWorkspace,
    config: &Option<Config>,
    current_dir: &Path,
    file_path: Option<String>,
    fix: bool,
    recursive: bool,
    verbose: bool,
) {
    use diaryx_core::fs::RealFileSystem as CoreRealFileSystem;
    use diaryx_core::validate::{
        ValidationError, ValidationFixer, ValidationResult, ValidationWarning, Validator,
    };

    let async_fs = SyncToAsyncFs::new(CoreRealFileSystem);
    let validator = Validator::new(async_fs.clone());
    let fixer = ValidationFixer::new(async_fs);
    let app = DiaryxAppSync::new(CoreRealFileSystem);

    // If a specific path is provided, validate it (file or directory)
    if let Some(ref path_str) = file_path {
        let input_path = PathBuf::from(path_str);
        let resolved_path = if input_path.is_absolute() {
            input_path
        } else {
            current_dir.join(&input_path)
        };

        // Check if it's a directory
        if resolved_path.is_dir() {
            // Validate all markdown files in the directory
            let files = if recursive {
                collect_md_files_recursive(&resolved_path)
            } else {
                collect_md_files(&resolved_path)
            };

            if files.is_empty() {
                println!("✓ No markdown files found in {}", resolved_path.display());
                return;
            }

            if verbose {
                println!(
                    "Validating {} file(s) in {}",
                    files.len(),
                    resolved_path.display()
                );
            }

            let mut total_result = ValidationResult::default();

            for file in &files {
                match block_on(validator.validate_file(file)) {
                    Ok(result) => {
                        total_result.files_checked += result.files_checked;
                        total_result.errors.extend(result.errors);
                        total_result.warnings.extend(result.warnings);
                    }
                    Err(e) => {
                        if verbose {
                            eprintln!("  ⚠ Error validating {}: {}", file.display(), e);
                        }
                    }
                }
            }

            // Report and fix using the aggregated result
            report_and_fix_validation(&fixer, &app, &total_result, fix, &resolved_path, verbose);
            return;
        }

        // Single file validation
        if verbose {
            println!("Validating file: {}", resolved_path.display());
        }

        let result = match block_on(validator.validate_file(&resolved_path)) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("✗ Error validating file: {}", e);
                return;
            }
        };

        report_and_fix_validation(&fixer, &app, &result, fix, &resolved_path, verbose);
        return;
    }

    // Full workspace validation
    // Find workspace root
    let root_path = if let Some(override_path) = workspace_override {
        override_path.clone()
    } else if let Ok(Some(detected)) = block_on(ws.detect_workspace(current_dir)) {
        detected
    } else if let Some(cfg) = config {
        if let Ok(Some(root)) = block_on(ws.find_root_index_in_dir(&cfg.default_workspace)) {
            root
        } else {
            eprintln!("✗ No workspace found");
            eprintln!("  Run 'diaryx init' or 'diaryx workspace init' first");
            return;
        }
    } else {
        eprintln!("✗ No workspace found");
        eprintln!("  Run 'diaryx init' or 'diaryx workspace init' first");
        return;
    };

    if verbose {
        println!("Validating workspace: {}", root_path.display());
    }

    let result = match block_on(validator.validate_workspace(&root_path)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("✗ Error validating workspace: {}", e);
            return;
        }
    };

    if result.is_ok() && result.warnings.is_empty() {
        println!(
            "✓ Workspace validation passed ({} files checked)",
            result.files_checked
        );
        return;
    }

    let mut fixed_count = 0;

    // Report and optionally fix errors
    if !result.errors.is_empty() {
        println!("Errors ({}):", result.errors.len());
        for err in &result.errors {
            match err {
                ValidationError::BrokenPartOf { file, target } => {
                    if fix {
                        let result = block_on(fixer.fix_broken_part_of(file));
                        if result.success {
                            println!(
                                "  ✓ Fixed: Removed broken part_of '{}' from {}",
                                target,
                                file.display()
                            );
                            fixed_count += 1;
                        } else {
                            println!(
                                "  ✗ Broken part_of: {} -> {} (failed to fix)",
                                file.display(),
                                target
                            );
                        }
                    } else {
                        println!("  ✗ Broken part_of: {} -> {}", file.display(), target);
                    }
                }
                ValidationError::BrokenContentsRef { index, target } => {
                    if fix {
                        let result = block_on(fixer.fix_broken_contents_ref(index, target));
                        if result.success {
                            println!(
                                "  ✓ Fixed: Removed broken contents ref '{}' from {}",
                                target,
                                index.display()
                            );
                            fixed_count += 1;
                        } else {
                            println!(
                                "  ✗ Broken contents ref: {} -> {} (failed to fix)",
                                index.display(),
                                target
                            );
                        }
                    } else {
                        println!("  ✗ Broken contents ref: {} -> {}", index.display(), target);
                    }
                }
                ValidationError::BrokenAttachment { file, attachment } => {
                    if fix {
                        let result = block_on(fixer.fix_broken_attachment(file, attachment));
                        if result.success {
                            println!(
                                "  ✓ Fixed: Removed broken attachment '{}' from {}",
                                attachment,
                                file.display()
                            );
                            fixed_count += 1;
                        } else {
                            println!(
                                "  ✗ Broken attachment: {} -> {} (failed to fix)",
                                file.display(),
                                attachment
                            );
                        }
                    } else {
                        println!(
                            "  ✗ Broken attachment: {} -> {}",
                            file.display(),
                            attachment
                        );
                    }
                }
            }
        }
    }

    // Report warnings
    if !result.warnings.is_empty() {
        println!("Warnings ({}):", result.warnings.len());
        for warn in &result.warnings {
            match warn {
                ValidationWarning::OrphanFile { file } => {
                    println!("  ⚠ Orphan file: {}", file.display());
                }
                ValidationWarning::CircularReference { files } => {
                    println!("  ⚠ Circular reference involving: {:?}", files);
                }
                ValidationWarning::UnlinkedEntry { path, is_dir } => {
                    let icon = if *is_dir { "📁" } else { "📄" };
                    println!("  {} Unlinked: {}", icon, path.display());
                }
                ValidationWarning::UnlistedFile { index, file } => {
                    if fix {
                        let result = block_on(fixer.fix_unlisted_file(index, file));
                        if result.success {
                            println!(
                                "  ✓ Fixed: Added '{}' to {}",
                                file.display(),
                                index.display()
                            );
                            fixed_count += 1;
                        } else {
                            println!("  ⚠ Unlisted file: {} (failed to add)", file.display());
                        }
                    } else {
                        println!("  ⚠ Unlisted file: {}", file.display());
                    }
                }
                ValidationWarning::NonPortablePath {
                    file,
                    property,
                    value,
                    suggested,
                } => {
                    if fix {
                        let result =
                            block_on(fixer.fix_non_portable_path(file, property, value, suggested));
                        if result.success {
                            println!(
                                "  ✓ Fixed: Normalized {} '{}' -> '{}' in {}",
                                property,
                                value,
                                suggested,
                                file.display()
                            );
                            fixed_count += 1;
                        } else {
                            println!(
                                "  ⚠ Non-portable {}: '{}' (suggested: '{}') in {} (failed to fix)",
                                property,
                                value,
                                suggested,
                                file.display()
                            );
                        }
                    } else {
                        println!(
                            "  ⚠ Non-portable {}: '{}' -> '{}' in {}",
                            property,
                            value,
                            suggested,
                            file.display()
                        );
                    }
                }
                ValidationWarning::MultipleIndexes { directory, indexes } => {
                    // Can't auto-fix - requires user decision
                    println!(
                        "  ⚠ Multiple indexes in {}: {:?}",
                        directory.display(),
                        indexes
                            .iter()
                            .map(|p| p.file_name().unwrap_or_default().to_string_lossy())
                            .collect::<Vec<_>>()
                    );
                }
                ValidationWarning::OrphanBinaryFile {
                    file,
                    suggested_index,
                } => {
                    if fix {
                        if let Some(index) = suggested_index {
                            let result = block_on(fixer.fix_orphan_binary_file(index, file));
                            if result.success {
                                println!(
                                    "  ✓ Fixed: Added '{}' to attachments in {}",
                                    file.display(),
                                    index.display()
                                );
                                fixed_count += 1;
                            } else {
                                println!(
                                    "  ⚠ Orphan binary file: {} (failed to add to attachments)",
                                    file.display()
                                );
                            }
                        } else {
                            println!(
                                "  ⚠ Orphan binary file: {} (no single index found)",
                                file.display()
                            );
                        }
                    } else {
                        println!("  ⚠ Orphan binary file: {}", file.display());
                    }
                }
                ValidationWarning::MissingPartOf {
                    file,
                    suggested_index,
                } => {
                    if fix {
                        let index_to_use = if let Some(idx) = suggested_index {
                            Some(idx.clone())
                        } else {
                            // Check if directory has ANY index
                            let dir = file.parent().unwrap_or(Path::new("."));
                            let ws = Workspace::new(SyncToAsyncFs::new(RealFileSystem));
                            if let Ok(None) = block_on(ws.find_any_index_in_dir(dir)) {
                                // No index exists. Create one.
                                create_new_index(&app, dir)
                            } else {
                                None
                            }
                        };

                        if let Some(index) = index_to_use {
                            let result = block_on(fixer.fix_missing_part_of(file, &index));
                            if result.success {
                                println!(
                                    "  ✓ Fixed: Set part_of to '{}' in {}",
                                    index.display(),
                                    file.display()
                                );
                                fixed_count += 1;
                            } else {
                                println!("  ⚠ Missing part_of: {} (failed to fix)", file.display());
                            }
                        } else {
                            println!(
                                "  ⚠ Missing part_of (orphan): {} (no single index found)",
                                file.display()
                            );
                        }
                    } else {
                        println!("  ⚠ Missing part_of (orphan): {}", file.display());
                    }
                }
            }
        }
    }

    println!();
    if fix && fixed_count > 0 {
        println!(
            "Summary: {} issue(s) fixed, {} files checked",
            fixed_count, result.files_checked
        );
    } else {
        println!(
            "Summary: {} error(s), {} warning(s), {} files checked",
            result.errors.len(),
            result.warnings.len(),
            result.files_checked
        );
    }
}

/// Helper function to report validation results and optionally fix issues
fn report_and_fix_validation(
    fixer: &ValidationFixer<SyncToAsyncFs<RealFileSystem>>,
    app: &CliDiaryxAppSync,
    result: &diaryx_core::validate::ValidationResult,
    fix: bool,
    context_path: &Path,
    verbose: bool,
) {
    use diaryx_core::validate::{ValidationError, ValidationWarning};

    if result.is_ok() && result.warnings.is_empty() {
        println!(
            "✓ Validation passed: {} ({} file(s) checked)",
            context_path.display(),
            result.files_checked
        );
        return;
    }

    let mut fixed_count = 0;

    // Report and optionally fix errors
    if !result.errors.is_empty() {
        if verbose {
            println!("Errors ({}):", result.errors.len());
        }
        for err in &result.errors {
            match err {
                ValidationError::BrokenPartOf { file, target } => {
                    if fix {
                        let fix_result = block_on(fixer.fix_broken_part_of(file));
                        if fix_result.success {
                            println!(
                                "  ✓ Fixed: Removed broken part_of '{}' from {}",
                                target,
                                file.display()
                            );
                            fixed_count += 1;
                        } else {
                            println!(
                                "  ✗ Broken part_of: {} -> {} (failed to fix)",
                                file.display(),
                                target
                            );
                        }
                    } else {
                        println!("  ✗ Broken part_of: {} -> {}", file.display(), target);
                    }
                }
                ValidationError::BrokenContentsRef { index, target } => {
                    if fix {
                        let fix_result = block_on(fixer.fix_broken_contents_ref(index, target));
                        if fix_result.success {
                            println!(
                                "  ✓ Fixed: Removed broken contents ref '{}' from {}",
                                target,
                                index.display()
                            );
                            fixed_count += 1;
                        } else {
                            println!(
                                "  ✗ Broken contents ref: {} -> {} (failed to fix)",
                                index.display(),
                                target
                            );
                        }
                    } else {
                        println!("  ✗ Broken contents ref: {} -> {}", index.display(), target);
                    }
                }
                ValidationError::BrokenAttachment { file, attachment } => {
                    if fix {
                        let fix_result = block_on(fixer.fix_broken_attachment(file, attachment));
                        if fix_result.success {
                            println!(
                                "  ✓ Fixed: Removed broken attachment '{}' from {}",
                                attachment,
                                file.display()
                            );
                            fixed_count += 1;
                        } else {
                            println!(
                                "  ✗ Broken attachment: {} -> {} (failed to fix)",
                                file.display(),
                                attachment
                            );
                        }
                    } else {
                        println!(
                            "  ✗ Broken attachment: {} -> {}",
                            file.display(),
                            attachment
                        );
                    }
                }
            }
        }
    }

    // Report and optionally fix warnings
    if !result.warnings.is_empty() {
        if verbose {
            println!("Warnings ({}):", result.warnings.len());
        }
        for warn in &result.warnings {
            match warn {
                ValidationWarning::UnlistedFile { index, file } => {
                    if fix {
                        let fix_result = block_on(fixer.fix_unlisted_file(index, file));
                        if fix_result.success {
                            println!(
                                "  ✓ Fixed: Added '{}' to {}",
                                file.display(),
                                index.display()
                            );
                            fixed_count += 1;
                        } else {
                            println!("  ⚠ Unlisted file: {} (failed to add)", file.display());
                        }
                    } else {
                        println!("  ⚠ Unlisted file: {}", file.display());
                    }
                }
                ValidationWarning::NonPortablePath {
                    file,
                    property,
                    value,
                    suggested,
                } => {
                    if fix {
                        let fix_result =
                            block_on(fixer.fix_non_portable_path(file, property, value, suggested));
                        if fix_result.success {
                            println!(
                                "  ✓ Fixed: Normalized {} '{}' -> '{}' in {}",
                                property,
                                value,
                                suggested,
                                file.display()
                            );
                            fixed_count += 1;
                        } else {
                            println!(
                                "  ⚠ Non-portable {}: '{}' (suggested: '{}') in {} (failed to fix)",
                                property,
                                value,
                                suggested,
                                file.display()
                            );
                        }
                    } else {
                        println!(
                            "  ⚠ Non-portable {}: '{}' -> '{}' in {}",
                            property,
                            value,
                            suggested,
                            file.display()
                        );
                    }
                }
                ValidationWarning::OrphanFile { file } => {
                    println!("  ⚠ Orphan file: {}", file.display());
                }
                ValidationWarning::CircularReference { files } => {
                    println!("  ⚠ Circular reference involving: {:?}", files);
                }
                ValidationWarning::UnlinkedEntry { path, is_dir } => {
                    let icon = if *is_dir { "📁" } else { "📄" };
                    println!("  {} Unlinked: {}", icon, path.display());
                }
                ValidationWarning::MultipleIndexes { directory, indexes } => {
                    println!(
                        "  ⚠ Multiple indexes in {}: {:?}",
                        directory.display(),
                        indexes
                            .iter()
                            .map(|p| p.file_name().unwrap_or_default().to_string_lossy())
                            .collect::<Vec<_>>()
                    );
                }
                ValidationWarning::OrphanBinaryFile {
                    file,
                    suggested_index,
                } => {
                    if fix {
                        if let Some(index) = suggested_index {
                            let fix_result = block_on(fixer.fix_orphan_binary_file(index, file));
                            if fix_result.success {
                                println!(
                                    "  ✓ Fixed: Added '{}' to attachments in {}",
                                    file.display(),
                                    index.display()
                                );
                                fixed_count += 1;
                            } else {
                                println!(
                                    "  ⚠ Orphan binary file: {} (failed to add)",
                                    file.display()
                                );
                            }
                        } else {
                            println!(
                                "  ⚠ Orphan binary file: {} (no single index found)",
                                file.display()
                            );
                        }
                    } else {
                        println!("  ⚠ Orphan binary file: {}", file.display());
                    }
                }
                ValidationWarning::MissingPartOf {
                    file,
                    suggested_index,
                } => {
                    if fix {
                        let index_to_use = if let Some(idx) = suggested_index {
                            Some(idx.clone())
                        } else {
                            // Check if directory has ANY index
                            let dir = file.parent().unwrap_or(Path::new("."));
                            let ws = Workspace::new(SyncToAsyncFs::new(RealFileSystem));
                            if let Ok(None) = block_on(ws.find_any_index_in_dir(dir)) {
                                // No index exists. Create one.
                                create_new_index(app, dir)
                            } else {
                                None
                            }
                        };

                        if let Some(index) = index_to_use {
                            let fix_result = block_on(fixer.fix_missing_part_of(file, &index));
                            if fix_result.success {
                                println!(
                                    "  ✓ Fixed: Set part_of to '{}' in {}",
                                    index.display(),
                                    file.display()
                                );
                                fixed_count += 1;
                            } else {
                                println!("  ⚠ Missing part_of: {} (failed to fix)", file.display());
                            }
                        } else {
                            println!(
                                "  ⚠ Missing part_of (orphan): {} (no single index found)",
                                file.display()
                            );
                        }
                    } else {
                        println!("  ⚠ Missing part_of (orphan): {}", file.display());
                    }
                }
            }
        }
    }

    println!();
    if fix && fixed_count > 0 {
        println!(
            "Summary: {} issue(s) fixed, {} file(s) checked",
            fixed_count, result.files_checked
        );
    } else {
        println!(
            "Summary: {} error(s), {} warning(s), {} file(s) checked",
            result.errors.len(),
            result.warnings.len(),
            result.files_checked
        );
    }
}

// Note: The fix functions (fix_broken_contents_ref, add_file_to_contents, fix_non_portable_path,
// fix_broken_attachment, add_file_to_attachments, fix_missing_part_of) have been moved to
// diaryx_core::validate::ValidationFixer for code reuse across CLI, WASM, and Tauri backends.

/// Handle the 'workspace mv' command
/// Moves/renames a file while updating workspace hierarchy references
fn handle_mv(
    app: &CliDiaryxAppSync,
    config: &Config,
    ws: &CliWorkspace,
    source: &str,
    dest: &str,
    new_index: Option<String>,
    dry_run: bool,
) {
    // Resolve source path (should be a single file)
    let source_paths = resolve_paths(source, config, app);
    if source_paths.is_empty() {
        eprintln!("✗ No files matched source: {}", source);
        return;
    }
    if source_paths.len() > 1 {
        eprintln!("✗ Source must be a single file, but matched multiple:");
        for p in &source_paths {
            eprintln!("  {}", p.display());
        }
        return;
    }
    let source_path = &source_paths[0];

    if !source_path.exists() {
        eprintln!("✗ Source file does not exist: {}", source_path.display());
        return;
    }

    // Determine destination path
    let dest_input = PathBuf::from(dest);
    // Canonicalize the destination directory if it exists, to get clean paths
    let dest_path = if dest_input.is_dir() {
        // If dest is a directory, move file into it with same name
        let canonical_dir = dest_input.canonicalize().unwrap_or(dest_input);
        canonical_dir.join(source_path.file_name().unwrap_or_default())
    } else if !dest.ends_with(".md") {
        // Add .md extension if not present
        let with_ext = PathBuf::from(format!("{}.md", dest));
        // Try to canonicalize the parent directory
        if let Some(parent) = with_ext.parent() {
            if parent.exists() {
                if let Ok(canonical_parent) = parent.canonicalize() {
                    canonical_parent.join(with_ext.file_name().unwrap_or_default())
                } else {
                    with_ext
                }
            } else {
                with_ext
            }
        } else {
            with_ext
        }
    } else {
        // Try to canonicalize the parent directory
        if let Some(parent) = dest_input.parent() {
            if parent.exists() {
                if let Ok(canonical_parent) = parent.canonicalize() {
                    canonical_parent.join(dest_input.file_name().unwrap_or_default())
                } else {
                    dest_input
                }
            } else {
                dest_input
            }
        } else {
            dest_input
        }
    };

    if dest_path.exists() {
        eprintln!("✗ Destination already exists: {}", dest_path.display());
        return;
    }

    // Use shared utility for workspace-aware rename/move
    let result = rename_file_with_refs(app, source_path, &dest_path, dry_run);

    // If --new-index is specified and move succeeded, create/use index as parent
    if result.success && !dry_run {
        if let Some(index_name) = new_index {
            set_new_index_as_parent(app, ws, &dest_path, &index_name);
        }
    } else if dry_run && let Some(index_name) = new_index {
        let index_filename = if index_name.ends_with(".md") {
            index_name
        } else {
            format!("{}.md", index_name)
        };
        let index_path = dest_path
            .parent()
            .map(|p| p.join(&index_filename))
            .unwrap_or_else(|| PathBuf::from(&index_filename));
        if index_path.exists() {
            println!(
                "Would set part_of to existing index '{}'",
                index_path.display()
            );
        } else {
            println!(
                "Would create new index '{}' and set as parent",
                index_path.display()
            );
        }
    }
}

/// Set a new or existing index as the parent of a file
fn set_new_index_as_parent(
    app: &CliDiaryxAppSync,
    ws: &CliWorkspace,
    file_path: &Path,
    index_name: &str,
) {
    let file_dir = file_path.parent().unwrap_or(Path::new("."));

    // Create index filename
    let index_filename = if index_name.ends_with(".md") {
        index_name.to_string()
    } else {
        format!("{}.md", index_name)
    };
    let index_path = file_dir.join(&index_filename);
    let index_str = index_path.to_string_lossy();

    // Check if index exists, if not create it
    if !index_path.exists() {
        // Create title from index name
        let title = index_name.trim_end_matches(".md").replace(['_', '-'], " ");
        let title = title
            .split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        // Create the index with title and empty contents
        if let Err(e) =
            app.set_frontmatter_property(&index_str, "title", serde_yaml::Value::String(title))
        {
            eprintln!("✗ Error creating index file: {}", e);
            return;
        }

        if let Err(e) = app.set_frontmatter_property(
            &index_str,
            "contents",
            serde_yaml::Value::Sequence(vec![]),
        ) {
            eprintln!("✗ Error setting contents in index: {}", e);
            return;
        }

        // Find parent index for the new index
        if let Ok(Some(parent_index)) = block_on(ws.find_any_index_in_dir(file_dir)) {
            // Don't set parent if it's the same as the new index
            if parent_index != index_path {
                let relative_parent = calculate_relative_path(&index_path, &parent_index);
                if let Err(e) = app.set_frontmatter_property(
                    &index_str,
                    "part_of",
                    serde_yaml::Value::String(relative_parent),
                ) {
                    eprintln!("⚠ Error setting part_of in new index: {}", e);
                }

                // Add new index to parent's contents
                let parent_str = parent_index.to_string_lossy();
                let relative_index = calculate_relative_path(&parent_index, &index_path);

                if let Ok(Some(serde_yaml::Value::Sequence(mut items))) =
                    app.get_frontmatter_property(&parent_str, "contents")
                {
                    items.push(serde_yaml::Value::String(relative_index.clone()));
                    if let Err(e) = app.set_frontmatter_property(
                        &parent_str,
                        "contents",
                        serde_yaml::Value::Sequence(items),
                    ) {
                        eprintln!("⚠ Error updating parent contents: {}", e);
                    } else {
                        println!(
                            "✓ Added '{}' to parent '{}'",
                            relative_index,
                            parent_index.display()
                        );
                    }
                }
            }
        }

        println!("✓ Created index '{}'", index_path.display());
    }

    // Add file to index's contents
    let relative_file = calculate_relative_path(&index_path, file_path);
    match app.get_frontmatter_property(&index_str, "contents") {
        Ok(Some(serde_yaml::Value::Sequence(mut items))) => {
            let file_value = serde_yaml::Value::String(relative_file.clone());
            if !items.contains(&file_value) {
                items.push(file_value);
                if let Err(e) = app.set_frontmatter_property(
                    &index_str,
                    "contents",
                    serde_yaml::Value::Sequence(items),
                ) {
                    eprintln!("✗ Error updating index contents: {}", e);
                    return;
                }
                println!(
                    "✓ Added '{}' to index '{}'",
                    relative_file,
                    index_path.display()
                );
            }
        }
        Ok(None) => {
            let items = vec![serde_yaml::Value::String(relative_file.clone())];
            if let Err(e) = app.set_frontmatter_property(
                &index_str,
                "contents",
                serde_yaml::Value::Sequence(items),
            ) {
                eprintln!("✗ Error creating index contents: {}", e);
                return;
            }
            println!(
                "✓ Added '{}' to index '{}'",
                relative_file,
                index_path.display()
            );
        }
        _ => {
            eprintln!("✗ Index contents is not a list");
            return;
        }
    }

    // Set part_of in the moved file
    let file_str = file_path.to_string_lossy();
    let relative_index = calculate_relative_path(file_path, &index_path);
    if let Err(e) = app.set_frontmatter_property(
        &file_str,
        "part_of",
        serde_yaml::Value::String(relative_index),
    ) {
        eprintln!("✗ Error setting part_of in moved file: {}", e);
    } else {
        println!("✓ Set part_of in '{}'", file_path.display());
    }
}

/// Collect markdown files in a directory (non-recursive)
fn collect_md_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "md") {
                files.push(path);
            }
        }
    }
    files
}

/// Collect markdown files in a directory (recursive)
fn collect_md_files_recursive(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_md_files_recursive_helper(dir, &mut files);
    files
}

fn collect_md_files_recursive_helper(dir: &Path, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                collect_md_files_recursive_helper(&path, files);
            } else if path.is_file() && path.extension().is_some_and(|ext| ext == "md") {
                files.push(path);
            }
        }
    }
}

/// Resolve parent and child arguments, using local index as default parent if only one arg provided
fn resolve_parent_child(
    ws: &CliWorkspace,
    current_dir: &Path,
    parent_or_child: &str,
    child: Option<String>,
) -> (Option<String>, Option<String>) {
    match child {
        // Two arguments provided: parent_or_child is parent, child is child
        Some(c) => (Some(parent_or_child.to_string()), Some(c)),
        // One argument provided: find local index as parent, parent_or_child is child
        None => match block_on(ws.find_any_index_in_dir(current_dir)) {
            Ok(Some(index_path)) => {
                let parent = index_path.to_string_lossy().to_string();
                (Some(parent), Some(parent_or_child.to_string()))
            }
            Ok(None) => {
                eprintln!("✗ No index file found in current directory");
                eprintln!("  Either specify a parent explicitly or create an index first");
                (None, None)
            }
            Err(e) => {
                eprintln!("✗ Error finding index: {}", e);
                (None, None)
            }
        },
    }
}

/// Resolve parent and name arguments for create, using local index as default parent if only one arg provided
fn resolve_parent_name(
    ws: &CliWorkspace,
    current_dir: &Path,
    parent_or_name: &str,
    name: Option<String>,
) -> (Option<String>, Option<String>) {
    match name {
        // Two arguments provided: parent_or_name is parent, name is name
        Some(n) => (Some(parent_or_name.to_string()), Some(n)),
        // One argument provided: find local index as parent, parent_or_name is name
        None => match block_on(ws.find_any_index_in_dir(current_dir)) {
            Ok(Some(index_path)) => {
                let parent = index_path.to_string_lossy().to_string();
                (Some(parent), Some(parent_or_name.to_string()))
            }
            Ok(None) => {
                eprintln!("✗ No index file found in current directory");
                eprintln!("  Either specify a parent explicitly or create an index first");
                (None, None)
            }
            Err(e) => {
                eprintln!("✗ Error finding index: {}", e);
                (None, None)
            }
        },
    }
}

/// Handle the 'workspace info' command
fn handle_info(
    workspace_override: Option<PathBuf>,
    ws: &CliWorkspace,
    config: &Option<Config>,
    current_dir: &Path,
    path: Option<String>,
    max_depth: usize,
) {
    // If a path is provided, resolve it (supports "." for local index)
    let root_path = if let Some(ref p) = path {
        if p == "." {
            // Resolve to local index in current directory
            match block_on(ws.find_any_index_in_dir(current_dir)) {
                Ok(Some(index)) => index,
                Ok(None) => {
                    eprintln!("✗ No index found in current directory");
                    return;
                }
                Err(e) => {
                    eprintln!("✗ Error finding index: {}", e);
                    return;
                }
            }
        } else {
            // Treat as a path to resolve
            let path_buf = Path::new(p);
            if path_buf.is_file() {
                path_buf.to_path_buf()
            } else if path_buf.is_dir() {
                // Find index in that directory
                match block_on(ws.find_any_index_in_dir(path_buf)) {
                    Ok(Some(index)) => index,
                    Ok(None) => {
                        eprintln!("✗ No index found in directory: {}", p);
                        return;
                    }
                    Err(e) => {
                        eprintln!("✗ Error finding index: {}", e);
                        return;
                    }
                }
            } else {
                eprintln!("✗ Path not found: {}", p);
                return;
            }
        }
    } else if let Some(override_path) = workspace_override {
        override_path.clone()
    } else if let Ok(Some(detected)) = block_on(ws.detect_workspace(current_dir)) {
        detected
    } else if let Some(cfg) = config {
        if let Ok(Some(root)) = block_on(ws.find_root_index_in_dir(&cfg.default_workspace)) {
            root
        } else {
            eprintln!("✗ No workspace found");
            eprintln!("  Run 'diaryx init' or 'diaryx workspace init' first");
            return;
        }
    } else {
        eprintln!("✗ No workspace found");
        eprintln!("  Run 'diaryx init' or 'diaryx workspace init' first");
        return;
    };

    // Convert 0 to None (unlimited), otherwise Some(depth)
    let depth_limit = if max_depth == 0 {
        None
    } else {
        Some(max_depth)
    };

    match block_on(ws.workspace_info_with_depth(&root_path, depth_limit)) {
        Ok(tree_output) => {
            println!("{}", tree_output);
        }
        Err(e) => eprintln!("✗ Error reading workspace: {}", e),
    }
}

/// Handle the 'workspace init' command
fn handle_init(
    ws: &CliWorkspace,
    dir: Option<PathBuf>,
    title: Option<String>,
    description: Option<String>,
    current_dir: &Path,
) {
    let target_dir = dir.unwrap_or_else(|| current_dir.to_path_buf());

    match block_on(ws.init_workspace(&target_dir, title.as_deref(), description.as_deref())) {
        Ok(readme_path) => {
            println!("✓ Initialized workspace");
            println!("  Index file: {}", readme_path.display());
        }
        Err(e) => eprintln!("✗ Error initializing workspace: {}", e),
    }
}

/// Handle the 'workspace path' command
fn handle_path(
    workspace_override: Option<PathBuf>,
    ws: &CliWorkspace,
    config: &Option<Config>,
    current_dir: &Path,
) {
    let root_path = if let Some(override_path) = workspace_override {
        Some(override_path.clone())
    } else if let Ok(Some(detected)) = block_on(ws.detect_workspace(current_dir)) {
        Some(detected)
    } else if let Some(cfg) = config {
        block_on(ws.find_root_index_in_dir(&cfg.default_workspace))
            .ok()
            .flatten()
    } else {
        None
    };

    match root_path {
        Some(path) => {
            if let Some(dir) = path.parent() {
                println!("{}", dir.display());
            } else {
                println!("{}", path.display());
            }
        }
        None => {
            eprintln!("✗ No workspace found");
            eprintln!("  Run 'diaryx init' or 'diaryx workspace init' first");
        }
    }
}

/// Handle the 'workspace add --recursive' command
/// Recursively creates indexes for a directory hierarchy and connects them
fn handle_add_recursive(
    app: &CliDiaryxAppSync,
    config: &Config,
    ws: &CliWorkspace,
    current_dir: &Path,
    dir_path: &str,
    yes: bool,
    dry_run: bool,
) {
    use crate::cli::util::{ConfirmResult, prompt_confirm};

    // Resolve the directory path
    let path = Path::new(dir_path);
    let dir = if path.is_absolute() || path.exists() {
        PathBuf::from(dir_path)
    } else {
        current_dir.join(dir_path)
    };

    if !dir.exists() {
        eprintln!("✗ Directory does not exist: {}", dir.display());
        return;
    }

    if !dir.is_dir() {
        eprintln!("✗ Path is not a directory: {}", dir.display());
        eprintln!("  Use 'diaryx w add' without --recursive for files");
        return;
    }

    // Collect the directory structure
    let mut plan = RecursiveAddPlan::new();
    build_recursive_plan(&dir, &mut plan, ws);

    if plan.directories.is_empty() {
        eprintln!("✗ No directories to process");
        return;
    }

    // Show the plan
    println!("Recursive add plan for '{}':", dir.display());
    println!();

    let mut total_indexes = 0;
    let mut total_files = 0;

    for dir_plan in &plan.directories {
        let action = if dir_plan.index_exists {
            "use existing"
        } else {
            total_indexes += 1;
            "create"
        };
        println!(
            "  {} ({} index): {}",
            dir_plan.dir.display(),
            action,
            dir_plan
                .index_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        );
        for file in &dir_plan.files {
            println!(
                "    + {}",
                file.file_name().unwrap_or_default().to_string_lossy()
            );
            total_files += 1;
        }
        for subdir in &dir_plan.subdirs {
            println!(
                "    → {}",
                subdir.file_name().unwrap_or_default().to_string_lossy()
            );
        }
    }

    println!();
    println!(
        "Summary: {} new index(es), {} file(s) to add",
        total_indexes, total_files
    );

    if dry_run {
        println!();
        println!("Dry run - no changes made");
        return;
    }

    // Confirm
    if !yes {
        println!();
        match prompt_confirm("Proceed?") {
            ConfirmResult::Yes | ConfirmResult::All => {}
            _ => {
                println!("Aborted");
                return;
            }
        }
    }

    // Execute the plan (process directories from deepest to shallowest)
    // This ensures child indexes exist before we link them to parents
    let mut sorted_dirs = plan.directories.clone();
    sorted_dirs.sort_by(|a, b| {
        let depth_a = a.dir.components().count();
        let depth_b = b.dir.components().count();
        depth_b.cmp(&depth_a) // Deepest first
    });

    for dir_plan in &sorted_dirs {
        execute_dir_plan(app, config, ws, dir_plan);
    }

    // Connect root index to workspace if applicable
    if let Some(root_plan) = plan.directories.first() {
        // Find parent index for the root directory
        if let Some(parent_dir) = root_plan.dir.parent()
            && let Ok(Some(parent_index)) = block_on(ws.find_any_index_in_dir(parent_dir))
        {
            // Check if root index is already in parent's contents
            let parent_str = parent_index.to_string_lossy();
            let relative_root = calculate_relative_path(&parent_index, &root_plan.index_path);

            let already_linked = match app.get_frontmatter_property(&parent_str, "contents") {
                Ok(Some(serde_yaml::Value::Sequence(items))) => items.iter().any(|item| {
                    if let serde_yaml::Value::String(s) = item {
                        s == &relative_root
                    } else {
                        false
                    }
                }),
                _ => false,
            };

            if !already_linked {
                // Add to parent's contents
                match app.get_frontmatter_property(&parent_str, "contents") {
                    Ok(Some(serde_yaml::Value::Sequence(mut items))) => {
                        items.push(serde_yaml::Value::String(relative_root.clone()));
                        if let Err(e) = app.set_frontmatter_property(
                            &parent_str,
                            "contents",
                            serde_yaml::Value::Sequence(items),
                        ) {
                            eprintln!("⚠ Error updating parent contents: {}", e);
                        } else {
                            println!(
                                "✓ Added '{}' to workspace index '{}'",
                                relative_root,
                                parent_index.display()
                            );
                        }
                    }
                    Ok(None) | Ok(Some(_)) => {
                        let items = vec![serde_yaml::Value::String(relative_root.clone())];
                        if let Err(e) = app.set_frontmatter_property(
                            &parent_str,
                            "contents",
                            serde_yaml::Value::Sequence(items),
                        ) {
                            eprintln!("⚠ Error creating parent contents: {}", e);
                        } else {
                            println!(
                                "✓ Added '{}' to workspace index '{}'",
                                relative_root,
                                parent_index.display()
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("⚠ Error reading parent contents: {}", e);
                    }
                }

                // Set part_of in root index
                let root_str = root_plan.index_path.to_string_lossy();
                let relative_parent = calculate_relative_path(&root_plan.index_path, &parent_index);
                if let Err(e) = app.set_frontmatter_property(
                    &root_str,
                    "part_of",
                    serde_yaml::Value::String(relative_parent),
                ) {
                    eprintln!("⚠ Error setting part_of in root index: {}", e);
                }
            }
        }
    }

    println!();
    println!("✓ Recursive add complete");
}

/// Plan for a single directory in recursive add
#[derive(Clone)]
struct DirPlan {
    dir: PathBuf,
    index_path: PathBuf,
    index_exists: bool,
    files: Vec<PathBuf>,
    subdirs: Vec<PathBuf>,
}

/// Overall plan for recursive add
struct RecursiveAddPlan {
    directories: Vec<DirPlan>,
}

impl RecursiveAddPlan {
    fn new() -> Self {
        Self {
            directories: Vec::new(),
        }
    }
}

/// Build a plan for recursive directory processing
fn build_recursive_plan(dir: &Path, plan: &mut RecursiveAddPlan, ws: &CliWorkspace) {
    // Determine index path for this directory
    let dir_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| "index".into());
    let index_filename = format!("{}_index.md", dir_name);
    let index_path = dir.join(&index_filename);

    // Check if an index already exists
    let (final_index_path, index_exists) = match block_on(ws.find_any_index_in_dir(dir)) {
        Ok(Some(existing)) => (existing, true),
        _ => (index_path, false),
    };

    // Collect files and subdirectories
    let mut files = Vec::new();
    let mut subdirs = Vec::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                subdirs.push(path);
            } else if path.is_file()
                && path.extension().is_some_and(|ext| ext == "md")
                && path != final_index_path
            {
                files.push(path);
            }
        }
    }

    // Sort for consistent ordering
    files.sort();
    subdirs.sort();

    // Add this directory to the plan
    plan.directories.push(DirPlan {
        dir: dir.to_path_buf(),
        index_path: final_index_path,
        index_exists,
        files,
        subdirs: subdirs.clone(),
    });

    // Recurse into subdirectories
    for subdir in subdirs {
        build_recursive_plan(&subdir, plan, ws);
    }
}

/// Execute the plan for a single directory
fn execute_dir_plan(
    app: &CliDiaryxAppSync,
    _config: &Config,
    ws: &CliWorkspace,
    dir_plan: &DirPlan,
) {
    let index_str = dir_plan.index_path.to_string_lossy();

    // Create index if it doesn't exist
    if !dir_plan.index_exists {
        let dir_name = dir_plan
            .dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Index".to_string());

        // Title case the directory name
        let title = dir_name
            .replace(['_', '-'], " ")
            .split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        // Create with title
        if let Err(e) = app.set_frontmatter_property(
            &index_str,
            "title",
            serde_yaml::Value::String(title.clone()),
        ) {
            eprintln!(
                "✗ Error creating index '{}': {}",
                dir_plan.index_path.display(),
                e
            );
            return;
        }
        println!("✓ Created index '{}'", dir_plan.index_path.display());
    } else {
        println!("✓ Using existing index '{}'", dir_plan.index_path.display());
    }

    // Build contents list
    let mut contents: Vec<String> = Vec::new();

    // Get existing contents if index existed
    if dir_plan.index_exists
        && let Ok(Some(serde_yaml::Value::Sequence(items))) =
            app.get_frontmatter_property(&index_str, "contents")
    {
        for item in items {
            if let serde_yaml::Value::String(s) = item {
                contents.push(s);
            }
        }
    }

    // Add files
    for file in &dir_plan.files {
        let relative = calculate_relative_path(&dir_plan.index_path, file);
        if !contents.contains(&relative) {
            contents.push(relative.clone());

            // Set part_of in the file
            let file_str = file.to_string_lossy();
            let relative_to_index = calculate_relative_path(file, &dir_plan.index_path);
            if let Err(e) = app.set_frontmatter_property(
                &file_str,
                "part_of",
                serde_yaml::Value::String(relative_to_index),
            ) {
                eprintln!("⚠ Error setting part_of in '{}': {}", file.display(), e);
            }
        }
    }

    // Add subdirectory indexes
    for subdir in &dir_plan.subdirs {
        // Find the index in the subdirectory
        if let Ok(Some(subdir_index)) = block_on(ws.find_any_index_in_dir(subdir)) {
            let relative = calculate_relative_path(&dir_plan.index_path, &subdir_index);
            if !contents.contains(&relative) {
                contents.push(relative.clone());

                // Set part_of in the subdirectory index
                let subdir_str = subdir_index.to_string_lossy();
                let relative_to_parent =
                    calculate_relative_path(&subdir_index, &dir_plan.index_path);
                if let Err(e) = app.set_frontmatter_property(
                    &subdir_str,
                    "part_of",
                    serde_yaml::Value::String(relative_to_parent),
                ) {
                    eprintln!(
                        "⚠ Error setting part_of in '{}': {}",
                        subdir_index.display(),
                        e
                    );
                }
            }
        } else {
            // Subdirectory index should have been created - construct the expected path
            let subdir_name = subdir
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_else(|| "index".into());
            let subdir_index = subdir.join(format!("{}_index.md", subdir_name));

            if subdir_index.exists() {
                let relative = calculate_relative_path(&dir_plan.index_path, &subdir_index);
                if !contents.contains(&relative) {
                    contents.push(relative.clone());

                    // Set part_of in the subdirectory index
                    let subdir_str = subdir_index.to_string_lossy();
                    let relative_to_parent =
                        calculate_relative_path(&subdir_index, &dir_plan.index_path);
                    if let Err(e) = app.set_frontmatter_property(
                        &subdir_str,
                        "part_of",
                        serde_yaml::Value::String(relative_to_parent),
                    ) {
                        eprintln!(
                            "⚠ Error setting part_of in '{}': {}",
                            subdir_index.display(),
                            e
                        );
                    }
                }
            }
        }
    }

    // Update contents in index
    let contents_yaml: Vec<serde_yaml::Value> = contents
        .iter()
        .map(|s| serde_yaml::Value::String(s.clone()))
        .collect();

    if let Err(e) = app.set_frontmatter_property(
        &index_str,
        "contents",
        serde_yaml::Value::Sequence(contents_yaml),
    ) {
        eprintln!(
            "✗ Error setting contents in '{}': {}",
            dir_plan.index_path.display(),
            e
        );
    }
}

/// Handle the 'workspace add --new-index' command
/// Creates a new index file and adds files to it
#[allow(clippy::too_many_arguments)]
fn handle_add_with_new_index(
    app: &CliDiaryxAppSync,
    config: &Config,
    ws: &CliWorkspace,
    current_dir: &Path,
    file_pattern: &str,
    additional_pattern: Option<String>,
    index_name: &str,
    yes: bool,
    dry_run: bool,
) {
    use crate::cli::util::{ConfirmResult, prompt_confirm};

    // Collect all file patterns
    let mut all_patterns = vec![file_pattern.to_string()];
    if let Some(additional) = additional_pattern {
        all_patterns.push(additional);
    }

    // Resolve all file paths
    let mut all_files: Vec<PathBuf> = Vec::new();
    for pattern in &all_patterns {
        let paths = resolve_paths(pattern, config, app);
        all_files.extend(paths);
    }

    if all_files.is_empty() {
        eprintln!("✗ No files matched the pattern(s)");
        return;
    }

    // Determine the directory for the new index
    // Use the directory of the first file, or current directory
    let index_dir = all_files
        .first()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| current_dir.to_path_buf());

    // Create index filename
    let index_filename = if index_name.ends_with(".md") {
        index_name.to_string()
    } else {
        format!("{}.md", index_name)
    };
    let index_path = index_dir.join(&index_filename);
    let index_dir = &index_path.parent().unwrap_or(&index_dir);

    if index_path.exists() {
        eprintln!("✗ Index file already exists: {}", index_path.display());
        eprintln!(
            "  Use 'diaryx w add {}' to add files to it",
            index_path.display()
        );
        return;
    }

    // Filter out the new index path from files (in case of glob matching)
    let index_canonical = index_path.canonicalize().ok();
    let all_files: Vec<_> = all_files
        .into_iter()
        .filter(|p| {
            if let Some(ref ic) = index_canonical {
                p.canonicalize().ok().as_ref() != Some(ic)
            } else {
                true
            }
        })
        .collect();

    if all_files.is_empty() {
        eprintln!("✗ No files to add after filtering");
        return;
    }

    // Find parent index for the new index (local index in that directory)
    let parent_index = block_on(ws.find_any_index_in_dir(index_dir)).ok().flatten();

    if dry_run {
        println!("Would create new index: {}", index_path.display());
        if let Some(ref parent) = parent_index {
            println!("Would add new index to parent: {}", parent.display());
        }
        println!("Would add {} file(s) to new index:", all_files.len());
        for f in &all_files {
            println!("  {}", f.display());
        }
        return;
    }

    // Confirm creation
    if !yes {
        println!(
            "Create new index '{}' with {} file(s)?",
            index_path.display(),
            all_files.len()
        );
        match prompt_confirm("Proceed?") {
            ConfirmResult::Yes | ConfirmResult::All => {}
            _ => {
                println!("Aborted");
                return;
            }
        }
    }

    // Create the index file with title
    let title = index_name.replace(['_', '-'], " ");
    let title = title
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    // Build initial contents list
    let contents: Vec<String> = all_files
        .iter()
        .map(|f| calculate_relative_path(&index_path, f))
        .collect();

    let contents_yaml: Vec<serde_yaml::Value> = contents
        .iter()
        .map(|s| serde_yaml::Value::String(s.clone()))
        .collect();

    // Create the index file
    let index_str = index_path.to_string_lossy();

    // First create with title
    if let Err(e) = app.set_frontmatter_property(
        &index_str,
        "title",
        serde_yaml::Value::String(title.clone()),
    ) {
        eprintln!("✗ Error creating index file: {}", e);
        return;
    }

    // Add contents
    if let Err(e) = app.set_frontmatter_property(
        &index_str,
        "contents",
        serde_yaml::Value::Sequence(contents_yaml),
    ) {
        eprintln!("✗ Error setting contents: {}", e);
        return;
    }

    println!("✓ Created index '{}'", index_path.display());

    // Add part_of to new index if there's a parent
    if let Some(ref parent) = parent_index {
        let relative_parent = calculate_relative_path(&index_path, parent);
        if let Err(e) = app.set_frontmatter_property(
            &index_str,
            "part_of",
            serde_yaml::Value::String(relative_parent.clone()),
        ) {
            eprintln!("⚠ Error setting part_of in new index: {}", e);
        }

        // Add new index to parent's contents
        let parent_str = parent.to_string_lossy();
        let relative_index = calculate_relative_path(parent, &index_path);

        match app.get_frontmatter_property(&parent_str, "contents") {
            Ok(Some(serde_yaml::Value::Sequence(mut items))) => {
                items.push(serde_yaml::Value::String(relative_index.clone()));
                if let Err(e) = app.set_frontmatter_property(
                    &parent_str,
                    "contents",
                    serde_yaml::Value::Sequence(items),
                ) {
                    eprintln!("⚠ Error updating parent contents: {}", e);
                } else {
                    println!(
                        "✓ Added '{}' to parent '{}'",
                        relative_index,
                        parent.display()
                    );
                }
            }
            Ok(None) => {
                let items = vec![serde_yaml::Value::String(relative_index.clone())];
                if let Err(e) = app.set_frontmatter_property(
                    &parent_str,
                    "contents",
                    serde_yaml::Value::Sequence(items),
                ) {
                    eprintln!("⚠ Error creating parent contents: {}", e);
                } else {
                    println!(
                        "✓ Added '{}' to parent '{}'",
                        relative_index,
                        parent.display()
                    );
                }
            }
            _ => {}
        }
    }

    // Update part_of in all added files
    for file_path in &all_files {
        let file_str = file_path.to_string_lossy();
        let relative_to_index = calculate_relative_path(file_path, &index_path);

        if let Err(e) = app.set_frontmatter_property(
            &file_str,
            "part_of",
            serde_yaml::Value::String(relative_to_index),
        ) {
            eprintln!(
                "⚠ Error setting part_of in '{}': {}",
                file_path.display(),
                e
            );
        } else {
            println!("✓ Set part_of in '{}'", file_path.display());
        }
    }

    println!(
        "✓ Added {} file(s) to '{}'",
        all_files.len(),
        index_path.display()
    );
}

/// Handle the 'workspace add' command
/// Adds existing file(s) as children of a parent index
fn handle_add(
    app: &CliDiaryxAppSync,
    config: &Config,
    parent: &str,
    child_pattern: &str,
    yes: bool,
    dry_run: bool,
) {
    use crate::cli::util::{ConfirmResult, prompt_confirm};

    // Resolve parent path (should be a single file)
    let parent_paths = resolve_paths(parent, config, app);
    if parent_paths.is_empty() {
        eprintln!("✗ No files matched parent: {}", parent);
        return;
    }
    if parent_paths.len() > 1 {
        eprintln!("✗ Parent must be a single file, but matched multiple:");
        for p in &parent_paths {
            eprintln!("  {}", p.display());
        }
        return;
    }
    let parent_path = &parent_paths[0];

    // Resolve child paths (can be multiple)
    let child_paths = resolve_paths(child_pattern, config, app);
    if child_paths.is_empty() {
        eprintln!("✗ No files matched child pattern: {}", child_pattern);
        return;
    }

    // Filter out the parent from children (auto-skip)
    let parent_canonical = parent_path.canonicalize().ok();
    let child_paths: Vec<_> = child_paths
        .into_iter()
        .filter(|p| {
            let dominated = parent_canonical
                .as_ref()
                .map(|pc| p.canonicalize().ok().as_ref() == Some(pc))
                .unwrap_or(false);
            if dominated {
                println!("⚠ Skipping parent file: {}", p.display());
            }
            !dominated
        })
        .collect();

    if child_paths.is_empty() {
        eprintln!("✗ No child files to add (all matched files were skipped)");
        return;
    }

    let multiple = child_paths.len() > 1;
    let mut confirm_all = yes;

    for child_path in &child_paths {
        // Calculate relative paths
        let relative_child = calculate_relative_path(parent_path, child_path);
        let relative_parent = calculate_relative_path(child_path, parent_path);

        if dry_run {
            println!(
                "Would add '{}' to contents of '{}'",
                relative_child,
                parent_path.display()
            );
            println!(
                "Would set part_of to '{}' in '{}'",
                relative_parent,
                child_path.display()
            );
            continue;
        }

        // Confirm if multiple files and not auto-confirming
        if multiple && !confirm_all {
            println!(
                "Add '{}' to '{}'?",
                child_path.display(),
                parent_path.display()
            );
            match prompt_confirm("Proceed?") {
                ConfirmResult::Yes => {}
                ConfirmResult::No => {
                    println!("Skipped");
                    continue;
                }
                ConfirmResult::All => {
                    confirm_all = true;
                }
                ConfirmResult::Quit => {
                    println!("Aborted");
                    return;
                }
            }
        }

        // Add single child
        add_single_child(
            app,
            parent_path,
            child_path,
            &relative_child,
            &relative_parent,
        );
    }
}

/// Add a single child to a parent index
fn add_single_child(
    app: &CliDiaryxAppSync,
    parent_path: &Path,
    child_path: &Path,
    relative_child: &str,
    relative_parent: &str,
) {
    let parent_str = parent_path.to_string_lossy();
    let child_str = child_path.to_string_lossy();

    // Update parent's contents
    match app.get_frontmatter_property(&parent_str, "contents") {
        Ok(Some(Value::Sequence(mut items))) => {
            // Check if already present
            let child_value = Value::String(relative_child.to_string());
            if items.contains(&child_value) {
                println!(
                    "⚠ '{}' is already in contents of '{}'",
                    relative_child,
                    parent_path.display()
                );
                return;
            } else {
                items.push(child_value);
                if let Err(e) =
                    app.set_frontmatter_property(&parent_str, "contents", Value::Sequence(items))
                {
                    eprintln!("✗ Error updating parent contents: {}", e);
                    return;
                }
                println!(
                    "✓ Added '{}' to contents of '{}'",
                    relative_child,
                    parent_path.display()
                );
            }
        }
        Ok(Some(_)) => {
            eprintln!("✗ Parent's 'contents' property is not a list");
            return;
        }
        Ok(None) => {
            // Create contents with just this child
            let items = vec![Value::String(relative_child.to_string())];
            if let Err(e) =
                app.set_frontmatter_property(&parent_str, "contents", Value::Sequence(items))
            {
                eprintln!("✗ Error creating parent contents: {}", e);
                return;
            }
            println!(
                "✓ Created contents with '{}' in '{}'",
                relative_child,
                parent_path.display()
            );
        }
        Err(e) => {
            eprintln!("✗ Error reading parent: {}", e);
            return;
        }
    }

    // Update child's part_of
    if let Err(e) = app.set_frontmatter_property(
        &child_str,
        "part_of",
        Value::String(relative_parent.to_string()),
    ) {
        eprintln!("✗ Error updating child part_of: {}", e);
        return;
    }
    println!(
        "✓ Set part_of to '{}' in '{}'",
        relative_parent,
        child_path.display()
    );
}

/// Handle the 'workspace create' command
/// Creates a new child file under a parent index
#[allow(clippy::too_many_arguments)]
fn handle_create(
    app: &CliDiaryxAppSync,
    config: &Config,
    parent: &str,
    name: &str,
    title: Option<String>,
    description: Option<String>,
    template: Option<String>,
    is_index: bool,
    edit: bool,
) {
    // Resolve parent path
    let parent_paths = resolve_paths(parent, config, app);
    if parent_paths.is_empty() {
        eprintln!("✗ No files matched parent: {}", parent);
        return;
    }
    if parent_paths.len() > 1 {
        eprintln!("✗ Parent must be a single file, but matched multiple:");
        for p in &parent_paths {
            eprintln!("  {}", p.display());
        }
        return;
    }
    let parent_path = &parent_paths[0];

    // Determine child path (same directory as parent)
    let parent_dir = parent_path.parent().unwrap_or(Path::new("."));
    let child_filename = if name.ends_with(".md") {
        name.to_string()
    } else {
        format!("{}.md", name)
    };
    let child_path = parent_dir.join(&child_filename);

    if child_path.exists() {
        eprintln!("✗ File already exists: {}", child_path.display());
        return;
    }

    // Calculate relative paths
    let relative_child = child_filename.clone();
    let relative_parent = parent_path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| parent_path.to_string_lossy().to_string());

    // Build title
    let display_title = title.unwrap_or_else(|| {
        // Convert filename to title (capitalize, remove extension)
        let stem = name.trim_end_matches(".md");
        prettify_filename(stem)
    });

    // Get template manager and template
    let manager = app.template_manager(Some(&config.default_workspace));
    let template_name = template
        .as_deref()
        .or(config.default_template.as_deref())
        .unwrap_or("note");

    // Try to get the template, fall back to building content manually if not found
    let content = if let Some(tmpl) = manager.get(template_name) {
        // Use template system
        let filename = child_filename.trim_end_matches(".md");
        let mut context = TemplateContext::new()
            .with_title(&display_title)
            .with_filename(filename)
            .with_part_of(&relative_parent);

        // Add description as custom variable if provided
        if let Some(ref desc) = description {
            context = context.with_custom("description", desc.as_str());
        }

        let mut rendered = tmpl.render(&context);

        // If this should be an index, we need to add contents: [] to frontmatter
        // We do this by modifying the rendered content
        if is_index && !rendered.contains("contents:") {
            // Insert contents: [] before the closing ---
            if let Some(idx) = rendered.find("\n---\n") {
                // Find the position after the first ---\n
                if rendered.starts_with("---\n") {
                    let insert_pos = idx;
                    rendered.insert_str(insert_pos, "\ncontents: []");
                }
            }
        }

        rendered
    } else {
        // Fallback: build content manually (template not found)
        eprintln!(
            "⚠ Template '{}' not found, using default format",
            template_name
        );
        let mut frontmatter = format!("---\ntitle: {}\n", display_title);
        if let Some(ref desc) = description {
            frontmatter.push_str(&format!("description: {}\n", desc));
        }
        frontmatter.push_str(&format!("part_of: {}\n", relative_parent));
        if is_index {
            frontmatter.push_str("contents: []\n");
        }
        frontmatter.push_str("---\n\n");
        let body = format!("# {}\n\n", display_title);
        format!("{}{}", frontmatter, body)
    };

    // Create the file
    if let Err(e) = std::fs::write(&child_path, &content) {
        eprintln!("✗ Error creating file: {}", e);
        return;
    }
    println!("✓ Created '{}'", child_path.display());

    // Update parent's contents
    let parent_str = parent_path.to_string_lossy();
    match app.get_frontmatter_property(&parent_str, "contents") {
        Ok(Some(Value::Sequence(mut items))) => {
            items.push(Value::String(relative_child.clone()));
            if let Err(e) =
                app.set_frontmatter_property(&parent_str, "contents", Value::Sequence(items))
            {
                eprintln!("✗ Error updating parent contents: {}", e);
                return;
            }
            println!(
                "✓ Added '{}' to contents of '{}'",
                relative_child,
                parent_path.display()
            );
        }
        Ok(Some(_)) => {
            eprintln!("⚠ Parent's 'contents' property is not a list, skipping update");
        }
        Ok(None) => {
            // Create contents with just this child
            let items = vec![Value::String(relative_child.clone())];
            if let Err(e) =
                app.set_frontmatter_property(&parent_str, "contents", Value::Sequence(items))
            {
                eprintln!("✗ Error creating parent contents: {}", e);
                return;
            }
            println!(
                "✓ Created contents with '{}' in '{}'",
                relative_child,
                parent_path.display()
            );
        }
        Err(e) => {
            eprintln!("⚠ Error reading parent (file was still created): {}", e);
        }
    }

    // Open in editor if requested
    if edit && let Ok(cfg) = Config::load() {
        println!("Opening: {}", child_path.display());
        if let Err(e) = launch_editor(&child_path, &cfg) {
            eprintln!("✗ Error launching editor: {}", e);
        }
    }
}

/// Handle the 'workspace remove' command
/// Removes a child from a parent's hierarchy (does not delete the file)
fn handle_remove(
    app: &CliDiaryxAppSync,
    config: &Config,
    parent: &str,
    child: &str,
    dry_run: bool,
) {
    // Resolve parent path
    let parent_paths = resolve_paths(parent, config, app);
    if parent_paths.is_empty() {
        eprintln!("✗ No files matched parent: {}", parent);
        return;
    }
    if parent_paths.len() > 1 {
        eprintln!("✗ Parent must be a single file, but matched multiple:");
        for p in &parent_paths {
            eprintln!("  {}", p.display());
        }
        return;
    }
    let parent_path = &parent_paths[0];

    // Resolve child path
    let child_paths = resolve_paths(child, config, app);
    if child_paths.is_empty() {
        eprintln!("✗ No files matched child: {}", child);
        return;
    }
    if child_paths.len() > 1 {
        eprintln!("✗ Child must be a single file, but matched multiple:");
        for p in &child_paths {
            eprintln!("  {}", p.display());
        }
        return;
    }
    let child_path = &child_paths[0];

    // Calculate relative path from parent to child
    let relative_child = calculate_relative_path(parent_path, child_path);

    if dry_run {
        println!(
            "Would remove '{}' from contents of '{}'",
            relative_child,
            parent_path.display()
        );
        println!("Would remove part_of from '{}'", child_path.display());
        return;
    }

    let parent_str = parent_path.to_string_lossy();
    let child_str = child_path.to_string_lossy();

    // Update parent's contents
    match app.get_frontmatter_property(&parent_str, "contents") {
        Ok(Some(Value::Sequence(mut items))) => {
            let child_value = Value::String(relative_child.clone());
            let original_len = items.len();
            items.retain(|item| item != &child_value);

            if items.len() == original_len {
                println!(
                    "⚠ '{}' was not in contents of '{}'",
                    relative_child,
                    parent_path.display()
                );
            } else {
                if let Err(e) =
                    app.set_frontmatter_property(&parent_str, "contents", Value::Sequence(items))
                {
                    eprintln!("✗ Error updating parent contents: {}", e);
                    return;
                }
                println!(
                    "✓ Removed '{}' from contents of '{}'",
                    relative_child,
                    parent_path.display()
                );
            }
        }
        Ok(Some(_)) => {
            eprintln!("✗ Parent's 'contents' property is not a list");
            return;
        }
        Ok(None) => {
            println!(
                "⚠ Parent '{}' has no contents property",
                parent_path.display()
            );
        }
        Err(e) => {
            eprintln!("✗ Error reading parent: {}", e);
            return;
        }
    }

    // Remove child's part_of
    if let Err(e) = app.remove_frontmatter_property(&child_str, "part_of") {
        eprintln!("✗ Error removing child part_of: {}", e);
        return;
    }
    println!("✓ Removed part_of from '{}'", child_path.display());
}

/// Create a new index file in the given directory if none exists.
/// Returns the path to the created index.
fn create_new_index(app: &CliDiaryxAppSync, dir: &Path) -> Option<PathBuf> {
    // 1. Determine name
    let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("index");
    let safe_name = slugify(dir_name);
    let index_name = format!("{}.md", safe_name);
    let index_path = dir.join(&index_name);

    // 2. Check existence using a local FS instance (since app.fs is private)
    let fs = RealFileSystem;
    if fs.exists(&index_path) {
        return Some(index_path);
    }

    // 3. Create file with basic frontmatter
    let title = prettify_filename(dir_name);
    let path_str = index_path.to_string_lossy();

    // Create title
    if let Err(e) = app.set_frontmatter_property(&path_str, "title", Value::String(title.clone())) {
        eprintln!("Error creating index: {}", e);
        return None;
    }
    // Create contents
    if let Err(e) = app.set_frontmatter_property(&path_str, "contents", Value::Sequence(vec![])) {
        eprintln!("Error initializing index contents: {}", e);
        return None;
    }
    // Add title header
    let _ = app.set_content(&path_str, &format!("# {}", title));

    println!("  ✓ Created new index: {}", index_path.display());
    Some(index_path)
}
