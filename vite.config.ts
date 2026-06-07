import { defineConfig } from "vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;
// @ts-expect-error process is a nodejs global
const debug = !!process.env.TAURI_ENV_DEBUG;

// https://vite.dev/config/ — tuned for Tauri (see https://tauri.app/start/frontend/vite/)
export default defineConfig({
  // Prevent Vite from clearing Rust compiler errors from the terminal.
  clearScreen: false,
  // Only VITE_*/TAURI_ENV_* env vars are exposed to the client (no secret leakage).
  envPrefix: ["VITE_", "TAURI_ENV_"],
  server: {
    // Tauri expects a fixed port; fail rather than silently pick another.
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    // src-tauri is watched by cargo, not Vite — avoid duplicate reloads.
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    // The app only ships on Windows WebView2 (evergreen Chromium), so target a
    // modern baseline — esbuild then skips legacy transpilation/polyfills,
    // producing a smaller, faster bundle.
    target: "chrome105",
    // Minify release bundles with Vite's default minifier (Oxc on this build);
    // keep readable + sourcemapped for debug builds.
    minify: !debug,
    sourcemap: debug,
    // Lit components are small; warn only if a chunk gets genuinely large.
    chunkSizeWarningLimit: 700,
  },
});
