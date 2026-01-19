use crate::config::Config;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::header::ContentType,
    transport::smtp::{PoolConfig, authentication::Credentials, client::Tls},
};
use std::sync::Arc;
use tracing::{error, info};

/// Email service for sending magic links
pub struct EmailService {
    config: Arc<Config>,
    transport: Option<AsyncSmtpTransport<Tokio1Executor>>,
}

/// Error types for email operations
#[derive(Debug)]
pub enum EmailError {
    /// Email service not configured
    NotConfigured,
    /// Failed to build email
    BuildError(String),
    /// Failed to send email
    SendError(String),
}

impl std::fmt::Display for EmailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmailError::NotConfigured => write!(f, "Email service not configured"),
            EmailError::BuildError(e) => write!(f, "Failed to build email: {}", e),
            EmailError::SendError(e) => write!(f, "Failed to send email: {}", e),
        }
    }
}

impl std::error::Error for EmailError {}

impl EmailService {
    /// Create a new EmailService
    pub fn new(config: Arc<Config>) -> Self {
        let transport = if config.is_email_configured() {
            match Self::create_transport(&config) {
                Ok(t) => {
                    info!(
                        "Email service configured with SMTP host: {}",
                        config.smtp.host
                    );
                    Some(t)
                }
                Err(e) => {
                    error!("Failed to configure email transport: {}", e);
                    None
                }
            }
        } else {
            info!("Email service not configured (SMTP credentials missing)");
            None
        };

        Self { config, transport }
    }

    fn create_transport(
        config: &Config,
    ) -> Result<AsyncSmtpTransport<Tokio1Executor>, lettre::transport::smtp::Error> {
        let creds = Credentials::new(config.smtp.username.clone(), config.smtp.password.clone());

        // Use STARTTLS for port 587, implicit TLS for port 465
        let builder = if config.smtp.port == 465 {
            AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp.host)?
                .port(config.smtp.port)
                .tls(Tls::Wrapper(
                    lettre::transport::smtp::client::TlsParameters::new(config.smtp.host.clone())?,
                ))
        } else {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.smtp.host)?
                .port(config.smtp.port)
        };

        Ok(builder
            .credentials(creds)
            .pool_config(PoolConfig::new().max_size(5))
            .build())
    }

    /// Check if email service is configured
    pub fn is_configured(&self) -> bool {
        self.transport.is_some()
    }

    /// Send a magic link email
    pub async fn send_magic_link(
        &self,
        to_email: &str,
        magic_link_url: &str,
    ) -> Result<(), EmailError> {
        let transport = self.transport.as_ref().ok_or(EmailError::NotConfigured)?;

        let from = format!(
            "{} <{}>",
            self.config.smtp.from_name, self.config.smtp.from_email
        );

        let subject = "Sign in to Diaryx";
        let body = self.build_magic_link_email_body(magic_link_url);

        let email = Message::builder()
            .from(
                from.parse()
                    .map_err(|e| EmailError::BuildError(format!("{}", e)))?,
            )
            .to(to_email
                .parse()
                .map_err(|e| EmailError::BuildError(format!("{}", e)))?)
            .subject(subject)
            .header(ContentType::TEXT_HTML)
            .body(body)
            .map_err(|e| EmailError::BuildError(e.to_string()))?;

        transport
            .send(email)
            .await
            .map_err(|e| EmailError::SendError(e.to_string()))?;

        info!("Magic link email sent to {}", to_email);
        Ok(())
    }

    fn build_magic_link_email_body(&self, magic_link_url: &str) -> String {
        format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Sign in to Diaryx</title>
</head>
<body style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; line-height: 1.6; color: #333; max-width: 600px; margin: 0 auto; padding: 20px;">
    <div style="text-align: center; margin-bottom: 30px;">
        <h1 style="color: #1a1a1a; margin-bottom: 10px;">Diaryx</h1>
    </div>

    <div style="background-color: #f9f9f9; border-radius: 8px; padding: 30px; margin-bottom: 20px;">
        <h2 style="margin-top: 0; color: #1a1a1a;">Sign in to your account</h2>
        <p>Click the button below to sign in to Diaryx. This link will expire in {} minutes.</p>

        <div style="text-align: center; margin: 30px 0;">
            <a href="{}" style="display: inline-block; background-color: #0066cc; color: white; text-decoration: none; padding: 14px 28px; border-radius: 6px; font-weight: 500;">
                Sign in to Diaryx
            </a>
        </div>

        <p style="color: #666; font-size: 14px;">
            If the button doesn't work, copy and paste this link into your browser:
        </p>
        <p style="word-break: break-all; color: #0066cc; font-size: 14px;">
            <a href="{}" style="color: #0066cc;">{}</a>
        </p>
    </div>

    <div style="text-align: center; color: #999; font-size: 12px;">
        <p>If you didn't request this email, you can safely ignore it.</p>
        <p>&copy; Diaryx</p>
    </div>
</body>
</html>"#,
            self.config.magic_link_expiry_minutes, magic_link_url, magic_link_url, magic_link_url
        )
    }
}
