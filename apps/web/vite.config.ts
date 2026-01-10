import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import path from "path";

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [tailwindcss(), svelte() as any],
  // Base path for GitHub Pages deployment
  // Set VITE_BASE_PATH env var to deploy to a subdirectory (e.g., "/repo-name/")
  base: process.env.VITE_BASE_PATH || "/",
  server: { port: 5174, strictPort: false },
  build: { target: "es2020", sourcemap: true },
  resolve: {
    alias: {
      // Stub out Tauri API for web builds - will be tree-shaken when not used
      "@tauri-apps/api/core": "@tauri-apps/api/core",
      $lib: path.resolve("./src/lib"),
      "@": path.resolve(__dirname, "./src"),
    },
  },
  optimizeDeps: {
    // Exclude Tauri API from optimization since it's optional
    exclude: ["@tauri-apps/api"],
  },
  worker: {
    format: 'es',
  },
});
