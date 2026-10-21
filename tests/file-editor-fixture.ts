import { expect, type Page } from "@playwright/test";

export async function setupEditor(
  page: Page,
  options: { text?: string; path?: string; loadError?: boolean } = {},
) {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.addInitScript((options) => {
    const w = window as any;
    let serial = 1;
    const callbacks = new Map<number, Function>();
    const listeners = new Map<number, { event: string; handler: number }>();
    const calls: { command: string; args: any }[] = [];
    const doc = {
      serverName: "Editor fixture",
      path: options.path ?? "/etc/service/config.yaml",
      text: options.text ?? "name: service\nport: 8080\n",
      version: "v1",
      size: 0,
    };
    const snapshot = () => ({
      ...doc,
      size: new TextEncoder().encode(doc.text).length,
    });
    let version = 1;
    let failLoad = options.loadError ?? false;
    let releaseLoad: (() => void) | undefined;
    let releaseSave: (() => void) | undefined;
    let loadGate: Promise<void> | undefined;
    let saveGate: Promise<void> | undefined;
    const emit = async (event: string, payload: unknown) => {
      for (const [id, listener] of [...listeners])
        if (listener.event === event)
          await callbacks.get(listener.handler)?.({ event, id, payload });
    };
    w.__EDITOR_TEST__ = {
      calls,
      snapshot,
      failLoad: (value: boolean) => {
        failLoad = value;
      },
      changeRemote: (text: string) => {
        doc.text = text;
        doc.version = `v${++version}`;
      },
      holdLoad: () => {
        loadGate = new Promise<void>((resolve) => {
          releaseLoad = resolve;
        });
      },
      releaseLoad: () => {
        releaseLoad?.();
        loadGate = undefined;
      },
      holdSave: () => {
        saveGate = new Promise<void>((resolve) => {
          releaseSave = resolve;
        });
      },
      releaseSave: () => {
        releaseSave?.();
        saveGate = undefined;
      },
      close: () => emit("tauri://close-requested", null),
    };
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: "editor-fixture" },
        currentWebview: { label: "editor-fixture" },
      },
      transformCallback: (callback: Function) => {
        const id = serial++;
        callbacks.set(id, callback);
        return id;
      },
      unregisterCallback: (id: number) => callbacks.delete(id),
      invoke: async (command: string, args: any = {}) => {
        calls.push({ command, args });
        if (command === "window_chrome") return "custom";
        if (command === "plugin:event|listen") {
          const id = serial++;
          listeners.set(id, { event: args.event, handler: args.handler });
          return id;
        }
        if (command === "plugin:event|unlisten")
          return listeners.delete(args.eventId);
        if (command === "plugin:window|is_focused") return true;
        if (
          command === "plugin:window|is_maximized" ||
          command === "plugin:window|is_fullscreen"
        )
          return false;
        if (command === "plugin:window|close")
          return emit("tauri://close-requested", null);
        if (command === "editor_load") {
          await loadGate;
          if (failLoad) throw { kind: "io", message: "读取失败（测试）" };
          return snapshot();
        }
        if (command === "editor_save") {
          await saveGate;
          if (!args.force && args.version !== doc.version)
            throw { kind: "conflict", message: "远端文件已发生变化" };
          doc.text = args.text;
          doc.version = `v${++version}`;
          return snapshot();
        }
        return null;
      },
    };
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
  }, options);
  await page.goto("/?editor=1");
  if (options.loadError)
    await expect(page.getByRole("alert")).toContainText("读取失败");
  else
    await expect(
      page.getByRole("textbox", { name: "远程文件内容" }),
    ).toBeVisible();
  return { errors };
}

export async function editorCalls(page: Page, command: string) {
  return page.evaluate(
    (command) =>
      (window as any).__EDITOR_TEST__.calls.filter(
        (call: any) => call.command === command,
      ),
    command,
  );
}
