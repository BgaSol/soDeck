import { openWorkspaceTool } from "./workspace-tool-helpers";
import { test, expect, type Page } from "@playwright/test";
import { setup } from "./resource-fixture";
async function management(
  page: Page,
  options: { state?: string; autoRemove?: boolean } = {},
) {
  const errors = await setup(page);
  await page.evaluate(
    ({ runtime, autoRemove }) => {
      const w = window as any,
        base = w.__TAURI_INTERNALS__.invoke;
      let channel: any,
        revision = 0,
        target: string | null = null;
      const id = "a".repeat(64);
      const engine = {
        token: "monitor-generation/1",
        generation: "monitor-generation",
        engineGeneration: 1,
        endpoint: "unix:///var/run/docker.sock",
        daemonId: "fixture-engine",
      };
      const detail = {
        id,
        name: "web",
        image: "nginx:alpine",
        state: runtime,
        health: "healthy",
        failingStreak: 0,
        exitCode: 0,
        oomKilled: false,
        error: "",
        restartCount: 2,
        restartPolicy: "unless-stopped",
        restartMaximum: 0,
        autoRemove,
        stopTimeout: 10,
        created: "2026-10-01T00:00:00Z",
        startedAt: "2026-10-17T01:00:00Z",
        finishedAt: "0001-01-01T00:00:00Z",
        ports: [
          { containerPort: "80/tcp", hostIp: "0.0.0.0", hostPort: "8080" },
        ],
        mounts: [
          {
            kind: "bind",
            name: null,
            source: "/data/apps/web",
            destination: "/var/www",
            readOnly: true,
          },
        ],
        networks: [
          {
            name: "app_default",
            id: "n".repeat(64),
            address: "172.20.0.2",
            ipv6: "",
          },
        ],
        updatedAt: Date.now(),
      };
      const previews = new Map<string, any>();
      const remember = (p: any) => {
        p.draftToken = p.id + "-draft";
        previews.set(p.draftToken, p);
        return p;
      };
      const tasks: any[] = [],
        calls: any[] = [];
      const metric = (value: any) => ({
        value,
        status: "ready",
        updatedAt: Date.now(),
        error: null,
      });
      const detailState: any = {};
      const emit = () =>
        channel?.onmessage({
          subscriptionId: "docker-sub",
          revision: ++revision,
          engine,
          detail: target
            ? {
                containerId: target,
                value: detail,
                loading: false,
                stale: false,
                missing: false,
                error: null,
                ...detailState,
              }
            : null,
          operations: tasks,
          error: null,
        });
      const list = () =>
        w.__MOUNTS__.sendDocker({
          containers: metric([
            {
              id,
              name: "web",
              image: "nginx:alpine",
              state: detail.state,
              status: detail.state,
            },
          ]),
        });
      w.__DOCKER__ = {
        calls,
        detail,
        detailState,
        tasks,
        emit,
        engine,
        finish: (phase = "completed") => {
          const t = tasks.at(-1);
          t.phase = phase;
          t.commandResult = phase === "unknown" ? "unknown" : "succeeded";
          t.message =
            phase === "unknown"
              ? "执行结果待确认，不会自动重试。"
              : "命令已完成";
          t.observedState =
            phase === "unknown"
              ? null
              : t.action.type === "stop"
                ? "exited"
                : "running";
          if (phase !== "unknown") detail.state = t.observedState;
          t.finishedAt = Date.now();
          list();
          emit();
        },
      };
      w.__TAURI_INTERNALS__.invoke = async (
        command: string,
        args: any = {},
      ) => {
        if (
          !command.includes("docker_management") &&
          !command.includes("docker_operation") &&
          !command.startsWith("get_docker_") &&
          !command.startsWith("prepare_docker_") &&
          command !== "renew_docker_preview" &&
          command !== "release_docker_preview" &&
          command !== "open_port_forward_window"
        )
          return base(command, args);
        calls.push({ command, args });
        if (command === "list_docker_operations")
          return {
            operations: [
              {
                ...tasks[0],
                id: "older-task",
                targetName: "earlier-container",
                phase: "completed",
                message: "较早的命令已完成",
                createdAt: Date.now() - 60000,
              },
            ],
            nextCursor: null,
          };
        if (command === "get_docker_container_diagnostics")
          return args.reveal
            ? "PASSWORD=fixture-secret-value"
            : "PASSWORD=••••••";
        if (command === "get_docker_host_link")
          return args.request.type === "mount"
            ? { path: "/data/apps/web", host: null, port: null, name: "web" }
            : {
                path: null,
                host: "127.0.0.1",
                port: 8080,
                name: "web · 80/tcp",
              };
        if (command === "open_port_forward_window") return;
        if (command === "get_docker_image_detail")
          return {
            id: args.imageId,
            tags: ["nginx:alpine", "nginx:stable"],
            digests: [],
            created: "2026-10-01T00:00:00Z",
            platform: "linux/amd64",
            size: 52428800,
            containers: [{ id, name: "web", state: detail.state }],
          };
        if (command === "get_docker_compose_projects")
          return {
            available: true,
            message: "",
            projects: [
              {
                name: "website",
                directory: "/opt/website",
                files: ["/opt/website/compose.yaml"],
                containers: [
                  { id, name: "web", service: "web", state: detail.state },
                ],
              },
            ],
            associations: [
              {
                id: "association-1",
                revision: 1,
                serverId: "fixture",
                endpoint: engine.endpoint,
                daemonId: engine.daemonId,
                project: "website",
                directory: "/opt/website",
                files: ["/opt/website/compose.yaml"],
                envFiles: [],
                profiles: [],
              },
            ],
          };
        if (command === "get_docker_resources")
          return {
            usage: [
              {
                Type: "Images",
                TotalCount: 2,
                Active: 1,
                Size: "50MB",
                Reclaimable: "10MB (20%)",
              },
            ],
            volumes: [
              {
                id: "app-data",
                name: "app-data",
                kind: "volume",
                driver: "local",
                created: "2026-10-01T00:00:00Z",
                protected: false,
                mountpoint: "/var/lib/docker/volumes/app-data/_data",
                containers: [],
                configuration: { Name: "app-data" },
              },
            ],
            networks: [
              {
                id: "e".repeat(64),
                name: "bridge",
                kind: "network",
                driver: "bridge",
                created: "2026-10-01T00:00:00Z",
                protected: true,
                mountpoint: "",
                containers: [],
                configuration: { Name: "bridge" },
              },
            ],
            candidates: [
              { id: "b".repeat(64), name: "stopped-job", kind: "container" },
              { id: "app-data", name: "app-data", kind: "volume" },
            ],
          };
        if (command === "prepare_docker_create")
          return remember({
            id: "create-preview",
            engine,
            targetId: args.specification.name,
            targetName: args.specification.name,
            action: { type: "create", draftId: "create-preview" },
            consequences: ["使用本地镜像创建容器。创建后启动失败会保留容器。"],
            expiresAt: Date.now() + 60000,
          });
        if (command === "prepare_docker_transfer")
          return remember({
            id: "transfer-preview",
            engine,
            targetId: args.targetId,
            targetName: "selected.tar",
            action: {
              type: "transfer",
              draftId: "transfer-preview",
              kind: args.kind,
            },
            consequences: ["本机目标：/tmp/selected.tar。不会在本机自动解包。"],
            expiresAt: Date.now() + 60000,
          });
        if (command === "subscribe_docker_management") {
          channel = args.events;
          emit();
          return "docker-sub";
        }
        if (command === "update_docker_management_interest") {
          target = args.containerId;
          emit();
          return;
        }
        if (command === "refresh_docker_management") {
          emit();
          return;
        }
        if (command === "prepare_docker_operation")
          return remember({
            id: "preview",
            engine,
            targetId: args.targetId,
            targetName: args.targetId === id ? "web" : args.targetId,
            action: args.action,
            consequences: [
              args.action.type === "imageRemove"
                ? "移除所选 Tag，保留其他引用，不强制删除。"
                : args.action.type === "batch"
                  ? "仅删除所选对象，卷数据无法恢复。"
                  : args.action.type === "compose"
                    ? "重新核对项目配置与影响范围；不会把拉取成功当作部署完成。"
                    : args.action.type === "remove"
                      ? "删除容器及其可写层，无法撤销。保留挂载卷和镜像。"
                      : "此容器启用了自动删除。退出后容器及匿名卷可能被移除。",
            ],
            expiresAt: Date.now() + 60000,
          });
        if (command === "renew_docker_preview") {
          const old = previews.get(args.draftToken);
          if (!old) throw { kind: "previewUnavailable", message: "草稿已失效" };
          if (old.id !== args.expectedPreviewId) return old;
          const next = {
            ...old,
            id: old.id + "-renewed",
            expiresAt: Date.now() + 60000,
          };
          if (["create", "transfer"].includes(next.action.type))
            next.action = { ...next.action, draftId: next.id };
          previews.set(args.draftToken, next);
          return next;
        }
        if (command === "release_docker_preview") {
          previews.delete(args.draftToken);
          return;
        }
        if (command === "submit_docker_operation") {
          const duplicate = tasks.find(
            (t) => t.requestId === args.request.requestId,
          );
          if (duplicate) return duplicate.id;
          const t = {
            id: "op-" + tasks.length,
            requestId: args.request.requestId,
            engine: { ...engine },
            targetId: args.request.targetId,
            targetName:
              args.request.targetId === id ? "web" : args.request.targetId,
            action: args.request.action,
            phase: "running",
            commandResult: "notSubmitted",
            message: "正在执行…",
            observedState: detail.state,
            observedHealth: detail.health,
            createdAt: Date.now(),
            finishedAt: null,
            reconciling: false,
          };
          tasks.push(t);
          emit();
          return t.id;
        }
        if (command === "reconcile_docker_operation") {
          tasks.at(-1).observedState = detail.state;
          tasks.at(-1).message = "已读取当前状态；这不证明先前命令是否完成。";
          emit();
          return;
        }
        if (command === "acknowledge_docker_operation") {
          tasks.at(-1).phase = "acknowledged";
          emit();
          return;
        }
        if (command === "docker_operation_summary")
          return {
            active: tasks.filter((t) => t.phase === "running").length,
            attention: tasks.filter((t) => t.phase === "unknown").length,
          };
        return;
      };
      list();
      w.__MOUNTS__.sendDocker({
        images: metric([
          {
            id: "sha256:" + "c".repeat(64),
            name: "nginx:alpine",
            size: "50MB",
          },
        ]),
      });
    },
    {
      runtime: options.state ?? "running",
      autoRemove: options.autoRemove ?? false,
    },
  );
  // Explicitly verify a new tool generation after installing the service fixture.
  await page.getByRole("button", { name: "更新工具连接", exact: true }).click();
  await openWorkspaceTool(page, "docker");
  const panel = page.getByRole("region", { name: "Docker 引擎与对象" });
  await panel
    .getByRole("list", { name: "容器列表" })
    .locator(".docker-object-row")
    .first()
    .click();
  await expect(panel).toContainText("app_default");
  return { panel, errors };
}
test("container lifecycle stays available while logs and background tasks remain independent", async ({
  page,
}) => {
  const { panel, errors } = await management(page);
  await expect(panel).toContainText("0.0.0.0:8080 → 80/tcp");
  await panel
    .locator(".docker-detail-group > summary")
    .filter({ hasText: "存储" })
    .click();
  await expect(panel).toContainText("/data/apps/web");
  await panel.getByRole("button", { name: "停止 web", exact: true }).dblclick();
  await expect(
    panel.getByRole("button", { name: "重启 web", exact: true }),
  ).toBeDisabled();
  await expect(
    panel.getByRole("button", { name: "查看 web 日志" }),
  ).toBeEnabled();
  await panel.getByRole("button", { name: "返回列表" }).click();
  await expect(panel.getByRole("list")).toContainText("操作中");
  await openWorkspaceTool(page, "monitor");
  await page.evaluate(() => (window as any).__DOCKER__.finish());
  await openWorkspaceTool(page, "docker");
  await panel.getByRole("list").locator(".docker-object-row").first().click();
  await expect(
    panel.getByText("/data/apps/web", { exact: true }),
  ).toBeVisible();
  await expect(
    panel.getByRole("button", { name: "启动 web", exact: true }),
  ).toBeEnabled();
  expect(
    await page.evaluate(
      () =>
        (window as any).__DOCKER__.calls.filter(
          (c: any) => c.command === "submit_docker_operation",
        ).length,
    ),
  ).toBe(1);
  expect(errors).toEqual([]);
});
test("delete and auto-remove consequences require confirmation with safe initial focus", async ({
  page,
}) => {
  const { panel, errors } = await management(page, { state: "exited" });
  await panel.getByRole("button", { name: "web 更多操作" }).click();
  await page.getByRole("menuitem", { name: "删除容器…" }).click();
  const dialog = page.getByRole("dialog", { name: "删除容器？" });
  await expect(
    dialog.getByRole("button", { name: "取消", exact: true }),
  ).toBeFocused();
  await expect(dialog).toContainText("无法撤销");
  await page.keyboard.press("Escape");
  expect(
    await page.evaluate(() => (window as any).__DOCKER__.tasks.length),
  ).toBe(0);
  await panel.getByRole("button", { name: "web 更多操作" }).click();
  await page.getByRole("menuitem", { name: "删除容器…" }).click();
  await dialog.getByRole("button", { name: "确认删除", exact: true }).click();
  await expect(dialog).toBeHidden();
  expect(
    await page.evaluate(
      () =>
        (window as any).__DOCKER__.calls.find(
          (c: any) => c.command === "submit_docker_operation",
        ).args.request.previewId,
    ),
  ).toBe("preview");
  expect(errors).toEqual([]);
});
test("uncertain result offers read-only reconciliation and never retries restart", async ({
  page,
}) => {
  const { panel, errors } = await management(page);
  await panel.getByRole("button", { name: "重启 web", exact: true }).click();
  await page.evaluate(() => (window as any).__DOCKER__.finish("unknown"));
  await panel.locator(".docker-operations > summary").click();
  await panel.getByRole("button", { name: "读取更早记录" }).click();
  await expect(panel).toContainText("earlier-container");
  await expect(panel.getByRole("button", { name: "读取更早记录" })).toHaveCount(
    0,
  );
  await panel.getByRole("button", { name: "核对当前状态" }).click();
  await expect(panel).toContainText("不证明先前命令是否完成");
  await panel.getByText("继续管理此对象", { exact: true }).click();
  await panel.getByRole("button", { name: "已知晓，解除操作锁" }).click();
  await expect(
    panel.getByRole("button", { name: "重启 web", exact: true }),
  ).toBeEnabled();
  expect(
    await page.evaluate(
      () =>
        (window as any).__DOCKER__.calls.filter(
          (c: any) => c.command === "submit_docker_operation",
        ).length,
    ),
  ).toBe(1);
  expect(errors).toEqual([]);
});
test("auto-remove and narrow dark layouts retain explicit consequences", async ({
  page,
}, testInfo) => {
  const { panel, errors } = await management(page, { autoRemove: true });
  await page.evaluate(() => (document.documentElement.dataset.theme = "dark"));
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.setViewportSize({ width: 650, height: 620 });
  await openWorkspaceTool(page, "docker");
  await panel.getByRole("button", { name: "停止 web", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "停止容器？" });
  await expect(dialog).toContainText("匿名卷可能被移除");
  await expect(
    dialog.getByRole("button", { name: "取消", exact: true }),
  ).toBeFocused();
  await page.screenshot({
    path: `.qa/docker-management-confirm-${testInfo.project.name}.png`,
  });
  await dialog.getByRole("button", { name: "取消", exact: true }).click();
  await expect(panel).toContainText("健康");
  await page.screenshot({
    path: `.qa/docker-management-detail-${testInfo.project.name}.png`,
  });
  expect(
    await panel.evaluate(
      (element) => element.scrollWidth <= element.clientWidth + 1,
    ),
  ).toBe(true);
  expect(errors).toEqual([]);
});

