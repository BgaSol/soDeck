// Windows native QA against the isolated app described in docs/server-password-copy-implementation.md.
// Uses real Tauri IPC and the system clipboard, with test credentials only.
import { chromium, expect } from "@playwright/test";
import { spawn, execFileSync } from "node:child_process";
import { once } from "node:events";
import fs from "node:fs";

const phase = process.env.SODECK_PASSWORD_COPY_PHASE || "initial";
const browser = await chromium.connectOverCDP(
  process.env.SODECK_PASSWORD_COPY_CDP_URL || "http://127.0.0.1:9249",
);
const appUrl =
  process.env.SODECK_PASSWORD_COPY_APP_URL || "http://127.0.0.1:1479";
const page = browser
  .contexts()[0]
  .pages()
  .find((p) => p.url().startsWith(appUrl));
if (!page) throw new Error("Isolated password-copy QA webview was not found");
const ipc = (command, args = {}, target = page) =>
  target.evaluate(
    ({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args),
    { command, args },
  );
const readClipboard = () => ipc("plugin:clipboard-manager|read_text");
const copyFailure = (id, target = page) =>
  target.evaluate(
    (id) =>
      window.__TAURI_INTERNALS__
        .invoke("copy_saved_server_password", { id })
        .then(
          () => ({ unexpectedSuccess: true }),
          (error) => error,
        ),
    id,
  );
const checks = [];
const errors = [];
page.on("pageerror", (error) => errors.push(error.message));
const secret = "  本机-copy'\"$\\  ";
const replacement = "  replacement-测试  ";
const fixturePath = ".qa/server-password-copy-native-fixture.json";
let original;
let lastOwned;
let lock;
let history;
const lockEvent = `Local\\SoDeckPasswordCopyQA-${process.pid}`;
async function releaseClipboardLock() {
  if (!lock || lock.exitCode !== null) return;
  const exited = once(lock, "exit");
  execFileSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      `[Threading.EventWaitHandle]::OpenExisting('${lockEvent}').Set() | Out-Null`,
    ],
    { windowsHide: true },
  );
  await exited;
  lock = undefined;
}
try {
  original = await readClipboard().catch(() => undefined);
  const servers = await ipc("list_servers");
  expect(
    servers.every((server) => server.name.startsWith("Password copy QA")),
  ).toBe(true);
  let server;
  if (phase === "restart") {
    const savedId = JSON.parse(fs.readFileSync(fixturePath, "utf8")).id;
    server = servers.find((s) => s.id === savedId);
    expect(server?.hasSavedPassword).toBe(true);
  } else {
    // Remove only fixtures owned by this isolated test app.
    for (const previous of servers)
      await ipc("delete_server", { id: previous.id });
    server = await ipc("save_server", {
      server: {
        id: "",
        name: "Password copy QA",
        host: "127.0.0.1",
        port: 22,
        username: "fixture",
        tags: [],
        authType: "password",
        privateKey: "",
        hasSavedPassword: false,
        jumpServerId: null,
        directHost: null,
        directPort: 22,
        passwordChange: { type: "set", value: secret },
      },
    });
    fs.writeFileSync(fixturePath, JSON.stringify({ id: server.id }));
  }
  expect(server).not.toHaveProperty("password");
  await page.reload();
  const row = page.locator(`.server-row[data-server-id="${server.id}"]`);
  await row.locator('[data-server-action="edit"]').click();
  const editor = page.getByRole("dialog", { name: "服务器配置", exact: true });
  const copy = editor.getByRole("button", {
    name: "复制已保存密码",
    exact: true,
  });
  const feedback = editor.locator(".password-copy-feedback");
  for (let i = 0; i < 2; i++) {
    await copy.click();
    lastOwned = phase === "restart" ? replacement : secret;
    await expect(feedback).toHaveText("密码已复制");
    expect(await readClipboard()).toBe(lastOwned);
    await expect(editor.getByLabel("密码", { exact: true })).toHaveValue("");
  }
  checks.push(
    phase === "restart"
      ? "repeated copy after application restart"
      : "repeated exact native copy with hidden password",
  );

  if (process.platform === "win32") {
    const formats = execFileSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        `
      Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public class PasswordCopyFormats {
        [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern uint RegisterClipboardFormat(string name);
        [DllImport("user32.dll")] public static extern bool IsClipboardFormatAvailable(uint format);
      }';
      [PasswordCopyFormats]::IsClipboardFormatAvailable([PasswordCopyFormats]::RegisterClipboardFormat('ExcludeClipboardContentFromMonitorProcessing'))
    `,
      ],
      { windowsHide: true, encoding: "utf8" },
    );
    expect(formats.trim()).toBe("True");
    checks.push("Windows sensitive clipboard exclusion flag is present");
  }

  if (phase === "initial") {
    await editor.getByLabel("密码", { exact: true }).fill(replacement);
    await copy.click();
    await expect(feedback).toHaveText("密码已复制");
    expect(await readClipboard()).toBe(secret);
    await editor.getByRole("button", { name: "保存修改", exact: true }).click();
    const response = await ipc("copy_saved_server_password", { id: server.id });
    lastOwned = replacement;
    expect(response).toBeNull();
    expect(await readClipboard()).toBe(replacement);
    checks.push(
      "unsaved replacement copies old value; saved replacement copies new value without IPC payload",
    );

    await row.locator('[data-server-action="copy"]').click();
    await expect(
      editor.getByRole("button", { name: "新建服务器", exact: true }),
    ).toBeVisible();
    await expect(
      editor.getByRole("button", { name: "保存修改", exact: true }),
    ).toBeVisible();
    await copy.click();
    await expect(feedback).toHaveText("密码已复制");
    expect(await readClipboard()).toBe(replacement);
    await editor.getByLabel(/^名称/).fill("Password copy QA renamed");
    await editor.getByRole("button", { name: "保存修改", exact: true }).click();
    const renamed = await ipc("list_servers");
    expect(renamed).toHaveLength(1);
    expect(renamed[0]).toMatchObject({
      id: server.id,
      name: "Password copy QA renamed",
      hasSavedPassword: true,
    });
    checks.push(
      "copy entry can copy the stored password and update the fixed source ID",
    );

    await row.locator('[data-server-action="edit"]').click();
    await editor.getByLabel(/^名称/).fill("Password copy QA independent");
    await editor.getByLabel("地址", { exact: true }).fill("127.0.0.2");
    await editor.getByLabel("标签", { exact: true }).fill("native-unadded-tag");
    await editor
      .getByRole("button", { name: "新建服务器", exact: true })
      .click();
    const independentServers = await ipc("list_servers");
    const independent = independentServers.find((s) => s.id !== server.id);
    expect(independent).toMatchObject({
      name: "Password copy QA independent",
      host: "127.0.0.2",
      tags: ["native-unadded-tag"],
      hasSavedPassword: true,
    });
    expect(independentServers.find((s) => s.id === server.id)).toEqual(
      renamed[0],
    );
    await ipc("copy_saved_server_password", { id: independent.id });
    expect(await readClipboard()).toBe(replacement);
    await ipc("delete_server", { id: independent.id });
    await ipc("copy_saved_server_password", { id: server.id });
    expect(await readClipboard()).toBe(replacement);
    checks.push(
      "edit entry creates an independent full draft with inherited credential and preserves the source",
    );
    await page.reload();
    await row.locator('[data-server-action="edit"]').click();
    // Hold the actual Windows clipboard in another process to provoke a real write failure.
    lock = spawn(
      "powershell.exe",
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        `
      Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public class PasswordCopyLock {
        [DllImport("user32.dll")] public static extern bool OpenClipboard(IntPtr handle);
        [DllImport("user32.dll")] public static extern bool CloseClipboard();
        [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr CreateWindowEx(
          int exStyle, string className, string title, int style, int x, int y, int w, int h,
          IntPtr parent, IntPtr menu, IntPtr instance, IntPtr param);
        [DllImport("user32.dll")] public static extern bool DestroyWindow(IntPtr window);
        public static void Hold(string name) {
          var window = CreateWindowEx(0, "STATIC", "", 0, 0, 0, 0, 0, new IntPtr(-3), IntPtr.Zero, IntPtr.Zero, IntPtr.Zero);
          if (window == IntPtr.Zero || !OpenClipboard(window)) throw new Exception("Cannot lock clipboard");
          try {
            using (var release = new System.Threading.EventWaitHandle(false, System.Threading.EventResetMode.ManualReset, name)) {
              Console.WriteLine("READY");
              release.WaitOne(20000);
            }
          } finally { CloseClipboard(); DestroyWindow(window); }
        }
      }';
      [PasswordCopyLock]::Hold('${lockEvent}')
    `,
      ],
      { windowsHide: true, stdio: ["pipe", "pipe", "pipe"] },
    );
    const ready = await Promise.race([
      once(lock.stdout, "data").then(([data]) => String(data).trim()),
      once(lock, "exit").then(() => "EXITED"),
      new Promise((resolve) =>
        setTimeout(() => resolve("TIMEOUT"), 10000).unref(),
      ),
    ]);
    expect(ready).toBe("READY");
    expect(lock.exitCode).toBeNull();
    expect(
      await readClipboard().then(
        () => "readable",
        () => "locked",
      ),
    ).toBe("locked");
    await copy.click();
    await expect(feedback.getByRole("alert")).toHaveText(
      "密码复制失败，请重试。",
    );
    await releaseClipboardLock();
    await copy.click();
    await expect(feedback).toHaveText("密码已复制");
    expect(await readClipboard()).toBe(replacement);
    checks.push(
      "real clipboard contention reports failure and retries successfully",
    );
    await page.screenshot({ path: ".qa/server-password-copy-native.png" });
    await editor.getByRole("button", { name: "取消", exact: true }).click();

    const opened = browser.contexts()[0].waitForEvent("page");
    await ipc("open_terminal_history_window");
    history = await opened;
    await history.waitForURL(/terminal-history=1/);
    await history.waitForLoadState();
    await history.waitForFunction(
      () => typeof window.__TAURI_INTERNALS__?.invoke === "function",
    );
    const denied = await copyFailure(server.id, history);
    expect(denied.kind).toBe("permission");
    expect(await readClipboard()).toBe(replacement);
    await ipc("plugin:window|close", {}, history).catch(() => {});
    history = undefined;
    checks.push("non-main webview denied without changing clipboard");
  } else {
    await editor
      .getByRole("button", { name: "清空已保存密码", exact: true })
      .click();
    await copy.click();
    await expect(feedback).toHaveText("密码已复制");
    expect(await readClipboard()).toBe(replacement);
    await editor.getByRole("button", { name: "保存修改", exact: true }).click();
    const cleared = await copyFailure(server.id);
    expect(cleared.kind).toBe("unavailable");
    expect(await readClipboard()).toBe(replacement);
    await row.locator('[data-server-action="edit"]').click();
    await expect(copy).toHaveCount(0);
    await editor.getByRole("button", { name: "取消", exact: true }).click();
    await ipc("delete_server", { id: server.id });
    const deleted = await copyFailure(server.id);
    expect(deleted.kind).toBe("notFound");
    expect(await readClipboard()).toBe(replacement);
    checks.push(
      "pending clear still copies; committed clear and deletion do not write clipboard",
    );
  }
  expect(errors).toEqual([]);
  fs.writeFileSync(
    `.qa/server-password-copy-native-${phase}.json`,
    JSON.stringify({ checks, errors }, null, 2),
  );
  console.log(JSON.stringify({ checks, errors }, null, 2));
} finally {
  await releaseClipboardLock();
  if (history) await ipc("plugin:window|close", {}, history).catch(() => {});
  if (
    lastOwned !== undefined &&
    (await readClipboard().catch(() => null)) === lastOwned
  )
    await ipc("plugin:clipboard-manager|write_text", { text: original ?? "" });
  await browser.close();
}
