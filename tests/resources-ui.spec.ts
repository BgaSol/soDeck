import { test, expect, type Page } from "@playwright/test";
import { setup, row } from "./resource-fixture";
const overview = (page: Page) =>
  page.getByRole("region", { name: "服务器资源监控", exact: true });
const docker = (page: Page) =>
  page.getByRole("region", { name: "Docker 引擎与对象" });
async function sendContainers(
  page: Page,
  count: number,
  patch: Record<string, unknown> = {},
) {
  await page.evaluate(
    ({ count, patch }) => {
      const metric = (value: any) => ({
        value,
        status: "ready",
        updatedAt: Date.now(),
        error: null,
      });
      (window as any).__MOUNTS__.sendDocker({
        containers: metric(
          Array.from({ length: count }, (_, i) => ({
            id: `id-${i}`,
            name: `container-${String(i).padStart(3, "0")}`,
            image: "registry.example/service:stable",
            state:
              i % 3 === 0 ? "running" : i % 3 === 1 ? "exited" : "restarting",
            status: i % 3 === 0 ? "Up" : "Waiting",
          })),
        ),
        ...patch,
      });
    },
    { count, patch },
  );
}
test("monitor overview contains only server metrics and storage", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.setViewportSize({ width: 1280, height: 800 });
  const panel = overview(page);
  await expect(panel.locator('[data-resource="cpu"]')).toContainText("23.4%");
  await expect(panel.locator('[data-resource="memory"]')).toContainText(
    "37.5%",
  );
  await expect(panel.locator('[data-resource="network"]')).toContainText(
    "2.4 MiB/s",
  );
  await expect(
    panel.getByRole("button", { name: "查看 / 挂载详情", exact: true }),
  ).toBeInViewport();
  await expect(
    panel.getByRole("region", { name: "SoDeck 通信", exact: true }),
  ).toHaveCount(0);
  await expect(page.getByRole("button", { name: "展开检查器" })).toHaveCount(0);
  await expect(
    page.getByRole("region", { name: "Docker 摘要", exact: true }),
  ).toHaveCount(0);
  const geometry = await panel.evaluate((el) => ({
    width: el.clientWidth,
    contentHeight: el.querySelector(".resource-scroll")!.scrollHeight,
  }));
  expect(geometry.width).toBeLessThanOrEqual(360);
  expect(geometry.contentHeight).toBeLessThanOrEqual(650);
  await page.screenshot({ path: ".qa/overview-1280-light.png" });
  expect(errors).toEqual([]);
});

test("opening a monitor window passes the current metric without changing the terminal layout", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.getByRole("button", { name: "查看内存详情", exact: true }).click();
  await expect(
    overview(page).getByRole("region", { name: "指标详情" }),
  ).toContainText("Swap");
  await overview(page)
    .getByRole("button", { name: "15 分钟", exact: true })
    .click();
  const before = await page.locator(".workspace-terminal").boundingBox();
  await page
    .getByRole("button", { name: "在独立窗口打开", exact: true })
    .click();
  const calls = await page.evaluate(() => (window as any).__MOUNTS__.calls);
  expect(
    calls.find((call: any) => call.command === "open_monitor_window").args,
  ).toMatchObject({
    sessionId: "mount-session",
    preferences: { selectedMetric: "memory", minutes: 15 },
  });
  expect(await page.locator(".workspace-terminal").boundingBox()).toEqual(
    before,
  );
  await expect(page.locator(".workspace-terminal")).toBeVisible();
  await page.screenshot({ path: ".qa/monitor-sidebar-detail.png" });
  await page.evaluate(() => {
    const w = window as any,
      original = w.__TAURI_INTERNALS__.invoke;
    w.__TAURI_INTERNALS__.invoke = (command: string, args: any) =>
      command === "open_monitor_window"
        ? Promise.reject({ message: "无法创建监控窗口" })
        : original(command, args);
  });
  await page
    .getByRole("button", { name: "在独立窗口打开", exact: true })
    .click();
  await expect(overview(page).getByRole("alert")).toContainText(
    "无法创建监控窗口",
  );
  await expect(page.locator(".workspace-terminal")).toBeVisible();
  expect(errors).toEqual([]);
});

