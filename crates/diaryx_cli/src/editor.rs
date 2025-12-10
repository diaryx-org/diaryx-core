use std::path::Path;
use std::process::Command;

use diaryx_core::config::Config;
use diaryx_core::error::{DiaryxError, Result};

/// Launch an editor to open a file
pub fn launch_editor(path: &Path, config: &Config) -> Result<()> {
    let editor = determine_editor(config)?;

    let status =
        Command::new(&editor)
            .arg(path)
            .status()
            .map_err(|e| DiaryxError::EditorLaunchFailed {
                editor: editor.clone(),
                source: e,
            })?;

    if !status.success() {
        return Err(DiaryxError::EditorExited(status.code().unwrap_or(-1)));
    }

    Ok(())
}

/// Determine which editor to use
fn determine_editor(config: &Config) -> Result<String> {
    // 1. Check config file
    if let Some(ref editor) = config.editor {
        return Ok(editor.clone());
    }

    // 2. Check $EDITOR environment variable
    if let Ok(editor) = std::env::var("EDITOR") {
        return Ok(editor);
    }

    // 3. Check $VISUAL environment variable
    if let Ok(visual) = std::env::var("VISUAL") {
        return Ok(visual);
    }

    // 4. Platform-specific defaults
    #[cfg(target_os = "windows")]
    {
        return Ok("notepad.exe".to_string());
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Try common editors
        for editor in &["vim", "vi", "nano", "emacs"] {
            if which(editor) {
                return Ok(editor.to_string());
            }
        }
    }

    Err(DiaryxError::NoEditorFound)
}

/// Check if a command exists in PATH
#[cfg(not(target_os = "windows"))]
fn which(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}
