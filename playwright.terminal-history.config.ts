import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  testMatch: "terminal-history-ui.spec.ts",
  workers: 1,
  timeout: 30000,
  outputDir: ".qa/playwright-terminal-history",
  projects: [
    { name: "chromium", use: { browserName: "chromium" } },
    { name: "webkit", use: { browserName: "webkit" } },
  ],
  use: {
    browserName: "chromium",
    baseURL: "http://127.0.0.1:1438",
    viewport: { width: 1100, height: 760 },
    timezoneId: "Asia/Shanghai",
  },
  webServer: {
    command: "npm run dev -- --port 1438 --strictPort",
    url: "http://127.0.0.1:1438",
    reuseExistingServer: false,
  },
});
