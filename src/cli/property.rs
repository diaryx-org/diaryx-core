//! Property command handlers

use diaryx_core::entry::DiaryxApp;
use diaryx_core::fs::RealFileSystem;
use serde_yaml::Value;

use crate::cli::args::PropertyOperation;
use crate::cli::util::{format_value, load_config, prompt_confirm, resolve_paths, ConfirmResult};

/// Describes what a mutating operation will do (for dry-run and confirmation)
pub fn describe_operation(operation: &PropertyOperation, key: &str) -> String {
    match operation {
        PropertyOperation::Set { value } => format!("Set '{}' to '{}'", key, value),
        PropertyOperation::Remove => format!("Remove '{}'", key),
        PropertyOperation::Rename { new_key } => format!("Rename '{}' to '{}'", key, new_key),
        PropertyOperation::Append { value } => format!("Append '{}' to '{}'", value, key),
        PropertyOperation::Prepend { value } => format!("Prepend '{}' to '{}'", value, key),
        PropertyOperation::Pop { index } => format!("Pop index {} from '{}'", index, key),
        PropertyOperation::SetAt { index, value } => {
            format!("Set index {} to '{}' in '{}'", index, value, key)
        }
        PropertyOperation::RemoveValue { value } => {
            format!("Remove value '{}' from '{}'", value, key)
        }
        _ => String::new(),
    }
}

/// Check if an operation is read-only
pub fn is_read_only(operation: &Option<PropertyOperation>, _has_key: bool) -> bool {
    match operation {
        None => true, // listing all or getting a key
        Some(PropertyOperation::Get) => true,
        Some(PropertyOperation::Show) => true,
        _ => false,
    }
}

