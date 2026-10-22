import { expect, type Page, type Locator } from "@playwright/test";
export type WorkspaceTool = "files" | "docker" | "monitor" | "netbird";
export async function openWorkspaceTool(
  scope: Page | Locator,
  tool: WorkspaceTool,
) {
  const button = scope
    .locator(`[data-tool="${tool}"]`)
    .filter({ visible: true });
  if ((await button.getAttribute("aria-expanded")) !== "true")
    await button.click();
  await expect(button).toHaveAttribute("aria-expanded", "true");
}
