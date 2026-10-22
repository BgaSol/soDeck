import { openWorkspaceTool } from "./workspace-tool-helpers";
import { test, expect, type Page } from "@playwright/test";
import { setup } from "./resource-fixture";
async function netbird(page: Page, path = "independent") {
  const errors = await setup(page);
  await page.evaluate(
    ({ path }) => {
      const w = window as any,
        base = w.__TAURI_INTERNALS__.invoke;
      let channel: any,
        revision = 0;
      const peer = (
        id: string,
        name: string,
        state: string,
        mode: string,
        ipv4: string | null,
        ipv6: string | null = null,
      ) => ({
        id,
        name,
        fqdn: name,
        ipv4,
        ipv6,
        state,
        rawState: state,
        connectionMode: mode,
        latencyMs: state === "connected" ? 18 : null,
        latencyMeasuredAt: state === "connected" ? Date.now() : null,
        lastHandshakeAt: null,
        stateChangedAt: Date.now(),
        receivedBytes: null,
        sentBytes: "0",
        networks: [],
      });
      const ok = { state: "connected", address: null, message: null };
      const frame = {
        subscriptionId: "netbird-sub",
        revision,
        connectionIdentity: "fixture",
        refreshing: false,
        operations: [] as any[],
        snapshot: {
          binding: {
            token: "binding",
            cliPath: "/usr/bin/netbird",
            daemonAddress: "unix:///var/run/netbird.sock",
            clientVersion: "0.73.0",
            daemonVersion: "0.73.0",
            profile: "default",
            capabilities: {
              json: true,
              up: false,
              down: true,
              serviceStart: false,
              serviceRestart: true,
              reason: null,
            },
          },
          collectionStatus: "ready",
          updatedAt: Date.now(),
          error: null,
          installation: "installed",
          service: "running",
          authentication: "authenticated",
          connection: "connected",
          localPeer: peer(
            "local",
            "linux-server",
            "connected",
            "unknown",
            "100.80.0.1",
          ),
          management: { ...ok },
          signal: { ...ok },
          relay: { ...ok },
          dns: { state: "notConfigured", address: null, message: null },
          peers: [
            peer("db", "db-prod-01", "connected", "direct", "100.80.0.2"),
            peer("web", "web-relay", "connected", "relayed", "100.80.0.3"),
            peer(
              "idle",
              "idle-node",
              "idle",
              "unknown",
              null,
              "fd00:1234:5678:1234::4",
            ),
          ],
          peersComplete: true,
          reportedTotal: 3,
        },
      };
      const calls: any[] = [],
        emit = () =>
          channel?.onmessage({
            ...structuredClone(frame),
            revision: ++revision,
          });
      w.__NETBIRD__ = {
        calls,
        frame,
        emit,
        path,
        submitReplyLost: false,
        finish: (phase: string) => {
          const op = frame.operations[0];
          op.phase = phase;
          op.message =
            phase === "uncertain"
              ? "未收到完成回执，不会自动重试"
              : phase === "verifying"
                ? "命令已返回，正在核对"
                : "已核对远端状态";
          emit();
        },
      };
      w.__TAURI_INTERNALS__.invoke = async (
        command: string,
        args: any = {},
      ) => {
        if (!command.includes("netbird")) return base(command, args);
        calls.push({ command, args });
        if (command === "subscribe_netbird") {
          channel = args.events;
          emit();
          return "netbird-sub";
        }
        if (
          command === "update_netbird_interest" ||
          command === "refresh_netbird"
        ) {
          emit();
          return;
        }
        if (command === "prepare_netbird_operation")
          return {
            id: "preview",
            bindingToken: "binding",
            action: args.action,
            allowed:
              w.__NETBIRD__.path === "independent" || args.action === "up",
            expiresAt: Date.now() + 30000,
            path: {
              result: w.__NETBIRD__.path,
              checkedAt: Date.now(),
              reasons: [
                w.__NETBIRD__.path === "independent"
                  ? "SSH 使用 eth0 返回路由"
                  : "无法确认独立路径",
              ],
            },
            consequences: ["此操作会中断 NetBird 隧道。"],
          };
        if (command === "submit_netbird_operation") {
          const prior = frame.operations.find(
            (o) => o.requestId === args.request.requestId,
          );
          if (prior) return prior.id;
          const op = {
            id: "operation",
            requestId: args.request.requestId,
            action: args.request.action,
            phase: "running",
            message: "正在执行远端操作",
            createdAt: Date.now(),
            finishedAt: null,
            acknowledged: false,
          };
          frame.operations.unshift(op);
          emit();
          if (w.__NETBIRD__.submitReplyLost) throw new Error("提交回复丢失");
          return op.id;
        }
        if (command === "reconcile_netbird_operation") {
          w.__NETBIRD__.finish("succeeded");
          return;
        }
        if (command === "acknowledge_netbird_operation") {
          frame.operations.find((o) => o.id === args.operationId).acknowledged =
            true;
          emit();
          return;
        }
        if (
          command === "get_netbird_raw_status" ||
          command === "get_netbird_logs"
        )
          return {
            text:
              command === "get_netbird_raw_status"
                ? '{"daemonStatus":"Connected"}'
                : "fixture log entry",
            source: "远端客户端",
            checkedAt: Date.now(),
            truncated: false,
          };
        if (command === "probe_netbird_peer")
          return {
            state: "noResponse",
            message: "未收到响应；这不代表节点离线",
            address: "100.80.0.2",
            source: "Mount fixture",
            checkedAt: Date.now(),
          };
        if (command === "set_netbird_cli_path") {
          frame.snapshot.binding.cliPath = args.path ?? "/usr/bin/netbird";
          emit();
        }
        return;
      };
    },
    { path },
  );
  await page.getByRole("button", { name: "断开", exact: true }).click();
  await page.getByRole("button", { name: "重新连接", exact: true }).click();
  await openWorkspaceTool(page, "netbird");
  await expect(page.locator(".nb-summary")).toContainText("已连接");
  return errors;
}
test("overview, peer filters, IPv6, detail probe and preserved navigation", async ({
  page,
}) => {
  const errors = await netbird(page);
  const panel = page.locator(".nb-panel");
  await expect(panel).toContainText("3 个可见节点中 2 个已连接");
  await panel.getByRole("button", { name: "全部", exact: true }).click();
  await page.getByRole("textbox", { name: "搜索 NetBird 节点" }).fill("fd00");
  await expect(panel.locator(".nb-peer-row")).toHaveCount(1);
  await panel.locator(".nb-peer-row").click();
  await expect(panel).toContainText("空闲");
  await expect(panel).toContainText("fd00:1234:5678:1234::4");
  await panel.getByRole("button", { name: "检测", exact: true }).click();
  await expect(panel).toContainText("这不代表节点离线");
  await panel.getByRole("button", { name: "返回 NetBird 上一页" }).click();
  await expect(
    page.getByRole("textbox", { name: "搜索 NetBird 节点" }),
  ).toHaveValue("fd00");
  await page.getByRole("textbox", { name: "搜索 NetBird 节点" }).fill("");
  await panel.getByRole("button", { name: "筛选", exact: true }).click();
  await panel
    .getByRole("combobox", { name: "连接方式", exact: true })
    .selectOption("relayed");
  await expect(panel.locator(".nb-peer-row")).toHaveCount(1);
  await expect(panel.locator(".nb-peer-row")).toContainText("中继");
  expect(errors).toEqual([]);
});
test("retains stale values and diagnostic contents across tool changes", async ({
  page,
}) => {
  const errors = await netbird(page);
  await page.evaluate(() => {
    const nb = (window as any).__NETBIRD__;
    nb.frame.snapshot.management.state = "disconnected";
    nb.emit();
  });
  await expect(page.locator(".nb-summary")).toContainText("连接受限");
  await page.evaluate(() => {
    const nb = (window as any).__NETBIRD__;
    nb.frame.snapshot.collectionStatus = "stale";
    nb.frame.snapshot.error = "权限不足";
    nb.emit();
  });
  await expect(page.locator(".nb-panel")).toContainText("权限不足");
  await expect(page.locator(".nb-panel")).toContainText("db-prod-01");
  await page.getByRole("button", { name: "检测与日志", exact: true }).click();
  await page.getByRole("button", { name: "状态原文", exact: true }).click();
  await expect(page.locator(".nb-output pre")).toContainText("daemonStatus");
  await openWorkspaceTool(page, "monitor");
  await openWorkspaceTool(page, "netbird");
  await expect(page.locator(".nb-output pre")).toContainText("daemonStatus");
  await expect
    .poll(() =>
      page.evaluate(() =>
        (window as any).__NETBIRD__.calls
          .filter((c: any) => c.command === "update_netbird_interest")
          .map((c: any) => c.args.interest.level),
      ),
    )
    .toContain("none");
  expect(errors).toEqual([]);
});
for (const path of ["dependent", "unknown"])
  test(`${path} SSH path blocks disruptive operations`, async ({ page }) => {
    await netbird(page, path);
    await page.getByLabel("NetBird 更多操作").click();
    await page
      .getByRole("button", { name: "断开 NetBird", exact: true })
      .click();
    const dialog = page.getByRole("dialog");
    await expect(dialog).toContainText("操作已阻止");
    await expect(
      dialog.getByRole("button", { name: "断开 NetBird", exact: true }),
    ).toBeDisabled();
    await expect(dialog.getByRole("button", { name: "取消" })).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
    expect(
      await page.evaluate(() =>
        (window as any).__NETBIRD__.calls.some(
          (c: any) => c.command === "submit_netbird_operation",
        ),
      ),
    ).toBe(false);
  });
