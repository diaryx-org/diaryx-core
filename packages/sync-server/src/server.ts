/**
 * Diaryx Sync Server
 * 
 * A Hocuspocus-based Y.js collaboration server with SQLite persistence.
 * Can be deployed to Railway, Render, or Fly.io.
 */

import { Server } from "@hocuspocus/server";
import { SQLite } from "@hocuspocus/extension-sqlite";
import { config } from "dotenv";

// Load environment variables
config();

const port = parseInt(process.env.PORT || "1234");
const databasePath = process.env.DATABASE_PATH || "sync.db";

// Create Hocuspocus server
const server = new Server({
  port,

  async onConnect({ documentName }) {
    console.log(`[Hocuspocus] Client connected to: ${documentName}`);
  },

  async onDisconnect({ documentName }) {
    console.log(`[Hocuspocus] Client disconnected from: ${documentName}`);
  },

  extensions: [
    new SQLite({
      database: databasePath,
    }),
  ],
});

// Start the server
console.log(`[Hocuspocus] Starting server on port ${port}...`);
server.listen();

// Graceful shutdown
process.on("SIGTERM", () => {
  console.log("[Hocuspocus] Received SIGTERM, shutting down...");
  server.destroy();
  process.exit(0);
});

process.on("SIGINT", () => {
  console.log("[Hocuspocus] Received SIGINT, shutting down...");
  server.destroy();
  process.exit(0);
});
