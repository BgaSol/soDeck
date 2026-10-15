import { test, expect, type Page } from "@playwright/test";
async function setup(page: Page) {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.addInitScript(() => {
    const w = window as any;
    let serial = 1;
    const callbacks = new Map<number, Function>();
    const events = new Map<string, Map<number, number>>();
    const calls: any[] = [];
    const tasks: any[] = [];
    const servers = [
      {
        id: "a",
        name: "Server A",
        host: "entry.example",
        port: 2222,
        username: "ops",
        tags: [],
        authType: "password",
        privateKey: "",
        hasSavedPassword: true,
        jumpServerId: null,
        directHost: "10.0.0.1",
        directPort: 22,
      },
    ];
    const entries = [
      {
        name: "folder",
        path: "/home/ops/folder",
        isDir: true,
        size: 0,
        modified: 0,
      },
      {
        name: "one.txt",
        path: "/home/ops/one.txt",
        isDir: false,
        size: 24,
        modified: 0,
      },
      {
        name: "two.txt",
        path: "/home/ops/two.txt",
        isDir: false,
        size: 30,
        modified: 0,
      },
    ];
    const directory = (path: string) => ({
      path,
      resolvedPath: path,
      entries:
        path === "/home/ops"
          ? entries
          : path === "/"
            ? [
                {
                  name: "home",
                  path: "/home",
                  isDir: true,
                  size: 0,
                  modified: 0,
                },
              ]
            : path === "/home"
              ? [
                  {
                    name: "ops",
                    path: "/home/ops",
                    isDir: true,
                    size: 0,
                    modified: 0,
                  },
                ]
              : [],
    });
    w.__TEST__ = {
      calls,
      tasks,
      entries,
      missingEndpoint: false,
      events,
      emit: (name: string, payload: any) =>
        events
          .get(name)
          ?.forEach((handler, id) =>
            callbacks.get(handler)?.({ event: name, id, payload }),
          ),
    };
    w.isTauri = true;
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    w.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main" },
      },
      transformCallback: (fn: Function) => {
        const id = serial++;
        callbacks.set(id, fn);
        return id;
      },
      unregisterCallback: (id: number) => callbacks.delete(id),
      invoke: async (command: string, args: any = {}) => {
        calls.push({ command, args: JSON.parse(JSON.stringify(args)) });
        if (command === "window_chrome") return "custom";
        if (command === "plugin:event|listen") {
          const id = serial++;
          if (!events.has(args.event)) events.set(args.event, new Map());
          events.get(args.event)!.set(id, args.handler);
          return id;
        }
        if (command === "plugin:event|unlisten") {
          events.get(args.event)?.delete(args.eventId);
          return;
        }
        if (command === "get_remote_clipboard")
          return structuredClone(w.__TEST__.clipboard || null);
        if (command === "set_remote_clipboard") {
          w.__TEST__.clipboard = {
            id: "clipboard-" + serial++,
            paths: args.paths,
            intent: args.intent,
            sameServer: true,
            reserved: false,
          };
          w.__TEST__.emit("remote-clipboard-changed", null);
          return;
        }
        if (command === "clear_remote_clipboard") {
          w.__TEST__.clipboard = null;
          w.__TEST__.emit("remote-clipboard-changed", null);
          return;
        }
        if (command === "prepare_remote_paste")
          return {
            id: "paste-draft",
            kind:
              w.__TEST__.clipboard.intent === "move"
                ? "serverMove"
                : "serverCopy",
            sourceName: "Server A",
            sourcePaths: w.__TEST__.clipboard.paths,
            count: w.__TEST__.clipboard.paths.length,
            targetName: "Server A",
            targetPath: args.parentPath,
            directEndpoint: null,
            endpointRequired: false,
          };
        if (command === "list_servers") return structuredClone(servers);
        if (command === "list_transfers") {
          if (w.__TEST__.failList) throw { message: "任务列表暂时不可用" };
          if (w.__TEST__.holdNextList) {
            w.__TEST__.holdNextList = false;
            const snapshot = structuredClone(tasks);
            await new Promise<void>((resolve) => {
              w.__TEST__.releaseList = resolve;
            });
            delete w.__TEST__.releaseList;
            return snapshot;
          }
          return structuredClone(tasks);
        }
        if (command === "save_server") return args.server;
        if (command === "open_connection") {
          const id = "session-a";
          setTimeout(() => {
            args.events.onmessage({
              type: "ready",
              connection: {
                target: servers[0],
                hops: [{ index: 0, role: "target", server: servers[0] }],
                identity: "identity-a",
              },
            });
            args.events.onmessage({
              type: "data",
              data: btoa("fixture ready\r\n"),
            });
          }, 0);
          return id;
        }
        if (command === "list_directory_chain")
          return {
            homePath: "/home/ops",
            path: args.path || "/home/ops",
            directories: ["/", "/home", "/home/ops"].map((path) => ({
              path,
              directory: directory(path),
            })),
          };
        if (command === "list_directory") return directory(args.path);
        if (command === "create_remote_entry") {
          if (args.name === "exists")
            throw { kind: "conflict", message: "同名项目已存在" };
          const path = args.parentPath + "/" + args.name;
          entries.push({
            name: args.name,
            path,
            isDir: args.kind === "directory",
            size: 0,
            modified: 0,
          });
          return path;
        }
        if (command === "pick_upload_files" || command === "claim_file_drop")
          return {
            id: command === "pick_upload_files" ? "upload-draft" : "drop-draft",
            kind:
              command === "pick_upload_files"
                ? "upload"
                : w.__TEST__.dropKind || "direct",
            sourceName: "Source",
            sourcePaths: ["/data/report"],
            count: 1,
            targetName: "Server A",
            targetPath: args.parentPath,
            directEndpoint: w.__TEST__.missingEndpoint
              ? null
              : { host: "10.0.0.2", port: 2222, username: "ops" },
            endpointRequired:
              command === "claim_file_drop" && w.__TEST__.missingEndpoint,
          };
        if (command === "set_transfer_endpoint")
          return {
            id: "updated-draft",
            kind: "direct",
            sourceName: "Source",
            sourcePaths: ["/data/report"],
            count: 1,
            targetName: "Server B",
            targetPath: "/home/ops/folder",
            directEndpoint: {
              host: args.host,
              port: args.port,
              username: "ops",
            },
            endpointRequired: false,
          };
        if (command === "list_copy_targets")
          return [
            { token: "target-b", serverName: "Server B", path: "/srv/inbox" },
            { token: "target-c", serverName: "Server C", path: "/backups" },
          ];
        if (command === "prepare_server_copy")
          return {
            id: "copy-draft",
            kind: "direct",
            sourceName: "Server A",
            sourcePaths: args.paths,
            count: args.paths.length,
            targetName: "Server B",
            targetPath: "/srv/inbox",
            directEndpoint: { host: "10.0.0.2", port: 22, username: "ops" },
            endpointRequired: false,
          };
        if (command === "start_transfer") {
          tasks.push({
            id: "task-1",
            runId: "run-1",
            kind: "direct",
            sourceName: "Server A",
            sourcePath: "/data/report",
            targetName: "Server B",
            targetPath: "/home/ops/folder",
            conflict: args.conflict,
            status: "checking",
            bytesDone: 0,
            bytesTotal: 0,
            filesDone: 0,
            filesTotal: 0,
            skipped: 0,
            error: null,
            warnings: [],
          });
          return "task-1";
        }
        if (command === "cancel_transfer") {
          tasks.find((t) => t.id === args.id).status = "cancelling";
          return;
        }
        if (command === "retry_transfer") {
          const task = tasks.find((t) => t.id === args.id);
          task.lastAttempt = {
            runId: task.runId,
            startedAt: task.lastStartedAt ?? null,
            status: task.status,
            error: task.error,
          };
          task.runId = "retry-" + serial++;
          task.sortOrder =
            Math.max(0, ...tasks.map((t) => t.sortOrder ?? 0)) + 1;
          task.lastStartedAt = Date.now();
          task.status = "queued";
          task.error = null;
          w.__TEST__.emit("transfer-updated", {});
          return;
        }
        if (command === "cleanup_transfer") {
          tasks.find((t) => t.id === args.id).cleanupPending = false;
          return;
        }
        if (command === "detect_docker")
          return { state: "missing", message: "fixture", endpoint: "" };
        if (
          [
            "session_count",
            "log_session_count",
            "unsaved_editor_count",
          ].includes(command)
        )
          return 0;
        return null;
      },
    };
  });
  await page.goto("/");
  await page.getByRole("button", { name: "新建连接", exact: true }).click();
  await page.locator(".server-row").filter({ hasText: "Server A" }).dblclick();
  await expect(
    page.locator(".file-row[data-path='/home/ops/one.txt']"),
  ).toBeVisible();
  return errors;
}
async function emitDrop(
  page: Page,
  selector: string,
  kind = "localFiles",
  phase = "drop",
) {
  await page.locator(selector).scrollIntoViewIfNeeded();
  const box = await page.locator(selector).boundingBox();
  expect(box).toBeTruthy();
  await page.evaluate(
    ({ box, kind, phase }) =>
      (window as any).__TEST__.emit("desktop-drag", {
        kind,
        phase,
        token: "native-token",
        x: box!.x + box!.width / 2,
        y: box!.y + box!.height / 2,
        count: 2,
        label: "2 个项目",
      }),
    { box, kind, phase },
  );
}

