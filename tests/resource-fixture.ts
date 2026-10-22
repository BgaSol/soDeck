import { installWorkspaceFixture } from "./workspace-fixture";
import { expect, type Page } from "@playwright/test";

export async function setup(
  page: Page,
  mode: "workspace" | "monitor" = "workspace",
) {
  await installWorkspaceFixture(page);
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.addInitScript(() => {
    const w = window as any;
    let serial = 1;
    const server = {
      id: "mount-test",
      name: "Mount fixture",
      host: "fixture.invalid",
      port: 22,
      username: "ops",
      authType: "password",
      hasSavedPassword: true,
      privateKey: "",
      tags: [],
      jumpServerId: null,
      directHost: null,
      directPort: 22,
    };
    const entry = (path: string, isDir = true, isSymlink = false) => ({
      path,
      name: path.split("/").pop() || "/",
      isDir,
      isSymlink,
      size: isDir ? 0 : 42,
      modified: 0,
    });
    const entries = [
      entry("/data"),
      entry("/backup"),
      entry("/proc"),
      entry("/link", true, true),
      entry("/中文 '文件.txt", false),
    ];
    const directory = (path: string) => ({
      path,
      resolvedPath: path === "/link" ? "/data" : path,
      entries:
        path === "/"
          ? entries
          : path === "/data"
            ? [entry("/data/apps"), entry("/data/archive")]
            : [],
    });
    const table = {
      version: "v1",
      status: "ready",
      mounts: [
        {
          key: "root",
          target: "/",
          source: "/dev/sda1",
          fsRoot: "/",
          fsType: "ext4",
          readOnly: false,
        },
        {
          key: "data",
          target: "/data",
          source: "/dev/sdb1",
          fsRoot: "/",
          fsType: "xfs",
          readOnly: false,
        },
        {
          key: "backup",
          target: "/backup",
          source: "nas:/backup",
          fsRoot: "/",
          fsType: "nfs4",
          readOnly: true,
        },
        {
          key: "archive",
          target: "/data/archive",
          source: "/dev/sdc1",
          fsRoot: "/archive",
          fsType: "ext4",
          readOnly: false,
        },
        {
          key: "proc",
          target: "/proc",
          source: "proc",
          fsRoot: "/",
          fsType: "proc",
          readOnly: false,
        },
      ],
    };
    const calls: any[] = [];
    const callbacks = new Map<number, Function>();
    w.__MOUNTS__ = {
      calls,
      table,
      copied: "",
      failCopy: false,
      failUsage: false,
      clipboard: {
        id: "existing",
        paths: ["/data/apps"],
        intent: "move",
        sameServer: true,
        reserved: false,
      },
    };
    let monitor: any = null;
    let mountRevision = 0;
    let metricsRevision = 1;
    let interest: any = { visible: true, mountPaths: [] };
    let cached: any = {};
    let version = "";
    const metric = (value: any) => ({
      value,
      status: "ready",
      updatedAt: Date.now(),
      error: null,
    });
    const metrics = () => ({
      cpu: metric({
        percent: 23.4,
        cores: { cpu0: 18, cpu1: 28.8 },
        load: [0.12, 0.2, 0.15],
      }),
      memory: metric({
        totalBytes: "17179869184",
        availableBytes: "10737418240",
        usedBytes: "6442450944",
        usedPercent: 37.5,
        cachedBytes: "2147483648",
        swapTotalBytes: "2147483648",
        swapUsedBytes: "0",
      }),
      network: metric([
        {
          name: "eth0",
          index: 2,
          isDefault: true,
          virtualInterface: false,
          rxBytes: "1099511627776",
          txBytes: "2147483648",
          rxPerSecond: 2516582,
          txPerSecond: 880640,
          rxErrors: "0",
          txErrors: "1",
          rxDropped: "2",
          txDropped: "0",
        },
        {
          name: "docker0",
          index: 3,
          isDefault: false,
          virtualInterface: true,
          rxBytes: "1024",
          txBytes: "2048",
          rxPerSecond: 128,
          txPerSecond: 256,
          rxErrors: "0",
          txErrors: "0",
          rxDropped: "0",
          txDropped: "0",
        },
      ]),
    });
    const mountState = () => {
      const table = w.__MOUNTS__.table;
      if (version !== table.version) {
        version = table.version;
        cached = {};
      }
      for (const m of table.mounts.filter((m: any) =>
        interest.mountPaths.includes(m.target),
      )) {
        if (w.__MOUNTS__.failUsage) {
          cached[m.key] = {
            ...cached[m.key],
            loading: false,
            stale: true,
            error: "网络挂载查询超时",
          };
          continue;
        }
        cached[m.key] = {
          loading: false,
          stale: false,
          error: null,
          value: {
            mountKey: m.key,
            status: m.key === "proc" ? "unsupported" : "ready",
            totalBytes: "1099511627776",
            usedBytes: "747667906888",
            availableBytes: "351843720888",
            usedPercent:
              m.key === "proc" ? null : m.key === "archive" ? 96 : 68,
            updatedAt: Date.now(),
            error: m.key === "proc" ? "此文件系统不提供常规容量" : null,
          },
        };
      }
      return {
        table: structuredClone(table),
        usage: structuredClone(cached),
        fetchedAt: Date.now(),
        loading: false,
        error: "",
      };
    };
    w.__MOUNTS__.push = () =>
      monitor?.onmessage({
        type: "mounts",
        generation: "monitor-generation",
        revision: ++mountRevision,
        mounts: mountState(),
      });
    w.__MOUNTS__.sendMetrics = (patch: any = {}, gap = false) => {
      const value = { ...metrics(), ...patch };
      monitor?.onmessage({
        type: "metrics",
        generation: "monitor-generation",
        revision: ++metricsRevision,
        metrics: value,
        point: {
          sequence: metricsRevision,
          at: Date.now(),
          cpu: gap ? null : value.cpu.value?.percent,
          memory: gap ? null : value.memory.value?.usedPercent,
          network: gap ? {} : { eth0: [2516582, 880640], docker0: [128, 256] },
          gap,
        },
      });
    };
    let dockerRevision = 1;
    const docker = () => ({
      engineGeneration: 1,
      refreshing: false,
      engine: metric({
        state: "running",
        message: "Docker 引擎可访问",
        endpoint: "unix:///var/run/docker.sock",
      }),
      containers: metric([
        {
          id: "a".repeat(64),
          name: "web",
          image: "nginx:alpine",
          state: "running",
          status: "Up",
        },
      ]),
      images: metric([
        { id: "sha256:fixture", name: "nginx:alpine", size: "50MB" },
      ]),
      summary: metric({
        total: 3,
        byState: { running: 1, exited: 1, paused: 1 },
        unhealthy: 0,
        healthError: null,
      }),
    });
    const trafficValues = (sent: number, received: number) => ({
      sentBytes: String(sent * 100),
      receivedBytes: String(received * 100),
      sentPerSecond: sent,
      receivedPerSecond: received,
    });
    const traffic = () => ({
      epoch: "traffic-epoch",
      revision: 1,
      startedAt: Date.now() - 200000,
      sampledAt: Date.now(),
      activeConnections: 4,
      measurement: "sshStream",
      status: "ready",
      total: trafficValues(10240, 20480),
      monitoring: trafficValues(128, 2048),
      terminalAndLogs: trafficValues(112, 432),
      files: trafficValues(10000, 18000),
      other: trafficValues(0, 0),
    });
    w.__MOUNTS__.sendDocker = (patch: any = {}) =>
      monitor?.onmessage({
        type: "docker",
        generation: "monitor-generation",
        revision: ++dockerRevision,
        docker: { ...docker(), ...patch },
      });
    w.__MOUNTS__.sendTraffic = (patch: any = {}) =>
      monitor?.onmessage({
        type: "traffic",
        generation: "monitor-generation",
        traffic: { ...traffic(), revision: 2, ...patch },
        point: {
          sequence: 2,
          at: Date.now(),
          sent: 10240,
          received: 20480,
          monitorSent: 128,
          monitorReceived: 2048,
          gap: false,
        },
      });
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: async (text: string) => {
          if (w.__MOUNTS__.failCopy) throw new Error("denied");
          w.__MOUNTS__.copied = text;
        },
      },
    });
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
        if (command === "list_servers") return [server];
        if (command === "list_transfers") return [];
        if (command === "list_file_tasks")
          return {
            tasks: [],
            summary: { total: 0, active: 0, attention: 0, completed: 0 },
            nextCursor: null,
          };
        if (command === "plugin:event|listen") return serial++;
        if (command === "monitor_window_info")
          return {
            serverName: "Mount fixture",
            preferences: {
              minutes: 5,
              networkInterface: "",
              selectedMetric: "cpu",
            },
          };
        if (
          command === "subscribe_monitor" ||
          command === "subscribe_monitor_window"
        ) {
          monitor = args.events;
          w.__MOUNTS__.monitor = monitor;
          interest = args.interest;
          monitor.onmessage({
            type: "snapshot",
            generation: "monitor-generation",
            metricsRevision,
            mountsRevision: ++mountRevision,
            metrics: metrics(),
            mounts: mountState(),
            dockerRevision,
            docker: docker(),
            traffic: traffic(),
          });
          return "monitor-subscription";
        }
        if (command === "update_monitor_interest") {
          interest = args.interest;
          w.__MOUNTS__.push();
          return;
        }
        if (command === "refresh_monitor") {
          w.__MOUNTS__.push();
          w.__MOUNTS__.sendMetrics();
          w.__MOUNTS__.sendDocker();
          return;
        }
        if (command === "get_monitor_history")
          return {
            generation: "monitor-generation",
            truncated: false,
            traffic: {
              epoch: "traffic-epoch",
              truncated: false,
              points: Array.from({ length: 80 }, (_, i) => ({
                sequence: i - 80,
                at: Date.now() - (80 - i) * 2000,
                sent: 10240,
                received: 20480 + Math.sin(i / 4) * 2000,
                monitorSent: 128,
                monitorReceived: 2048,
                gap: false,
              })),
            },
            points: Array.from({ length: 80 }, (_, i) => ({
              sequence: i - 80,
              at: Date.now() - (80 - i) * 2000,
              cpu: 20 + Math.sin(i / 5) * 12,
              memory: 37 + Math.sin(i / 12) * 2,
              network: {
                eth0: [
                  2500000 + Math.sin(i / 3) * 1000000,
                  800000 + Math.cos(i / 8) * 300000,
                ],
                docker0: [128 + i, 256],
              },
              gap: false,
            })),
          };
        if (command === "open_connection") {
          w.__MOUNTS__.terminal = args.events;
          setTimeout(
            () =>
              args.events.onmessage({
                type: "ready",
                connection: {
                  target: server,
                  hops: [{ index: 0, role: "target", server }],
                  identity: "fixture",
                },
              }),
            0,
          );
          return "mount-session";
        }
        if (command === "get_remote_clipboard")
          return structuredClone(w.__MOUNTS__.clipboard);
        if (command === "list_directory_chain") {
          if (w.__MOUNTS__.failReveal)
            throw { kind: "permission", message: "无法读取目标目录" };
          const target = args.path || "/";
          const parts = target.split("/").filter(Boolean);
          const paths = [
            "/",
            ...parts.map(
              (_: string, i: number) => "/" + parts.slice(0, i + 1).join("/"),
            ),
          ];
          return {
            homePath: "/",
            path: target,
            directories: paths.map((path) => ({
              path,
              directory: directory(path),
            })),
          };
        }
        if (command === "list_directory") return directory(args.path);
        if (command === "list_remote_mounts")
          return structuredClone(w.__MOUNTS__.table);
        if (command === "get_remote_mount_usage") {
          if (w.__MOUNTS__.failUsage)
            throw { kind: "timeout", message: "网络挂载查询超时" };
          return args.mountKeys.map((key: string) => ({
            mountKey: key,
            status: key === "proc" ? "unsupported" : "ready",
            totalBytes: "1099511627776",
            usedBytes: "747667906888",
            availableBytes: "351843720888",
            usedPercent: key === "proc" ? null : key === "archive" ? 96 : 68,
            updatedAt: key === "proc" ? null : Date.now(),
            error: key === "proc" ? "此文件系统不提供常规容量" : null,
          }));
        }
        if (command === "detect_docker")
          return { state: "missing", message: "fixture", endpoint: "" };
        if (
          command.endsWith("session_count") ||
          command === "unsaved_editor_count"
        )
          return 0;
        return null;
      },
    };
  });
  await page.goto(mode === "monitor" ? "/?monitor=1" : "/");
  if (mode === "monitor") {
    await expect(
      page.getByRole("region", { name: "服务器资源监控", exact: true }),
    ).toContainText("23.4%");
    return errors;
  }
  await page.getByRole("button", { name: "新建连接", exact: true }).click();
  await page
    .locator(".server-row")
    .filter({ hasText: "Mount fixture" })
    .dblclick();
  await expect(row(page, "/data").locator(".mount-usage")).toContainText("68%");
  return errors;
}
export const row = (page: Page, path: string) =>
  page.locator(`[role=treeitem][data-path=${JSON.stringify(path)}]`);
