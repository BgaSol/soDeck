import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  testMatch: ["desktop.spec.ts", "features.spec.ts"],
  workers: 1,
  timeout: 120_000,
  expect: { timeout: 15_000 },
  outputDir: ".qa/playwright",
});