test("supplementary actions and sensitive diagnostics reset when leaving the Docker view", async ({
  page,
}) => {
  const { panel, errors } = await management(page);
  await panel.locator("summary").filter({ hasText: /^配置/ }).click();
  await expect(panel.locator(".docker-raw-config")).toContainText("••••••");
  await panel.getByLabel("显示完整配置与敏感值").check();
  await expect(panel.locator(".docker-raw-config")).toContainText(
    "fixture-secret-value",
  );
  await openWorkspaceTool(page, "monitor");
  await openWorkspaceTool(page, "docker");
  await expect(panel).not.toContainText("fixture-secret-value");
  await panel.getByRole("button", { name: "web 更多操作" }).click();
  await page.getByRole("menuitem", { name: "暂停", exact: true }).click();
  const calls = await page.evaluate(() => (window as any).__DOCKER__.calls);
  expect(
    calls.find((c: any) => c.command === "submit_docker_operation").args.request
      .action,
  ).toEqual({ type: "pause" });
  expect(errors).toEqual([]);
});

test("image removal identifies a reference and creation does not broadcast environment values", async ({
  page,
}) => {
  const { panel, errors } = await management(page);
  await panel.getByRole("button", { name: "返回列表" }).click();
  await panel.getByRole("button", { name: /^镜像/ }).click();
  await panel
    .getByRole("list", { name: "镜像列表" })
    .locator(".docker-object-row")
    .first()
    .click();
  await expect(panel).toContainText("linux/amd64");
  await panel.getByRole("button", { name: "移除引用", exact: true }).click();
  await expect(page.getByRole("dialog")).toContainText("Tag");
  const prepared = await page.evaluate(() =>
    (window as any).__DOCKER__.calls.find(
      (c: any) => c.command === "prepare_docker_operation",
    ),
  );
  expect(prepared.args.action).toEqual({
    type: "imageRemove",
    reference: "nginx:alpine",
  });
  await page.getByRole("button", { name: "取消", exact: true }).click();
  await panel.getByRole("button", { name: "创建容器…" }).click();
  await page.getByLabel("容器名称", { exact: true }).fill("new-web");
  await page
    .getByRole("dialog")
    .locator("summary")
    .filter({ hasText: /^环境变量/ })
    .click();
  await page.getByLabel("每行 KEY=value").fill("PASSWORD=private-value");
  await page.getByRole("button", { name: "预览创建", exact: true }).click();
  await expect(page.getByRole("dialog")).not.toContainText("private-value");
  await page.getByRole("button", { name: "确认创建容器", exact: true }).click();
  const submitted = await page.evaluate(
    () =>
      (window as any).__DOCKER__.calls.find(
        (c: any) => c.command === "submit_docker_operation",
      ).args.request,
  );
  expect(submitted.action.type).toBe("create");
  expect(JSON.stringify(submitted)).not.toContain("private-value");
  expect(errors).toEqual([]);
});

