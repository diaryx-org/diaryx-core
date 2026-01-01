//! Live sync module for Tauri application.
//!
//! Implements WebSocket-based sync provider for real-time collaboration.

mod websocket_provider;

pub use websocket_provider::WebSocketSyncProvider;
