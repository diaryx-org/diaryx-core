use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();

    // Process README files with frontmatter stripping
    let readmes = [
        ("README.md", "README.md"),
        ("src/crdt/README.md", "crdt_README.md"),
        ("src/cloud/README.md", "cloud_README.md"),
    ];

    for (src, dest) in readmes {
        if let Ok(content) = fs::read_to_string(src) {
            println!("cargo:rerun-if-changed={}", src);
            let body = strip_frontmatter(&content);
            fs::write(Path::new(&out_dir).join(dest), body)
                .unwrap_or_else(|_| panic!("Failed to write {}", dest));
        }
    }
}

/// Strip YAML frontmatter (content between --- delimiters)
fn strip_frontmatter(content: &str) -> &str {
    if let Some(stripped) = content.strip_prefix("---")
        && let Some(end) = stripped.find("\n---")
    {
        return stripped[end + 4..].trim_start();
    }
    content
}