test("Compose keeps restart and apply separate and cleanup excludes volumes until selected", async ({
  page,
}) => {
  const { panel, errors } = await management(page);
  await panel.getByRole("button", { name: "返回列表" }).click();
  await panel.getByRole("button", { name: "项目", exact: true }).click();
  await panel.getByRole("button", { name: /website/ }).click();
  await expect(panel).toContainText("重启不会应用配置变更");
  await panel.getByRole("button", { name: "重启", exact: true }).click();
  await page.getByRole("button", { name: "确认重启项目", exact: true }).click();
  expect(
    await page.evaluate(
      () => (window as any).__DOCKER__.tasks.at(-1).action.mode,
    ),
  ).toBe("restart");
  await page.evaluate(() => (window as any).__DOCKER__.finish());
  await panel.getByRole("button", { name: "Docker 更多功能" }).click();
  await page.getByRole("menuitem", { name: "空间与资源", exact: true }).click();
  await panel.getByRole("button", { name: "清理", exact: true }).click();
  await expect(panel.getByText("app-data", { exact: true })).toHaveCount(0);
  await panel.getByLabel(/stopped-job/).check();
  await panel.getByRole("button", { name: /预览删除所选 1 项/ }).click();
  const preview = await page.evaluate(
    () =>
      (window as any).__DOCKER__.calls
        .filter((c: any) => c.command === "prepare_docker_operation")
        .at(-1).args.action,
  );
  expect(preview.entries).toEqual([
    { targetId: "b".repeat(64), action: "removeContainer" },
  ]);
  await page.getByRole("button", { name: "取消", exact: true }).click();
  await panel.getByLabel(/显示未被引用的数据卷/).check();
  await expect(panel.getByText("app-data", { exact: true })).toBeVisible();
  expect(errors).toEqual([]);
});

