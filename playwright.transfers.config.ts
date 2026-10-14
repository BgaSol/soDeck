import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests", testMatch: "transfer-ui.spec.ts", workers: 1,
  timeout: 30000, outputDir: ".qa/playwright-transfers",
  projects: [ { name: "chromium", use: { browserName: "chromium" } }, { name: "webkit", use: { browserName: "webkit" } } ],
  use: { baseURL: "http://127.0.0.1:1434", viewport: { width: 1440, height: 950 } },
  webServer: { command: "npm run dev -- --port 1434 --strictPort", url: "http://127.0.0.1:1434", reuseExistingServer: false },
});
