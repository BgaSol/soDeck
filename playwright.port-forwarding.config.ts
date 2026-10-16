import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  testMatch: "port-forwarding-ui.spec.ts",
  workers: 1,
  timeout: 30_000,
  outputDir: ".qa/playwright-port-forwarding",
  projects: [
    { name: "chromium", use: { browserName: "chromium" } },
    { name: "webkit", use: { browserName: "webkit" } },
  ],
  use: {
    baseURL: "http://127.0.0.1:1454",
    viewport: { width: 1100, height: 760 },
  },
  webServer: {
    command: "npm run dev -- --port 1454 --strictPort",
    url: "http://127.0.0.1:1454",
    reuseExistingServer: false,
  },
});
