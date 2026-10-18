import { test, expect, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
import { setup as workspace, row } from "./resource-fixture";

async function setup(page: Page, active = false, auxiliary = false) {
  const scripts = JSON.parse(
    readFileSync(".qa/system-settings/browser-policy.json", "utf8"),
  );
  await page.addInitScript({
    content: active ? scripts.enabled : scripts.disabled,
  });
  await page.addInitScript(() => {
    const w = window as any;
    w.__UNGUARDED_DEVTOOLS__ = 0;
    // Model Tauri's built-in listener, which does not check defaultPrevented.
    window.addEventListener("DOMContentLoaded", () => {
      document.addEventListener("keydown", (e) => {
        if (
          e.code === "KeyI" &&
          ((e.metaKey && e.altKey) || (e.ctrlKey && e.shiftKey))
        )
          w.__UNGUARDED_DEVTOOLS__++;
      });
    });
  });
  const errors = await workspace(page, auxiliary ? "monitor" : "workspace");
  await page.evaluate((active) => {
    const w = window as any;
    const ipc = w.__TAURI_INTERNALS__;
    const originalInvoke = ipc.invoke;
    const transform = ipc.transformCallback;
    const callbacks = new Map<number, Function>();
    const listeners = new Map<number, number>();
    const mock = (w.__SETTINGS__ = {
      state: {
        developerMode: active,
        activeDeveloperMode: active,
        revision: 0,
        loadError: null,
      },
      calls: [] as any[],
      failSave: false,
      delaySave: false,
      release: null as Function | null,
      emit(payload: any) {
        for (const [id, callback] of listeners)
          callbacks.get(callback)?.({
            event: "system-settings-changed",
            id,
            payload,
          });
      },
      change(developerMode: boolean) {
        mock.state = {
          ...mock.state,
          developerMode,
          revision: mock.state.revision + 1,
        };
        mock.emit(mock.state);
      },
    });
    ipc.transformCallback = (fn: Function, ...args: any[]) => {
      const id = transform(fn, ...args);
      callbacks.set(id, fn);
      return id;
    };
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ ??= { unregisterListener() {} };
    ipc.invoke = async (command: string, args: any = {}) => {
      if (
        command === "plugin:event|listen" &&
        args.event === "system-settings-changed"
      ) {
        listeners.set(args.handler, args.handler);
        return args.handler;
      }
      if (
        command === "plugin:event|unlisten" &&
        args.event === "system-settings-changed"
      ) {
        listeners.delete(args.eventId);
        return;
      }
      if (command === "get_system_settings") return structuredClone(mock.state);
      if (command === "set_developer_mode") {
        mock.calls.push({ command, args });
        if (mock.delaySave)
          await new Promise<void>((resolve) => {
            mock.release = resolve;
          });
        if (mock.failSave)
          throw { kind: "settings", message: "保存系统设置失败：目录不可写" };
        if (args.expectedRevision !== mock.state.revision)
          throw {
            kind: "settingsConflict",
            message: "设置已在其他窗口更新，请核对后重试。",
          };
        mock.change(args.developerMode);
        return structuredClone(mock.state);
      }
      if (command === "open_window_devtools") {
        mock.calls.push({ command });
        if (!mock.state.activeDeveloperMode)
          throw new Error("developer mode is inactive");
        return;
      }
      return originalInvoke(command, args);
    };
  }, active);
  return errors;
}

const settings = (page: Page) =>
  page.getByRole("dialog", { name: "系统设置", exact: true });
const checkbox = (page: Page) =>
  settings(page).getByRole("checkbox", { name: "开启开发者模式" });
async function open(page: Page) {
  await page.getByRole("button", { name: "系统设置", exact: true }).click();
  await expect(checkbox(page)).toBeEnabled();
}

test("settings save, remain pending, can be undone, and failed saves restore the value", async ({
  page,
}) => {
  const errors = await setup(page);
  await open(page);
  await expect(checkbox(page)).not.toBeChecked();
  await expect(settings(page)).toContainText("当前状态：已关闭");
  await checkbox(page).check();
  await expect(settings(page)).toContainText(
    "已保存，下次启动后开启开发者模式。",
  );
  await expect(
    settings(page).getByRole("button", { name: "打开当前窗口开发者工具" }),
  ).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("button", { name: "系统设置", exact: true }),
  ).toBeFocused();
  await open(page);
  await expect(checkbox(page)).toBeChecked();
  await checkbox(page).uncheck();
  await expect(settings(page)).toContainText("已保存，与当前状态一致。");
  await page.evaluate(() => {
    (window as any).__SETTINGS__.failSave = true;
  });
  await checkbox(page).click();
  await expect(settings(page).getByRole("alert")).toContainText("目录不可写");
  await expect(checkbox(page)).not.toBeChecked();
  expect(errors).toEqual([]);
});

