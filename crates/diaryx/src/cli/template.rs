//! Template command handlers

use diaryx_core::config::Config;
use diaryx_core::fs::RealFileSystem;
use diaryx_core::template::{TEMPLATE_VARIABLES, TemplateManager, TemplateSource};
use std::io::{self, Write};

use crate::cli::args::TemplateCommands;
use crate::cli::CliDiaryxAppSync;
use crate::editor::launch_editor;

/// Handle template subcommands
pub fn handle_template_command(command: TemplateCommands, app: &CliDiaryxAppSync) {
    let config = Config::load().ok();
    let workspace_dir = config.as_ref().map(|c| c.default_workspace.as_path());
    let manager = app.template_manager(workspace_dir);

    match command {
        TemplateCommands::List { paths } => {
            handle_list(&manager, paths);
        }

        TemplateCommands::Show { name } => {
            handle_show(&manager, &name);
        }

        TemplateCommands::New { name, from, edit } => {
            handle_new(&manager, &name, from.as_deref(), edit, config.as_ref());
        }

        TemplateCommands::Edit { name } => {
            handle_edit(&manager, &name, config.as_ref());
        }

        TemplateCommands::Delete { name, yes } => {
            handle_delete(&manager, &name, yes);
        }

        TemplateCommands::Path => {
            handle_path(&manager);
        }

        TemplateCommands::Variables => {
            handle_variables();
        }
    }
}

/// Handle the 'template list' command
fn handle_list(manager: &TemplateManager<&RealFileSystem>, show_paths: bool) {
    let templates = manager.list();

    if templates.is_empty() {
        println!("No templates found.");
        return;
    }

    println!("Available templates:\n");

    for info in templates {
        if show_paths {
            let path_str = info
                .path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "(built-in)".to_string());
            println!("  {} [{}]", info.name, info.source);
            println!("    {}", path_str);
        } else {
            println!("  {} [{}]", info.name, info.source);
        }
    }

    println!();
    println!("Use 'diaryx template show <name>' to view a template's contents.");
}

/// Handle the 'template show' command
fn handle_show(manager: &TemplateManager<&RealFileSystem>, name: &str) {
    match manager.get(name) {
        Some(template) => {
            println!("Template: {}\n", template.name);
            println!("{}", template.raw_content);
        }
        None => {
            eprintln!("✗ Template not found: {}", name);
            eprintln!("  Use 'diaryx template list' to see available templates.");
        }
    }
}

/// Handle the 'template new' command
fn handle_new(
    manager: &TemplateManager<&RealFileSystem>,
    name: &str,
    from: Option<&str>,
    edit: bool,
    config: Option<&Config>,
) {
    // Check if template already exists in user directory
    let user_templates = manager.list();
    let exists_in_user = user_templates
        .iter()
        .any(|t| t.name == name && t.source == TemplateSource::User);

    if exists_in_user {
        eprintln!("✗ Template '{}' already exists in user templates.", name);
        eprintln!("  Use 'diaryx template edit {}' to modify it.", name);
        return;
    }

    // Get initial content
    let content = if let Some(source_name) = from {
        // Copy from existing template
        match manager.get(source_name) {
            Some(template) => template.raw_content.clone(),
            None => {
                eprintln!("✗ Source template not found: {}", source_name);
                return;
            }
        }
    } else {
        // Create a default template structure
        default_template_content(name)
    };

    // Create the template
    match manager.create_template(name, &content) {
        Ok(path) => {
            println!("✓ Created template: {}", path.display());

            if edit {
                if let Some(cfg) = config {
                    println!("Opening in editor...");
                    if let Err(e) = launch_editor(&path, cfg) {
                        eprintln!("✗ Error launching editor: {}", e);
                    }
                } else {
                    eprintln!("⚠ No config found, cannot open editor.");
                }
            } else {
                println!("  Use 'diaryx template edit {}' to customize it.", name);
            }
        }
        Err(e) => {
            eprintln!("✗ Error creating template: {}", e);
        }
    }
}

