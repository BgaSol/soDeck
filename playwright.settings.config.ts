import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  testMatch: ["system-settings-ui.spec.ts"],
  workers: 1,
  timeout: 30_000,
  outputDir: ".qa/playwright-settings",
  globalSetup: "./tests/system-settings-setup.ts",
  projects: [
    { name: "chromium", use: { browserName: "chromium" } },
    { name: "webkit", use: { browserName: "webkit" } },
  ],
  use: {
    baseURL: "http://127.0.0.1:1453",
    viewport: { width: 1280, height: 800 },
  },
  webServer: {
    command: "npm run dev -- --port 1453 --strictPort",
    url: "http://127.0.0.1:1453",
    reuseExistingServer: false,
  },
});