test("monitor window retains samples across disconnect and fits a narrow dark window", async ({
  page,
}) => {
  const errors = await setup(page, "monitor");
  await page.evaluate(() =>
    (window as any).__MOUNTS__.monitor.onmessage({
      type: "connection",
      generation: "monitor-generation",
      connected: false,
    }),
  );
  await expect(overview(page)).toContainText("已断开 · 保留最后采样");
  await expect(overview(page)).toContainText("23.4%");
  await expect(
    page.getByRole("button", { name: "刷新资源监控" }),
  ).toBeDisabled();
  await page.evaluate(() => {
    const fixture = (window as any).__MOUNTS__;
    fixture.monitor.onmessage({
      type: "connection",
      generation: "monitor-generation",
      connected: true,
    });
    fixture.sendMetrics();
  });
  await expect(
    page.getByRole("button", { name: "刷新资源监控" }),
  ).toBeEnabled();
  await expect(overview(page)).not.toContainText("已断开 · 保留最后采样");
  await page.setViewportSize({ width: 440, height: 540 });
  await page.emulateMedia({
    colorScheme: "dark",
    reducedMotion: "reduce",
    contrast: "more",
  });
  await page.evaluate(() => {
    document.documentElement.style.fontSize = "17px";
  });
  expect(
    await overview(page).evaluate((el) => el.scrollWidth <= el.clientWidth),
  ).toBe(true);
  await page.screenshot({ path: ".qa/monitor-window-narrow-dark.png" });
  const calls = await page.evaluate(() => (window as any).__MOUNTS__.calls);
  expect(
    calls.filter((call: any) => call.command === "subscribe_monitor_window"),
  ).toHaveLength(1);
  expect(
    calls.filter((call: any) => call.command === "open_connection"),
  ).toHaveLength(0);
  expect(errors).toEqual([]);
});