test("published ports create a draft and archive export requires a concrete preview", async ({
  page,
}) => {
  const { panel, errors } = await management(page);
  await panel.getByRole("button", { name: "转发端口", exact: true }).click();
  const forward = await page.evaluate(() =>
    (window as any).__DOCKER__.calls.find(
      (c: any) => c.command === "open_port_forward_window",
    ),
  );
  expect(forward.args.seed).toEqual({
    name: "web · 80/tcp",
    targetHost: "127.0.0.1",
    targetPort: 8080,
  });
  await panel.getByRole("button", { name: "web 更多操作" }).click();
  await page.getByRole("menuitem", { name: "导出容器文件（tar）…" }).click();
  await page.getByLabel("容器内文件或目录").fill("/app/data");
  await page.getByRole("button", { name: "选择保存位置…" }).click();
  await expect(page.getByRole("dialog")).toContainText("/tmp/selected.tar");
  await expect(page.getByRole("dialog")).toContainText("不会在本机自动解包");
  await page.getByRole("button", { name: "确认导出容器文件" }).click();
  expect(
    await page.evaluate(
      () => (window as any).__DOCKER__.tasks.at(-1).action.kind,
    ),
  ).toBe("copyOut");
  expect(errors).toEqual([]);
});

for (const width of [320, 360, 480]) {
  test(`Docker pages fit a ${width}px sidebar in both themes`, async ({
    page,
  }, testInfo) => {
    const { panel, errors } = await management(page);
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.locator(".responsive-workspace").evaluate((element, size) => {
      (element as HTMLElement).style.setProperty(
        "--service-column",
        `${size}px`,
        "important",
      );
    }, width);
    const side = page.locator(".workspace-services");
    await expect
      .poll(async () => Math.round((await side.boundingBox())!.width))
      .toBe(width);
    const capture = async (name: string) => {
      for (const theme of ["light", "dark"]) {
        await page.evaluate((value) => {
          document.documentElement.dataset.theme = value;
        }, theme);
        expect(
          await panel.evaluate((element) => {
            return [
              element,
              ...element.querySelectorAll<HTMLElement>(
                ".docker-object-detail, .docker-secondary-page, .docker-management-toolbar",
              ),
            ].every((node) => node.scrollWidth <= node.clientWidth + 1);
          }),
        ).toBe(true);
        await side.screenshot({
          path: `.qa/docker-${name}-${width}-${theme}-${testInfo.project.name}.png`,
        });
      }
    };
    await capture("container");
    await panel.getByRole("button", { name: "返回列表" }).click();
    await panel.getByRole("button", { name: /^镜像/ }).click();
    await panel
      .getByRole("list", { name: "镜像列表" })
      .locator(".docker-object-row")
      .first()
      .click();
    await expect(panel).toContainText("linux/amd64");
    await capture("image");
    await panel.getByRole("button", { name: "创建容器…" }).click();
    const form = page.getByRole("dialog", { name: "创建容器" });
    await expect(form).toBeVisible();
    expect(
      await form.evaluate((el) => el.scrollWidth <= el.clientWidth + 1),
    ).toBe(true);
    await form.screenshot({
      path: `.qa/docker-create-${width}-${testInfo.project.name}.png`,
    });
    await form.getByRole("button", { name: "取消", exact: true }).click();
    await panel.getByRole("button", { name: "返回列表" }).click();
    await panel.getByRole("button", { name: "项目", exact: true }).click();
    await panel.getByRole("button", { name: /website/ }).click();
    await expect(panel).toContainText("重启不会应用配置变更");
    await capture("compose");
    await panel.getByRole("button", { name: "Docker 更多功能" }).click();
    await page
      .getByRole("menuitem", { name: "空间与资源", exact: true })
      .click();
    await expect(panel).toContainText("50MB");
    await capture("resources");
    expect(errors).toEqual([]);
  });
}

