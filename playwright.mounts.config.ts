import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  testMatch: "mounts-ui.spec.ts",
  workers: 1,
  timeout: 30_000,
  outputDir: ".qa/playwright-mounts",
  projects: [
    { name: "chromium", use: { browserName: "chromium" } },
    { name: "webkit", use: { browserName: "webkit" } },
  ],
  use: {
    baseURL: "http://127.0.0.1:1443",
    viewport: { width: 1440, height: 950 },
  },
  webServer: {
    command: "npm run dev -- --port 1443 --strictPort",
    url: "http://127.0.0.1:1443",
    reuseExistingServer: false,
  },
});