test("communication has an independent range and a bounded keyboard-accessible popover", async ({
  page,
}) => {
  const errors = await setup(page);
  await overview(page).getByRole("button", { name: "1 小时" }).click();
  const summary = page.getByRole("group", { name: "本机通信摘要" });
  await expect(summary).toContainText("20.0 KiB/s");
  await expect(summary).toContainText("累计");
  await expect(page.getByRole("group", { name: "会话连接摘要" })).toContainText(
    "UTF-8",
  );
  await expect(page.getByRole("dialog")).toHaveCount(0);
  const terminal = page.locator(".terminal-surface");
  const before = await terminal.boundingBox();
  await page.getByRole("button", { name: "通信详情", exact: true }).click();
  expect(await terminal.boundingBox()).toEqual(before);
  await expect(
    page.getByRole("img", { name: /SoDeck 通信趋势/ }),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: "返回连接详情" })).toHaveCount(
    0,
  );
  await page.screenshot({ path: ".qa/communication-direct-light.png" });
  await page
    .getByRole("group", { name: "通信时间范围" })
    .getByRole("button", { name: "15 分钟" })
    .click();
  await expect(
    overview(page).getByRole("button", { name: "1 小时" }),
  ).toHaveAttribute("aria-pressed", "true");
  await page.setViewportSize({ width: 650, height: 480 });
  await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" });
  const popover = page.getByRole("dialog", { name: "通信详情" });
  await expect(page.locator(".responsive-workspace")).toHaveAttribute(
    "data-layout",
    "single",
  );
  await expect
    .poll(async () => {
      const bounds = await popover.boundingBox();
      return (
        !!bounds &&
        bounds.x >= 0 &&
        bounds.y >= 0 &&
        bounds.x + bounds.width <= 650 &&
        bounds.y + bounds.height <= 480
      );
    })
    .toBe(true);
  const box = await popover.boundingBox();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.y).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(650);
  expect(box!.y + box!.height).toBeLessThanOrEqual(480);
  const footer = await page.locator(".session-statusbar").boundingBox();
  expect(box!.y + box!.height).toBeLessThan(footer!.y);
  expect(
    await page
      .locator(".session-statusbar")
      .evaluate((el) => el.scrollWidth <= el.clientWidth),
  ).toBe(true);
  await page.screenshot({ path: ".qa/connection-traffic-narrow-dark.png" });
  await page.keyboard.press("Escape");
  await expect(popover).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "通信详情", exact: true }),
  ).toBeFocused();
  await page.getByRole("button", { name: "通信详情", exact: true }).click();
  await expect(
    page
      .getByRole("group", { name: "通信时间范围" })
      .getByRole("button", { name: "15 分钟" }),
  ).toHaveAttribute("aria-pressed", "true");
  await page.getByRole("button", { name: "关闭通信详情" }).click();
  await expect(
    page.getByRole("button", { name: "通信详情", exact: true }),
  ).toBeFocused();
  await page.getByRole("button", { name: "通信详情", exact: true }).click();
  await page.getByRole("button", { name: "通信详情", exact: true }).click();
  await expect(popover).toHaveCount(0);
  await page.getByRole("button", { name: "通信详情", exact: true }).click();
  await page
    .locator(".terminal-surface")
    .click({ position: { x: 20, y: 15 } });
  await expect(popover).toHaveCount(0);
  await page.evaluate(() => {
    (window as any).__MOUNTS__.monitor.onmessage({
      type: "connection",
      generation: "monitor-generation",
      connected: false,
    });
  });
  await expect(summary).toContainText("最后数据");
  await expect(summary).toContainText("20.0 KiB/s");
  await page.getByRole("button", { name: "通信详情", exact: true }).click();
  await expect(popover).toContainText("已断开 · 最后数据");
  await page.getByRole("button", { name: "关闭通信详情" }).click();
  await page.setViewportSize({ width: 440, height: 540 });
  await page.evaluate(() => {
    document.documentElement.style.fontSize = "17px";
  });
  await expect(summary).toBeVisible();
  expect(
    await page
      .locator(".session-statusbar")
      .evaluate((el) => el.scrollWidth <= el.clientWidth),
  ).toBe(true);
  await page.screenshot({ path: ".qa/connection-summary-narrow-dark.png" });
  expect(errors).toEqual([]);
});
test("communication retains its scope and does not depend on resource or Docker success", async ({
  page,
}) => {
  const errors = await setup(page);
  const traffic = page.getByRole("region", {
    name: "SoDeck 通信",
    exact: true,
  });
  await page.getByRole("button", { name: "通信详情", exact: true }).click();
  await expect(traffic).toContainText("20.0 KiB/s");
  await traffic.getByRole("button", { name: "其中：监控查询" }).click();
  await expect(traffic).toContainText("128 B/s");
  await page.evaluate(() => {
    const f = (window as any).__MOUNTS__;
    f.sendDocker({
      engine: {
        value: {
          state: "permission",
          endpoint: "unix:///var/run/docker.sock",
          message: "Docker 权限不足",
        },
        status: "ready",
        updatedAt: Date.now(),
        error: null,
      },
      summary: {
        value: null,
        status: "unavailable",
        updatedAt: null,
        error: "Docker 权限不足",
      },
    });
    f.sendMetrics(
      {
        cpu: {
          value: null,
          status: "unsupported",
          updatedAt: null,
          error: "不支持资源采样",
        },
      },
      true,
    );
    f.sendTraffic();
  });
  await expect(traffic).toContainText("2.0 KiB/s");
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("button", { name: "通信详情", exact: true }),
  ).toBeFocused();
  await page.getByRole("tab", { name: "Docker", exact: true }).click();
  await page.getByRole("tab", { name: "监控", exact: true }).click();
  await page.getByRole("button", { name: "通信详情", exact: true }).click();
  await expect(
    traffic.getByRole("button", { name: "其中：监控查询" }),
  ).toHaveAttribute("aria-pressed", "true");
  await traffic.screenshot({ path: ".qa/communication-light.png" });
  expect(errors).toEqual([]);
});
test("independent monitor charts share a frozen actual sample, keyboard reading and interface selection", async ({
  page,
}) => {
  const errors = await setup(page, "monitor");
  const panel = overview(page),
    chart = panel.getByRole("img", { name: /CPU趋势/ });
  await chart.focus();
  await chart.press("End");
  await expect(panel.locator(".resource-reading")).toContainText("已固定");
  const captions = panel.locator(".resource-metrics .metric-chart-caption");
  const times = await captions.evaluateAll((els) =>
    els.map((el) => el.firstElementChild?.textContent),
  );
  expect(new Set(times).size).toBe(1);
  const before = (await captions.first().textContent())!;
  await page.evaluate(() => (window as any).__MOUNTS__.sendMetrics());
  await expect(captions.first()).toHaveText(before);
  await panel.getByRole("button", { name: "查看服务器网络详情" }).click();
  await panel
    .getByRole("combobox", { name: "监控网卡" })
    .selectOption("docker0");
  await expect(panel.locator(".resource-reading")).not.toContainText("已固定");
  await expect(panel.locator('[data-resource="network"]')).toContainText(
    "128 B/s",
  );
  await page.evaluate(() =>
    (window as any).__MOUNTS__.sendMetrics({
      network: {
        value: [],
        status: "ready",
        updatedAt: Date.now(),
        error: null,
      },
    }),
  );
  await expect(panel.getByRole("combobox", { name: "监控网卡" })).toHaveValue(
    "docker0",
  );
  await expect(panel).toContainText("保留原有选择");
  await panel.getByRole("button", { name: "查看CPU详情", exact: true }).click();
  await expect(panel.getByRole("region", { name: "指标详情" })).toContainText(
    "最新核心采样",
  );
  await expect(panel).toContainText("cpu0");
  await panel.getByRole("button", { name: "1 小时", exact: true }).click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).__MOUNTS__.calls
            .filter((c: any) => c.command === "get_monitor_history")
            .at(-1).args.minutes,
      ),
    )
    .toBe(60);
  await page.screenshot({ path: ".qa/monitor-expanded-light.png" });
  expect(errors).toEqual([]);
});
test("sidebar details and narrow regions keep the same terminal and subscription", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.evaluate(() => {
    (window as any).__terminalNode =
      document.querySelector(".terminal-surface");
  });
  for (let i = 0; i < 3; i++) {
    await page
      .getByRole("button", { name: "查看 CPU 监控", exact: true })
      .click();
    await expect(page.locator(".workspace-terminal")).toBeVisible();
    await expect(page.locator(".workspace-terminal")).not.toHaveAttribute(
      "inert",
    );
    await page.evaluate(
      (iteration) =>
        (window as any).__MOUNTS__.terminal.onmessage({
          type: "data",
          data: btoa(`output-during-focus-${iteration}\r\n`),
        }),
      i,
    );

    await page.getByRole("button", { name: "返回监控", exact: true }).click();
    await expect(
      page.getByRole("button", { name: "查看CPU详情", exact: true }),
    ).toBeFocused();
  }
  for (const [width, height] of [
    [1440, 900],
    [1280, 800],
    [860, 580],
    [650, 480],
  ]) {
    await page.setViewportSize({ width, height });
    await expect
      .poll(() =>
        page
          .locator(".responsive-workspace")
          .evaluate((el) => el.scrollWidth <= el.clientWidth),
      )
      .toBe(true);
    if (width < 760) {
      await page
        .getByRole("navigation", { name: "工作区域" })
        .getByRole("button", { name: "终端", exact: true })
        .click();
      await expect(overview(page)).toBeHidden();
      await page
        .getByRole("navigation", { name: "工作区域" })
        .getByRole("button", { name: "监控与 Docker" })
        .click();
      await expect(overview(page)).toBeVisible();
      await page
        .getByRole("navigation", { name: "工作区域" })
        .getByRole("button", { name: "文件", exact: true })
        .click();
      await expect(row(page, "/data")).toBeVisible();
      await page
        .getByRole("navigation", { name: "工作区域" })
        .getByRole("button", { name: "终端", exact: true })
        .click();
    }
    await page.screenshot({ path: `.qa/workspace-${width}.png` });
  }
  expect(
    await page.evaluate(
      () =>
        (window as any).__terminalNode ===
        document.querySelector(".terminal-surface"),
    ),
  ).toBe(true);
  await expect(page.locator(".terminal-surface")).toContainText(
    "output-during-focus-2",
  );
  const calls = await page.evaluate(() => (window as any).__MOUNTS__.calls);
  expect(
    calls.filter((c: any) => c.command === "open_connection"),
  ).toHaveLength(1);
  expect(
    calls.filter((c: any) => c.command === "subscribe_monitor"),
  ).toHaveLength(1);
  expect(
    calls
      .filter((c: any) => c.command === "terminal_resize")
      .every((c: any) => c.args.cols > 0 && c.args.rows > 0),
  ).toBe(true);
  expect(
    calls.filter((c: any) =>
      ["detect_docker", "docker_images", "docker_containers"].includes(
        c.command,
      ),
    ),
  ).toHaveLength(0);
  expect(errors).toEqual([]);
});
test("Docker virtualizes 200 objects, supports keyboard and preserves filtered or deleted selections", async ({
  page,
}) => {
  const errors = await setup(page);
  await sendContainers(page, 200);
  await page.getByRole("tab", { name: "Docker", exact: true }).click();
  const panel = docker(page),
    list = panel.getByRole("listbox", { name: "容器列表" });
  await expect(list.getByRole("option").first()).toContainText("container-000");
  expect(await list.getByRole("option").count()).toBeLessThan(40);
  await list.getByRole("option").first().focus();
  await list.getByRole("option").first().press("End");
  await expect(
    list.getByRole("option").filter({ hasText: "container-199" }),
  ).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(
    panel.getByRole("region", { name: "Docker 对象详情" }),
  ).toContainText("container-199");
  await sendContainers(page, 5);
  await expect(panel).toContainText("此对象已从引擎移除");
  await expect(
    panel.getByRole("button", { name: "进入 container-199 终端" }),
  ).toBeDisabled();
  await panel.getByRole("button", { name: "返回列表" }).click();
  await panel.getByRole("textbox", { name: "搜索容器" }).fill("container-001");
  await expect(list.getByRole("option")).toHaveCount(1);
  await panel.getByRole("textbox", { name: "搜索容器" }).fill("no-match");
  await expect(panel).toContainText("没有匹配的对象");
  await panel.getByRole("button", { name: "清除 Docker 搜索" }).click();
  await panel.getByRole("button", { name: /^镜像/ }).click();
  await expect(panel.getByRole("listbox", { name: "镜像列表" })).toContainText(
    "nginx:alpine",
  );
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).__MOUNTS__.calls
            .filter((c: any) => c.command === "update_monitor_interest")
            .at(-1)?.args.interest.docker.images,
      ),
    )
    .toBe(true);
  await page.getByRole("tab", { name: "监控", exact: true }).click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).__MOUNTS__.calls
            .filter((c: any) => c.command === "update_monitor_interest")
            .at(-1)?.args.interest.docker.images,
      ),
    )
    .toBe(false);
  expect(errors).toEqual([]);
});
test("Docker keeps row positions until explicit sort or refresh, and health unknown stays unknown", async ({
  page,
}) => {
  const errors = await setup(page);
  await sendContainers(page, 5);
  await page.getByRole("tab", { name: "Docker", exact: true }).click();
  const panel = docker(page),
    list = panel.getByRole("listbox", { name: "容器列表" });
  await panel
    .getByRole("combobox", { name: "Docker 排序" })
    .selectOption("attention");
  await expect(list.getByRole("option").first()).toContainText("container-002");
  await page.evaluate(() => {
    const metric = (value: any) => ({
      value,
      status: "ready",
      updatedAt: Date.now(),
      error: null,
    });
    (window as any).__MOUNTS__.sendDocker({
      containers: metric([
        {
          id: "id-0",
          name: "container-000",
          image: "nginx",
          state: "dead",
          status: "Dead",
        },
        {
          id: "id-2",
          name: "container-002",
          image: "nginx",
          state: "running",
          status: "Up",
        },
      ]),
      summary: metric({
        total: 2,
        byState: { dead: 1, running: 1 },
        unhealthy: null,
        healthError: "health unavailable",
      }),
    });
  });
  await expect(list.getByRole("option").first()).toContainText("container-002");
  await expect(panel).toContainText("健康统计暂不可用");
  await panel
    .getByRole("combobox", { name: "Docker 排序" })
    .selectOption("name");
  await expect(list.getByRole("option").first()).toContainText("container-000");
  await list.getByRole("option").first().click();
  await expect(
    panel.getByRole("button", { name: "进入 container-000 终端" }),
  ).toBeDisabled();
  await expect(
    panel.getByRole("button", { name: "查看 container-000 日志" }),
  ).toBeEnabled();
  await sendContainers(page, 0);
  await panel.getByRole("button", { name: "返回列表" }).click();
  await expect(panel).toContainText("还没有容器");
  expect(errors).toEqual([]);
});
test("container operations are independent and late failures from old engines are ignored", async ({
  page,
}) => {
  const errors = await setup(page);
  await sendContainers(page, 5);
  await page.evaluate(() => {
    const w = window as any,
      original = w.__TAURI_INTERNALS__.invoke;
    w.pending = {};
    w.__TAURI_INTERNALS__.invoke = (command: string, args: any) =>
      ["open_container", "open_container_logs"].includes(command)
        ? new Promise((resolve, reject) => {
            w.pending[`${args.target.id}/${command}`] = { resolve, reject };
          })
        : original(command, args);
  });
  await page.getByRole("tab", { name: "Docker", exact: true }).click();
  const panel = docker(page),
    list = panel.getByRole("listbox", { name: "容器列表" });
  await list.getByRole("option").filter({ hasText: "container-000" }).click();
  await panel.getByRole("button", { name: "查看 container-000 日志" }).click();
  await expect(
    panel.getByRole("button", { name: "进入 container-000 终端" }),
  ).toBeEnabled();
  await panel.getByRole("button", { name: "返回列表" }).click();
  await list.getByRole("option").filter({ hasText: "container-003" }).click();
  await panel.getByRole("button", { name: "查看 container-003 日志" }).click();
  await page.evaluate(() => {
    (window as any).pending["id-3/open_container_logs"].reject({
      message: "container-003 logs failed",
    });
  });
  await expect(panel).toContainText("container-003 logs failed");
  await panel.getByRole("button", { name: "返回列表" }).click();
  await list.getByRole("option").filter({ hasText: "container-000" }).click();
  await expect(panel).not.toContainText("container-003 logs failed");
  await expect(
    panel.getByRole("button", { name: "查看 container-000 日志" }),
  ).toBeDisabled();
  await sendContainers(page, 5, { engineGeneration: 2 });
  await page.evaluate(() => {
    (window as any).pending["id-0/open_container_logs"].reject({
      message: "old engine failure",
    });
  });
  await expect(panel.getByRole("listbox")).toBeVisible();
  await expect(panel).not.toContainText("old engine failure");
  expect(errors).toEqual([]);
});
test("Docker details stay inside the sidebar at every width", async ({
  page,
}) => {
  const errors = await setup(page);
  await sendContainers(page, 5);
  await page.getByRole("tab", { name: "Docker", exact: true }).click();
  const panel = docker(page);
  await panel.getByRole("listbox").getByRole("option").first().click();
  await expect(panel.getByRole("listbox")).toBeHidden();
  await expect(page.locator(".workspace-terminal")).toBeVisible();
  await page.screenshot({ path: ".qa/docker-detail-light.png" });
  await page.setViewportSize({ width: 650, height: 480 });
  await page
    .getByRole("navigation", { name: "工作区域" })
    .getByRole("button", { name: "监控与 Docker" })
    .click();
  await expect(panel.getByRole("listbox")).toBeHidden();
  await expect(panel.getByRole("button", { name: "返回列表" })).toBeVisible();
  await page.screenshot({ path: ".qa/docker-detail-narrow.png" });
  await panel.getByRole("button", { name: "返回列表" }).click();
  await expect(panel.getByRole("listbox")).toBeVisible();
  expect(errors).toEqual([]);
});
test("mount pinning, search, current mount identity and file reveal share the existing cache", async ({
  page,
}) => {
  const errors = await setup(page);
  const disk = page.getByRole("region", { name: "磁盘总览" });
  await disk.getByRole("button", { name: /全部 \d+ 个/ }).click();
  await disk.getByRole("button", { name: "关注 /data", exact: true }).click();
  await disk.getByRole("button", { name: "关注 /backup", exact: true }).click();
  await disk
    .getByRole("button", { name: "关注 /data/archive", exact: true })
    .click();
  await expect(disk).toContainText("最多关注 3 个");
  await disk.getByRole("textbox", { name: "搜索挂载路径或来源" }).fill("nas:");
  await expect(disk.locator(".disk-overview-row")).toHaveCount(1);
  await disk.getByRole("button", { name: "返回关注" }).click();
  await expect(
    disk.getByRole("button", { name: "查看 /data 挂载详情", exact: true }),
  ).toContainText("68%");
  await page.evaluate(() => {
    (window as any).__MOUNTS__.failUsage = true;
  });
  await page.getByRole("button", { name: "刷新资源监控" }).click();
  await expect(
    disk.getByRole("button", { name: "查看 /data 挂载详情", exact: true }),
  ).toContainText("上次数据");
  await expect(row(page, "/data").locator(".mount-usage")).toContainText("68%");
  await disk
    .getByRole("button", { name: "查看 /data 挂载详情", exact: true })
    .click();
  await expect(page.getByRole("dialog", { name: "挂载详情" })).toContainText(
    "网络挂载查询超时",
  );
  await page.getByRole("button", { name: "在远程文件中定位" }).click();
  await expect(row(page, "/data")).toHaveAttribute("aria-selected", "true");
  expect(
    await page.evaluate(() =>
      (window as any).__MOUNTS__.calls.filter(
        (c: any) => c.command === "terminal_input",
      ),
    ),
  ).toHaveLength(0);
  await page.evaluate(() => {
    const f = (window as any).__MOUNTS__;
    f.table = {
      ...f.table,
      version: "remount",
      mounts: f.table.mounts.map((m: any) =>
        m.target === "/data" ? { ...m, key: "data-new" } : m,
      ),
    };
    f.push();
  });
  await expect(
    disk.getByRole("button", { name: "查看 /data 挂载详情", exact: true }),
  ).not.toContainText("68%");
  expect(errors).toEqual([]);
});
test("stale metrics remain explicit in dark, reduced-motion and larger-text layouts", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.emulateMedia({
    colorScheme: "dark",
    reducedMotion: "reduce",
    contrast: "more",
  });
  await page.setViewportSize({ width: 860, height: 580 });
  await page.evaluate(() => {
    document.documentElement.style.fontSize = "17px";
    (window as any).__MOUNTS__.sendMetrics(
      {
        cpu: {
          value: { percent: 23.4, cores: {}, load: null },
          status: "stale",
          updatedAt: Date.now() - 30000,
          error: "资源查询超时",
        },
        memory: {
          value: null,
          status: "unsupported",
          updatedAt: null,
          error: "缺少 MemAvailable",
        },
      },
      true,
    );
  });
  await expect(overview(page)).toContainText("资源查询超时");
  await expect(overview(page)).toContainText("缺少 MemAvailable");
  await expect(
    page.getByRole("button", { name: "查看 CPU 监控", exact: true }),
  ).toHaveAttribute("title", /上次采样/);
  expect(
    await overview(page).evaluate((el) => el.scrollWidth <= el.clientWidth),
  ).toBe(true);
  await page.screenshot({ path: ".qa/overview-dark-large-text.png" });
  expect(errors).toEqual([]);
});

