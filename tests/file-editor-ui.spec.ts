import { test, expect } from "@playwright/test";
import { editorCalls, setupEditor } from "./file-editor-fixture";
import { selectTheme } from "./theme-fixture";

async function openSearch(page: import("@playwright/test").Page, query = "") {
  await page.getByRole("button", { name: "查找", exact: true }).click();
  const panel = page.getByRole("search", { name: "文件内查找与替换" });
  const field = panel.getByRole("textbox", { name: "查找内容" });
  await expect(field).toBeFocused();
  if (query) await field.fill(query);
  return { panel, field, count: panel.getByLabel("匹配数量") };
}

test("editor baseline: theme and language preserve unsaved edits, undo and load count", async ({
  page,
}) => {
  const { errors } = await setupEditor(page, { text: "original\n" });
  const content = page.getByRole("textbox", { name: "远程文件内容" });
  await content.fill("changed\n");
  await selectTheme(page, "dark");
  await page.getByLabel("文件语言").selectOption("text");
  await expect(content).toContainText("changed");
  await expect(page.getByLabel("未保存", { exact: true })).toBeVisible();
  expect(await editorCalls(page, "editor_load")).toHaveLength(1);
  await page.getByRole("button", { name: "撤销", exact: true }).click();
  await expect(content).toContainText("original");
  await page.getByRole("button", { name: "重做", exact: true }).click();
  await expect(content).toContainText("changed");
  expect(errors).toEqual([]);
});

test("editor baseline: reload cancellation, conflict and save before close", async ({
  page,
}) => {
  await setupEditor(page, { text: "original\n" });
  const content = page.getByRole("textbox", { name: "远程文件内容" });
  await content.fill("local edit\n");
  await page.getByRole("button", { name: "重新加载", exact: true }).click();
  await page.getByRole("button", { name: "继续编辑", exact: true }).click();
  await expect(content).toContainText("local edit");
  expect(await editorCalls(page, "editor_load")).toHaveLength(1);
  await page.evaluate(() =>
    (window as any).__EDITOR_TEST__.changeRemote("remote edit\n"),
  );
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await expect(page.getByRole("dialog")).toContainText("远端文件已发生变化");
  await page.getByRole("button", { name: "继续编辑", exact: true }).click();
  await expect(content).toContainText("local edit");
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await page.getByRole("button", { name: "覆盖远端", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "保存", exact: true }),
  ).toBeDisabled();
  await content.fill("close edit\n");
  await page.getByRole("button", { name: "关闭窗口", exact: true }).click();
  await expect(page.getByRole("dialog")).toContainText("保存修改后关闭");
  await page.getByRole("button", { name: "保存并关闭", exact: true }).click();
  await expect
    .poll(async () => (await editorCalls(page, "plugin:window|destroy")).length)
    .toBe(1);
  expect((await editorCalls(page, "editor_save")).at(-1).args.text).toBe(
    "close edit\n",
  );
});

test("editor baseline: retry read and editing during an in-flight save", async ({
  page,
}) => {
  await setupEditor(page, { loadError: true });
  await page.evaluate(() => (window as any).__EDITOR_TEST__.failLoad(false));
  await page.getByRole("button", { name: "重试读取", exact: true }).click();
  const content = page.getByRole("textbox", { name: "远程文件内容" });
  await content.fill("first edit");
  await page.evaluate(() => (window as any).__EDITOR_TEST__.holdSave());
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await content.fill("second edit");
  await page.evaluate(() => (window as any).__EDITOR_TEST__.releaseSave());
  await expect(
    page.getByRole("button", { name: "保存", exact: true }),
  ).toBeEnabled();
  await expect(page.getByLabel("未保存", { exact: true })).toBeVisible();
  await expect(content).toHaveText("second edit");
  expect((await editorCalls(page, "editor_save"))[0].args.text).toBe(
    "first edit",
  );
});

