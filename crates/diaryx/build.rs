use std::env;
use std::fs;
use std::path::Path;

use diaryx_core::frontmatter::extract_body;

fn main() {
    println!("cargo:rerun-if-changed=README.md");

    let readme = fs::read_to_string("README.md").expect("Failed to read README.md");

    let body = extract_body(&readme);

    let out_dir = env::var("OUT_DIR").unwrap();
    fs::write(Path::new(&out_dir).join("README.md"), body).expect("Failed to write README.md");
}