test("create folder and file, retain validation errors and refresh only the target", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.getByRole("button", { name: "新建文件夹", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toContainText("/home/ops");
  await dialog.getByLabel("新项目名称").fill("exists");
  await dialog.getByRole("button", { name: "创建", exact: true }).click();
  await expect(dialog.getByRole("alert")).toHaveText("同名项目已存在");
  await expect(dialog.getByLabel("新项目名称")).toHaveValue("exists");
  await dialog.getByLabel("新项目名称").fill("新目录");
  await dialog.getByLabel("新项目名称").press("Enter");
  await expect(
    page.locator(".file-row[data-path='/home/ops/新目录']"),
  ).toBeVisible();
  await page.locator(".file-row[data-path='/home/ops']").click();
  await page.getByRole("button", { name: "新建文件", exact: true }).click();
  await dialog.getByLabel("新项目名称").fill("notes.txt");
  await dialog.getByRole("button", { name: "创建", exact: true }).click();
  await expect(
    page.locator(".file-row[data-path='/home/ops/notes.txt']"),
  ).toBeVisible();
  expect(errors).toEqual([]);
});

test("same-server drop uses backend classification without direct credentials", async ({
  page,
}) => {
  const errors = await setup(page);
  for (const kind of ["serverCopy", "serverMove"]) {
    await page.evaluate((kind) => {
      (window as any).__TEST__.dropKind = kind;
    }, kind);
    await emitDrop(
      page,
      ".file-row[data-path='/home/ops/folder']",
      "remoteFiles",
    );
    const dialog = page.getByRole("dialog");
    await expect(dialog.getByRole("heading")).toHaveText(
      kind === "serverMove" ? "服务器内移动" : "服务器内复制",
    );
    await expect(dialog.getByLabel("本次直连真实地址")).toHaveCount(0);
    await expect(dialog.locator("input[type=password]")).toHaveCount(0);
    if (kind === "serverMove")
      await page.screenshot({ path: ".qa/same-server-move.png" });
    await dialog.getByRole("button", { name: "开始传输", exact: true }).click();
  }
  expect(
    await page.evaluate(() =>
      (window as any).__TEST__.calls
        .filter((c: any) => c.command === "start_transfer")
        .every((c: any) => !c.args.auth),
    ),
  ).toBe(true);
  expect(errors).toEqual([]);
});

test("remote clipboard shortcuts are scoped and cut markers clear with Escape", async ({
  page,
}) => {
  const errors = await setup(page);
  const file = page.locator(".file-row[data-path='/home/ops/one.txt']");
  await file.click();
  await file.press("Control+x");
  await expect(file).toHaveClass(/cut-pending/);
  const folder = page.locator(".file-row[data-path='/home/ops/folder']");
  await folder.click();
  await folder.press("Control+v");
  await expect(page.getByRole("dialog")).toContainText("服务器内移动");
  await expect(page.getByRole("dialog")).toContainText("/home/ops/folder");
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "取消", exact: true })
    .click();
  await file.focus();
  await file.press("Escape");
  await expect(file).not.toHaveClass(/cut-pending/);
  const input = page.getByLabel("远程目录路径");
  await input.focus();
  await input.press("Control+c");
  expect(
    await page.evaluate(
      () =>
        (window as any).__TEST__.calls.filter(
          (c: any) => c.command === "set_remote_clipboard",
        ).length,
    ),
  ).toBe(1);
  expect(errors).toEqual([]);
});

