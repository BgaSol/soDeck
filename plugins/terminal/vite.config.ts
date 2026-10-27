import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
export default defineConfig({
  base: "./", plugins: [react()],
  define: { __TERMINAL_VERSION__: JSON.stringify(process.env.TERMINAL_PLUGIN_VERSION ?? "0.5.0") },
  build: { outDir: "dist/ui", emptyOutDir: true, target: "es2022" },
});
