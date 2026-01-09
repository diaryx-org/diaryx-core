//! Property command handlers

use serde_yaml::Value;

use crate::cli::args::PropertyCommands;
use crate::cli::util::{ConfirmResult, format_value, load_config, prompt_confirm, resolve_paths};
use crate::cli::{block_on, CliDiaryxAppSync};

/// Handle the property command
pub fn handle_property_command(app: &CliDiaryxAppSync, operation: PropertyCommands) {
    let config = match load_config() {
        Some(c) => c,
        None => return,
    };

    match operation {
        PropertyCommands::Get { path, key, yes } => {
            handle_get_command(app, &config, &path, &key, yes);
        }
        PropertyCommands::Set {
            path,
            key,
            value,
            yes,
            dry_run,
        } => {
            handle_set_command(app, &config, &path, &key, &value, yes, dry_run);
        }
        PropertyCommands::Remove {
            path,
            key,
            yes,
            dry_run,
        } => {
            handle_remove_command(app, &config, &path, &key, yes, dry_run);
        }
        PropertyCommands::Rename {
            path,
            old_key,
            new_key,
            yes,
            dry_run,
        } => {
            handle_rename_command(app, &config, &path, &old_key, &new_key, yes, dry_run);
        }
        PropertyCommands::List { path, yes } => {
            handle_list_command(app, &config, &path, yes);
        }
        PropertyCommands::Append {
            path,
            key,
            value,
            yes,
            dry_run,
        } => {
            handle_list_append_command(app, &config, &path, &key, &value, yes, dry_run);
        }
        PropertyCommands::Prepend {
            path,
            key,
            value,
            yes,
            dry_run,
        } => {
            handle_list_prepend_command(app, &config, &path, &key, &value, yes, dry_run);
        }
        PropertyCommands::Pop {
            path,
            key,
            index,
            yes,
            dry_run,
        } => {
            handle_list_pop_command(app, &config, &path, &key, index, yes, dry_run);
        }
        PropertyCommands::SetAt {
            path,
            key,
            index,
            value,
            yes,
            dry_run,
        } => {
            handle_list_set_at_command(app, &config, &path, &key, index, &value, yes, dry_run);
        }
        PropertyCommands::RemoveValue {
            path,
            key,
            value,
            yes,
            dry_run,
        } => {
            handle_list_remove_value_command(app, &config, &path, &key, &value, yes, dry_run);
        }
        PropertyCommands::Show { path, key, yes } => {
            handle_show_command(app, &config, &path, &key, yes);
        }
    }
}

/// Handle get command
fn handle_get_command(
    app: &CliDiaryxAppSync,
    config: &diaryx_core::config::Config,
    path: &str,
    key: &str,
    _yes: bool,
) {
    let paths = resolve_paths(path, config, app);
    if paths.is_empty() {
        eprintln!("✗ No files matched: {}", path);
        return;
    }

    let multiple_files = paths.len() > 1;

    for file_path in paths {
        let path_str = file_path.to_string_lossy();
        let prefix = if multiple_files {
            format!("{}: ", file_path.display())
        } else {
            String::new()
        };

        match app.get_frontmatter_property(&path_str, key) {
            Ok(Some(value)) => match &value {
                Value::Sequence(items) => {
                    for item in items {
                        println!("{}{}", prefix, format_value(item));
                    }
                }
                Value::String(s) => {
                    println!("{}{}", prefix, s);
                }
                _ => {
                    println!(
                        "{}{}",
                        prefix,
                        serde_yaml::to_string(&value).unwrap_or_default().trim()
                    );
                }
            },
            Ok(None) => {
                eprintln!("{}Property '{}' not found", prefix, key);
            }
            Err(e) => eprintln!("{}✗ Error: {}", prefix, e),
        }
    }
}

