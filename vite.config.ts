import { defineConfig } from "vite";

// Vite config for the ssharden Tauri frontend.
// The dev server must run on a fixed port that matches `tauri.conf.json`'s devUrl.
export default defineConfig({
  // Prevent Vite from obscuring Rust errors.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  // Produce assets into ../dist consumed by Tauri's frontendDist.
  build: {
    target: "es2021",
    outDir: "dist",
    emptyOutDir: true,
  },
});
