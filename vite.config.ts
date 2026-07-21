import { svelte } from "@sveltejs/vite-plugin-svelte";
import { svelteTesting } from "@testing-library/svelte/vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

const host = process.env.TAURI_DEV_HOST;

// @sveltejs/vite-plugin-svelte + Tailwind v4, tuned for Tauri.
// https://v2.tauri.app/start/frontend/
export default defineConfig({
  plugins: [svelte(), tailwindcss(), svelteTesting()],

  // Prevent Vite from obscuring Rust errors, and pin the dev server to the
  // fixed port Tauri points `devUrl` at.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // Tauri owns the Rust side; don't let Vite watch it.
      ignored: ["**/src-tauri/**"],
    },
  },

  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test-setup.ts"],
  },
});
