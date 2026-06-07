import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri serves the built frontend; in dev it points at this server.
export default defineConfig({
  plugins: [react()],
  // Tauri expects a fixed dev port and no clearScreen so its logs show.
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  // Produce relative asset paths so the webview loads them from the bundle.
  base: "./",
  build: {
    target: "es2021",
    outDir: "dist",
    emptyOutDir: true,
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
  },
});
