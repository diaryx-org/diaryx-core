import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import path from "path";

const isTauri = !!process.env.TAURI_ENV_PLATFORM;
const tauriDevHost = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [tailwindcss(), svelte() as any],
  // Base path for GitHub Pages deployment
  // Set VITE_BASE_PATH env var to deploy to a subdirectory (e.g., "/repo-name/")
  base: process.env.VITE_BASE_PATH || "/",
  // Prevent vite from obscuring rust errors
  clearScreen: false,
  server: {
    port: 5174,
    strictPort: isTauri, // Tauri expects a fixed port
    host: tauriDevHost || false,
    hmr: tauriDevHost
      ? {
          protocol: "ws",
          host: tauriDevHost,
          port: 1421,
        }
      : undefined,
    watch: {
      // Tell vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    // Tauri uses Chromium on Windows and WebKit on macOS and Linux
    target: isTauri
      ? process.env.TAURI_ENV_PLATFORM === "windows"
        ? "chrome105"
        : "safari13"
      : "es2020",
    // Don't minify for debug builds
    minify: isTauri && process.env.TAURI_ENV_DEBUG ? false : "esbuild",
    // Produce sourcemaps for debug builds
    sourcemap: isTauri ? !!process.env.TAURI_ENV_DEBUG : true,
  },
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
  // Env variables starting with the item of `envPrefix` will be exposed in tauri's source code through `import.meta.env`.
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  worker: {
    format: "es",
  },
});