test("late preflight waits for its tool, keeps its expiry and survives a narrow layout", async ({
  page,
}) => {
  const errors = await netbird(page);
  await page.clock.install();
  await page.evaluate(() => {
    const w = window as any,
      base = w.__TAURI_INTERNALS__.invoke;
    w.__TAURI_INTERNALS__.invoke = async (command: string, args: any) => {
      const result = await base(command, args);
      if (command !== "prepare_netbird_operation") return result;
      return new Promise((resolve) => {
        w.__releaseNetBirdPreview = () =>
          resolve({ ...result, expiresAt: Date.now() + 1000 });
      });
    };
  });
  await page.getByLabel("NetBird 更多操作").click();
  await page.getByRole("button", { name: "断开 NetBird", exact: true }).click();
  await expect
    .poll(() =>
      page.evaluate(() => typeof (window as any).__releaseNetBirdPreview),
    )
    .toBe("function");
  await openWorkspaceTool(page, "monitor");
  const refresh = page.getByRole("button", { name: "刷新资源监控" });
  await refresh.focus();
  await page.evaluate(() => (window as any).__releaseNetBirdPreview());
  const dialog = page.locator(".nb-confirm");
  await expect(dialog).toHaveCount(1);
  await expect(dialog).toBeHidden();
  await expect(refresh).toBeFocused();
  await page.clock.fastForward(2000);
  await openWorkspaceTool(page, "netbird");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("预检已过期，请关闭后重新检查。");
  await expect(
    dialog.getByRole("button", { name: "断开 NetBird", exact: true }),
  ).toBeDisabled();
  await expect(
    dialog.getByRole("button", { name: "取消", exact: true }),
  ).toBeFocused();
  await page.setViewportSize({ width: 650, height: 540 });
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(page.locator('[data-tool="netbird"]')).toBeFocused();
  expect(errors).toEqual([]);
});