test("saving disables the checkbox and closing the dialog does not cancel persistence", async ({
  page,
}) => {
  const errors = await setup(page);
  await open(page);
  await page.evaluate(() => {
    (window as any).__SETTINGS__.delaySave = true;
  });
  await checkbox(page).check();
  await expect(checkbox(page)).toBeDisabled();
  await expect(settings(page)).toContainText("正在保存…");
  await page.keyboard.press("Escape");
  await page.evaluate(() => {
    (window as any).__SETTINGS__.release();
  });
  await open(page);
  await expect(checkbox(page)).toBeChecked();
  await expect(settings(page)).toContainText("当前状态：已关闭");
  expect(errors).toEqual([]);
});

test("cross-window changes synchronize and out-of-order events do not roll back the UI", async ({
  page,
}) => {
  const errors = await setup(page, false, true);
  await open(page);
  await page.evaluate(() => {
    (window as any).__SETTINGS__.change(true);
  });
  await expect(checkbox(page)).toBeChecked();
  await page.evaluate(() => {
    const mock = (window as any).__SETTINGS__;
    mock.emit({ ...mock.state, revision: 0, developerMode: false });
  });
  await expect(checkbox(page)).toBeChecked();
  await expect(settings(page)).toContainText("当前状态：已关闭");
  await page.setViewportSize({ width: 420, height: 600 });
  await page
    .getByRole("dialog")
    .screenshot({ path: ".qa/system-settings/narrow.png" });
  expect(
    await settings(page).evaluate((el) => el.scrollWidth <= el.clientWidth),
  ).toBe(true);
  expect(errors).toEqual([]);
});

test("active mode exposes current-window devtools, including while a disable is pending", async ({
  page,
}) => {
  const errors = await setup(page, true, true);
  await open(page);
  await expect(settings(page)).toContainText("当前状态：已开启");
  await settings(page)
    .getByRole("button", { name: "打开当前窗口开发者工具" })
    .click();
  await checkbox(page).uncheck();
  await expect(settings(page)).toContainText(
    "已保存，下次启动后关闭开发者模式。",
  );
  await expect(
    settings(page).getByRole("button", { name: "打开当前窗口开发者工具" }),
  ).toBeEnabled();
  const allowed = await page.evaluate(() => {
    const event = new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
    });
    document.body.dispatchEvent(event);
    return !event.defaultPrevented;
  });
  expect(allowed).toBe(true);
  expect(
    await page.evaluate(() =>
      [
        { key: "r", ctrlKey: true },
        { key: "r", metaKey: true },
        { key: "F5" },
      ].every((options) => {
        const event = new KeyboardEvent("keydown", {
          ...options,
          bubbles: true,
          cancelable: true,
        });
        document.body.dispatchEvent(event);
        return !event.defaultPrevented;
      }),
    ),
  ).toBe(true);
  await page.keyboard.press("Escape");
  await page.evaluate(() =>
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "i",
        code: "KeyI",
        metaKey: true,
        altKey: true,
        bubbles: true,
        cancelable: true,
      }),
    ),
  );
  expect(
    await page.evaluate(() => (window as any).__UNGUARDED_DEVTOOLS__),
  ).toBe(0);
  expect(
    await page.evaluate(
      () =>
        (window as any).__SETTINGS__.calls.filter(
          (c: any) => c.command === "open_window_devtools",
        ).length,
    ),
  ).toBe(2);
  expect(errors).toEqual([]);
});

