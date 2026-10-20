import { expect, type Page } from "@playwright/test";
export async function selectTheme(
  page: Page,
  theme: "system" | "light" | "dark",
) {
  await page.getByRole("button", { name: "外观主题", exact: true }).click();
  await page
    .getByRole("menuitemradio", {
      name: { system: "跟随系统", light: "浅色主题", dark: "深色主题" }[theme],
      exact: true,
    })
    .click();
}
export async function expectThemePreference(
  page: Page,
  theme: "system" | "light" | "dark",
) {
  await page.getByRole("button", { name: "外观主题", exact: true }).click();
  await expect(
    page.getByRole("menuitemradio", {
      name: { system: "跟随系统", light: "浅色主题", dark: "深色主题" }[theme],
      exact: true,
    }),
  ).toHaveAttribute("aria-checked", "true");
  await page.keyboard.press("Escape");
}
