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
    // Windows reserves shifting port bands (Hyper-V/WSL) around 1030-1629, which
    // swallow 1420/1500 with EACCES; 3000 sits outside them.
    port: 3000,
    strictPort: true,
    host: host || false,
    hmr: host
      ? { protocol: "ws", host, port: 3001 }
      : undefined,
    watch: {
      // Don't watch the Rust side; Tauri handles that.
      ignored: ["**/src-tauri/**"],
    },
  },
}));
