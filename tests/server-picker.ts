import { expect, type Page } from "@playwright/test";

export async function showServers(page: Page) {
  const home = page.getByRole("region", { name: "服务器", exact: true });
  if (!(await home.isVisible())) {
    await page.getByRole("button", { name: "服务器首页", exact: true }).click();
  }
  await expect(home).toBeVisible();
  return home;
}

export async function connectSelectedServer(page: Page) {
  const home = await showServers(page);
  await home.locator(".server-row.selected").click({ button: "right" });
  await page.getByRole("menuitem", { name: "新建会话", exact: true }).click();
}

export async function serverAction(page: Page, action: string) {
  const home = await showServers(page);
  await home.locator(".server-row.selected").click({ button: "right" });
  await page.getByRole("menuitem", { name: action, exact: true }).click();
}
