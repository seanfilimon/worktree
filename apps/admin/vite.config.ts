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
    rollupOptions: {
      output: {
        // Long-lived vendor chunks cache across releases of the app code.
        manualChunks(id) {
          if (!id.includes("node_modules")) return undefined;
          if (/[\\/](recharts|d3-[^\\/]+|victory-vendor)[\\/]/.test(id)) return "charts";
          if (/[\\/](radix-ui|@radix-ui|cmdk|sonner|@floating-ui)[\\/]/.test(id)) return "ui";
          if (/[\\/](react|react-dom|react-router|scheduler|@tanstack)[\\/]/.test(id)) return "react";
          return "vendor";
        },
      },
    },
  },
}));
