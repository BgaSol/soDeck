import { expect, type Page } from "@playwright/test";

export async function showServers(page: Page) {
  const picker = page.getByRole("dialog", { name: "服务器", exact: true });
  if (!(await picker.isVisible())) {
    await page.getByRole("button", { name: "新建连接", exact: true }).click();
  }
  await expect(picker).toBeVisible();
  return picker;
}

export async function connectSelectedServer(page: Page) {
  const picker = await showServers(page);
  await picker.getByRole("button", { name: "连接", exact: true }).click();
}
