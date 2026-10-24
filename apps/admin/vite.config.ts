import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

/** The Go server (services/server-go) listens here by default. */
const SERVER = process.env.WT_SERVER_URL ?? "http://localhost:7420";

export default defineConfig(({ command }) => ({
  // Production builds are served by the Go server at /admin/ (--admin-dir).
  base: command === "build" ? "/admin/" : "/",
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      "/api": { target: SERVER, changeOrigin: true },
      "/healthz": { target: SERVER, changeOrigin: true },
      "/metrics": { target: SERVER, changeOrigin: true },
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: true,
  },
}));
