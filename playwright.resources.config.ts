import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  testMatch: ["resources-ui.spec.ts", "mounts-ui.spec.ts"],
  workers: 1,
  timeout: 30_000,
  outputDir: ".qa/playwright-resources",
  projects: [
    { name: "chromium", use: { browserName: "chromium" } },
    { name: "webkit", use: { browserName: "webkit" } },
  ],
  use: {
    baseURL: "http://127.0.0.1:1446",
    viewport: { width: 1440, height: 950 },
  },
  webServer: {
    command: "npm run dev -- --port 1446 --strictPort",
    url: "http://127.0.0.1:1446",
    reuseExistingServer: false,
  },
});
