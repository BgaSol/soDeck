import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  testMatch: "terminal-clipboard-ui.spec.ts",
  workers: 1,
  timeout: 30000,
  outputDir: ".qa/playwright-terminal-clipboard",
  projects: [
    { name: "chromium", use: { browserName: "chromium" } },
    { name: "webkit", use: { browserName: "webkit" } },
  ],
  use: {
    baseURL: "http://127.0.0.1:1456",
    viewport: { width: 1100, height: 760 },
  },
  webServer: {
    command: "npm run dev -- --port 1456 --strictPort",
    url: "http://127.0.0.1:1456",
    reuseExistingServer: false,
  },
});
