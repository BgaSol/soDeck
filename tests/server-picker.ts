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
  await home
    .locator('.server-row.selected [data-server-action="new-session"]')
    .click();
}

export async function serverAction(page: Page, action: string) {
  const home = await showServers(page);
  const label = action === "删除服务器" ? "删除配置" : action;
  await home
    .locator(".server-row.selected")
    .getByRole("button", { name: label, exact: true })
    .click();
}
