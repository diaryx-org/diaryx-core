#[cfg(feature = "cli")]
mod cli;

#[cfg(feature = "cli")]
fn main() {
    cli::run_cli();
}

#[cfg(not(feature = "cli"))]
fn main() {
    eprintln!("This binary requires the 'cli' feature to be enabled.");
    eprintln!("Build with: cargo build --features cli");
    std::process::exit(1);
}
