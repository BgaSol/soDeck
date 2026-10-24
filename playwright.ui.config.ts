import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests/ui", fullyParallel: false, workers: 1,
  use: { baseURL: "http://127.0.0.1:1422", viewport: { width: 1280, height: 800 }, trace: "retain-on-failure" },
  outputDir: ".qa/ui/results", reporter: [["list"], ["json", { outputFile: ".qa/ui/results.json" }]],
  webServer: [
    { command: "npm run dev --workspace @sodeck/desktop", url: "http://127.0.0.1:1422", reuseExistingServer: true, timeout: 30_000 },
    { command: "npx vite plugins/terminal --host 127.0.0.1 --port 1432 --strictPort", url: "http://127.0.0.1:1432", reuseExistingServer: true, timeout: 30_000 },
  ],
});
