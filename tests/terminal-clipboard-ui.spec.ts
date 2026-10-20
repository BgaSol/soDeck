import { selectTheme } from "./theme-fixture";
import { test, expect, type Page } from "@playwright/test";
import { setup } from "./terminal-fixture";
const inputs = (page: Page) =>
  page.evaluate(() =>
    (window as any).__HISTORY_TEST__.calls
      .filter((c: any) => c.command === "terminal_input")
      .map((c: any) => c.args.data),
  );
async function ready(page: Page) {
  const errors = await setup(page);
  await page.goto("/?container=1");
  await expect(
    page.getByRole("button", { name: "粘贴到终端", exact: true }),
  ).toBeEnabled();
  return errors;
}
async function output(page: Page, text: string) {
  await page.evaluate(
    (text) =>
      (window as any).__HISTORY_TEST__.emit({ type: "data", data: btoa(text) }),
    text,
  );
}
async function nativePaste(page: Page, text: string) {
  await page
    .locator(".terminal-surface .xterm-helper-textarea")
    .evaluate((node, text) => {
      const data = new DataTransfer();
      data.setData("text/plain", text);
      node.dispatchEvent(
        new ClipboardEvent("paste", {
          bubbles: true,
          cancelable: true,
          clipboardData: data,
        }),
      );
    }, text);
}
test("Windows shortcuts copy without interrupt and paste exactly once", async ({
  page,
}) => {
  const errors = await ready(page);
  const surface = page.locator(".terminal-surface");
  await surface.click({ button: "right", position: { x: 40, y: 20 } });
  await page.getByRole("menuitem", { name: "全选缓冲区" }).click();
  await page.keyboard.press("Control+c");
  await expect
    .poll(() =>
      page.evaluate(() => (window as any).__HISTORY_TEST__.writes.length),
    )
    .toBe(1);
  expect(await inputs(page)).toEqual([]);
  await page.keyboard.press("Escape");
  await page.keyboard.press("Control+c");
  await expect.poll(() => inputs(page)).toEqual(["\x03"]);
  await page.evaluate(() => {
    (window as any).__HISTORY_TEST__.clipboard = "echo single";
  });
  await page.keyboard.press("Control+v");
  await expect.poll(() => inputs(page)).toEqual(["\x03", "echo single"]);
  expect(errors).toEqual([]);
});
test("native multiline paste previews, edits and sends bracketed data once", async ({
  page,
}) => {
  const errors = await ready(page);
  await output(page, "\x1b[?2004h");
  await nativePaste(page, "echo hello\r\n");
  const dialog = page.getByRole("dialog", { name: "粘贴到终端", exact: true });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("ops@10.0.0.1:22");
  expect(await inputs(page)).toEqual([]);
  await dialog.getByRole("button", { name: "移除末尾换行" }).click();
  await dialog.getByRole("button", { name: "粘贴", exact: true }).click();
  await expect
    .poll(() => inputs(page))
    .toEqual(["\x1b[200~echo hello\x1b[201~"]);
  await expect(dialog).not.toBeVisible();
  await nativePaste(page, "a\nb");
  await expect(dialog).toBeVisible();
  await page.screenshot({ path: ".qa/terminal-clipboard-preview.png" });
  await dialog.getByRole("button", { name: "取消", exact: true }).click();
  expect(await inputs(page)).toHaveLength(1);
  expect(errors).toEqual([]);
});
test("late clipboard reads are discarded when switching history and background DSR still works", async ({
  page,
}) => {
  const errors = await ready(page);
  await page.evaluate(() => {
    const s = (window as any).__HISTORY_TEST__;
    s.delayRead = true;
    s.clipboard = "stale";
  });
  await page.getByRole("button", { name: "粘贴到终端", exact: true }).click();
  await page.getByRole("button", { name: "历史", exact: true }).click();
  await expect(page.getByLabel("搜索终端历史")).toBeVisible();
  await output(page, "\x1b[5n");
  await expect.poll(() => inputs(page)).toEqual(["\x1b[0n"]);
  await page.evaluate(() => (window as any).__HISTORY_TEST__.releaseRead());
  await nativePaste(page, "hidden input");
  await page.getByRole("button", { name: "返回实时终端" }).click();
  expect(await inputs(page)).toEqual(["\x1b[0n"]);
  expect(errors).toEqual([]);
});
test("clipboard failures preserve selection and controls require explicit removal", async ({
  page,
}) => {
  await ready(page);
  await page
    .locator(".terminal-surface")
    .click({ button: "right", position: { x: 40, y: 20 } });
  await page.getByRole("menuitem", { name: "全选缓冲区" }).click();
  await page.evaluate(() => {
    (window as any).__HISTORY_TEST__.failWrite = true;
  });
  await page.keyboard.press("Control+c");
  await expect(
    page.getByText("复制失败，请重试", { exact: true }),
  ).toBeVisible();
  expect(await inputs(page)).toEqual([]);
  await expect(
    page.getByRole("button", { name: "复制终端选区" }),
  ).toBeEnabled();
  await nativePaste(page, "x\x1b");
  const dialog = page.getByRole("dialog", { name: "粘贴到终端", exact: true });
  await expect(
    dialog.getByRole("button", { name: "粘贴", exact: true }),
  ).toBeDisabled();
  await dialog.getByRole("button", { name: "移除控制字符" }).click();
  await dialog.getByRole("button", { name: "粘贴", exact: true }).click();
  await expect.poll(() => inputs(page)).toEqual(["x"]);
});
test("settings enable direct right-click paste while Shift keeps the menu", async ({
  page,
}) => {
  const errors = await ready(page);
  const surface = page.locator(".terminal-surface");
  await surface.click({ button: "right", position: { x: 40, y: 20 } });
  await page.getByRole("menuitem", { name: "复制粘贴设置…" }).click();
  const settings = page.getByRole("dialog", { name: "系统设置" });
  await settings
    .getByRole("checkbox", { name: "右键直接粘贴", exact: true })
    .check();
  await expect(settings.getByText("终端设置已保存，立即生效。")).toBeVisible();
  await settings.getByRole("button", { name: "关闭", exact: true }).click();
  await surface.click({ button: "right", position: { x: 40, y: 20 } });
  await expect.poll(() => inputs(page)).toEqual(["echo hello"]);
  await surface.click({
    button: "right",
    position: { x: 40, y: 20 },
    modifiers: ["Shift"],
  });
  await expect(page.getByRole("menu", { name: "终端菜单" })).toBeVisible();
  expect(await inputs(page)).toHaveLength(1);
  expect(errors).toEqual([]);
});
test("composition ending after history activation does not leak input", async ({
  page,
}) => {
  await ready(page);
  await page.locator(".xterm-helper-textarea").evaluate((node) => {
    node.dispatchEvent(
      new CompositionEvent("compositionstart", { bubbles: true, data: "" }),
    );
    (node as HTMLTextAreaElement).value = "待输入";
    node.dispatchEvent(
      new CompositionEvent("compositionupdate", {
        bubbles: true,
        data: "待输入",
      }),
    );
  });
  await page.getByRole("button", { name: "历史", exact: true }).click();
  await page.locator(".xterm-helper-textarea").evaluate((node) => {
    (node as HTMLTextAreaElement).value = "待输入";
    node.dispatchEvent(
      new CompositionEvent("compositionend", { bubbles: true, data: "待输入" }),
    );
  });
  await output(page, "\x1b[5n");
  await expect.poll(() => inputs(page)).toEqual(["\x1b[0n"]);
});