test("top search navigates, reports results and returns focus without resetting edits", async ({
  page,
}) => {
  const { errors } = await setupEditor(page, { text: "cat cat\ncat\n" });
  const content = page.getByRole("textbox", { name: "远程文件内容" });
  await content.fill("cat cat\ncat dog\n");
  const { panel, field, count } = await openSearch(page, "cat");
  await expect(count).toHaveText("3 个匹配");
  const panelBox = await panel.boundingBox(),
    contentBox = await content.boundingBox();
  expect(panelBox!.y + panelBox!.height).toBeLessThanOrEqual(contentBox!.y + 1);
  await field.press("Enter");
  await expect(count).toHaveText(/^[123] \/ 3$/);
  const first = await count.textContent();
  await field.press("Enter");
  await expect(count).not.toHaveText(first!);
  await field.press("Shift+Enter");
  await expect(count).toHaveText(first!);
  await panel.getByRole("button", { name: "匹配选项", exact: true }).click();
  await panel.getByLabel("区分大小写").focus();
  await page.keyboard.press("Escape");
  await expect(
    panel.getByRole("button", { name: "匹配选项", exact: true }),
  ).toBeFocused();
  await expect(panel).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(panel).toHaveCount(0);
  await expect(content).toBeFocused();
  await expect(content).toContainText("cat dog");
  await expect(page.getByLabel("未保存", { exact: true })).toBeVisible();
  expect(await editorCalls(page, "editor_load")).toHaveLength(1);
  expect(errors).toEqual([]);
});

test("match options, invalid regex and empty queries have distinct accessible states", async ({
  page,
}) => {
  await setupEditor(page, { text: "cat Cat scatter\n中文 中文\n" });
  const { panel, field, count } = await openSearch(page, "cat");
  await expect(count).toHaveText("3 个匹配");
  await panel.getByRole("button", { name: "匹配选项", exact: true }).click();
  await panel.getByLabel("整词匹配").check();
  await expect(count).toHaveText("2 个匹配");
  await panel.getByLabel("区分大小写").check();
  await expect(count).toHaveText("1 个匹配");
  await panel.getByLabel("正则表达式", { exact: true }).check();
  await field.fill("[");
  await expect(field).toHaveAttribute("aria-invalid", "true");
  await expect(
    panel.getByText("请检查正则表达式的括号、转义或量词。"),
  ).toBeVisible();
  await expect(
    panel.getByRole("button", { name: "下一处匹配" }),
  ).toBeDisabled();
  await field.fill("");
  await expect(field).toHaveAttribute("aria-invalid", "false");
  await expect(count).toHaveText("输入查找内容");
  await field.fill("nothing");
  await expect(count).toHaveText("无匹配结果");
  await expect(field).toHaveAttribute("aria-invalid", "false");
  await field.fill("中文");
  await expect(count).toHaveText("2 个匹配");
});

