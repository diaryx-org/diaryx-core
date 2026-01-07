# Diaryx Sync Server

A Hocuspocus-based Y.js collaboration server for Diaryx cross-device sync.

## Development

```bash
# Install dependencies
npm install

# Start development server
npm run dev
```

Server runs on `ws://localhost:1234` by default.

## Deployment

Can be deployed to Railway, Render, or Fly.io.

### Environment Variables

- `PORT` - Server port (default: 1234)
- `DATABASE_PATH` - Path to SQLite database file (default: sync.db)

### Railway

```bash
railway up
```

### Render

1. Create a new Web Service
2. Set build command: `npm install && npm run build`
3. Set start command: `npm start`

### Fly.io

```bash
fly launch
fly deploy
```