test("mount groups paginate many entries, request visible capacities and report reveal failures", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.evaluate(() => {
    const f = (window as any).__MOUNTS__;
    const mount = (target: string, fsType = "ext4") => ({
      key: target,
      target,
      fsType,
      source: fsType,
      fsRoot: "/",
      readOnly: false,
    });
    f.table = {
      version: "many",
      status: "ready",
      mounts: [
        mount("/"),
        ...Array.from({ length: 24 }, (_, i) =>
          mount(`/volumes/disk-${String(i).padStart(2, "0")}`),
        ),
        mount("/proc", "proc"),
        mount("/dev/shm", "tmpfs"),
        mount("/run/netns/test", "nsfs"),
        mount("/snap/core/1", "squashfs"),
        mount("/var/lib/docker/overlay2/x/merged", "overlay"),
      ],
    };
    f.push();
  });
  const disk = page.getByRole("region", { name: "磁盘总览" });
  await disk.getByRole("button", { name: /全部 \d+ 个/ }).click();
  await expect(disk.locator(".disk-overview-row")).toHaveCount(8);
  await expect(disk).toContainText("1 / 4 · 25 项");
  await disk.getByRole("button", { name: "下一页挂载" }).click();
  await expect(disk).toContainText("2 / 4 · 25 项");
  await disk
    .getByRole("textbox", { name: "搜索挂载路径或来源" })
    .fill("disk-20");
  await expect(disk.locator(".disk-overview-row")).toHaveCount(1);
  await disk.locator(".disk-overview-row").scrollIntoViewIfNeeded();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).__MOUNTS__.calls
            .filter((c: any) => c.command === "update_monitor_interest")
            .at(-1)?.args.interest.mountPaths,
      ),
    )
    .toEqual(expect.arrayContaining(["/volumes/disk-20"]));
  const visibleMounts = await page.evaluate(
    () =>
      (window as any).__MOUNTS__.calls
        .filter((c: any) => c.command === "update_monitor_interest")
        .at(-1).args.interest.mountPaths,
  );
  expect(
    visibleMounts.filter((path: string) => path.startsWith("/volumes/")),
  ).toEqual(["/volumes/disk-20"]);
  await disk.getByRole("textbox", { name: "搜索挂载路径或来源" }).fill("");
  await disk.getByRole("button", { name: "显示所有分组" }).click();
  await disk.getByRole("textbox", { name: "搜索挂载路径或来源" }).fill("/proc");
  await expect(disk.locator(".disk-overview-row")).not.toContainText("0%");
  await page.evaluate(() => {
    (window as any).__MOUNTS__.failReveal = true;
  });
  await disk.locator(".disk-overview-row").click();
  await page.getByRole("button", { name: "在远程文件中定位" }).click();
  await expect(
    page.getByRole("region", { name: "远程文件", exact: true }),
  ).toContainText("无法读取目标目录");
  expect(
    await page.evaluate(() =>
      (window as any).__MOUNTS__.calls.filter(
        (c: any) => c.command === "terminal_input",
      ),
    ),
  ).toHaveLength(0);
  expect(errors).toEqual([]);
});

