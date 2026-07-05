import { defineConfig } from "vite";
import { resolve } from "node:path";

// Tauri expects a fixed port and no cleared screen so its CLI can detect readiness.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  clearScreen: false,
  server: {
    // 1425/1426 to avoid clashing with other local Tauri apps (default 1420/1421).
    port: 1425,
    strictPort: true,
    host: host || false,
    hmr: host
      ? { protocol: "ws", host, port: 1426 }
      : undefined,
    watch: {
      // src-tauri is watched by the Rust side, not Vite.
      ignored: ["**/src-tauri/**"],
    },
  },
  // Produce a build in ../dist that Tauri serves in production.
  build: {
    target: "es2021",
    minify: "esbuild",
    sourcemap: false,
    rollupOptions: {
      // Two webview entry points: the main window and the tray popover.
      input: {
        main: resolve(__dirname, "index.html"),
        popover: resolve(__dirname, "popover.html"),
      },
    },
  },
});