test("deduplicates a lost submit reply, shows verification and reconciles uncertainty", async ({
  page,
}) => {
  await netbird(page);
  await page.evaluate(() => {
    (window as any).__NETBIRD__.submitReplyLost = true;
  });
  await page.getByLabel("NetBird 更多操作").click();
  await page.getByRole("button", { name: "断开 NetBird", exact: true }).click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "断开 NetBird", exact: true })
    .click();
  await expect(page.getByRole("dialog")).toContainText("提交回复丢失");
  await page.getByRole("button", { name: "检查提交结果", exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  const requests = await page.evaluate(() =>
    (window as any).__NETBIRD__.calls
      .filter((c: any) => c.command === "submit_netbird_operation")
      .map((c: any) => c.args.request.requestId),
  );
  expect(requests).toHaveLength(2);
  expect(requests[0]).toBe(requests[1]);
  await page.evaluate(() => (window as any).__NETBIRD__.finish("verifying"));
  await expect(page.locator(".nb-operations")).toContainText("正在核对");
  await page.evaluate(() => (window as any).__NETBIRD__.finish("uncertain"));
  await page.getByRole("button", { name: "检查当前状态", exact: true }).click();
  await expect(page.locator(".nb-operations")).toContainText("已完成");
});
test("rail keyboard navigation, narrow dark panel and reduced motion", async ({
  page,
}, testInfo) => {
  const errors = await netbird(page);
  await page.emulateMedia({ reducedMotion: "reduce", contrast: "more" });
  const button = page.locator('[data-tool="netbird"]');
  await button.focus();
  await page.keyboard.press("Shift+Tab");
  await expect(page.locator('[data-tool="monitor"]')).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(button).toBeFocused();
  await expect(button).toHaveAttribute("aria-expanded", "true");
  await page.evaluate(() => {
    document.documentElement.dataset.theme = "dark";
    const panel = document.querySelector(
      ".resource-service-panel",
    ) as HTMLElement;
    panel.style.width = "320px";
    panel.style.maxWidth = "320px";
  });
  const panel = page.locator(".nb-panel");
  expect(
    await panel.evaluate((el) => el.scrollWidth <= el.clientWidth + 1),
  ).toBe(true);
  await panel.screenshot({ path: testInfo.outputPath("netbird-dark-320.png") });
  expect(errors).toEqual([]);
});