test("readonly replay supports copy while rejecting all paste inputs", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.goto("/?terminal-history=1");
  await expect(page.locator(".terminal-history-row").last()).toBeVisible();
  await page.locator(".history-select-record").last().click();
  await page.getByRole("button", { name: "查看当时屏幕" }).click();
  await expect(page.getByText("已恢复选中记录时的屏幕")).toBeVisible();
  await page
    .locator(".terminal-replay .xterm")
    .click({ button: "right", position: { x: 30, y: 10 } });
  await page.getByRole("menuitem", { name: "全选缓冲区" }).click();
  await page.keyboard.press("Control+c");
  await expect
    .poll(() =>
      page.evaluate(() => (window as any).__HISTORY_TEST__.writes.at(-1)),
    )
    .toContain("historical screen");
  await page.keyboard.press("Control+v");
  await page
    .locator(".terminal-replay .xterm-helper-textarea")
    .evaluate((node) => {
      const data = new DataTransfer();
      data.setData("text/plain", "forbidden");
      node.dispatchEvent(
        new ClipboardEvent("paste", {
          bubbles: true,
          cancelable: true,
          clipboardData: data,
        }),
      );
    });
  expect(await inputs(page)).toEqual([]);
  expect(errors).toEqual([]);
});
test("history text copy excludes timestamps and takes precedence over record selection", async ({
  page,
}) => {
  await setup(page);
  await page.goto("/?terminal-history=1");
  await expect(page.locator(".terminal-history-row").last()).toBeVisible();
  await page.locator(".history-select-record").last().click();
  await page
    .locator(".history-row-content pre")
    .last()
    .evaluate((node) => {
      const range = document.createRange();
      range.selectNodeContents(node);
      const selection = document.getSelection()!;
      selection.removeAllRanges();
      selection.addRange(range);
    });
  await page.keyboard.press("Control+c");
  await expect
    .poll(() =>
      page.evaluate(() => (window as any).__HISTORY_TEST__.writes.at(-1)),
    )
    .toMatch(/^历史输出 \d+ 中文 ERROR$/);
});
test("remote mouse tracking keeps plain right click remote and Shift right click local", async ({
  page,
}) => {
  await ready(page);
  await output(page, "\x1b[?1000h\x1b[?1006h");
  const surface = page.locator(".terminal-surface");
  await surface.click({ button: "right", position: { x: 40, y: 20 } });
  await expect(page.getByRole("menu", { name: "终端菜单" })).not.toBeVisible();
  const prior = await inputs(page);
  expect(prior.length).toBeGreaterThan(0);
  await surface.click({
    button: "right",
    position: { x: 40, y: 20 },
    modifiers: ["Shift"],
  });
  await expect(page.getByRole("menu", { name: "终端菜单" })).toBeVisible();
  expect(await inputs(page)).toEqual(prior);
});
test("read failures, disconnect and window blur never replay stale paste", async ({
  page,
}) => {
  await ready(page);
  await page.evaluate(() => {
    const s = (window as any).__HISTORY_TEST__;
    s.failRead = true;
  });
  await page.getByRole("button", { name: "粘贴到终端", exact: true }).click();
  await expect(
    page.getByText("无法读取系统剪贴板，请重试", { exact: true }),
  ).toBeVisible();
  await page.evaluate(() => {
    const s = (window as any).__HISTORY_TEST__;
    s.failRead = false;
    s.delayRead = true;
  });
  await page.getByRole("button", { name: "粘贴到终端", exact: true }).click();
  await page.evaluate(() => {
    window.dispatchEvent(new Event("blur"));
    window.dispatchEvent(new Event("focus"));
    (window as any).__HISTORY_TEST__.releaseRead();
  });
  expect(await inputs(page)).toEqual([]);
  await page.evaluate(() => {
    const s = (window as any).__HISTORY_TEST__;
    s.delayRead = false;
    s.clipboard = "echo one\n";
  });
  await page.getByRole("button", { name: "粘贴到终端", exact: true }).click();
  await expect(
    page.getByRole("dialog", { name: "粘贴到终端", exact: true }),
  ).toBeVisible();
  await page.evaluate(() =>
    (window as any).__HISTORY_TEST__.emit({
      type: "closed",
      reason: "ended",
      message: "closed",
    }),
  );
  await expect(
    page.getByRole("dialog", { name: "粘贴到终端", exact: true }),
  ).not.toBeVisible();
  await expect(
    page.getByRole("button", { name: "粘贴到终端", exact: true }),
  ).toBeDisabled();
  expect(await inputs(page)).toEqual([]);
});