test("normal mode preserves CodeMirror editing, undo, find and save shortcuts", async ({
  page,
}) => {
  const scripts = JSON.parse(
    readFileSync(".qa/system-settings/browser-policy.json", "utf8"),
  );
  await page.addInitScript({ content: scripts.disabled });
  await page.addInitScript(() => {
    const w = window as any;
    let serial = 1;
    const calls: any[] = [];
    w.__EDITOR_CALLS__ = calls;
    const document = {
      serverName: "Fixture",
      path: "/test.txt",
      text: "original\n",
      version: "v1",
      size: 9,
    };
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
    w.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: "editor-test" },
        currentWebview: { label: "editor-test" },
      },
      transformCallback: () => serial++,
      unregisterCallback() {},
      invoke: async (command: string, args: any = {}) => {
        calls.push({ command, args });
        if (command === "window_chrome") return "custom";
        if (command === "plugin:event|listen") return serial++;
        if (command === "editor_load") return { ...document };
        if (command === "editor_save") {
          Object.assign(document, {
            text: args.text,
            version: "v2",
            size: args.text.length,
          });
          return { ...document };
        }
        return null;
      },
    };
  });
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/?editor=1");
  const editor = page.getByRole("textbox", { name: "远程文件内容" });
  await expect(editor).toContainText("original");
  await editor.click();
  const mod = await page.evaluate(() =>
    /Mac/.test(navigator.platform) ? "Meta" : "Control",
  );
  await page.keyboard.press(`${mod}+a`);
  await page.keyboard.type("changed");
  await expect(editor).toHaveText("changed");
  await page.keyboard.press(`${mod}+z`);
  await expect(editor).toContainText("original");
  await page.keyboard.press(`${mod}+a`);
  await page.keyboard.type("saved value");
  await page.keyboard.press(`${mod}+s`);
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).__EDITOR_CALLS__
            .filter((c: any) => c.command === "editor_save")
            .at(-1)?.args.text,
      ),
    )
    .toBe("saved value");
  await page.keyboard.press(`${mod}+f`);
  await expect(page.locator(".cm-search")).toBeVisible();
  await page.keyboard.press("Escape");
  expect(errors).toEqual([]);
});

test("normal mode cancels browser defaults but keeps business menus and real xterm key input", async ({
  page,
}) => {
  const errors = await setup(page);
  const blocked = await page.evaluate(() => {
    const events = [
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
      ...[
        { key: "r", ctrlKey: true },
        { key: "R", ctrlKey: true, shiftKey: true },
        { key: "r", metaKey: true },
        { key: "R", metaKey: true, shiftKey: true },
        { key: "F5" },
        { key: "F12" },
        { key: "i", code: "KeyI", ctrlKey: true, shiftKey: true },
        { key: "i", code: "KeyI", metaKey: true, altKey: true },
      ].map(
        (options) =>
          new KeyboardEvent("keydown", {
            ...options,
            bubbles: true,
            cancelable: true,
          }),
      ),
    ];
    return events.map((event) => {
      document.body.dispatchEvent(event);
      return event.defaultPrevented;
    });
  });
  expect(blocked.every(Boolean)).toBe(true);
  const editingAllowed = await page.evaluate(() =>
    [
      { key: "c", ctrlKey: true },
      { key: "v", ctrlKey: true },
      { key: "c", ctrlKey: true, shiftKey: true },
      { key: "v", ctrlKey: true, shiftKey: true },
      { key: "c", metaKey: true },
      { key: "v", metaKey: true },
      { key: "f", ctrlKey: true },
      { key: "f", metaKey: true },
    ].every((options) => {
      const event = new KeyboardEvent("keydown", {
        ...options,
        bubbles: true,
        cancelable: true,
      });
      document.body.dispatchEvent(event);
      return !event.defaultPrevented;
    }),
  );
  expect(editingAllowed).toBe(true);
  expect(
    await page.evaluate(() => (window as any).__UNGUARDED_DEVTOOLS__),
  ).toBe(0);
  expect(await page.evaluate(() => (window as any).__SETTINGS__.calls)).toEqual(
    [],
  );

  await row(page, "/data").click({ button: "right" });
  await expect(page.getByRole("menu", { name: "文件操作" })).toBeVisible();
  await page.keyboard.press("Escape");
  await page
    .getByRole("tab", { name: "Mount fixture，已连接", exact: true })
    .click({ button: "right" });
  await expect(page.getByRole("menu", { name: "会话操作" })).toBeVisible();
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "新建连接", exact: true }).click();
  await page.locator(".server-row").first().click({ button: "right" });
  await expect(
    page.getByRole("menu", { name: /Mount fixture 操作/ }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  await page.locator(".terminal-surface textarea").focus();
  for (const key of ["Control+r", "Control+c", "Control+u", "F5", "F12"])
    await page.keyboard.press(key);
  const input = await page.evaluate(() =>
    (window as any).__MOUNTS__.calls
      .filter((c: any) => c.command === "terminal_input")
      .map((c: any) => c.args.data)
      .join(""),
  );
  for (const sequence of ["\x12", "\x03", "\x15", "\x1b[15~", "\x1b[24~"])
    expect(input).toContain(sequence);
  await open(page);
  await page
    .getByRole("dialog")
    .screenshot({ path: ".qa/system-settings/light.png" });
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "黑夜主题" }).click();
  await open(page);
  await page
    .getByRole("dialog")
    .screenshot({ path: ".qa/system-settings/dark.png" });
  expect(errors).toEqual([]);
});
