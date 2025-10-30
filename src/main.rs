use diaryx_core::model::Document;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Test with naive date
    println!("=== Testing naive date (test.md) ===");
    let markdown_content = fs::read_to_string("test.md")?;
    let doc = Document::parse(&markdown_content)?;

    // Print the parsed content
    println!("Document parsed successfully!\n");

    if let Some(frontmatter) = doc.frontmatter {
        println!("Frontmatter:");
        println!("  Title: {}", frontmatter.title);
        println!("  Author: {:?}", frontmatter.author);
        println!("  Audience: {:?}", frontmatter.audience);
        println!("  created: {:?}", frontmatter.created);
        println!("  Extra fields: {:?}", frontmatter.extra);
        println!();
    }

    println!("Content:");
    println!("{}", doc.content);

    Ok(())
}
