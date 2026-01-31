#![doc = include_str!(concat!(env!("OUT_DIR"), "/README.md"))]

//! Diaryx Sync Server
//!
//! A multi-device sync server for Diaryx with magic link authentication.
//!
//! ## Features
//!
//! - **Magic link authentication**: Passwordless login via email
//! - **Real-time sync**: WebSocket-based Y-sync protocol using diaryx_core's CRDT
//! - **Multi-device support**: Track and manage connected devices
//! - **Persistent storage**: SQLite-based storage for user data and CRDT state
//!
//! ## Environment Variables
//!
//! - `HOST`: Server host (default: 0.0.0.0)
//! - `PORT`: Server port (default: 3030)
//! - `DATABASE_PATH`: Path to SQLite database (default: ./diaryx_sync.db)
//! - `APP_BASE_URL`: Base URL for magic link verification
//! - `SMTP_HOST`: SMTP server host
//! - `SMTP_PORT`: SMTP server port
//! - `SMTP_USERNAME`: SMTP username
//! - `SMTP_PASSWORD`: SMTP password/API key
//! - `SMTP_FROM_EMAIL`: From email address
//! - `SMTP_FROM_NAME`: From name
//! - `SESSION_EXPIRY_DAYS`: Session token expiration (default: 30)
//! - `MAGIC_LINK_EXPIRY_MINUTES`: Magic link expiration (default: 15)
//! - `CORS_ORIGINS`: Comma-separated list of allowed origins

pub mod auth;
pub mod config;
pub mod db;
pub mod email;
pub mod handlers;
pub mod sync;

pub use config::Config;
