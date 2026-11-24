import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed port and must not be hidden behind Vite's screen clearing.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, host: "127.0.0.1", watch: { ignored: ["**/src-tauri/**", "**/test-env/**"] } },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  // The lazily loaded graph chunk is mostly Cytoscape (~575 kB); it's read
  // from disk by the desktop app, so a larger limit is fine there.
  build: { target: "es2022", sourcemap: false, chunkSizeWarningLimit: 700 },
});
