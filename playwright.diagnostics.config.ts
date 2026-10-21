import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  testMatch: "diagnostics-ui.spec.ts",
  workers: 1,
  timeout: 30000,
  outputDir: ".qa/playwright-diagnostics",
  use: {
    baseURL: "http://127.0.0.1:1471",
    viewport: { width: 1100, height: 900 },
  },
  globalSetup: "./tests/diagnostics-ui-server.mjs",
});
