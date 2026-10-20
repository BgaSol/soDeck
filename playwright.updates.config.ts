import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  testMatch: "updates-ui.spec.ts",
  workers: 1,
  timeout: 30000,
  outputDir: ".qa/playwright-updates",
  use: {
    baseURL: "http://127.0.0.1:1458",
    viewport: { width: 1280, height: 900 },
  },
  webServer: {
    command: "npm run dev -- --port 1458 --strictPort",
    url: "http://127.0.0.1:1458",
    reuseExistingServer: false,
  },
});