/// Handle set command
fn handle_set_command(
    app: &CliDiaryxAppSync,
    config: &diaryx_core::config::Config,
    path: &str,
    key: &str,
    value: &str,
    yes: bool,
    dry_run: bool,
) {
    let paths = resolve_paths(path, config, app);
    if paths.is_empty() {
        eprintln!("✗ No files matched: {}", path);
        return;
    }

    let multiple_files = paths.len() > 1;
    let mut skip_confirm = yes || !multiple_files;

    let yaml_value = match serde_yaml::from_str::<Value>(value) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("✗ Invalid YAML value: {}", e);
            return;
        }
    };

    for file_path in paths {
        let path_str = file_path.to_string_lossy();
        let prefix = if multiple_files {
            format!("{}: ", file_path.display())
        } else {
            String::new()
        };

        if !skip_confirm && !dry_run {
            let msg = format!("Set '{}' in {}?", key, file_path.display());
            match prompt_confirm(&msg) {
                ConfirmResult::Yes => {}
                ConfirmResult::No => continue,
                ConfirmResult::All => skip_confirm = true,
                ConfirmResult::Quit => return,
            }
        }

        if dry_run {
            println!("{}Would set '{}' to '{}'", prefix, key, value);
            continue;
        }

        match app.set_frontmatter_property(&path_str, key, yaml_value.clone()) {
            Ok(_) => println!("{}✓ Set '{}'", prefix, key),
            Err(e) => eprintln!("{}✗ Error: {}", prefix, e),
        }
    }
}

/// Handle remove command
fn handle_remove_command(
    app: &CliDiaryxAppSync,
    config: &diaryx_core::config::Config,
    path: &str,
    key: &str,
    yes: bool,
    dry_run: bool,
) {
    let paths = resolve_paths(path, config, app);
    if paths.is_empty() {
        eprintln!("✗ No files matched: {}", path);
        return;
    }

    let multiple_files = paths.len() > 1;
    let mut skip_confirm = yes || !multiple_files;

    for file_path in paths {
        let path_str = file_path.to_string_lossy();
        let prefix = if multiple_files {
            format!("{}: ", file_path.display())
        } else {
            String::new()
        };

        if !skip_confirm && !dry_run {
            let msg = format!("Remove '{}' from {}?", key, file_path.display());
            match prompt_confirm(&msg) {
                ConfirmResult::Yes => {}
                ConfirmResult::No => continue,
                ConfirmResult::All => skip_confirm = true,
                ConfirmResult::Quit => return,
            }
        }

        if dry_run {
            println!("{}Would remove '{}'", prefix, key);
            continue;
        }

        match app.remove_frontmatter_property(&path_str, key) {
            Ok(_) => println!("{}✓ Removed '{}'", prefix, key),
            Err(e) => eprintln!("{}✗ Error: {}", prefix, e),
        }
    }
}

/// Handle rename command
fn handle_rename_command(
    app: &CliDiaryxAppSync,
    config: &diaryx_core::config::Config,
    path: &str,
    old_key: &str,
    new_key: &str,
    yes: bool,
    dry_run: bool,
) {
    let paths = resolve_paths(path, config, app);
    if paths.is_empty() {
        eprintln!("✗ No files matched: {}", path);
        return;
    }

    let multiple_files = paths.len() > 1;
    let mut skip_confirm = yes || !multiple_files;

    for file_path in paths {
        let path_str = file_path.to_string_lossy();
        let prefix = if multiple_files {
            format!("{}: ", file_path.display())
        } else {
            String::new()
        };

        if !skip_confirm && !dry_run {
            let msg = format!(
                "Rename '{}' to '{}' in {}?",
                old_key,
                new_key,
                file_path.display()
            );
            match prompt_confirm(&msg) {
                ConfirmResult::Yes => {}
                ConfirmResult::No => continue,
                ConfirmResult::All => skip_confirm = true,
                ConfirmResult::Quit => return,
            }
        }

        if dry_run {
            println!("{}Would rename '{}' to '{}'", prefix, old_key, new_key);
            continue;
        }

        match app.rename_frontmatter_property(&path_str, old_key, new_key) {
            Ok(true) => println!("{}✓ Renamed '{}' to '{}'", prefix, old_key, new_key),
            Ok(false) => eprintln!("{}⚠ Property '{}' not found", prefix, old_key),
            Err(e) => eprintln!("{}✗ Error: {}", prefix, e),
        }
    }
}

