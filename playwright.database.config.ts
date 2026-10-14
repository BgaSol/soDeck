import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  testMatch: "database-ui.spec.ts",
  workers: 1,
  timeout: 30000,
  outputDir: ".qa/playwright-database",
  projects: [
    { name: "chromium", use: { browserName: "chromium" } },
    { name: "webkit", use: { browserName: "webkit" } },
  ],
  use: {
    baseURL: "http://127.0.0.1:1433",
    viewport: { width: 1360, height: 860 },
  },
  webServer: {
    command: "npm run dev -- --port 1433 --strictPort",
    url: "http://127.0.0.1:1433",
    reuseExistingServer: false,
  },
});
