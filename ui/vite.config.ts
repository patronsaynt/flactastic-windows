import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri serves the dev build from this fixed port (see app/src-tauri/tauri.conf.json).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    target: ["es2022", "chrome110", "safari16"],
    sourcemap: true,
  },
  test: {
    environment: "jsdom",
  },
});
