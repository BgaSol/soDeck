import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  testMatch: "connection-ui.spec.ts",
  workers: 1,
  timeout: 30000,
  outputDir: ".qa/playwright-connection",
  projects: [
    { name: "chromium", use: { browserName: "chromium" } },
    { name: "webkit", use: { browserName: "webkit" } },
  ],
  use: {
    baseURL: "http://127.0.0.1:1432",
    viewport: { width: 1440, height: 950 },
  },
  webServer: {
    command: "npm run dev -- --port 1432 --strictPort",
    url: "http://127.0.0.1:1432",
    reuseExistingServer: false,
  },
});
