import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export default defineConfig({
  plugins: [
    react(),
    {
      name: "archive-diagnostic-source-maps",
      enforce: "post",
      generateBundle(_options, bundle) {
        const directory = join(
          process.cwd(),
          ".qa",
          "diagnostic-symbols",
          "frontend",
        );
        for (const [name, artifact] of Object.entries(bundle)) {
          if (artifact.type !== "asset" || !name.endsWith(".map")) continue;
          const destination = join(directory, name);
          mkdirSync(join(destination, ".."), { recursive: true });
          writeFileSync(destination, artifact.source);
          delete bundle[name];
        }
      },
    },
  ],
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/.qa/**", "**/test-results/**"] },
  },
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    sourcemap: "hidden",
    rollupOptions: {
      output: {
        manualChunks: { terminal: ["@xterm/xterm", "@xterm/addon-fit"] },
      },
    },
  },
});