test("blank-area paste and clipboard events support other windows and reservations", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.evaluate(() => {
    const qa = (window as any).__TEST__;
    qa.clipboard = {
      id: "from-other-window",
      paths: ["/srv/shared"],
      intent: "copy",
      sameServer: true,
      reserved: false,
    };
    qa.emit("remote-clipboard-changed", null);
  });
  await expect(page.getByLabel("粘贴项目", { exact: true })).toBeEnabled();
  const tree = page.getByRole("tree", { name: "远程文件树" });
  await tree.dispatchEvent("contextmenu", { clientX: 100, clientY: 400 });
  await expect(
    page.getByRole("menuitem", { name: "粘贴到此处", exact: true }),
  ).toBeVisible();
  await page.getByRole("menuitem", { name: "粘贴到此处", exact: true }).click();
  await expect(page.getByRole("dialog")).toContainText("服务器内复制");
  expect(
    await page.evaluate(
      () =>
        (window as any).__TEST__.calls.find(
          (c: any) => c.command === "prepare_remote_paste",
        ).args.parentPath,
    ),
  ).toBe("/home/ops");
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "取消", exact: true })
    .click();
  await page.evaluate(() => {
    const q = (window as any).__TEST__;
    q.clipboard.intent = "move";
    q.clipboard.reserved = true;
    q.emit("remote-clipboard-changed", null);
  });
  await expect(page.getByLabel("粘贴项目", { exact: true })).toBeDisabled();
  await page.evaluate(() => {
    const q = (window as any).__TEST__;
    q.clipboard.reserved = false;
    q.clipboard.sameServer = false;
    q.emit("remote-clipboard-changed", null);
  });
  await expect(page.getByLabel("粘贴项目", { exact: true })).toBeDisabled();
  expect(errors).toEqual([]);
});

