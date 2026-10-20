import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  testMatch: ["log-archives-ui.spec.ts", "container-logs-ui.spec.ts"],
  workers: 1,
  timeout: 30000,
  outputDir: ".qa/playwright-logs",
  use: {
    browserName: "chromium",
    baseURL: "http://127.0.0.1:1436",
    viewport: { width: 1040, height: 720 },
    timezoneId: "Asia/Shanghai",
  },
  webServer: {
    command: "npm run dev -- --port 1436 --strictPort",
    url: "http://127.0.0.1:1436",
    reuseExistingServer: false,
  },
});