test("timeouts preserve detail actions and recover without reopening", async ({
  page,
}) => {
  const { panel, errors } = await management(page);
  await page.evaluate(() => {
    const w = window as any;
    Object.assign(w.__DOCKER__.detailState, {
      stale: true,
      errorKind: "timeout",
      error: "远端资源查询超时",
    });
    w.__DOCKER__.emit();
    w.__MOUNTS__.sendDocker({
      engineErrorKind: "timeout",
      engine: {
        value: {
          state: "running",
          endpoint: w.__DOCKER__.engine.endpoint,
          message: "ok",
        },
        status: "stale",
        updatedAt: Date.now() - 30000,
        error: "远端资源查询超时",
      },
      containers: {
        value: [
          {
            id: w.__DOCKER__.detail.id,
            name: "web",
            image: "nginx:alpine",
            state: "running",
            status: "running",
          },
        ],
        status: "stale",
        updatedAt: Date.now() - 30000,
        error: "远端资源查询超时",
      },
    });
  });
  await expect(panel).toContainText("保留上次列表");
  await expect(panel).toContainText("保留上次详情");
  await expect(
    panel.getByRole("button", { name: "查看 web 日志" }),
  ).toBeEnabled();
  await expect(
    panel.getByRole("button", { name: "进入 web 终端" }),
  ).toBeEnabled();
  await expect(
    panel.getByRole("button", { name: "停止 web", exact: true }),
  ).toBeEnabled();
  await panel.getByRole("button", { name: "停止 web", exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => (window as any).__DOCKER__.tasks.length))
    .toBe(1);
  await expect(
    panel.getByRole("button", { name: "停止 web", exact: true }),
  ).toBeDisabled();
  await page.evaluate(() => {
    const w = window as any;
    Object.assign(w.__DOCKER__.detailState, {
      stale: false,
      errorKind: null,
      error: null,
    });
    w.__MOUNTS__.sendDocker({
      engineErrorKind: null,
      engine: {
        value: {
          state: "running",
          endpoint: w.__DOCKER__.engine.endpoint,
          message: "ok",
        },
        status: "ready",
        updatedAt: Date.now(),
        error: null,
      },
    });
    w.__DOCKER__.finish();
  });
  await expect(panel).not.toContainText("远端资源查询超时");
  await expect(
    panel.getByRole("button", { name: "启动 web", exact: true }),
  ).toBeEnabled();
  expect(errors).toEqual([]);
});