test("virtualized peers keep their scroll after refresh and a detail round trip", async ({
  page,
}) => {
  await netbird(page);
  await page.evaluate(() => {
    const nb = (window as any).__NETBIRD__;
    const base = nb.frame.snapshot.peers[0];
    nb.frame.snapshot.peers = Array.from({ length: 150 }, (_, index) => ({
      ...base,
      id: `peer-${index}`,
      name: `peer-${index.toString().padStart(3, "0")}`,
    }));
    nb.frame.snapshot.reportedTotal = 150;
    nb.emit();
  });
  await page.getByRole("button", { name: "全部", exact: true }).click();
  const list = page.locator(".nb-peer-list");
  await expect
    .poll(() => list.evaluate((el) => el.scrollHeight))
    .toBeGreaterThan(8000);
  await list.evaluate((el) => {
    el.scrollTop = 2600;
    el.dispatchEvent(new Event("scroll"));
  });
  await expect
    .poll(() => list.evaluate((el) => el.scrollTop))
    .toBeGreaterThan(2500);
  await page.evaluate(() => (window as any).__NETBIRD__.emit());
  const row = page.locator(".nb-peer-row").filter({ hasText: "peer-045" });
  await row.click();
  await expect(page.locator(".nb-detail-title")).toContainText("peer-045");
  await page.getByRole("button", { name: "返回 NetBird 上一页" }).click();
  await expect
    .poll(() => list.evaluate((el) => el.scrollTop))
    .toBeGreaterThan(2500);
});
test("missing, login required and genuinely empty peer lists have distinct states", async ({
  page,
}) => {
  await netbird(page);
  await page.evaluate(() => {
    const nb = (window as any).__NETBIRD__;
    nb.frame.snapshot.connection = "needsLogin";
    nb.emit();
  });
  await expect(page.locator(".nb-summary")).toContainText("需要登录");
  await expect(page.locator(".nb-panel")).toContainText(
    "在服务器终端完成 NetBird 登录",
  );
  await page.evaluate(() => {
    const nb = (window as any).__NETBIRD__;
    nb.frame.snapshot.installation = "missing";
    nb.frame.snapshot.updatedAt = null;
    nb.frame.snapshot.binding = null;
    nb.frame.snapshot.peers = [];
    nb.frame.snapshot.peersComplete = false;
    nb.frame.snapshot.connection = "unknown";
    nb.emit();
  });
  await expect(page.locator(".nb-summary")).toContainText("未检测到客户端");
  await page.evaluate(() => {
    const nb = (window as any).__NETBIRD__;
    nb.frame.snapshot.installation = "installed";
    nb.frame.snapshot.connection = "connected";
    nb.frame.snapshot.peersComplete = true;
    nb.frame.snapshot.reportedTotal = 0;
    nb.emit();
  });
  await expect(page.locator(".nb-panel")).toContainText("当前没有可见节点");
});
test("cancelled workspace moves resume subscriptions and preserve the NetBird snapshot", async ({
  page,
}) => {
  await netbird(page);
  await page.getByRole("button", { name: "全部", exact: true }).click();
  await page
    .getByRole("textbox", { name: "搜索 NetBird 节点" })
    .fill("db-prod");
  await page.evaluate(() => {
    const w = window as any,
      base = w.__TAURI_INTERNALS__.invoke;
    w.__TAURI_INTERNALS__.invoke = async (command: string, args: any = {}) => {
      if (command === "terminal_freeze") {
        w.__MOUNTS__.terminal.onmessage({ type: "frozen", token: args.token });
        return;
      }
      if (command === "move_workspace") {
        w.__NETBIRD__.moveSnapshot = args.payload;
        return new Promise((_, reject) => {
          w.__NETBIRD__.cancelMove = () => reject(new Error("模拟取消移动"));
        });
      }
      return base(command, args);
    };
  });
  await page
    .getByRole("tablist", { name: "SSH 会话", exact: true })
    .getByRole("tab")
    .click({ button: "right" });
  await page
    .getByRole("menuitem", { name: "移到独立窗口", exact: true })
    .click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).__NETBIRD__.calls.filter(
            (c: any) => c.command === "unsubscribe_netbird",
          ).length,
      ),
    )
    .toBeGreaterThan(0);
  const snapshot = await page.evaluate(
    () => (window as any).__NETBIRD__.moveSnapshot,
  );
  expect(snapshot.resources.tab).toBe("netbird");
  expect(snapshot.netbird.page).toBe("peers");
  expect(snapshot.netbird.filters.query).toBe("db-prod");
  await page.evaluate(() => (window as any).__NETBIRD__.cancelMove());
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).__NETBIRD__.calls.filter(
            (c: any) => c.command === "subscribe_netbird",
          ).length,
      ),
    )
    .toBe(2);
  await expect(
    page.getByRole("textbox", { name: "搜索 NetBird 节点" }),
  ).toHaveValue("db-prod");
  await expect(
    page.getByRole("button", { name: "刷新 NetBird", exact: true }),
  ).toBeEnabled();
});
