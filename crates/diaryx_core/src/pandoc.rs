//! Native pandoc integration for CLI and Tauri.
//!
//! Provides utilities for invoking the system `pandoc` binary to convert
//! markdown files to various output formats (DOCX, EPUB, PDF, LaTeX, etc.).
//! PDF output uses Typst as the PDF engine.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Supported export formats.
pub const SUPPORTED_FORMATS: &[&str] = &[
    "markdown", "html", "docx", "epub", "pdf", "latex", "odt", "rst",
];

/// Check if pandoc is available on PATH.
pub fn is_pandoc_available() -> bool {
    Command::new("pandoc")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// Get pandoc version string (first line of `pandoc --version`).
pub fn pandoc_version() -> Option<String> {
    Command::new("pandoc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.lines().next().map(|l| l.to_string()))
}

/// Convert a file on disk to a target format using pandoc.
pub fn convert_file(
    input_path: &Path,
    output_path: &Path,
    from: &str,
    to: &str,
    standalone: bool,
) -> Result<(), String> {
    let pandoc_to = pandoc_format_name(to);

    let mut cmd = Command::new("pandoc");
    cmd.arg(input_path)
        .arg("-f")
        .arg(from)
        .arg("-t")
        .arg(pandoc_to)
        .arg("-o")
        .arg(output_path);

    if standalone {
        cmd.arg("--standalone");
    }

    if to == "pdf" {
        cmd.arg("--pdf-engine=typst");
    }

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run pandoc: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("pandoc failed: {}", stderr));
    }

    Ok(())
}

/// Convert markdown content (string) to a target format via stdin/stdout.
pub fn convert_content(content: &str, to: &str, standalone: bool) -> Result<Vec<u8>, String> {
    let pandoc_to = pandoc_format_name(to);

    let mut cmd = Command::new("pandoc");
    cmd.arg("-f")
        .arg("markdown")
        .arg("-t")
        .arg(pandoc_to)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if standalone {
        cmd.arg("--standalone");
    }

    if to == "pdf" {
        cmd.arg("--pdf-engine=typst");
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to run pandoc: {}", e))?;

    child
        .stdin
        .take()
        .unwrap()
        .write_all(content.as_bytes())
        .map_err(|e| format!("Failed to write to pandoc stdin: {}", e))?;

    let output = child
        .wait_with_output()
        .map_err(|e| format!("Failed to wait for pandoc: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("pandoc failed: {}", stderr));
    }

    Ok(output.stdout)
}

/// Print installation instructions for pandoc (and Typst for PDF).
pub fn print_install_instructions() {
    eprintln!("pandoc is not installed or not found on PATH.");
    eprintln!();
    eprintln!("To install pandoc:");
    eprintln!("  macOS:   brew install pandoc");
    eprintln!("  Ubuntu:  sudo apt-get install pandoc");
    eprintln!("  Windows: choco install pandoc");
    eprintln!("  Other:   https://pandoc.org/installing.html");
    eprintln!();
    eprintln!("For PDF output, you also need Typst:");
    eprintln!("  macOS:   brew install typst");
    eprintln!("  Other:   https://github.com/typst/typst#installation");
}

/// Check if a format string is a supported export format.
pub fn is_supported_format(format: &str) -> bool {
    SUPPORTED_FORMATS.contains(&format)
}

/// Check if a format requires the pandoc binary.
pub fn requires_pandoc(format: &str) -> bool {
    matches!(format, "docx" | "epub" | "pdf" | "latex" | "odt" | "rst")
}

/// Get the file extension for a given format (without leading dot).
pub fn format_extension(format: &str) -> &str {
    match format {
        "markdown" => "md",
        "html" => "html",
        "docx" => "docx",
        "epub" => "epub",
        "pdf" => "pdf",
        "latex" => "tex",
        "odt" => "odt",
        "rst" => "rst",
        _ => format,
    }
}

/// Map our format names to pandoc's expected format names.
fn pandoc_format_name(format: &str) -> &str {
    match format {
        "pdf" => "typst", // PDF is produced via Typst
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supported_formats() {
        assert!(is_supported_format("markdown"));
        assert!(is_supported_format("docx"));
        assert!(is_supported_format("pdf"));
        assert!(!is_supported_format("unknown"));
    }

    #[test]
    fn test_requires_pandoc() {
        assert!(!requires_pandoc("markdown"));
        assert!(!requires_pandoc("html"));
        assert!(requires_pandoc("docx"));
        assert!(requires_pandoc("pdf"));
    }

    #[test]
    fn test_format_extension() {
        assert_eq!(format_extension("markdown"), "md");
        assert_eq!(format_extension("latex"), "tex");
        assert_eq!(format_extension("pdf"), "pdf");
    }

    #[test]
    fn test_pandoc_format_name() {
        assert_eq!(pandoc_format_name("pdf"), "typst");
        assert_eq!(pandoc_format_name("docx"), "docx");
        assert_eq!(pandoc_format_name("html"), "html");
    }

    #[test]
    fn test_pandoc_availability_does_not_panic() {
        // Just ensure the function doesn't panic, regardless of whether pandoc is installed
        let _ = is_pandoc_available();
        let _ = pandoc_version();
    }
}