test("detail permission failure blocks mutations but remains manually retryable", async ({
  page,
}) => {
  const { panel, errors } = await management(page);
  await page.evaluate(() => {
    const w = window as any;
    Object.assign(w.__DOCKER__.detailState, {
      stale: true,
      errorKind: "permission",
      error: "Docker 权限不足",
    });
    w.__DOCKER__.emit();
  });
  await expect(
    panel.getByRole("button", { name: "停止 web", exact: true }),
  ).toBeDisabled();
  await panel.getByRole("button", { name: "立即重试", exact: true }).click();
  expect(
    await page.evaluate(() =>
      (window as any).__DOCKER__.calls.some(
        (c: any) => c.command === "refresh_docker_management",
      ),
    ),
  ).toBe(true);
  expect(
    await page.evaluate(() => (window as any).__DOCKER__.tasks.length),
  ).toBe(0);
  expect(errors).toEqual([]);
});

test("stale list actions still open detail and submit while statistics are unavailable", async ({
  page,
}) => {
  const { panel, errors } = await management(page);
  await page.evaluate(() => {
    const w = window as any;
    w.__DOCKER__.detail.statsError = "远端资源查询超时";
    w.__DOCKER__.emit();
    w.__MOUNTS__.sendDocker({
      containers: {
        value: [
          {
            id: w.__DOCKER__.detail.id,
            name: "web",
            image: "nginx:alpine",
            state: "running",
            status: "running",
          },
        ],
        status: "stale",
        updatedAt: Date.now() - 10000,
        error: "远端资源查询超时",
      },
    });
  });
  await panel.getByRole("button", { name: "返回列表", exact: true }).click();
  await panel
    .getByRole("button", { name: "web 列表操作", exact: true })
    .click();
  await page.getByRole("menuitem", { name: "重启", exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => (window as any).__DOCKER__.tasks.length))
    .toBe(1);
  expect(
    await page.evaluate(() => (window as any).__DOCKER__.tasks[0].action.type),
  ).toBe("restart");
  expect(errors).toEqual([]);
});

test("a confirmed missing container never permits cached management actions", async ({
  page,
}) => {
  const { panel, errors } = await management(page);
  await page.evaluate(() => {
    const w = window as any;
    Object.assign(w.__DOCKER__.detailState, {
      stale: true,
      missing: true,
      errorKind: "notFound",
      error: "容器不存在",
    });
    w.__DOCKER__.emit();
  });
  await expect(panel).toContainText("容器不存在");
  await expect(
    panel.getByRole("button", { name: "停止 web", exact: true }),
  ).toBeDisabled();
  expect(
    await page.evaluate(() => (window as any).__DOCKER__.tasks.length),
  ).toBe(0);
  expect(errors).toEqual([]);
});

