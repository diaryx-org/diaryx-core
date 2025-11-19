use diaryx_core::fs::RealFileSystem;
use diaryx_core::app::DiaryxApp;

pub struct Config {
    pub command: String,
    pub option: String,
}

impl Config {
    pub fn from_args(args: Vec<String>) -> Config {
        let command = args.get(1).cloned().unwrap_or_default();
        let option = args.get(2).cloned().unwrap_or_default();
        
        Config { command, option }
    }
}

pub fn run_cli() {
    let args: Vec<String> = std::env::args().collect();
    
    // Handle Help immediately
    if args.len() < 3 {
        println!("Usage: diaryx <COMMAND> <OPTION>");
        return;
    }

    // 1. Setup Data
    let config = Config::from_args(args);

    // 2. Setup Dependencies
    let fs = RealFileSystem; // We choose Real because this is the CLI
    let app = DiaryxApp::new(fs);

    // 3. Execute
    match config.command.as_str() {
        "create" => {
            println!("Creating entry at: {}", config.option);
            match app.create_entry(&config.option) {
                Ok(_) => println!("Success!"),
                Err(e) => eprintln!("Error creating file: {}", e),
            }
        },
        _ => println!("Unknown command"),
    }
}
