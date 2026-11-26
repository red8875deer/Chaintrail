import { defineConfig } from "vite";
import react from "@vitejs/plugin-react-swc";

// The ChainTrail service (`chaintrail serve`) listens on 127.0.0.1:7979 by default.
const API_TARGET = process.env.CHAINTRAIL_API ?? "http://127.0.0.1:7979";

export default defineConfig({
  plugins: [react()],
  // Relative paths so the same build works from `chaintrail serve --ui dist` and Tauri.
  base: "./",
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      // Traces can take a minute on first run (uncached data).
      "/api": { target: API_TARGET, changeOrigin: false, timeout: 300_000, proxyTimeout: 300_000 },
    },
  },
  build: {
    target: "es2022",
    chunkSizeWarningLimit: 1200,
  },
});