async function openCreate(page: Page) {
  const result = await management(page);
  await result.panel.getByRole("button", { name: "返回列表" }).click();
  await result.panel.getByRole("button", { name: /^镜像/ }).click();
  await result.panel
    .getByRole("list", { name: "镜像列表" })
    .locator(".docker-object-row")
    .first()
    .click();
  await result.panel.getByRole("button", { name: "创建容器…" }).click();
  return result;
}
test("creation preserves input across review and discard, validates collapsed fields and keeps one modal", async ({
  page,
}) => {
  const { errors } = await openCreate(page);
  const dialog = page.getByRole("dialog");
  await page.getByLabel("容器名称", { exact: true }).fill("new-web");
  await page.getByLabel("重启策略", { exact: true }).selectOption("on-failure");
  await page.getByLabel("最大重试次数", { exact: true }).fill("7");
  await page.getByLabel("重启策略", { exact: true }).selectOption("always");
  await expect(page.getByLabel("最大重试次数", { exact: true })).toBeHidden();
  await page.getByLabel("重启策略", { exact: true }).selectOption("on-failure");
  await expect(page.getByLabel("最大重试次数", { exact: true })).toHaveValue(
    "7",
  );
  const group = dialog.locator("details").filter({ hasText: "每行 KEY=value" });
  await dialog
    .locator("summary")
    .filter({ hasText: /^环境变量/ })
    .click();
  await page.getByLabel("每行 KEY=value").fill("\nTOKEN=first\nTOKEN=second");
  await dialog
    .locator("summary")
    .filter({ hasText: /^环境变量/ })
    .click();
  await page.getByRole("button", { name: "预览创建", exact: true }).click();
  await expect(group).toHaveAttribute("open", "");
  await expect(page.getByLabel("每行 KEY=value")).toBeFocused();
  await expect(dialog).toContainText("第 3 行变量名重复");
  await page.getByLabel("每行 KEY=value").fill("TOKEN= a=b  ");
  await page.getByRole("button", { name: "预览创建", exact: true }).click();
  await expect(page.locator("dialog[open]")).toHaveCount(1);
  await expect(
    page.getByRole("button", { name: "返回编辑", exact: true }),
  ).toBeFocused();
  await page.getByRole("button", { name: "返回编辑", exact: true }).click();
  await expect(page.getByLabel("容器名称", { exact: true })).toHaveValue(
    "new-web",
  );
  await expect(page.getByLabel("最大重试次数", { exact: true })).toHaveValue(
    "7",
  );
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("button", { name: "继续编辑", exact: true }),
  ).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page.getByLabel("容器名称", { exact: true })).toHaveValue(
    "new-web",
  );
  await page.getByRole("button", { name: "取消", exact: true }).click();
  await page.getByRole("button", { name: "放弃修改", exact: true }).click();
  await expect(dialog).toBeHidden();
  expect(errors).toEqual([]);
});
test("expired creation renews in place and submits only the refreshed snapshot", async ({
  page,
}) => {
  await openCreate(page);
  await page.clock.install();
  await page.getByLabel("容器名称", { exact: true }).fill("new-web");
  await page.getByLabel("容器名称", { exact: true }).press("Enter");
  const confirm = page.getByRole("button", {
    name: "确认创建容器",
    exact: true,
  });
  await expect(confirm).toBeEnabled();
  await page.clock.runFor(61000);
  await expect(confirm).toBeDisabled();
  await page.getByRole("button", { name: "重新检查", exact: true }).click();
  await expect(confirm).toBeEnabled();
  await confirm.click();
  const calls = await page.evaluate(() => (window as any).__DOCKER__.calls);
  expect(
    calls.filter((c: any) => c.command === "prepare_docker_create"),
  ).toHaveLength(1);
  expect(
    calls.find((c: any) => c.command === "submit_docker_operation").args.request
      .previewId,
  ).toBe("create-preview-renewed");
});
test("a lost submission response keeps the original request and blocks new previews", async ({
  page,
}) => {
  await openCreate(page);
  await page.getByLabel("容器名称", { exact: true }).fill("new-web");
  await page.getByRole("button", { name: "预览创建", exact: true }).click();
  await page.evaluate(() => {
    const w = window as any,
      base = w.__TAURI_INTERNALS__.invoke;
    let first = true;
    w.__LOST_REQUESTS__ = [];
    w.__TAURI_INTERNALS__.invoke = async (command: string, args: any) => {
      if (command === "submit_docker_operation") {
        w.__LOST_REQUESTS__.push(structuredClone(args.request));
        if (first) {
          first = false;
          throw { kind: "ipc", message: "回包丢失" };
        }
      }
      return base(command, args);
    };
  });
  await page.getByRole("button", { name: "确认创建容器", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "返回编辑", exact: true }),
  ).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "重新检查", exact: true }),
  ).toBeHidden();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.getByRole("button", { name: "核对提交结果", exact: true }).click();
  await expect(page.getByRole("dialog")).toBeHidden();
  const requests = await page.evaluate(() => (window as any).__LOST_REQUESTS__);
  expect(requests).toHaveLength(2);
  expect(requests[1]).toEqual(requests[0]);
});
test("expired transfer reuses its chosen local file without another picker", async ({
  page,
}) => {
  const { panel } = await management(page);
  await page.clock.install();
  await panel.getByRole("button", { name: "web 更多操作" }).click();
  await page.getByRole("menuitem", { name: "导出容器文件（tar）…" }).click();
  await page
    .getByRole("button", { name: "选择保存位置…", exact: true })
    .click();
  await page.clock.runFor(61000);
  await page.getByRole("button", { name: "重新检查", exact: true }).click();
  const calls = await page.evaluate(() => (window as any).__DOCKER__.calls);
  expect(
    calls.filter((c: any) => c.command === "prepare_docker_transfer"),
  ).toHaveLength(1);
  expect(
    calls.filter((c: any) => c.command === "renew_docker_preview"),
  ).toHaveLength(1);
  await expect(page.locator("dialog[open]")).toHaveCount(1);
});