/// Handle list command (list all properties)
fn handle_list_command(
    app: &CliDiaryxAppSync,
    config: &diaryx_core::config::Config,
    path: &str,
    _yes: bool,
) {
    let paths = resolve_paths(path, config, app);
    if paths.is_empty() {
        eprintln!("✗ No files matched: {}", path);
        return;
    }

    let multiple_files = paths.len() > 1;

    for file_path in paths {
        let path_str = file_path.to_string_lossy();
        let prefix = if multiple_files {
            format!("{}:   ", file_path.display())
        } else {
            String::new()
        };

        match app.get_all_frontmatter(&path_str) {
            Ok(frontmatter) => {
                if multiple_files {
                    println!("{}:", file_path.display());
                }
                if frontmatter.is_empty() {
                    println!("{}No properties", prefix);
                } else {
                    for (key, value) in frontmatter {
                        println!("{}{}: {}", prefix, key, format_value(&value));
                    }
                }
                if multiple_files {
                    println!();
                }
            }
            Err(e) => eprintln!("{}✗ Error: {}", prefix, e),
        }
    }
}

/// Handle show command (show list with indices)
fn handle_show_command(
    app: &CliDiaryxAppSync,
    config: &diaryx_core::config::Config,
    path: &str,
    key: &str,
    _yes: bool,
) {
    let paths = resolve_paths(path, config, app);
    if paths.is_empty() {
        eprintln!("✗ No files matched: {}", path);
        return;
    }

    let multiple_files = paths.len() > 1;

    for file_path in paths {
        let path_str = file_path.to_string_lossy();
        let prefix = if multiple_files {
            format!("{}:   ", file_path.display())
        } else {
            String::new()
        };

        match app.get_frontmatter_property(&path_str, key) {
            Ok(Some(Value::Sequence(items))) => {
                if multiple_files {
                    println!("{}:", file_path.display());
                }
                if items.is_empty() {
                    println!("{}[] (empty list)", prefix);
                } else {
                    for (i, item) in items.iter().enumerate() {
                        println!("{}[{}] {}", prefix, i, format_value(item));
                    }
                }
                if multiple_files {
                    println!();
                }
            }
            Ok(Some(_)) => {
                eprintln!("{}✗ Property '{}' is not a list", prefix, key);
            }
            Ok(None) => {
                eprintln!("{}Property '{}' not found", prefix, key);
            }
            Err(e) => eprintln!("{}✗ Error: {}", prefix, e),
        }
    }
}

/// Handle list append command
fn handle_list_append_command(
    app: &CliDiaryxAppSync,
    config: &diaryx_core::config::Config,
    path: &str,
    key: &str,
    value: &str,
    yes: bool,
    dry_run: bool,
) {
    handle_list_operation(
        app,
        config,
        path,
        key,
        yes,
        dry_run,
        &format!("Append '{}' to '{}'", value, key),
        |items| match serde_yaml::from_str::<Value>(value) {
            Ok(yaml_value) => {
                items.push(yaml_value);
                Ok(format!("✓ Appended to '{}'", key))
            }
            Err(e) => Err(format!("✗ Invalid YAML value: {}", e)),
        },
    );
}

/// Handle list prepend command
fn handle_list_prepend_command(
    app: &CliDiaryxAppSync,
    config: &diaryx_core::config::Config,
    path: &str,
    key: &str,
    value: &str,
    yes: bool,
    dry_run: bool,
) {
    handle_list_operation(
        app,
        config,
        path,
        key,
        yes,
        dry_run,
        &format!("Prepend '{}' to '{}'", value, key),
        |items| match serde_yaml::from_str::<Value>(value) {
            Ok(yaml_value) => {
                items.insert(0, yaml_value);
                Ok(format!("✓ Prepended to '{}'", key))
            }
            Err(e) => Err(format!("✗ Invalid YAML value: {}", e)),
        },
    );
}

/// Handle list pop command
fn handle_list_pop_command(
    app: &CliDiaryxAppSync,
    config: &diaryx_core::config::Config,
    path: &str,
    key: &str,
    index: i32,
    yes: bool,
    dry_run: bool,
) {
    handle_list_operation(
        app,
        config,
        path,
        key,
        yes,
        dry_run,
        &format!("Pop index {} from '{}'", index, key),
        |items| {
            if items.is_empty() {
                return Err("✗ Cannot pop from empty list".to_string());
            }

            let actual_index = if index < 0 {
                let i = items.len() as i32 + index;
                if i < 0 {
                    return Err(format!("✗ Index {} out of range", index));
                }
                i as usize
            } else {
                index as usize
            };

            if actual_index >= items.len() {
                return Err(format!(
                    "✗ Index {} out of range (list has {} items)",
                    index,
                    items.len()
                ));
            }

            let removed = items.remove(actual_index);
            Ok(format!(
                "✓ Removed [{}]: {}",
                actual_index,
                format_value(&removed)
            ))
        },
    );
}

