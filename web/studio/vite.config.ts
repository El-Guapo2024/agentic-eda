import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// No @types/node in this project (dependency list is closed); this file
// still runs under Node via Vite, so `process` exists at runtime -- just
// declare the one shape used below instead of pulling in the package.
declare const process: { env: Record<string, string | undefined> };

// `eda board serve` (crates/cli/src/studio.rs) is the backend: a tiny
// single-threaded HTTP server bound to 127.0.0.1, default port 8765
// (crates/cli/src/board.rs). It owns /api/*; this dev server proxies
// those requests to it so the page can be developed with `npm run dev`
// while `eda board serve -C <dir>` runs separately. Override the target
// with EDA_STUDIO_API if that board is served on a different port
// (`eda board serve` picks the next free one when 8765 is taken).
const apiTarget = process.env.EDA_STUDIO_API ?? "http://127.0.0.1:8765";

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/api": {
        target: apiTarget,
        changeOrigin: true,
      },
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