/// Handle the 'template edit' command
fn handle_edit(manager: &TemplateManager<&RealFileSystem>, name: &str, config: Option<&Config>) {
    let templates = manager.list();

    // Find the template
    let template_info = templates.iter().find(|t| t.name == name);

    match template_info {
        Some(info) => {
            match &info.source {
                TemplateSource::Builtin => {
                    // Can't edit built-in, offer to copy to user templates
                    eprintln!("✗ Cannot edit built-in template '{}' directly.", name);
                    eprintln!(
                        "  Use 'diaryx template new {} --from {}' to create an editable copy.",
                        name, name
                    );
                }
                TemplateSource::User | TemplateSource::Workspace => {
                    if let Some(path) = &info.path {
                        if let Some(cfg) = config {
                            println!("Opening: {}", path.display());
                            if let Err(e) = launch_editor(path, cfg) {
                                eprintln!("✗ Error launching editor: {}", e);
                            }
                        } else {
                            eprintln!("✗ No config found, cannot determine editor.");
                            eprintln!("  Template file: {}", path.display());
                        }
                    } else {
                        eprintln!("✗ Template path not found.");
                    }
                }
            }
        }
        None => {
            eprintln!("✗ Template not found: {}", name);
            eprintln!("  Use 'diaryx template list' to see available templates.");
        }
    }
}

/// Handle the 'template delete' command
fn handle_delete(manager: &TemplateManager<&RealFileSystem>, name: &str, yes: bool) {
    let templates = manager.list();

    // Find the template
    let template_info = templates.iter().find(|t| t.name == name);

    match template_info {
        Some(info) => {
            match &info.source {
                TemplateSource::Builtin => {
                    eprintln!("✗ Cannot delete built-in template '{}'.", name);
                }
                TemplateSource::User | TemplateSource::Workspace => {
                    if let Some(path) = &info.path {
                        // Confirm deletion
                        if !yes {
                            print!("Delete template '{}' at {}? [y/N] ", name, path.display());
                            io::stdout().flush().unwrap();

                            let mut input = String::new();
                            if io::stdin().read_line(&mut input).is_err() {
                                eprintln!("✗ Failed to read input");
                                return;
                            }

                            let input = input.trim().to_lowercase();
                            if input != "y" && input != "yes" {
                                println!("Cancelled.");
                                return;
                            }
                        }

                        // Delete the file
                        match std::fs::remove_file(path) {
                            Ok(()) => {
                                println!("✓ Deleted template: {}", name);
                            }
                            Err(e) => {
                                eprintln!("✗ Error deleting template: {}", e);
                            }
                        }
                    } else {
                        eprintln!("✗ Template path not found.");
                    }
                }
            }
        }
        None => {
            eprintln!("✗ Template not found: {}", name);
            eprintln!("  Use 'diaryx template list' to see available templates.");
        }
    }
}

/// Handle the 'template path' command
fn handle_path(manager: &TemplateManager<&RealFileSystem>) {
    println!("Template directories (in priority order):\n");

    if let Some(workspace_dir) = manager.workspace_templates_dir() {
        let exists = workspace_dir.exists();
        let status = if exists { "✓" } else { "○" };
        println!("  {} Workspace: {}", status, workspace_dir.display());
    } else {
        println!("  ○ Workspace: (not configured)");
    }

    if let Some(user_dir) = manager.user_templates_dir() {
        let exists = user_dir.exists();
        let status = if exists { "✓" } else { "○" };
        println!("  {} User:      {}", status, user_dir.display());
    } else {
        println!("  ○ User:      (not available)");
    }

    println!("  ✓ Built-in:  (compiled into binary)");
    println!();
    println!("Legend: ✓ = exists, ○ = does not exist");
}

/// Handle the 'template variables' command
fn handle_variables() {
    println!("Available template variables:\n");

    for (name, description) in TEMPLATE_VARIABLES {
        println!("  {{{{{}}}}}  ", name);
        println!("      {}", description);
        println!();
    }

    println!("Custom format examples:");
    println!("  {{{{date:%B %d, %Y}}}}     → \"January 15, 2024\"");
    println!("  {{{{time:%H:%M:%S}}}}      → \"14:30:45\"");
    println!("  {{{{datetime:%A, %B %d}}}} → \"Monday, January 15\"");
    println!();
    println!("Format codes follow strftime conventions.");
}

/// Generate default content for a new template
fn default_template_content(_name: &str) -> String {
    r#"---
title: "{{{{title}}}}"
created: {{{{timestamp}}}}
---

# {{{{title}}}}

"#
    .to_string()
}
