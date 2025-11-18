struct Config {
    pub command: String,
    pub option: String,
}

impl Config {
    fn help() {
        println!("Usage:");
        std::process::exit(0);
    }

    pub fn build(args: Vec<String>) -> Config {
        let command = args.get(1)
            .map(|s| s.clone())
            .unwrap_or_else(|| { String::new() });
        let option = args.get(2)
            .map(|s| s.clone())
            .unwrap_or_else(|| { String::new() });
        if command.len() == 0 || option.len() == 0 {
            Config::help();
        }
        Config { command, option }
    }
}

pub struct DiaryxCli {
    config: Config,
}

impl DiaryxCli {
    pub fn from_args() -> DiaryxCli {
        let config = Config::build(std::env::args().collect());
        Self { config: Config { command: config.command, option: config.option } }
    }
    pub fn print_config(&self) {
        println!("Config command: {}", self.config.command);
        println!("Config option: {}", self.config.option);
    }
}