test("missing direct endpoint is saved and reviewed before starting the replacement draft", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.evaluate(() => {
    (window as any).__TEST__.missingEndpoint = true;
  });
  await emitDrop(
    page,
    ".file-row[data-path='/home/ops/folder']",
    "remoteFiles",
  );
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("本次直连真实地址").fill("10.20.30.40");
  await dialog.getByLabel("本次直连端口").fill("2200");
  await dialog.getByRole("button", { name: "保存地址并确认" }).click();
  await expect(dialog).toContainText("ops@10.20.30.40:2200");
  expect(
    await page.evaluate(
      () =>
        (window as any).__TEST__.calls.filter(
          (c: any) => c.command === "start_transfer",
        ).length,
    ),
  ).toBe(0);
  await page.screenshot({ path: ".qa/direct-endpoint-confirm.png" });
  await dialog.getByRole("button", { name: "开始传输", exact: true }).click();
  expect(
    await page.evaluate(
      () =>
        (window as any).__TEST__.calls.find(
          (c: any) => c.command === "start_transfer",
        ).args.draftId,
    ),
  ).toBe("updated-draft");
  expect(errors).toEqual([]);
});

test("keyboard copy chooses a connected directory and shows byte progress before commit", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.locator(".file-row[data-path='/home/ops/one.txt']").click();
  const copy = page.getByRole("button", {
    name: "复制到另一台服务器",
    exact: true,
  });
  await copy.focus();
  await copy.press("Enter");
  let dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("radio", { name: /Server B/ })).toBeChecked();
  await page.screenshot({ path: ".qa/direct-copy-target.png" });
  await dialog.getByRole("button", { name: "下一步" }).click();
  dialog = page.getByRole("dialog");
  await expect(dialog).toContainText("/srv/inbox");
  await expect(dialog).toContainText("ops@10.0.0.2:22");
  await dialog.getByRole("button", { name: "开始传输", exact: true }).click();
  await page.evaluate(() => {
    const qa = (window as any).__TEST__;
    Object.assign(qa.tasks[0], {
      status: "running",
      bytesDone: 1024 * 1024,
      bytesTotal: 2 * 1024 * 1024,
      filesDone: 0,
      filesTotal: 1,
      currentBytes: 1024 * 1024,
      committedBytes: 0,
      currentFile: "/data/large.bin",
    });
    qa.emit("transfer-updated", {});
  });
  const panel = page.getByRole("region", { name: "传输任务" });
  await page.getByRole("button", { name: "查看传输任务" }).click();
  await expect(panel.getByLabel("传输进度")).toHaveAttribute("value", "0.5");
  await panel.getByRole("button", { name: "详情", exact: true }).click();
  await expect(panel).toContainText("已提交 0 B");
  await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" });
  await page.screenshot({ path: ".qa/direct-progress-dark.png" });
  await page.evaluate(() => {
    const qa = (window as any).__TEST__;
    qa.tasks[0].status = "committing";
    qa.tasks[0].bytesDone = qa.tasks[0].bytesTotal;
    qa.emit("transfer-updated", {});
  });
  await expect(panel.getByLabel("传输进度")).not.toHaveAttribute("value");
  await expect(panel).toContainText("提交文件");
  expect(errors).toEqual([]);
});

test("native folder drop has a visible target and frozen destination", async ({
  page,
}) => {
  const errors = await setup(page);
  const folder = ".file-row[data-path='/home/ops/folder']";
  await emitDrop(page, folder, "localFiles", "over");
  await expect(page.locator(folder)).toHaveClass(/drop-target/);
  await page.screenshot({ path: ".qa/transfer-drop-light.png" });
  await emitDrop(page, folder);
  const dialog = page.getByRole("dialog");
  await expect(dialog).toContainText("/home/ops/folder");
  await dialog.getByLabel("传输同名处理").selectOption("overwrite");
  await expect(dialog).toContainText("将替换目标中的同名文件");
  await dialog.getByRole("button", { name: "确认覆盖并开始" }).click();
  await page.getByRole("button", { name: "查看传输任务" }).click();
  await expect(page.getByRole("region", { name: "传输任务" })).toContainText(
    "检查连接与权限",
  );
  const calls = await page.evaluate(() => (window as any).__TEST__.calls);
  expect(
    calls.find((c: any) => c.command === "claim_file_drop").args.parentPath,
  ).toBe("/home/ops/folder");
  await page.getByLabel("取消传输").click();
  await expect(page.getByRole("region", { name: "传输任务" })).toContainText(
    "正在停止",
  );
  expect(errors).toEqual([]);
});

test("regular file is not a drop target; remote drops open direct-copy confirmation", async ({
  page,
}) => {
  await setup(page);
  await emitDrop(page, ".file-row[data-path='/home/ops/one.txt']");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await emitDrop(
    page,
    ".file-row[data-path='/home/ops/folder']",
    "remoteFiles",
  );
  const dialog = page.getByRole("dialog");
  await expect(dialog).toContainText("源服务器直接连接目标服务器");
  await dialog.getByRole("button", { name: "取消", exact: true }).click();
  expect(
    await page.evaluate(
      () =>
        (window as any).__TEST__.calls.filter(
          (c: any) => c.command === "start_transfer",
        ).length,
    ),
  ).toBe(0);
});

