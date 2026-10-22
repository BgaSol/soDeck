// IPC is simulated here; db.rs tests exercise real SQLite backups and rebuilds.
import { expect, test, type Page } from "@playwright/test";
import { installWorkspaceFixture } from "./workspace-fixture";

const database =
  "/Users/test/Library/Application Support/com.sshworkspace.desktop/servers.db";
const backup = database.replace("servers.db", "servers.backup-test.db");

async function setup(page: Page) {
  await installWorkspaceFixture(page);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.addInitScript(
    ({ database, backup }) => {
      const w = window as any;
      let serial = 1;
      let rebuilt = false;
      const state = {
        calls: [] as string[],
        failRefresh: false,
        finish: (_error?: string) => {},
      };
      w.__DATABASE_TEST__ = state;
      // Reproduce a webview where browser confirmation dialogs cannot be used.
      w.confirm = () => {
        throw new Error("window.confirm is unavailable");
      };
      w.isTauri = true;
      w.__TAURI_INTERNALS__ = {
        metadata: {
          currentWindow: { label: "main" },
          currentWebview: { label: "main" },
        },
        transformCallback: () => serial++,
        unregisterCallback: () => {},
        invoke: async (command: string) => {
          state.calls.push(command);
          if (command === "list_servers") {
            if (!rebuilt)
              throw {
                kind: "databaseSchema",
                message: `缺少 servers.password。\n实际数据库：${database}`,
              };
            if (state.failRefresh)
              throw { kind: "database", message: "读取列表失败" };
            return [];
          }
          if (command === "reset_incompatible_database") {
            await new Promise<void>((resolve, reject) => {
              state.finish = (error?: string) =>
                error
                  ? reject({ kind: "database", message: error })
                  : resolve();
            });
            rebuilt = true;
            return backup;
          }
          if (command === "list_transfers") return [];
          if (command === "plugin:event|listen") return serial++;
          return null;
        },
      };
      w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    },
    { database, backup },
  );
  await page.goto("/");
  await page
    .getByRole("button", { name: "备份旧库并重建", exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: "备份旧库并重建？" });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText(database);
  return { dialog, errors };
}

const resetCalls = (page: Page) =>
  page.evaluate(
    () =>
      (window as any).__DATABASE_TEST__.calls.filter(
        (call: string) => call === "reset_incompatible_database",
      ).length,
  );

test("cancel and Escape preserve the database without calling reset", async ({
  page,
}) => {
  const { dialog, errors } = await setup(page);
  await expect(dialog.getByRole("button", { name: "取消" })).toBeFocused();
  await dialog.getByRole("button", { name: "取消" }).click();
  await expect(dialog).not.toBeVisible();
  await page
    .getByRole("button", { name: "备份旧库并重建", exact: true })
    .click();
  await page.keyboard.press("Escape");
  await expect(dialog).not.toBeVisible();
  expect(await resetCalls(page)).toBe(0);
  expect(errors).toEqual([]);
});

test("confirm runs once, blocks dismissal while busy and shows the backup path", async ({
  page,
}) => {
  const { dialog, errors } = await setup(page);
  await dialog.getByRole("button", { name: "确认备份并重建" }).click();
  await expect.poll(() => resetCalls(page)).toBe(1);
  await expect(
    dialog.getByRole("button", { name: "正在备份并重建…" }),
  ).toBeDisabled();
  await expect(dialog.getByRole("button", { name: "取消" })).toBeDisabled();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeVisible();
  await page.evaluate(() => (window as any).__DATABASE_TEST__.finish());
  await expect(dialog).not.toBeVisible();
  await expect(
    page.getByRole("status").filter({ hasText: "数据库已重新创建" }),
  ).toContainText(backup);
  await expect(
    page.getByRole("button", { name: "备份旧库并重建", exact: true }),
  ).toHaveCount(0);
  expect(await resetCalls(page)).toBe(1);
  expect(errors).toEqual([]);
});

test("backup failure stays visible and allows retry", async ({ page }) => {
  const { dialog, errors } = await setup(page);
  await dialog.getByRole("button", { name: "确认备份并重建" }).click();
  await expect.poll(() => resetCalls(page)).toBe(1);
  await page.evaluate(() =>
    (window as any).__DATABASE_TEST__.finish("备份失败：磁盘空间不足"),
  );
  await expect(dialog.getByRole("alert")).toHaveText("备份失败：磁盘空间不足");
  await expect(dialog.getByRole("button", { name: "取消" })).toBeEnabled();
  await dialog.getByRole("button", { name: "确认备份并重建" }).click();
  await expect.poll(() => resetCalls(page)).toBe(2);
  await page.evaluate(() => (window as any).__DATABASE_TEST__.finish());
  await expect(dialog).not.toBeVisible();
  await expect(
    page.getByRole("status").filter({ hasText: "数据库已重新创建" }),
  ).toContainText(backup);
  expect(errors).toEqual([]);
});

test("successful backup remains visible if refreshing the server list fails", async ({
  page,
}) => {
  const { dialog, errors } = await setup(page);
  await page.evaluate(() => {
    (window as any).__DATABASE_TEST__.failRefresh = true;
  });
  await dialog.getByRole("button", { name: "确认备份并重建" }).click();
  await expect.poll(() => resetCalls(page)).toBe(1);
  await page.evaluate(() => (window as any).__DATABASE_TEST__.finish());
  await expect(dialog).not.toBeVisible();
  await expect(
    page.getByRole("status").filter({ hasText: "数据库已重新创建" }),
  ).toContainText(backup);
  await expect(page.getByRole("alert")).toContainText("读取列表失败");
  expect(await resetCalls(page)).toBe(1);
  expect(errors).toEqual([]);
});
