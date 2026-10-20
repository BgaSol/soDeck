// Requires the isolated native QA app and loopback fixture described in docs/terminal-clipboard-verification.md.
import { chromium, expect } from "@playwright/test";
import fs from "node:fs";
let browser;
for (let attempt = 0; attempt < 40; attempt++) {
  try {
    browser = await chromium.connectOverCDP(
      process.env.SODECK_CLIPBOARD_CDP_URL || "http://127.0.0.1:9239",
    );
    break;
  } catch (error) {
    if (attempt === 39) throw error;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
}
const page = browser
  .contexts()[0]
  .pages()
  .find((page) => page.url().startsWith("http://127.0.0.1:1469"));
const ipc = (command, args = {}) =>
  page.evaluate(
    ({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args),
    { command, args },
  );
const errors = [];
page.on("pageerror", (error) => errors.push(error.message));
const checks = [];
let original;
let lastOwned;
const commands = () =>
  fs
    .readFileSync(
      process.env.SODECK_CLIPBOARD_FIXTURE_LOG ||
        ".qa/clipboard-ssh/ssh-events.jsonl",
      "utf8",
    )
    .trim()
    .split("\n")
    .map((line) => JSON.parse(line))
    .filter((event) => event.event === "terminal-input")
    .map((event) => event.command);
let baseline = commands().length;
try {
  const servers = await ipc("list_servers");
  if (!servers.length)
    await ipc("save_server", {
      server: {
        id: "",
        name: "Clipboard native fixture",
        host: "127.0.0.1",
        port: 22239,
        username: "ops",
        tags: [],
        authType: "password",
        privateKey: "",
        hasSavedPassword: false,
        jumpServerId: null,
        directHost: null,
        directPort: 22,
        passwordChange: { type: "set", value: "fixture-password" },
      },
    });
  await page.reload();
  await page.bringToFront();
  await page
    .locator(".server-row")
    .filter({ hasText: "Clipboard native fixture" })
    .dblclick();
  const trust = page.getByRole("button", { name: "确认指纹并连接" });
  await expect(async () => {
    if (await trust.isVisible()) await trust.click();
    await expect(
      page.locator(".session-page[role=tabpanel]:visible .connection-state"),
    ).toHaveText("已连接", { timeout: 1000 });
  }).toPass({ timeout: 20000 });
  checks.push("real SSH connection");
  original = await ipc("plugin:clipboard-manager|read_text");
  const input = page.locator(
    ".session-page[role=tabpanel]:visible .terminal-surface .xterm-helper-textarea",
  );
  await ipc("plugin:clipboard-manager|write_text", {
    text: "echo native_clipboard",
  });
  lastOwned = "echo native_clipboard";
  await input.focus();
  await page.keyboard.press("Control+v");
  await expect(
    page.locator(".session-page[role=tabpanel]:visible .xterm-rows"),
  ).toContainText("echo native_clipboard");
  expect(commands().slice(baseline)).toEqual([]);
  checks.push("native clipboard Ctrl+V exactly once without Enter");
  await page.keyboard.press("Enter");
  await expect
    .poll(() => commands().slice(baseline))
    .toEqual(["echo native_clipboard"]);
  const surface = page.locator(
    ".session-page[role=tabpanel]:visible .terminal-surface",
  );
  await surface.click({ button: "right", position: { x: 40, y: 20 } });
  await page.getByRole("menuitem", { name: "全选缓冲区" }).click();
  const before = commands().length;
  await page.keyboard.press("Control+c");
  await expect
    .poll(() => ipc("plugin:clipboard-manager|read_text"))
    .toContain("native_clipboard");
  lastOwned = await ipc("plugin:clipboard-manager|read_text");
  expect(commands().length).toBe(before);
  checks.push("native context menu and Ctrl+C copy without interrupt");
  await ipc("plugin:clipboard-manager|write_text", {
    text: "echo native_preview\n",
  });
  lastOwned = "echo native_preview\n";
  await page.keyboard.press("Control+v");
  const preview = page.getByRole("dialog", { name: "粘贴到终端", exact: true });
  await expect(preview).toBeVisible();
  await expect(preview).toContainText("ops@127.0.0.1:22239");
  expect(commands().length).toBe(before);
  await preview.getByRole("button", { name: "移除末尾换行" }).click();
  await page.screenshot({ path: ".qa/terminal-clipboard-native.png" });
  await preview.getByRole("button", { name: "粘贴", exact: true }).click();
  await expect(
    page.locator(".session-page[role=tabpanel]:visible .xterm-rows"),
  ).toContainText("echo native_preview");
  expect(commands().length).toBe(before);
  await input.focus();
  await page.keyboard.press("Enter");
  await expect
    .poll(() => commands().slice(baseline))
    .toEqual(["echo native_clipboard", "echo native_preview"]);
  checks.push(
    "native multiline preview, source label and explicit newline removal",
  );
  const panel = page.locator(".session-page[role=tabpanel]:visible");
  await panel.getByRole("tab", { name: "Docker", exact: true }).click();
  await panel
    .getByRole("group", { name: "Docker 对象类型" })
    .getByRole("button", { name: /^容器/ })
    .click();
  await panel.getByText("web-fixture", { exact: true }).click();
  const opened = browser.contexts()[0].waitForEvent("page");
  await page.getByRole("button", { name: "进入 web-fixture 终端" }).click();
  const container = await opened;
  await container.waitForLoadState();
  await expect(
    container.getByRole("button", { name: "粘贴到终端", exact: true }),
  ).toBeEnabled();
  await container.evaluate(() =>
    window.__TAURI_INTERNALS__.invoke("plugin:clipboard-manager|write_text", {
      text: "echo native_container",
    }),
  );
  lastOwned = "echo native_container";
  await container.locator(".xterm-helper-textarea").focus();
  await container.keyboard.press("Control+v");
  await expect(container.locator(".xterm-rows")).toContainText(
    "echo native_container",
  );
  await container.keyboard.press("Enter");
  await expect
    .poll(() => commands().slice(baseline))
    .toEqual([
      "echo native_clipboard",
      "echo native_preview",
      "echo native_container",
    ]);
  checks.push(
    "independent native container window clipboard and isolated SSH input",
  );
  await container
    .evaluate(() =>
      window.__TAURI_INTERNALS__.invoke("plugin:window|close", {
        label: window.__TAURI_INTERNALS__.metadata.currentWindow.label,
      }),
    )
    .catch(() => {});
  expect(errors).toEqual([]);
  fs.writeFileSync(
    ".qa/terminal-clipboard-native-result.json",
    JSON.stringify({ checks, errors }, null, 2),
  );
  console.log(checks.map((check) => "PASS " + check).join("\n"));
} finally {
  if (
    original !== undefined &&
    (await ipc("plugin:clipboard-manager|read_text").catch(() => undefined)) ===
      lastOwned
  )
    await ipc("plugin:clipboard-manager|write_text", { text: original });
  await browser.close();
}
