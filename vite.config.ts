import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/.qa/**", "**/test-results/**"] },
  },
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    rollupOptions: {
      output: {
        manualChunks: { terminal: ["@xterm/xterm", "@xterm/addon-fit"] },
      },
    },
  },
});
