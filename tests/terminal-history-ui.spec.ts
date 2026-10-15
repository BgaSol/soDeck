import { expect, test, type Page } from "@playwright/test";

async function setup(page: Page) {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.addInitScript(() => {
    const w = window as any;
    const source = {
      id: "source-container",
      serverId: "server-a",
      serverName: "生产服务器",
      address: "10.0.0.1:22",
      username: "ops",
      containerName: "api-service",
      containerId: "abc".repeat(21),
      endpoint: "unix:///var/run/docker.sock",
    };
    const sessions = [
      {
        id: "container-1",
        workspaceId: "container-window",
        source,
        startedAt: 1792044000000,
        endedAt: null,
        state: "recording",
        savedSeq: "12000",
        indexedSeq: "12000",
        lastRecord: "12000",
        bytes: 8000000,
        error: null,
      },
      {
        id: "server-1",
        workspaceId: "server-window",
        source: {
          ...source,
          id: "source-server",
          serverName: "预发布服务器",
          containerName: null,
          containerId: null,
        },
        startedAt: 1792040000000,
        endedAt: 1792041000000,
        state: "closed",
        savedSeq: "100",
        indexedSeq: "100",
        lastRecord: "100",
        bytes: 8000,
        error: null,
      },
    ];
    const row = (sessionId: string, n: number) => ({
      id: String(n),
      sessionId,
      seq: String(n),
      offset: 2,
      time: 1792044000000 + n * 1000,
      segment: n > 6000 ? 1 : 0,
      kind: n === 6000 ? "marker" : "text",
      text:
        n === 6000
          ? "屏幕已清除 · 以上为清屏前记录"
          : `历史输出 ${n} 中文 ERROR`,
      ansi: "",
    });
    let serial = 0;
    let channel: any;
    const state = {
      calls: [] as any[],
      sessions,
      emit: (e: any) => channel?.onmessage(e),
      fail: false,
    };
    w.__HISTORY_TEST__ = state;
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: "container-test" },
        currentWebview: { label: "container-test" },
      },
      transformCallback: () => ++serial,
      unregisterCallback: () => {},
      invoke: async (command: string, args: any = {}) => {
        state.calls.push({ command, args });
        if (command === "container_info")
          return { serverName: "生产服务器", containerName: "api-service" };
        if (command === "connect_container") {
          channel = args.output;
          setTimeout(() => {
            channel.onmessage({
              type: "ready",
              connection: {
                target: { name: "生产服务器" },
                hops: [],
                identity: "identity",
              },
            });
            channel.onmessage({
              type: "history",
              sessionId: "container-1",
              sourceId: source.id,
              workspaceId: "container-window",
            });
            channel.onmessage({ type: "data", data: btoa("ops@api:~$ ") });
          }, 10);
          return "container-1";
        }
        if (command === "terminal_clear_display") {
          channel.onmessage({ type: "displayCleared" });
          return null;
        }
        if (command === "terminal_history_status")
          return sessions.find((s) => s.id === args.sessionId);
        if (command === "terminal_history_sessions")
          return structuredClone(sessions);
        if (command === "terminal_history_page") {
          if (state.fail) throw { message: "历史文件暂时无法读取" };
          const q = args.query,
            session = sessions.find((s) => s.id === q.sessionId)!;
          const max = Math.min(
            Number(session.lastRecord),
            Number(q.snapshot ?? session.lastRecord),
          );
          let ids = Array.from({ length: max }, (_, i) => i + 1).filter(
            (id) =>
              id < Number(q.before ?? Infinity) && id > Number(q.after ?? 0),
          );
          const hasMore = ids.length > q.limit;
          ids =
            q.after !== undefined ? ids.slice(0, q.limit) : ids.slice(-q.limit);
          return {
            rows: ids.map((id) => row(session.id, id)),
            hasMore,
            snapshot: String(max),
          };
        }
        if (command === "terminal_history_search") {
          const q = args.query;
          const chosen = sessions.filter(
            (s) =>
              (!q.sessionId || s.id === q.sessionId) &&
              (!q.sourceId || s.source.id === q.sourceId),
          );
          return {
            hits: chosen.map((session) => ({
              session,
              row: row(session.id, session.id === "container-1" ? 15 : 25),
            })),
            cursor: null,
            snapshot: JSON.stringify({
              sessions: chosen.map((s) => [s.id, s.lastRecord]),
            }),
          };
        }
        if (command === "terminal_history_copy")
          return `copied ${args.from}-${args.to}`;
        if (command === "terminal_history_export")
          return "C:\\exports\\history.txt";
        if (command === "terminal_history_replay")
          return {
            events: [
              { seq: "1", time: 0, kind: "resize", cols: 100, rows: 30 },
              {
                seq: "2",
                time: 1,
                kind: "output",
                data: btoa("historical screen"),
              },
            ],
            after: args.through ?? "2",
            hasMore: false,
          };
        if (command === "list_servers" || command === "list_transfers")
          return [];
        if (command === "plugin:event|listen") return ++serial;
        return null;
      },
    };
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
  });
  return errors;
}
test("history preserves terminal instance, geometry and input ownership", async ({
  page,
}) => {
  await page.setViewportSize({ width: 640, height: 580 });
  const errors = await setup(page);
  await page.goto("/?container=1");
  await expect(page.getByText("正在保存历史", { exact: true })).toBeVisible();
  await page.locator(".xterm-helper-textarea").fill("abc");
  const before = await page.evaluate(() => {
    const state = (window as any).__HISTORY_TEST__;
    (window as any).__terminalNode =
      document.querySelector(".terminal-surface");
    return state.calls.filter((c: any) => c.command === "terminal_resize")
      .length;
  });
  await page.getByRole("button", { name: "历史", exact: true }).click();
  await expect(page.getByLabel("搜索终端历史")).toBeVisible();
  await page.getByLabel("搜索终端历史").fill("中文");
  await page.evaluate(() =>
    (window as any).__HISTORY_TEST__.emit({
      type: "data",
      data: btoa("background\r\n"),
    }),
  );
  await expect(page.getByText("实时终端有新输出")).toBeVisible();
  expect(
    await page.evaluate(
      () =>
        document.querySelector(".terminal-surface") ===
        (window as any).__terminalNode,
    ),
  ).toBe(true);
  expect(
    await page.evaluate(
      () =>
        (window as any).__HISTORY_TEST__.calls.filter(
          (c: any) => c.command === "terminal_resize",
        ).length,
    ),
  ).toBe(before);
  expect(
    await page.evaluate(
      () =>
        (window as any).__HISTORY_TEST__.calls.filter(
          (c: any) => c.command === "close_connection",
        ).length,
    ),
  ).toBe(0);
  expect(
    await page.evaluate(() =>
      (window as any).__HISTORY_TEST__.calls
        .filter((c: any) => c.command === "terminal_input")
        .some((c: any) => c.args.data.includes("中文")),
    ),
  ).toBe(false);
  await page.getByRole("button", { name: "返回实时终端" }).click();
  await expect(page.locator(".xterm-helper-textarea")).toBeFocused();
  await page.getByRole("button", { name: "清理显示", exact: true }).click();
  await expect(page.getByText("已清理 · 查看此前记录")).toBeVisible();
  expect(errors).toEqual([]);
});
test("offline global search reaches old records and opens bounded context", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.goto("/?terminal-history=1");
  await expect(
    page.getByRole("heading", { name: "终端历史", exact: true }),
  ).toBeVisible();
  await page.getByLabel("搜索终端历史").fill("中文");
  await page.getByRole("button", { name: "搜索", exact: true }).click();
  await expect(page.locator(".history-search-hit")).toHaveCount(2);
  await expect(page.locator(".history-search-hit").last()).toContainText(
    "预发布服务器",
  );
  await page
    .getByRole("button", { name: "上下文", exact: true })
    .first()
    .click();
  await expect(page.locator('[data-history-id="15"]')).toBeVisible();
  await expect(page.getByLabel("历史会话")).toHaveValue("container-1");
  await page
    .locator('[data-history-id="15"]')
    .getByRole("button", { name: "回放" })
    .click();
  await expect(
    page.getByRole("dialog", { name: "历史屏幕回放" }),
  ).toBeVisible();
  await expect(page.getByText("只读回放 · 已定位到历史记录")).toBeVisible();
  expect(
    await page.evaluate(() =>
      (window as any).__HISTORY_TEST__.calls.some(
        (c: any) => c.command === "terminal_input",
      ),
    ),
  ).toBe(false);
  await page.getByRole("button", { name: "关闭回放" }).click();
  await page.getByRole("button", { name: "导出原始记录" }).click();
  await expect(
    page.getByText("已导出到 C:\\exports\\history.txt"),
  ).toBeVisible();
  expect(errors).toEqual([]);
});
test("virtualized history pages past the old 5000-line limit and shows failures", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.goto("/?terminal-history=1");
  await expect(page.locator(".terminal-history-row").first()).toBeVisible();
  for (let n = 0; n < 13; n++) {
    await page.getByRole("button", { name: "更早记录" }).click();
    await expect(page.getByRole("button", { name: "更早记录" })).toBeEnabled();
  }
  const before = await page.evaluate(() =>
    Math.min(
      ...(window as any).__HISTORY_TEST__.calls
        .filter(
          (c: any) =>
            c.command === "terminal_history_page" && c.args.query.before,
        )
        .map((c: any) => Number(c.args.query.before)),
    ),
  );
  expect(before).toBeLessThan(7000);
  expect(await page.locator(".terminal-history-row").count()).toBeLessThan(100);
  await page.evaluate(() => {
    (window as any).__HISTORY_TEST__.fail = true;
  });
  await page.getByRole("button", { name: "更早记录" }).click();
  await expect(page.getByRole("alert")).toContainText("历史文件暂时无法读取");
  await page.setViewportSize({ width: 680, height: 540 });
  await page.screenshot({ path: ".qa/terminal-history-narrow.png" });
  expect(errors).toEqual([]);
});

test("clearing during full screen is disabled and history restores its reading position", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.goto("/?container=1");
  const emit = async (data: string) =>
    page.evaluate(
      (data) =>
        (window as any).__HISTORY_TEST__.emit({
          type: "data",
          data: btoa(data),
        }),
      data,
    );
  await expect(page.getByText("正在保存历史", { exact: true })).toBeVisible();
  await emit("\x1b[?1049hfullscreen");
  await expect(
    page.getByRole("button", { name: "清理显示", exact: true }),
  ).toBeDisabled();
  await emit("\x1b[?1049l");
  await expect(
    page.getByRole("button", { name: "清理显示", exact: true }),
  ).toBeEnabled();
  await page.getByRole("button", { name: "历史", exact: true }).click();
  await page.getByRole("button", { name: "更早记录" }).click();
  await expect(page.getByRole("button", { name: "更早记录" })).toBeEnabled();
  const rowId = await page
    .locator("[data-history-id]")
    .first()
    .getAttribute("data-history-id");
  await page.getByRole("button", { name: "返回实时终端" }).click();
  await page.getByRole("button", { name: "历史", exact: true }).click();
  await expect(page.locator(`[data-history-id="${rowId}"]`)).toBeVisible();
  expect(errors).toEqual([]);
});
