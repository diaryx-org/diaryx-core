use std::env;
use std::path::PathBuf;

/// Server configuration loaded from environment variables
#[derive(Debug, Clone)]
pub struct Config {
    /// Server host (default: 0.0.0.0)
    pub host: String,
    /// Server port (default: 3030)
    pub port: u16,
    /// Database file path (default: ./diaryx_sync.db)
    pub database_path: PathBuf,
    /// Base URL for magic link verification (e.g., https://app.diaryx.org)
    pub app_base_url: String,
    /// SMTP configuration for sending emails
    pub smtp: SmtpConfig,
    /// Session token expiration in days (default: 30)
    pub session_expiry_days: i64,
    /// Magic link token expiration in minutes (default: 15)
    pub magic_link_expiry_minutes: i64,
    /// CORS allowed origins (comma-separated)
    pub cors_origins: Vec<String>,
}

/// SMTP configuration for email sending
#[derive(Debug, Clone)]
pub struct SmtpConfig {
    /// SMTP host (e.g., smtp.resend.com)
    pub host: String,
    /// SMTP port (default: 465 for TLS)
    pub port: u16,
    /// SMTP username
    pub username: String,
    /// SMTP password or API key
    pub password: String,
    /// From email address
    pub from_email: String,
    /// From name (default: Diaryx)
    pub from_name: String,
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();

        let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = env::var("PORT")
            .unwrap_or_else(|_| "3030".to_string())
            .parse()
            .map_err(|_| ConfigError::InvalidPort)?;

        let database_path = PathBuf::from(
            env::var("DATABASE_PATH").unwrap_or_else(|_| "./diaryx_sync.db".to_string()),
        );

        let app_base_url =
            env::var("APP_BASE_URL").unwrap_or_else(|_| "http://localhost:5174".to_string());

        let smtp = SmtpConfig {
            host: env::var("SMTP_HOST").unwrap_or_else(|_| "smtp.resend.com".to_string()),
            port: env::var("SMTP_PORT")
                .unwrap_or_else(|_| "465".to_string())
                .parse()
                .map_err(|_| ConfigError::InvalidSmtpPort)?,
            username: env::var("SMTP_USERNAME").unwrap_or_default(),
            password: env::var("SMTP_PASSWORD").unwrap_or_default(),
            from_email: env::var("SMTP_FROM_EMAIL")
                .unwrap_or_else(|_| "noreply@diaryx.org".to_string()),
            from_name: env::var("SMTP_FROM_NAME").unwrap_or_else(|_| "Diaryx".to_string()),
        };

        let session_expiry_days = env::var("SESSION_EXPIRY_DAYS")
            .unwrap_or_else(|_| "30".to_string())
            .parse()
            .unwrap_or(30);

        let magic_link_expiry_minutes = env::var("MAGIC_LINK_EXPIRY_MINUTES")
            .unwrap_or_else(|_| "15".to_string())
            .parse()
            .unwrap_or(15);

        let cors_origins = env::var("CORS_ORIGINS")
            .unwrap_or_else(|_| "http://localhost:5174,http://localhost:5175".to_string())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(Config {
            host,
            port,
            database_path,
            app_base_url,
            smtp,
            session_expiry_days,
            magic_link_expiry_minutes,
            cors_origins,
        })
    }

    /// Check if email sending is configured
    pub fn is_email_configured(&self) -> bool {
        !self.smtp.username.is_empty() && !self.smtp.password.is_empty()
    }

    /// Get the server address
    pub fn server_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[derive(Debug)]
pub enum ConfigError {
    InvalidPort,
    InvalidSmtpPort,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::InvalidPort => write!(f, "Invalid PORT environment variable"),
            ConfigError::InvalidSmtpPort => write!(f, "Invalid SMTP_PORT environment variable"),
        }
    }
}

impl std::error::Error for ConfigError {}