test("multiselection survives directory refresh and controls editor availability", async ({
  page,
}) => {
  await setup(page);
  const one = page.locator(".file-row[data-path='/home/ops/one.txt']"),
    two = page.locator(".file-row[data-path='/home/ops/two.txt']");
  await one.click();
  await two.click({ modifiers: ["ControlOrMeta"] });
  await expect(one).toHaveAttribute("aria-selected", "true");
  await expect(two).toHaveAttribute("aria-selected", "true");
  await expect(page.getByLabel("编辑选中文件")).toBeDisabled();
  await page.getByLabel("刷新目录").click();
  await expect(one).toHaveAttribute("aria-selected", "true");
  await expect(two).toHaveAttribute("aria-selected", "true");
});

test("upload picker and new configuration fit dark small windows", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" });
  await page.getByLabel("上传文件", { exact: true }).click();
  await expect(page.getByRole("dialog")).toContainText("上传到服务器");
  await page.screenshot({ path: ".qa/transfer-upload-dark.png" });
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "取消", exact: true })
    .click();
  await page.getByRole("button", { name: "新建连接", exact: true }).click();
  await page.locator(".server-row").click();
  await page.getByRole("button", { name: "编辑", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("真实地址", { exact: true }).scrollIntoViewIfNeeded();
  await expect(dialog.getByLabel("直连端口")).toHaveValue("22");
  await dialog.getByRole("button", { name: "使用当前连接地址" }).click();
  await expect(dialog.getByLabel("真实地址", { exact: true })).toHaveValue(
    "entry.example",
  );
  await expect(dialog.getByLabel("直连端口")).toHaveValue("2222");
  await page.setViewportSize({ width: 860, height: 580 });
  await dialog.getByLabel("真实地址", { exact: true }).scrollIntoViewIfNeeded();
  await page.screenshot({ path: ".qa/transfer-server-small-dark.png" });
  expect(errors).toEqual([]);
});

test("keyboard range selection and native drag completion restore normal clicks", async ({
  page,
}) => {
  await setup(page);
  const one = page.locator(".file-row[data-path='/home/ops/one.txt']");
  const two = page.locator(".file-row[data-path='/home/ops/two.txt']");
  await one.click();
  await one.press("Shift+ArrowDown");
  await expect(one).toHaveAttribute("aria-selected", "true");
  await expect(two).toHaveAttribute("aria-selected", "true");
  const box = (await two.boundingBox())!;
  await page.mouse.move(box.x + 30, box.y + 10);
  await page.mouse.down();
  await page.mouse.move(box.x + 50, box.y + 10);
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).__TEST__.calls.filter(
            (c: any) => c.command === "begin_file_drag",
          ).length,
      ),
    )
    .toBe(1);
  await page.evaluate(() =>
    (window as any).__TEST__.emit("desktop-drag-ended", {
      kind: "remoteFiles",
      token: "token",
      cancelled: true,
    }),
  );
  await page.mouse.up();
  await page.waitForTimeout(650);
  await one.click();
  await expect(two).toHaveAttribute("aria-selected", "false");
});

test("uncertain commits require reconciliation; interrupted execution can retry with fresh staging", async ({
  page,
}) => {
  await setup(page);
  await page.getByLabel("上传文件", { exact: true }).click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "开始传输", exact: true })
    .click();
  await page.evaluate(() => {
    const qa = (window as any).__TEST__;
    qa.tasks[0].status = "unknown";
    qa.tasks[0].error = "完成确认丢失";
    qa.emit("transfer-updated", {});
  });
  await page.getByRole("button", { name: "查看传输任务" }).click();
  await expect(
    page.getByRole("button", { name: "核对状态", exact: true }),
  ).toBeVisible();
  await expect(page.getByLabel("重试传输")).toHaveCount(0);
  await page.evaluate(() => {
    const qa = (window as any).__TEST__;
    qa.tasks[0].status = "interrupted";
    qa.emit("transfer-updated", {});
  });
  await expect(page.getByRole("region", { name: "传输任务" })).toContainText(
    "已中断，请检查目标",
  );
  await expect(page.getByLabel("重试传输")).toHaveCount(0);
  await page.getByRole("button", { name: "重试未完成项", exact: true }).click();
  const retry = page.getByRole("dialog", { name: "重试未完成项目" });
  await expect(retry).toBeVisible();
  await expect(retry.getByLabel("复用已确认停止的临时数据")).toBeDisabled();
  await retry.getByLabel("直传认证方式").selectOption("password");
  await retry.getByLabel("直传本次密码").fill("test-only-password");
  await retry.getByRole("button", { name: "重试传输", exact: true }).click();
  expect(
    await page.evaluate(
      () =>
        (window as any).__TEST__.calls.find(
          (c: any) => c.command === "retry_transfer",
        ).args,
    ),
  ).toEqual({
    id: "task-1",
    auth: { mode: "password", password: "test-only-password" },
    resume: false,
  });
});