test("replacement uses engine capture groups and remains a single undoable operation", async ({
  page,
}) => {
  const { errors } = await setupEditor(page, {
    text: "port: 8080\nport: 9000\n",
  });
  const content = page.getByRole("textbox", { name: "远程文件内容" });
  const { panel, field, count } = await openSearch(page, "port: (\\d+)");
  await panel.getByRole("button", { name: "匹配选项", exact: true }).click();
  await panel.getByLabel("正则表达式", { exact: true }).check();
  await expect(count).toHaveText("2 个匹配");
  await panel.getByRole("button", { name: "展开替换" }).click();
  const replacement = panel.getByRole("textbox", { name: "替换为" });
  await replacement.fill("listen: $1");
  await panel.getByRole("button", { name: "全部替换", exact: true }).click();
  await expect(content).toContainText("listen: 8080");
  await expect(content).toContainText("listen: 9000");
  await expect(count).toHaveText("无匹配结果");
  await page.getByRole("button", { name: "撤销", exact: true }).click();
  await expect(content).toContainText("port: 8080");
  await expect(content).toContainText("port: 9000");
  await expect(page.getByLabel("未保存", { exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "重做", exact: true }).click();
  await expect(content).toContainText("listen: 8080");
  await field.fill("listen: ");
  await replacement.fill("");
  await expect(count).toHaveText("2 个匹配");
  await panel.getByRole("button", { name: "全部替换", exact: true }).click();
  await expect(content).toHaveText("80809000");
  expect(errors).toEqual([]);
});

test("composition doesn't navigate, replace or close and saving works from search", async ({
  page,
}) => {
  const { errors } = await setupEditor(page, { text: "中文 中文\n" });
  const content = page.getByRole("textbox", { name: "远程文件内容" });
  await content.fill("中文 中文 changed");
  const { panel, field, count } = await openSearch(page, "changed");
  await expect(count).toHaveText("1 个匹配");
  await field.evaluate((el: HTMLInputElement) => {
    el.dispatchEvent(
      new CompositionEvent("compositionstart", { bubbles: true }),
    );
    el.value = "中文";
    el.dispatchEvent(
      new InputEvent("input", { bubbles: true, isComposing: true }),
    );
    el.dispatchEvent(
      new KeyboardEvent("keydown", {
        bubbles: true,
        key: "Enter",
        isComposing: true,
      }),
    );
    el.dispatchEvent(
      new KeyboardEvent("keydown", {
        bubbles: true,
        key: "Escape",
        isComposing: true,
      }),
    );
  });
  await expect(panel).toBeVisible();
  await expect(count).toHaveText("1 个匹配");
  await field.dispatchEvent("compositionend", { data: "中文" });
  await expect(count).toHaveText("2 个匹配");
  await field.press("ControlOrMeta+s");
  await expect
    .poll(async () => (await editorCalls(page, "editor_save")).length)
    .toBe(1);
  await expect(page.getByLabel("未保存", { exact: true })).toHaveCount(0);
  for (let i = 0; i < 3; i++) {
    await panel.getByRole("button", { name: "关闭查找" }).click();
    await openSearch(page, "中文");
  }
  await field.press("ControlOrMeta+s");
  expect(await editorCalls(page, "editor_save")).toHaveLength(1);
  expect(errors).toEqual([]);
});

test("query changes cancel stale counting, the match cap doesn't truncate replace all", async ({
  page,
}) => {
  await setupEditor(page, { text: "cat ".repeat(1100) + "needle" });
  const { panel, field, count } = await openSearch(page, "cat");
  await field.fill("needle");
  await expect(count).toHaveText("1 个匹配");
  await field.fill("cat");
  await expect(count).toHaveText("1000+ 个匹配");
  await panel.getByRole("button", { name: "匹配选项", exact: true }).click();
  await expect(
    panel.getByRole("button", { name: "选中全部匹配" }),
  ).toBeDisabled();
  await expect(panel.getByText(/匹配超过 1000 项/)).toBeVisible();
  await panel.getByRole("button", { name: "展开替换" }).click();
  await panel.getByRole("textbox", { name: "替换为" }).fill("dog");
  await panel.getByRole("button", { name: "全部替换", exact: true }).click();
  // CodeMirror virtualizes even long single lines; verify the saved document,
  // not only the visible DOM slice.
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await expect
    .poll(async () => (await editorCalls(page, "editor_save")).length)
    .toBe(1);
  const text: string = (await editorCalls(page, "editor_save"))[0].args.text;
  expect(text.match(/dog/g)).toHaveLength(1100);
  expect(text).not.toContain("cat");
});

test("read-only reload prevents replacements and discards pending panel work", async ({
  page,
}) => {
  const { errors } = await setupEditor(page, { text: "cat cat" });
  const { panel, count } = await openSearch(page, "cat");
  await expect(count).toHaveText("2 个匹配");
  await panel.getByRole("button", { name: "展开替换" }).click();
  await panel.getByRole("textbox", { name: "替换为" }).fill("dog");
  await page.evaluate(() => (window as any).__EDITOR_TEST__.holdLoad());
  await page.getByRole("button", { name: "重新加载", exact: true }).click();
  await expect(
    panel.getByRole("button", { name: "替换", exact: true }),
  ).toBeDisabled();
  await expect(
    panel.getByRole("button", { name: "全部替换", exact: true }),
  ).toBeDisabled();
  await panel.getByRole("textbox", { name: "替换为" }).press("Enter");
  await expect(page.getByRole("textbox", { name: "远程文件内容" })).toHaveText(
    "cat cat",
  );
  await page.evaluate(() => (window as any).__EDITOR_TEST__.releaseLoad());
  await expect(panel).toHaveCount(0);
  await expect(
    page.getByRole("textbox", { name: "远程文件内容" }),
  ).toHaveAttribute("contenteditable", "true");
  expect(errors).toEqual([]);
});

test("search fields and editor theme stay legible with responsive layout and text scaling", async ({
  page,
}, testInfo) => {
  await setupEditor(page, {
    text: "# Service configuration\nserver:\n  host: 127.0.0.1\n  port: 8080\n\nlogging:\n  level: info\n",
  });
  const { panel, field } = await openSearch(page, "server");
  await panel.getByRole("button", { name: "展开替换" }).click();
  await panel.getByRole("textbox", { name: "替换为" }).fill("service");
  for (const theme of ["light", "dark"] as const) {
    await selectTheme(page, theme);
    for (const width of [1000, 560]) {
      await page.setViewportSize({ width, height: width === 560 ? 360 : 700 });
      await expect(field).toBeInViewport();
      await expect(
        panel.getByRole("button", { name: "关闭查找" }),
      ).toBeInViewport();
      await expect(
        panel.getByRole("textbox", { name: "替换为" }),
      ).toBeInViewport({ ratio: 1 });
      const replacementBox = await panel
        .getByRole("textbox", { name: "替换为" })
        .boundingBox();
      const editorTop = await page.locator(".cm-scroller").boundingBox();
      expect(replacementBox!.y + replacementBox!.height).toBeLessThanOrEqual(
        editorTop!.y + 1,
      );
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      ).toBe(true);
      const colors = await field.evaluate((el) => {
        const c = getComputedStyle(el);
        return {
          bg: c.backgroundColor,
          text: c.color,
          border: c.borderTopColor,
        };
      });
      expect(colors.bg).toBe(
        theme === "dark" ? "rgb(36, 37, 39)" : "rgb(255, 255, 255)",
      );
      expect(colors.text).toBe(
        theme === "dark" ? "rgb(237, 237, 239)" : "rgb(36, 36, 38)",
      );
      const luminance = (color: string) => {
        const c = color
          .match(/[\d.]+/g)!
          .slice(0, 3)
          .map(Number)
          .map((x) => x / 255)
          .map((x) =>
            x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4,
          );
        return c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
      };
      const contrast = (a: string, b: string) => {
        const x = luminance(a),
          y = luminance(b);
        return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
      };
      expect(contrast(colors.text, colors.bg)).toBeGreaterThanOrEqual(4.5);
      expect(contrast(colors.border, colors.bg)).toBeGreaterThanOrEqual(3);
      const matchColors = await page
        .locator(".cm-searchMatch")
        .first()
        .evaluate((el) => {
          const text = el.querySelector("span") ?? el;
          return {
            bg: getComputedStyle(el).backgroundColor,
            text: getComputedStyle(text).color,
          };
        });
      expect(contrast(matchColors.text, matchColors.bg)).toBeGreaterThanOrEqual(
        4.5,
      );
      await page.screenshot({
        path: `.qa/file-editor-${testInfo.project.name}-${theme}-${width}.png`,
      });
    }
  }
  await page.setViewportSize({ width: 1000, height: 700 });
  await panel.getByRole("button", { name: "匹配选项", exact: true }).click();
  await page.evaluate(() => {
    document.documentElement.style.fontSize = "28px";
  });
  await field.focus();
  await expect(field).toBeInViewport();
  await expect(
    panel.getByRole("button", { name: "关闭查找" }),
  ).toBeInViewport();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.screenshot({
    path: `.qa/file-editor-${testInfo.project.name}-scaled.png`,
  });
  await page.emulateMedia({ contrast: "more", reducedMotion: "reduce" });
  await field.focus();
  await expect(field).toHaveCSS("outline-style", "solid");
  await expect(field).toHaveCSS("outline-width", "2px");
  await page.setViewportSize({ width: 560, height: 360 });
  await field.focus();
  await expect(field).toBeInViewport();
  const replacement = panel.getByRole("textbox", { name: "替换为" });
  await replacement.focus();
  await expect(replacement).toBeInViewport();
  const scrollBounds = await page.locator(".cm-scroller").boundingBox();
  expect(scrollBounds!.height).toBeGreaterThan(20);
  expect(await panel.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(
    true,
  );
  expect(await editorCalls(page, "editor_load")).toHaveLength(1);
});

test("search input undo stays local and replacements preserve CRLF on save", async ({
  page,
}) => {
  await setupEditor(page, { text: "cat\r\ncat\r\n", path: "/windows.txt" });
  const content = page.getByRole("textbox", { name: "远程文件内容" });
  const { panel, field, count } = await openSearch(page);
  await field.pressSequentially("query");
  await field.press("ControlOrMeta+z");
  await expect(field).not.toHaveValue("query");
  await expect(content).toHaveText("catcat");
  await expect(page.getByLabel("未保存", { exact: true })).toHaveCount(0);
  await field.fill("cat");
  await expect(count).toHaveText("2 个匹配");
  await panel.getByRole("button", { name: "展开替换" }).click();
  await panel.getByRole("textbox", { name: "替换为" }).fill("dog");
  await panel.getByRole("button", { name: "全部替换", exact: true }).click();
  await field.press("ControlOrMeta+s");
  await expect
    .poll(
      async () => (await editorCalls(page, "editor_save")).at(-1)?.args.text,
    )
    .toBe("dog\r\ndog\r\n");
  await field.press("Escape");
  await content.press("Escape");
  await content.press("Tab");
  await expect(page.getByLabel("文件语言")).toBeFocused();
});

test("large files retain responsive bounded counts and don't rescan on navigation or replacement text", async ({
  page,
}) => {
  const text =
    "# sample line for search performance\n".repeat(26000) + "needle needle";
  expect(new TextEncoder().encode(text).length).toBeLessThan(1024 * 1024);
  await setupEditor(page, { text, path: "/large.txt" });
  const { panel, field, count } = await openSearch(page, "needle");
  await expect(count).toHaveText("2 个匹配");
  await count.evaluate((el) => {
    const w = window as any;
    w.__COUNT_TEXTS__ = [];
    w.__COUNT_OBSERVER__ = new MutationObserver(() =>
      w.__COUNT_TEXTS__.push(el.textContent),
    );
    w.__COUNT_OBSERVER__.observe(el, {
      childList: true,
      characterData: true,
      subtree: true,
    });
  });
  await field.press("Enter");
  await field.press("Enter");
  await panel.getByRole("button", { name: "展开替换" }).click();
  await panel.getByRole("textbox", { name: "替换为" }).fill("replacement");
  const values: string[] = await page.evaluate(() => {
    (window as any).__COUNT_OBSERVER__.disconnect();
    return (window as any).__COUNT_TEXTS__;
  });
  expect(values).not.toContain("正在统计…");
  await field.fill("sample");
  await expect(count).toHaveText("1000+ 个匹配");
  await panel.getByRole("button", { name: "关闭查找" }).click();
  await expect(
    page.getByRole("textbox", { name: "远程文件内容" }),
  ).toBeFocused();
});
