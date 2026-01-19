---
title: Diaryx Sync Server
author: adammharris
audience:
  - public
  - developers
part_of: ../../README.md
---

# Diaryx Sync Server

A Rust-based multi-device sync server for Diaryx with magic link authentication.

## Features

- **Magic link authentication**: Passwordless login via email
- **Real-time sync**: WebSocket-based Y-sync protocol using diaryx_core's CRDT infrastructure
- **Multi-device support**: Track and manage connected devices
- **Persistent storage**: SQLite-based storage for user data and CRDT state

## Quick Start

```bash
# Set required environment variables
export SMTP_HOST=smtp.resend.com
export SMTP_USERNAME=resend
export SMTP_PASSWORD=re_xxxx
export SMTP_FROM_EMAIL=noreply@yourapp.com
export APP_BASE_URL=https://yourapp.com

# Run the server
cargo run -p diaryx_sync_server
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `HOST` | `0.0.0.0` | Server host |
| `PORT` | `3030` | Server port |
| `DATABASE_PATH` | `./diaryx_sync.db` | Path to SQLite database |
| `APP_BASE_URL` | `http://localhost:5173` | Base URL for magic link verification |
| `SMTP_HOST` | `smtp.resend.com` | SMTP server host |
| `SMTP_PORT` | `465` | SMTP server port |
| `SMTP_USERNAME` | - | SMTP username |
| `SMTP_PASSWORD` | - | SMTP password/API key |
| `SMTP_FROM_EMAIL` | `noreply@diaryx.org` | From email address |
| `SMTP_FROM_NAME` | `Diaryx` | From name |
| `SESSION_EXPIRY_DAYS` | `30` | Session token expiration in days |
| `MAGIC_LINK_EXPIRY_MINUTES` | `15` | Magic link expiration in minutes |
| `CORS_ORIGINS` | `http://localhost:5173,http://localhost:1420` | Comma-separated CORS origins |

## API Endpoints

### Authentication

#### Request Magic Link
```
POST /auth/magic-link
Content-Type: application/json

{ "email": "user@example.com" }
```

Response:
```json
{
  "success": true,
  "message": "Check your email for a sign-in link."
}
```

#### Verify Magic Link
```
GET /auth/verify?token=XXX&device_name=My%20Device
```

Response:
```json
{
  "success": true,
  "token": "session_token_here",
  "user": {
    "id": "user_id",
    "email": "user@example.com"
  }
}
```

#### Get Current User
```
GET /auth/me
Authorization: Bearer <session_token>
```

#### Logout
```
POST /auth/logout
Authorization: Bearer <session_token>
```

#### List Devices
```
GET /auth/devices
Authorization: Bearer <session_token>
```

#### Delete Device
```
DELETE /auth/devices/{device_id}
Authorization: Bearer <session_token>
```

### API

#### Server Status
```
GET /api/status
```

Response:
```json
{
  "status": "ok",
  "version": "0.10.0",
  "active_connections": 5,
  "active_rooms": 2
}
```

#### List Workspaces
```
GET /api/workspaces
Authorization: Bearer <session_token>
```

### WebSocket Sync

```
GET /sync?doc=workspace_id&token=session_token
```

The WebSocket connection uses the Y-sync protocol (compatible with y-protocols). Messages are binary Y.js updates.

## Architecture

```
┌─────────────────┐     WebSocket      ┌─────────────────────────┐
│   Web/Tauri     │◄──────────────────►│  diaryx_sync_server     │
│   Client        │     (Y-sync)       │  (Rust + axum)          │
└─────────────────┘                    │                         │
                                       │  ┌─────────────────┐    │
                                       │  │  diaryx_core    │    │
                                       │  │  - WorkspaceCrdt│    │
                                       │  │  - SyncProtocol │    │
                                       │  │  - SqliteStorage│    │
                                       │  └─────────────────┘    │
                                       │           │             │
                                       │           ▼             │
                                       │  ┌─────────────────┐    │
                                       │  │    SQLite DB    │    │
                                       │  └─────────────────┘    │
                                       └─────────────────────────┘
```

## Development

### Running Locally

```bash
# Without email (dev mode - magic link returned in response)
cargo run -p diaryx_sync_server

# With email
SMTP_HOST=smtp.mailtrap.io \
SMTP_USERNAME=xxx \
SMTP_PASSWORD=xxx \
SMTP_FROM_EMAIL=test@example.com \
cargo run -p diaryx_sync_server
```

### Testing

```bash
cargo test -p diaryx_sync_server
```