test("direct authentication and digest options are explicit; cleanup remains independent", async ({
  page,
}) => {
  await setup(page);
  await emitDrop(
    page,
    ".file-row[data-path='/home/ops/folder']",
    "remoteFiles",
  );
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("直传认证方式").selectOption("key");
  await dialog.getByLabel("源服务器私钥路径").fill("/home/ops/.ssh/direct-key");
  await dialog.getByLabel(/逐文件 SHA-256/).check();
  await dialog.getByRole("button", { name: "开始传输", exact: true }).click();
  expect(
    await page.evaluate(
      () =>
        (window as any).__TEST__.calls.find(
          (c: any) => c.command === "start_transfer",
        ).args,
    ),
  ).toMatchObject({
    auth: { mode: "key", keyPath: "/home/ops/.ssh/direct-key" },
    verifySha256: true,
  });
  await page.evaluate(() => {
    const qa = (window as any).__TEST__;
    Object.assign(qa.tasks[0], {
      status: "completed",
      cleanupPending: true,
      temporaryPaths: ["B · /data/.ssh-workspace-transfer-fixture"],
    });
    qa.emit("transfer-updated", {});
  });
  await page.getByRole("button", { name: "查看传输任务" }).click();
  await expect(page.getByRole("region", { name: "传输任务" })).toContainText(
    "已完成",
  );
  await page
    .getByRole("region", { name: "传输任务" })
    .getByRole("button", { name: "详情", exact: true })
    .click();
  await page.getByRole("button", { name: "清理临时资源" }).click();
  expect(
    await page.evaluate(() =>
      (window as any).__TEST__.calls.some(
        (c: any) => c.command === "cleanup_transfer",
      ),
    ),
  ).toBe(true);
});

test("native tab drag preserves begin, drop and finish IPC", async ({
  page,
}) => {
  const errors = await setup(page);
  const tab = page.getByRole("tab", { name: /Server A/ });
  const box = (await tab.boundingBox())!;
  await page.mouse.move(box.x + 15, box.y + 15);
  await page.mouse.down();
  await page.mouse.move(box.x + 40, box.y + 15);
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).__TEST__.calls.filter(
            (c: any) => c.command === "begin_workspace_drag",
          ).length,
      ),
    )
    .toBe(1);
  const token = await page.evaluate(
    () =>
      (window as any).__TEST__.calls.find(
        (c: any) => c.command === "begin_workspace_drag",
      ).args.token,
  );
  await page.evaluate(
    ({ x, y, token }) =>
      (window as any).__TEST__.emit("desktop-drag", {
        phase: "drop",
        kind: "workspaceTab",
        token,
        x,
        y,
        count: 1,
        label: "移动会话",
      }),
    { x: box.x + 40, y: box.y + 15, token },
  );
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).__TEST__.calls.filter(
            (c: any) => c.command === "drop_tab",
          ).length,
      ),
    )
    .toBe(1);
  await page.evaluate(
    (token) =>
      (window as any).__TEST__.emit("desktop-drag-ended", {
        kind: "workspaceTab",
        token,
        cancelled: false,
      }),
    token,
  );
  await page.mouse.up();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).__TEST__.calls.filter(
            (c: any) => c.command === "finish_tab_drag",
          ).length,
      ),
    )
    .toBe(1);
  expect(errors).toEqual([]);
});

test("quitting includes uploads and direct transfers without native confirm", async ({
  page,
}) => {
  await setup(page);
  await page.evaluate(() => {
    window.confirm = () => {
      throw new Error("native confirm is unavailable");
    };
  });
  await page.getByLabel("上传文件", { exact: true }).click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "开始传输", exact: true })
    .click();
  await page.evaluate(() =>
    (window as any).__TEST__.emit("tauri://close-requested", {}),
  );
  const dialog = page.getByRole("dialog");
  await expect(dialog).toContainText("1 个传输尚未完成");
  await dialog.getByRole("button", { name: "继续使用" }).click();
  expect(
    await page.evaluate(
      () =>
        (window as any).__TEST__.calls.filter(
          (c: any) => c.command === "quit_app",
        ).length,
    ),
  ).toBe(0);
  await page.evaluate(() =>
    (window as any).__TEST__.emit("tauri://close-requested", {}),
  );
  await dialog.getByRole("button", { name: "确认退出" }).click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).__TEST__.calls.filter(
            (c: any) => c.command === "quit_app",
          ).length,
      ),
    )
    .toBe(1);
});

async function seedTransferTasks(
  page: Page,
  patches: Record<string, unknown>[],
) {
  await page.evaluate((patches) => {
    const qa = (window as any).__TEST__;
    qa.tasks.splice(
      0,
      qa.tasks.length,
      ...patches.map((patch, i) => ({
        id: `task-${i}`,
        runId: `run-${i}`,
        sortOrder: i + 1,
        createdAt: Date.now() - i * 60000,
        lastStartedAt: Date.now() - i * 60000,
        kind: "upload",
        sourceName: "本机",
        sourcePath: `/releases/release-${i}.tar.gz`,
        sourcePaths: [`/releases/release-${i}.tar.gz`],
        targetName: "production",
        targetPath: "/opt/releases",
        conflict: "rename",
        status: "completed",
        bytesDone: 100,
        bytesTotal: 200,
        filesDone: 1,
        filesTotal: 1,
        skipped: 0,
        error: null,
        warnings: [],
        ...patch,
      })),
    );
    qa.emit("transfer-updated", {});
  }, patches);
  await expect(
    page.getByRole("button", { name: "查看传输任务" }),
  ).toBeVisible();
}

