import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  testMatch: "workspace-tools-ui.spec.ts",
  workers: 1,
  timeout: 30_000,
  outputDir: ".qa/playwright-workspace-tools",
  projects: [
    { name: "chromium", use: { browserName: "chromium" } },
    { name: "webkit", use: { browserName: "webkit" } },
  ],
  use: {
    baseURL: "http://127.0.0.1:1468",
    viewport: { width: 1440, height: 950 },
  },
  webServer: {
    command: "npm run dev -- --port 1468 --strictPort",
    url: "http://127.0.0.1:1468",
    reuseExistingServer: false,
  },
});