test("Docker preserves its object on connection failure and list position across views", async ({
  page,
}) => {
  const errors = await setup(page);
  await sendContainers(page, 200);
  await page.getByRole("tab", { name: "Docker", exact: true }).click();
  const panel = docker(page),
    list = panel.getByRole("listbox");
  await list.getByRole("option").first().focus();
  await list.getByRole("option").first().press("End");
  const last = list.getByRole("option").filter({ hasText: "container-199" });
  await expect(last).toBeFocused();
  await last.press("Enter");
  await page.getByRole("tab", { name: "监控", exact: true }).click();
  await page.getByRole("tab", { name: "Docker", exact: true }).click();
  await expect(
    panel.getByRole("region", { name: "Docker 对象详情" }),
  ).toContainText("container-199");
  await panel.getByRole("button", { name: "返回列表" }).click();
  await expect(last).toBeInViewport();
  await last.click();
  await sendContainers(page, 200, {
    engine: {
      value: {
        state: "permission",
        endpoint: "unix:///var/run/docker.sock",
        message: "Docker 权限不足",
      },
      status: "ready",
      updatedAt: Date.now(),
      error: null,
    },
  });
  await expect(
    panel.getByRole("region", { name: "Docker 对象详情" }),
  ).toContainText("container-199");
  await expect(
    panel.getByRole("button", { name: "查看 container-199 日志" }),
  ).toBeDisabled();
  await expect(panel).toContainText("保留上次列表");
  expect(errors).toEqual([]);
});