test("transfer popover stays collapsed on new tasks, sorts all types and preserves workspace size", async ({
  page,
}) => {
  const errors = await setup(page);
  await seedTransferTasks(page, [
    { id: "old", sortOrder: 1, kind: "direct" },
    { id: "active", sortOrder: 2, status: "running" },
    {
      id: "new",
      sortOrder: 3,
      kind: "download",
      status: "failed",
      error: "连接中断",
    },
  ]);
  const trigger = page.getByRole("button", { name: "查看传输任务" });
  const panel = page.getByRole("region", { name: "传输任务" });
  await expect(trigger).toHaveAttribute("aria-expanded", "false");
  await expect(page.getByLabel("总传输进度")).toHaveAttribute("value", "0.5");
  const terminal = page.locator(".terminal-host").first();
  const before = await terminal.boundingBox();
  await trigger.click();
  await expect(panel).toBeVisible();
  await expect(panel.locator("[data-task-id]")).toHaveCount(3);
  expect(
    await panel
      .locator("[data-task-id]")
      .evaluateAll((rows) =>
        rows.map((row) => row.getAttribute("data-task-id")),
      ),
  ).toEqual(["new", "active", "old"]);
  expect(await terminal.boundingBox()).toEqual(before);
  await page.keyboard.press("Escape");
  await expect(trigger).toBeFocused();
  await expect(trigger).toHaveAttribute("aria-expanded", "false");
  await trigger.click();
  const file = page.locator(".file-row[data-path='/home/ops/one.txt']");
  await file.click();
  await expect(file).toHaveAttribute("aria-selected", "true");
  await expect(trigger).toHaveAttribute("aria-expanded", "false");
  await page.evaluate(() => {
    const qa = (window as any).__TEST__;
    qa.tasks.push({
      ...qa.tasks[0],
      id: "latest",
      runId: "new-run",
      sortOrder: 4,
      status: "queued",
    });
    window.dispatchEvent(new Event("workspace-transfer-started"));
  });
  await expect(trigger).toContainText("2 项进行中");
  await expect(trigger).toHaveAttribute("aria-expanded", "false");
  await trigger.click();
  await expect(panel.locator("[data-task-id]").first()).toHaveAttribute(
    "data-task-id",
    "latest",
  );
  await expect(panel.getByRole("progressbar").first()).not.toHaveAttribute(
    "value",
  );
  await page.screenshot({
    path: ".qa/transfer-popover-light.png",
    animations: "disabled",
  });
  expect(errors).toEqual([]);
});

test("retry modal owns Escape and moves only the retried task to the top", async ({
  page,
}) => {
  const errors = await setup(page);
  await seedTransferTasks(page, [
    { id: "failed", sortOrder: 1, status: "failed", error: "连接中断" },
    { id: "done", sortOrder: 2 },
  ]);
  await page.getByRole("button", { name: "查看传输任务" }).click();
  const panel = page.getByRole("region", { name: "传输任务" });
  const retry = panel.locator('[data-task-id="failed"]').getByLabel("重试传输");
  await retry.click();
  const dialog = page.getByRole("dialog", { name: "重试未完成项目" });
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(panel).toBeVisible();
  await retry.click();
  await dialog.getByRole("button", { name: "重试传输", exact: true }).click();
  await expect(dialog).toBeHidden();
  await expect(panel.locator("[data-task-id]").first()).toHaveAttribute(
    "data-task-id",
    "failed",
  );
  await panel
    .locator('[data-task-id="failed"]')
    .getByRole("button", { name: "详情", exact: true })
    .click();
  await expect(panel).toContainText("上次执行：失败 · 连接中断");
  await page.evaluate(() => {
    const qa = (window as any).__TEST__;
    qa.tasks.find((task: any) => task.id === "failed").status = "completed";
    qa.emit("transfer-updated", {});
  });
  await expect(page.getByLabel("总传输进度")).toHaveCount(0);
  await expect(panel).toBeVisible();
  await expect(panel.locator("[data-task-id]")).toHaveCount(2);
  expect(errors).toEqual([]);
});

