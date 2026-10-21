import { test, expect, type Page } from "@playwright/test";
async function setup(page: Page) {
  await page.addInitScript(() => {
    const w = window as any;
    const fixture = (w.__DIAGNOSTICS_TEST__ = {
      calls: [] as { command: string; args: any }[],
      mode: "ok",
      complete: null as null | ((value: unknown) => void),
      status: {
        directory: "C:\\LocalData\\SoDeck\\diagnostics",
        runId: "test-run",
        startedAt: 1792500000000,
        bytes: 1048576,
        runs: 3,
        dropped: 0,
        incomplete: false,
        fallback: false,
        unavailable: false,
        error: null,
        exportPhase: "idle",
        exportCompleted: 0,
        exportTotal: 0,
      },
    });
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      async invoke(command: string, args: any = {}) {
        fixture.calls.push({ command, args });
        if (command === "get_diagnostics_status") return { ...fixture.status };
        if (command === "export_diagnostics") {
          if (fixture.mode === "failure")
            throw { kind: "diagnostics", message: "目标目录无法写入" };
          if (fixture.mode === "pending")
            return new Promise((resolve) => {
              fixture.complete = resolve;
            });
          return { path: "C:\\Reports\\SoDeck-diagnostics.zip", warnings: 1 };
        }
        if (command === "cancel_diagnostics_export") fixture.complete?.(null);
        return null;
      },
    };
  });
  await page.goto("/tests/diagnostics.html");
  return page.getByRole("region", { name: "本地诊断" });
}
test("local status, directory and path are available without developer mode", async ({
  page,
}) => {
  const section = await setup(page);
  await expect(section).toContainText("正常");
  await expect(section).toContainText("3 次运行");
  await section.screenshot({ path: ".qa/diagnostics-settings-preview.png" });
  await section.getByRole("button", { name: "复制路径" }).click();
  await expect(section).toContainText("日志路径已复制");
  await section.getByRole("button", { name: "打开日志目录" }).click();
  const calls = await page.evaluate(
    () => (window as any).__DIAGNOSTICS_TEST__.calls,
  );
  expect(calls).toContainEqual({
    command: "plugin:clipboard-manager|write_text",
    args: { text: "C:\\LocalData\\SoDeck\\diagnostics" },
  });
  expect(
    calls.some((c: any) => c.command === "open_diagnostics_directory"),
  ).toBe(true);
});
test("export range, manifest notice and reveal result", async ({ page }) => {
  const section = await setup(page);
  await section.getByRole("combobox").selectOption("current");
  await section.getByRole("button", { name: "打包日志…" }).click();
  await expect(section).toContainText("manifest.json");
  await section.getByRole("button", { name: "在文件夹中显示日志包" }).click();
  const calls = await page.evaluate(
    () => (window as any).__DIAGNOSTICS_TEST__.calls,
  );
  expect(calls).toContainEqual({
    command: "export_diagnostics",
    args: { scope: "current" },
  });
  expect(
    calls.some((c: any) => c.command === "reveal_diagnostics_export"),
  ).toBe(true);
});
test("clear requires explicit confirmation and can be abandoned", async ({
  page,
}) => {
  const section = await setup(page);
  await section.getByRole("button", { name: "清理日志", exact: true }).click();
  await section.getByRole("button", { name: "保留日志" }).click();
  expect(
    await page.evaluate(() =>
      (window as any).__DIAGNOSTICS_TEST__.calls.some(
        (c: any) => c.command === "clear_diagnostics",
      ),
    ),
  ).toBe(false);
  await section.getByRole("button", { name: "清理日志", exact: true }).click();
  await section.getByRole("button", { name: "确认清理" }).click();
  await expect(section).toContainText("诊断记录已清理");
});
test("export cancellation unlocks controls and does not show a saved file", async ({
  page,
}) => {
  const section = await setup(page);
  await page.evaluate(
    () => ((window as any).__DIAGNOSTICS_TEST__.mode = "pending"),
  );
  await section.getByRole("button", { name: "打包日志…" }).click();
  await expect(
    section.getByRole("button", { name: "清理日志", exact: true }),
  ).toBeDisabled();
  await section.getByRole("button", { name: "取消打包" }).click();
  await expect(section).toContainText("已取消保存");
  await expect(
    section.getByRole("button", { name: "打包日志…" }),
  ).toBeEnabled();
  await expect(
    section.getByRole("button", { name: "在文件夹中显示日志包" }),
  ).toHaveCount(0);
});
test("write failure is visible and permits retry", async ({ page }) => {
  const section = await setup(page);
  await page.evaluate(
    () => ((window as any).__DIAGNOSTICS_TEST__.mode = "failure"),
  );
  await section.getByRole("button", { name: "打包日志…" }).click();
  await expect(section.getByRole("alert")).toHaveText("目标目录无法写入");
  await expect(
    section.getByRole("button", { name: "打包日志…" }),
  ).toBeEnabled();
  await page.evaluate(() => ((window as any).__DIAGNOSTICS_TEST__.mode = "ok"));
  await section.getByRole("button", { name: "打包日志…" }).click();
  await expect(section).toContainText("已保存");
});