test("settings events update terminal keys and ignore older revisions", async ({
  page,
}) => {
  await ready(page);
  await page.evaluate(() => {
    const s = (window as any).__HISTORY_TEST__;
    s.settings = {
      ...s.settings,
      revision: 2,
      terminalClipboard: {
        ...s.settings.terminalClipboard,
        shortcutMode: "traditional",
      },
    };
    s.broadcast(s.settings);
    s.broadcast({
      ...s.settings,
      revision: 1,
      terminalClipboard: {
        ...s.settings.terminalClipboard,
        shortcutMode: "convenient",
      },
    });
  });
  await page.locator(".terminal-surface .xterm-helper-textarea").focus();
  await page.keyboard.press("Control+v");
  await expect.poll(() => inputs(page)).toEqual(["\x16"]);
  await page.keyboard.press("Control+Shift+v");
  await expect.poll(() => inputs(page)).toEqual(["\x16", "echo hello"]);
});
test("copy-on-select only copies completed mouse selections", async ({
  page,
}) => {
  await ready(page);
  await page.evaluate(() => {
    const s = (window as any).__HISTORY_TEST__;
    s.settings = {
      ...s.settings,
      revision: 1,
      terminalClipboard: {
        ...s.settings.terminalClipboard,
        copyOnSelect: true,
      },
    };
    s.broadcast(s.settings);
  });
  const surface = page.locator(".terminal-surface");
  await surface.click({ button: "right", position: { x: 40, y: 20 } });
  await page.getByRole("menuitem", { name: "全选缓冲区" }).click();
  expect(
    await page.evaluate(() => (window as any).__HISTORY_TEST__.writes),
  ).toEqual([]);
  await page.keyboard.press("Escape");
  const rect = (await page
    .locator(".terminal-surface .xterm-screen")
    .boundingBox())!;
  await page.mouse.move(rect.x + 2, rect.y + 24);
  await page.mouse.down();
  await page.mouse.move(rect.x + 90, rect.y + 24, { steps: 4 });
  expect(
    await page.evaluate(() => (window as any).__HISTORY_TEST__.writes),
  ).toEqual([]);
  await page.mouse.up();
  await expect
    .poll(() =>
      page.evaluate(() => (window as any).__HISTORY_TEST__.writes.length),
    )
    .toBe(1);
  expect(await inputs(page)).toEqual([]);
});

test("preview stays usable in a narrow dark window and restores input focus", async ({
  page,
}) => {
  await ready(page);
  await page.setViewportSize({ width: 640, height: 580 });
  await selectTheme(page, "dark");
  await nativePaste(page, "echo first\necho second\n");
  const dialog = page.getByRole("dialog", { name: "粘贴到终端", exact: true });
  await expect(dialog).toBeVisible();
  await expect(
    dialog.getByRole("button", { name: "粘贴", exact: true }),
  ).toBeInViewport();
  const rect = (await dialog.boundingBox())!;
  expect(rect.x).toBeGreaterThanOrEqual(0);
  expect(rect.x + rect.width).toBeLessThanOrEqual(640);
  await page.screenshot({ path: ".qa/terminal-clipboard-dark-narrow.png" });
  await dialog.getByRole("button", { name: "取消", exact: true }).click();
  await expect(
    page.locator(".terminal-surface .xterm-helper-textarea"),
  ).toBeFocused();
  expect(await inputs(page)).toEqual([]);
});