test("new tasks preserve the history scroll anchor and details across collapse", async ({
  page,
}) => {
  await setup(page);
  await seedTransferTasks(
    page,
    Array.from({ length: 18 }, (_, i) => ({
      id: `history-${i}`,
      sortOrder: i + 1,
    })),
  );
  const trigger = page.getByRole("button", { name: "查看传输任务" });
  await trigger.click();
  const list = page.locator(".transfer-task-list");
  await list.evaluate((el) => {
    el.scrollTop = 400;
    el.dispatchEvent(new Event("scroll"));
  });
  const anchor = await list.evaluate((el) => {
    const top = el.getBoundingClientRect().top;
    const row = [...el.querySelectorAll<HTMLElement>("[data-task-id]")].find(
      (row) => row.getBoundingClientRect().bottom > top + 1,
    )!;
    return {
      id: row.dataset.taskId,
      offset: row.getBoundingClientRect().top - top,
    };
  });
  await page.evaluate(() => {
    const qa = (window as any).__TEST__;
    qa.tasks.push({
      ...qa.tasks[0],
      id: "incoming",
      runId: "incoming",
      sortOrder: 19,
    });
    qa.emit("transfer-updated", {});
  });
  await expect(
    page.getByRole("button", { name: /有 1 项新任务/ }),
  ).toBeVisible();
  const after = await list.evaluate((el, id) => {
    const row = [...el.querySelectorAll<HTMLElement>("[data-task-id]")].find(
      (row) => row.dataset.taskId === id,
    )!;
    return row.getBoundingClientRect().top - el.getBoundingClientRect().top;
  }, anchor.id);
  expect(Math.abs(after - anchor.offset)).toBeLessThan(2);
  await page.getByRole("button", { name: /有 1 项新任务/ }).click();
  await expect.poll(() => list.evaluate((el) => el.scrollTop)).toBe(0);
  const newest = page.locator('[data-task-id="incoming"]');
  await newest.getByRole("button", { name: "详情", exact: true }).click();
  await trigger.click();
  await trigger.click();
  await expect(
    newest.getByRole("button", { name: "收起详情" }),
  ).toHaveAttribute("aria-expanded", "true");
});

test("popover fits a narrow dark window and preserves rows when refresh fails", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" });
  await page.setViewportSize({ width: 380, height: 580 });
  await seedTransferTasks(page, [
    {
      id: "large",
      status: "running",
      sourcePaths: ["/data/很长的中文目录/very-long-release-name.tar.gz"],
      bytesDone: 124 * 1024 * 1024,
      bytesTotal: 200 * 1024 * 1024,
    },
    {
      id: "failed",
      status: "failed",
      error: "连接已经断开，请检查目标服务器。",
    },
    { id: "done", kind: "direct" },
  ]);
  const trigger = page.getByRole("button", { name: "查看传输任务" });
  await trigger.click();
  const panel = page.getByRole("region", { name: "传输任务" });
  const box = (await panel.boundingBox())!;
  expect(box.x).toBeGreaterThanOrEqual(0);
  expect(box.x + box.width).toBeLessThanOrEqual(380);
  expect(box.y).toBeGreaterThanOrEqual(0);
  expect(box.y + box.height).toBeLessThan((await trigger.boundingBox())!.y);
  expect(await panel.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(
    true,
  );
  await page.screenshot({
    path: ".qa/transfer-popover-narrow-dark.png",
    animations: "disabled",
  });
  await page.evaluate(() => {
    const qa = (window as any).__TEST__;
    qa.failList = true;
    qa.emit("transfer-updated", {});
  });
  await expect(panel.getByRole("alert")).toContainText("任务刷新失败");
  await expect(panel.locator("[data-task-id]")).toHaveCount(3);
  await page.evaluate(() => {
    const qa = (window as any).__TEST__;
    qa.failList = false;
    qa.emit("transfer-updated", {});
  });
  await expect(panel.getByRole("alert")).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("detached window shares task behavior and keeps its bottom entry inside the viewport", async ({
  page,
}) => {
  const errors = await setup(page);
  await page.goto("/?workspace=");
  await expect(page.locator(".detached-app")).toBeVisible();
  await page.setViewportSize({ width: 720, height: 580 });
  await seedTransferTasks(page, [{ id: "detached", status: "running" }]);
  const trigger = page.getByRole("button", { name: "查看传输任务" });
  await expect(trigger).toHaveAttribute("aria-expanded", "false");
  const workspaces = page.locator(".detached-app .workspaces");
  const before = await workspaces.boundingBox();
  await trigger.click();
  await expect(page.getByRole("region", { name: "传输任务" })).toBeVisible();
  expect(await workspaces.boundingBox()).toEqual(before);
  const box = (await trigger.boundingBox())!;
  expect(box.y + box.height).toBeLessThanOrEqual(580);
  await page.getByLabel("取消传输").click();
  await expect(page.getByRole("region", { name: "传输任务" })).toContainText(
    "正在停止",
  );
  expect(errors).toEqual([]);
});

test("an update received during a list request is fetched after that request finishes", async ({
  page,
}) => {
  await setup(page);
  await seedTransferTasks(page, [{ id: "existing", status: "running" }]);
  await page.evaluate(() => {
    const qa = (window as any).__TEST__;
    qa.holdNextList = true;
    qa.emit("transfer-updated", {});
  });
  await expect
    .poll(() =>
      page.evaluate(() => typeof (window as any).__TEST__.releaseList),
    )
    .toBe("function");
  await page.evaluate(() => {
    const qa = (window as any).__TEST__;
    qa.tasks.push({
      ...qa.tasks[0],
      id: "arriving",
      runId: "arriving",
      sortOrder: 2,
      status: "queued",
    });
    qa.emit("transfer-updated", {});
    qa.releaseList();
  });
  const trigger = page.getByRole("button", { name: "查看传输任务" });
  await expect(trigger).toContainText("2 项进行中");
  await expect(trigger).toHaveAttribute("aria-expanded", "false");
  await trigger.click();
  await expect(page.locator(".transfer-task-row").first()).toHaveAttribute(
    "data-task-id",
    "arriving",
  );
});
