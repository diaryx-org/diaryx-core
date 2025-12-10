import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [svelte()],
  server: {
    port: 5174,
    strictPort: false,
  },
  build: {
    target: "es2020",
    sourcemap: true,
  },
  resolve: {
    alias: {
      // Stub out Tauri API for web builds - will be tree-shaken when not used
      "@tauri-apps/api/core": "@tauri-apps/api/core",
    },
  },
  optimizeDeps: {
    // Exclude Tauri API from optimization since it's optional
    exclude: ["@tauri-apps/api"],
  },
});
