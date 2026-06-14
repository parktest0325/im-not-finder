import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [svelte()],
  // Vite options tailored for Tauri development, applied during `tauri dev`/`tauri build`.
  clearScreen: false,
  server: {
    // 1420 (Tauri default) sits inside common Windows reserved port ranges
    // (Hyper-V/WSL), causing EACCES; 1500 is outside them.
    port: 1500,
    strictPort: true,
    host: host || false,
    hmr: host
      ? { protocol: "ws", host, port: 1501 }
      : undefined,
    watch: {
      // Don't watch the Rust side; Tauri handles that.
      ignored: ["**/src-tauri/**"],
    },
  },
}));