test("long creation keeps title and actions reachable in a short enlarged-text window", async ({
  page,
}, info) => {
  await openCreate(page);
  await page.setViewportSize({ width: 420, height: 600 });
  await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" });
  await page.evaluate(() => {
    document.documentElement.style.fontSize = "18px";
  });
  for (let i = 0; i < 6; i++)
    await page.getByRole("button", { name: "添加端口", exact: true }).click();
  const dialog = page.getByRole("dialog"),
    body = dialog.locator(".docker-flow-body");
  const header = await dialog.locator(".dialog-head").boundingBox(),
    footer = await dialog.locator(".dialog-actions").boundingBox();
  expect(header!.y).toBeGreaterThanOrEqual(0);
  expect(footer!.y + footer!.height).toBeLessThanOrEqual(600);
  expect(
    await body.evaluate(
      (e) => e.scrollHeight > e.clientHeight && e.scrollWidth <= e.clientWidth,
    ),
  ).toBe(true);
  await body.evaluate((e) => {
    e.scrollTop = e.scrollHeight;
  });
  expect((await dialog.locator(".dialog-head").boundingBox())!.y).toBe(
    header!.y,
  );
  expect((await dialog.locator(".dialog-actions").boundingBox())!.y).toBe(
    footer!.y,
  );
  await dialog.screenshot({
    path: ".qa/ui-refinement-long-form-" + info.project.name + ".png",
  });
});

test("engine changes preserve the create draft and require another preview", async ({
  page,
}) => {
  await openCreate(page);
  await page.getByLabel("容器名称", { exact: true }).fill("keep-me");
  await page.getByRole("button", { name: "预览创建", exact: true }).click();
  await page.evaluate(() => {
    const qa = (window as any).__DOCKER__;
    qa.engine.token = "replacement-engine";
    qa.emit();
  });
  await expect(page.getByLabel("容器名称", { exact: true })).toHaveValue(
    "keep-me",
  );
  await expect(page.getByRole("dialog")).toContainText("引擎已变化");
  await expect(
    page.getByRole("button", { name: "确认创建容器", exact: true }),
  ).toBeHidden();
  await page.getByRole("button", { name: "预览创建", exact: true }).click();
  const call = await page.evaluate(() =>
    (window as any).__DOCKER__.calls
      .filter((c: any) => c.command === "prepare_docker_create")
      .at(-1),
  );
  expect(call.args.engineToken).toBe("replacement-engine");
});
test("discarding an in-flight preflight releases its late preview without reopening", async ({
  page,
}) => {
  await openCreate(page);
  await page.getByLabel("容器名称", { exact: true }).fill("discard-me");
  await page.evaluate(() => {
    const w = window as any,
      base = w.__TAURI_INTERNALS__.invoke;
    w.__TAURI_INTERNALS__.invoke = (command: string, args: any) =>
      command === "prepare_docker_create"
        ? new Promise((resolve) => {
            w.__FINISH_CREATE__ = async () =>
              resolve(await base(command, args));
          })
        : base(command, args);
  });
  await page.getByRole("button", { name: "预览创建", exact: true }).click();
  await page.getByRole("button", { name: "取消", exact: true }).click();
  await page.getByRole("button", { name: "放弃修改", exact: true }).click();
  await page.evaluate(() => (window as any).__FINISH_CREATE__());
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).__DOCKER__.calls.filter(
            (c: any) => c.command === "release_docker_preview",
          ).length,
      ),
    )
    .toBe(1);
  await expect(page.getByRole("dialog")).toBeHidden();
});