/// Handle list set-at command
#[allow(clippy::too_many_arguments)]
fn handle_list_set_at_command(
    app: &CliDiaryxAppSync,
    config: &diaryx_core::config::Config,
    path: &str,
    key: &str,
    index: usize,
    value: &str,
    yes: bool,
    dry_run: bool,
) {
    handle_list_operation(
        app,
        config,
        path,
        key,
        yes,
        dry_run,
        &format!("Set index {} to '{}' in '{}'", index, value, key),
        |items| {
            if index >= items.len() {
                return Err(format!(
                    "✗ Index {} out of range (list has {} items)",
                    index,
                    items.len()
                ));
            }

            match serde_yaml::from_str::<Value>(value) {
                Ok(yaml_value) => {
                    items[index] = yaml_value;
                    Ok(format!("✓ Set [{}] in '{}'", index, key))
                }
                Err(e) => Err(format!("✗ Invalid YAML value: {}", e)),
            }
        },
    );
}

/// Handle list remove-value command
fn handle_list_remove_value_command(
    app: &CliDiaryxAppSync,
    config: &diaryx_core::config::Config,
    path: &str,
    key: &str,
    value: &str,
    yes: bool,
    dry_run: bool,
) {
    handle_list_operation(
        app,
        config,
        path,
        key,
        yes,
        dry_run,
        &format!("Remove '{}' from '{}'", value, key),
        |items| match serde_yaml::from_str::<Value>(value) {
            Ok(yaml_value) => {
                let original_len = items.len();
                items.retain(|item| item != &yaml_value);
                let removed_count = original_len - items.len();
                if removed_count > 0 {
                    Ok(format!(
                        "✓ Removed {} occurrence(s) of '{}'",
                        removed_count, value
                    ))
                } else {
                    Err(format!("✗ Value '{}' not found in list", value))
                }
            }
            Err(e) => Err(format!("✗ Invalid YAML value: {}", e)),
        },
    );
}

/// Generic handler for list operations
#[allow(clippy::too_many_arguments)]
fn handle_list_operation<F>(
    app: &CliDiaryxAppSync,
    config: &diaryx_core::config::Config,
    path: &str,
    key: &str,
    yes: bool,
    dry_run: bool,
    description: &str,
    operation: F,
) where
    F: Fn(&mut Vec<Value>) -> Result<String, String> + Clone,
{
    let paths = resolve_paths(path, config, app);
    if paths.is_empty() {
        eprintln!("✗ No files matched: {}", path);
        return;
    }

    let multiple_files = paths.len() > 1;
    let mut skip_confirm = yes || !multiple_files;

    for file_path in paths {
        let path_str = file_path.to_string_lossy();
        let prefix = if multiple_files {
            format!("{}: ", file_path.display())
        } else {
            String::new()
        };

        if !skip_confirm && !dry_run {
            let msg = format!("{} in {}?", description, file_path.display());
            match prompt_confirm(&msg) {
                ConfirmResult::Yes => {}
                ConfirmResult::No => continue,
                ConfirmResult::All => skip_confirm = true,
                ConfirmResult::Quit => return,
            }
        }

        if dry_run {
            println!("{}Would {}", prefix, description.to_lowercase());
            continue;
        }

        // Get current value
        let current = match app.get_frontmatter_property(&path_str, key) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{}✗ Error reading property: {}", prefix, e);
                continue;
            }
        };

        // Convert to list or create empty list
        let mut items = match current {
            Some(Value::Sequence(items)) => items,
            Some(_) => {
                eprintln!("{}✗ Property '{}' is not a list", prefix, key);
                continue;
            }
            None => Vec::new(),
        };

        // Apply operation
        match operation.clone()(&mut items) {
            Ok(msg) => {
                // Save updated list
                match app.set_frontmatter_property(&path_str, key, Value::Sequence(items)) {
                    Ok(_) => println!("{}{}", prefix, msg),
                    Err(e) => eprintln!("{}✗ Error saving property: {}", prefix, e),
                }
            }
            Err(msg) => eprintln!("{}{}", prefix, msg),
        }
    }
}