/// Handle the property command
pub fn handle_property_command(
    app: &DiaryxApp<RealFileSystem>,
    path: String,
    key: Option<String>,
    operation: Option<PropertyOperation>,
    list: bool,
    yes: bool,
    dry_run: bool,
) {
    let config = match load_config() {
        Some(c) => c,
        None => return,
    };

    let paths = resolve_paths(&path, &config, app);

    if paths.is_empty() {
        eprintln!("✗ No files matched pattern: {}", path);
        return;
    }

    let multiple_files = paths.len() > 1;
    let read_only = is_read_only(&operation, key.is_some());
    let mut skip_confirm = yes || read_only || !multiple_files;

    for file_path in paths {
        let path_str = file_path.to_string_lossy();
        let display_path = file_path.display();

        // Handle confirmation for mutating operations on multiple files
        if !skip_confirm && !dry_run {
            if let Some(ref op) = operation {
                if let Some(ref k) = key {
                    let desc = describe_operation(op, k);
                    let msg = format!("{} in {}?", desc, display_path);
                    match prompt_confirm(&msg) {
                        ConfirmResult::Yes => {}
                        ConfirmResult::No => continue,
                        ConfirmResult::All => skip_confirm = true,
                        ConfirmResult::Quit => return,
                    }
                }
            }
        }

        // Handle dry-run
        if dry_run {
            if let Some(ref op) = operation {
                if let Some(ref k) = key {
                    let desc = describe_operation(op, k);
                    println!("Would {} in {}", desc.to_lowercase(), display_path);
                }
            }
            continue;
        }

        // Prefix output with filename for multiple files
        let prefix = if multiple_files {
            format!("{}: ", display_path)
        } else {
            String::new()
        };

        match (&key, &operation, list) {
            // No key: list all properties
            (None, _, _) => match app.get_all_frontmatter(&path_str) {
                Ok(frontmatter) => {
                    if multiple_files {
                        println!("{}:", display_path);
                    }
                    if frontmatter.is_empty() {
                        println!("{}No frontmatter properties", prefix);
                    } else {
                        for (key, value) in frontmatter {
                            println!("{}  {}: {}", prefix, key, format_value(&value));
                        }
                    }
                    if multiple_files {
                        println!();
                    }
                }
                Err(e) => eprintln!("{}✗ Error: {}", prefix, e),
            },

            // Key with no operation: default to get
            (Some(k), None, false) => {
                handle_get(app, &path_str, &prefix, k);
            }

            // Explicit get
            (Some(k), Some(PropertyOperation::Get), _) => {
                handle_get(app, &path_str, &prefix, k);
            }

            // Set
            (Some(k), Some(PropertyOperation::Set { value }), _) => {
                match serde_yaml::from_str::<Value>(value) {
                    Ok(yaml_value) => {
                        match app.set_frontmatter_property(&path_str, k, yaml_value) {
                            Ok(_) => println!("{}✓ Set '{}'", prefix, k),
                            Err(e) => eprintln!("{}✗ Error: {}", prefix, e),
                        }
                    }
                    Err(e) => eprintln!("{}✗ Invalid YAML value: {}", prefix, e),
                }
            }

            // Remove
            (Some(k), Some(PropertyOperation::Remove), _) => {
                match app.remove_frontmatter_property(&path_str, k) {
                    Ok(_) => println!("{}✓ Removed '{}'", prefix, k),
                    Err(e) => eprintln!("{}✗ Error: {}", prefix, e),
                }
            }

            // Rename
            (Some(k), Some(PropertyOperation::Rename { new_key }), _) => {
                match app.rename_frontmatter_property(&path_str, k, new_key) {
                    Ok(true) => println!("{}✓ Renamed '{}' to '{}'", prefix, k, new_key),
                    Ok(false) => {
                        eprintln!("{}⚠ Property '{}' not found, nothing to rename", prefix, k)
                    }
                    Err(e) => eprintln!("{}✗ Error: {}", prefix, e),
                }
            }

            // List operations: Show with indices
            (Some(k), Some(PropertyOperation::Show), _) | (Some(k), None, true) => {
                match app.get_frontmatter_property(&path_str, k) {
                    Ok(Some(Value::Sequence(items))) => {
                        if multiple_files {
                            println!("{}:", display_path);
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
                        eprintln!("{}✗ Property '{}' is not a list", prefix, k);
                    }
                    Ok(None) => {
                        eprintln!("{}Property '{}' not found", prefix, k);
                    }
                    Err(e) => eprintln!("{}✗ Error: {}", prefix, e),
                }
            }

            // List operations: Append
            (Some(k), Some(PropertyOperation::Append { value }), _) => {
                handle_list_operation(
                    app,
                    &path_str,
                    &prefix,
                    k,
                    |items| match serde_yaml::from_str::<Value>(value) {
                        Ok(yaml_value) => {
                            items.push(yaml_value);
                            Ok(format!("✓ Appended to '{}'", k))
                        }
                        Err(e) => Err(format!("✗ Invalid YAML value: {}", e)),
                    },
                );
            }

            // List operations: Prepend
            (Some(k), Some(PropertyOperation::Prepend { value }), _) => {
                handle_list_operation(
                    app,
                    &path_str,
                    &prefix,
                    k,
                    |items| match serde_yaml::from_str::<Value>(value) {
                        Ok(yaml_value) => {
                            items.insert(0, yaml_value);
                            Ok(format!("✓ Prepended to '{}'", k))
                        }
                        Err(e) => Err(format!("✗ Invalid YAML value: {}", e)),
                    },
                );
            }

            // List operations: Pop by index
            (Some(k), Some(PropertyOperation::Pop { index }), _) => {
                let idx = *index;
                handle_list_operation(app, &path_str, &prefix, k, |items| {
                    if items.is_empty() {
                        return Err("✗ Cannot pop from empty list".to_string());
                    }

                    let actual_index = if idx < 0 {
                        let i = items.len() as i32 + idx;
                        if i < 0 {
                            return Err(format!("✗ Index {} out of range", idx));
                        }
                        i as usize
                    } else {
                        idx as usize
                    };

                    if actual_index >= items.len() {
                        return Err(format!(
                            "✗ Index {} out of range (list has {} items)",
                            idx,
                            items.len()
                        ));
                    }

                    let removed = items.remove(actual_index);
                    Ok(format!(
                        "✓ Removed [{}]: {}",
                        actual_index,
                        format_value(&removed)
                    ))
                });
            }

            // List operations: Set at index
            (Some(k), Some(PropertyOperation::SetAt { index, value }), _) => {
                let idx = *index;
                handle_list_operation(app, &path_str, &prefix, k, |items| {
                    if idx >= items.len() {
                        return Err(format!(
                            "✗ Index {} out of range (list has {} items)",
                            idx,
                            items.len()
                        ));
                    }

                    match serde_yaml::from_str::<Value>(value) {
                        Ok(yaml_value) => {
                            items[idx] = yaml_value;
                            Ok(format!("✓ Set [{}] in '{}'", idx, k))
                        }
                        Err(e) => Err(format!("✗ Invalid YAML value: {}", e)),
                    }
                });
            }

            // List operations: Remove by value
            (Some(k), Some(PropertyOperation::RemoveValue { value }), _) => {
                handle_list_operation(
                    app,
                    &path_str,
                    &prefix,
                    k,
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
        }
    }
}

fn handle_get(app: &DiaryxApp<RealFileSystem>, path_str: &str, prefix: &str, key: &str) {
    match app.get_frontmatter_property(path_str, key) {
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

fn handle_list_operation<F>(
    app: &DiaryxApp<RealFileSystem>,
    path_str: &str,
    prefix: &str,
    key: &str,
    operation: F,
) where
    F: FnOnce(&mut Vec<Value>) -> Result<String, String>,
{
    // Get current value
    let current = match app.get_frontmatter_property(path_str, key) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}✗ Error reading property: {}", prefix, e);
            return;
        }
    };

    // Convert to list or create empty list
    let mut items = match current {
        Some(Value::Sequence(items)) => items,
        Some(_) => {
            eprintln!("{}✗ Property '{}' is not a list", prefix, key);
            return;
        }
        None => Vec::new(),
    };

    // Apply operation
    match operation(&mut items) {
        Ok(msg) => {
            // Save updated list
            match app.set_frontmatter_property(path_str, key, Value::Sequence(items)) {
                Ok(_) => println!("{}{}", prefix, msg),
                Err(e) => eprintln!("{}✗ Error saving property: {}", prefix, e),
            }
        }
        Err(msg) => eprintln!("{}{}", prefix, msg),
    }
}
