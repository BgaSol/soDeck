import { describe, expect, it, vi } from "vitest";
import {
  RemoteMounts,
  formatMountBytes,
  mountForFile,
  mountSource,
  usageSeverity,
} from "./remoteMounts";
import type {
  MountTable,
  RemoteMountApi,
  RemoteMountUsage,
} from "./remoteMounts";

const table = (version = "v1"): MountTable => ({
  status: "ready",
  version,
  mounts: [
    {
      key: "1",
      target: "/",
      source: "/dev/a",
      fsRoot: "/",
      fsType: "ext4",
      readOnly: false,
    },
    {
      key: "2",
      target: "/data",
      source: "/dev/b",
      fsRoot: "/projects",
      fsType: "xfs",
      readOnly: true,
    },
    {
      key: "3",
      target: "/data/archive",
      source: "nas:/archive",
      fsRoot: "/",
      fsType: "nfs4",
      readOnly: false,
    },
  ],
});
const value = (key: string, time = 100_000): RemoteMountUsage => ({
  mountKey: key,
  status: "ready",
  totalBytes: "102400",
  usedBytes: "69632",
  availableBytes: "32768",
  usedPercent: 68,
  updatedAt: time,
});
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
const tick = () => new Promise((r) => setTimeout(r, 0));

describe("remote mounts", () => {
  it("resumes interrupted capacity queries without waiting for the next table poll", async () => {
    const pending = deferred<RemoteMountUsage[]>();
    const api = {
      list: vi.fn(async () => table()),
      usage: vi
        .fn()
        .mockImplementationOnce(() => pending.promise)
        .mockResolvedValue([value("2")]),
    };
    const model = new RemoteMounts("a", api, undefined, () => 100_000);
    model.setVisible(["/data"]);
    model.setActive(true);
    await tick();
    model.setActive(false);
    await model.refresh(true);
    expect(api.list).toHaveBeenCalledTimes(1);
    model.setActive(true);
    await tick();
    expect(api.usage).toHaveBeenCalledTimes(2);
    expect(model.getState().usage["2"].value?.usedPercent).toBe(68);
    pending.resolve([{ ...value("2"), usedPercent: 99 }]);
    await tick();
    expect(model.getState().usage["2"].value?.usedPercent).toBe(68);
    model.setActive(false);
  });
  it("matches only mount roots, not children or symbolic links", () => {
    const file = {
      name: "data",
      path: "/data",
      isDir: true,
      modified: 0,
      size: 0,
    };
    expect(mountForFile(file, table().mounts)?.key).toBe("2");
    expect(
      mountForFile({ ...file, path: "/data/child" }, table().mounts),
    ).toBeUndefined();
    expect(
      mountForFile({ ...file, isSymlink: true }, table().mounts),
    ).toBeUndefined();
    expect(mountSource(table().mounts[1])).toBe("/dev/b[/projects]");
    expect(formatMountBytes("1125899906842624")).toBe("1 PiB");
    expect(formatMountBytes(null)).toBe("—");
    expect([79, 80, 94, 95].map(usageSeverity)).toEqual([
      "normal",
      "warning",
      "warning",
      "critical",
    ]);
  });
  it("loads visible roots only, limits concurrency and loads newly visible mounts", async () => {
    const queries = [
      deferred<RemoteMountUsage[]>(),
      deferred<RemoteMountUsage[]>(),
      deferred<RemoteMountUsage[]>(),
    ];
    const api = {
      list: vi.fn(async () => table()),
      usage: vi.fn(
        (_s: string, _v: string, keys: string[]) =>
          queries[Number(keys[0]) - 1].promise,
      ),
    };
    const model = new RemoteMounts("a", api, undefined, () => 100_000);
    model.setVisible(["/", "/data"]);
    model.setActive(true);
    await tick();
    expect(api.usage.mock.calls.map((c) => c[2][0])).toEqual(["1", "2"]);
    model.setVisible(["/", "/data", "/data/archive"]);
    expect(api.usage).toHaveBeenCalledTimes(2);
    queries[0].resolve([value("1")]);
    await tick();
    expect(api.usage).toHaveBeenCalledTimes(3);
    queries[1].resolve([value("2")]);
    queries[2].resolve([value("3")]);
    await tick();
    model.setActive(false);
  });
  it("preserves prior values on timeout, backs off, and supports explicit retry", async () => {
    let now = 100_000;
    const api = {
      list: vi.fn(async () => table()),
      usage: vi.fn(async () => [value("2", now)]),
    };
    const model = new RemoteMounts("a", api, undefined, () => now);
    model.setVisible(["/data"]);
    model.setActive(true);
    await tick();
    api.usage.mockRejectedValueOnce(new Error("网络超时"));
    now += 30_000;
    await model.refresh();
    await tick();
    expect(model.getState().usage["2"].value?.usedPercent).toBe(68);
    expect(model.getState().usage["2"].stale).toBe(true);
    model.setVisible(["/data"]);
    await tick();
    expect(api.usage).toHaveBeenCalledTimes(2);
    await model.refresh(true);
    await tick();
    expect(model.getState().usage["2"].stale).toBe(false);
    model.setActive(false);
  });
  it("discards late responses across deactivation and table replacement", async () => {
    const old = deferred<RemoteMountUsage[]>();
    let current = table();
    const api: RemoteMountApi = {
      list: async () => current,
      usage: vi
        .fn()
        .mockImplementationOnce(() => old.promise)
        .mockResolvedValue([value("2")]),
    };
    const model = new RemoteMounts("a", api, undefined, () => 100_000);
    model.setVisible(["/data"]);
    model.setActive(true);
    await tick();
    current = {
      ...table("v2"),
      mounts: [{ ...table().mounts[1], source: "/dev/new" }],
    };
    await model.refresh(true);
    old.resolve([{ ...value("2"), usedPercent: 99 }]);
    await tick();
    await tick();
    expect(model.getState().table?.version).toBe("v2");
    expect(model.getState().usage["2"].value?.usedPercent).toBe(68);
    const pending = deferred<RemoteMountUsage[]>();
    vi.mocked(api.usage).mockImplementationOnce(() => pending.promise);
    await model.refresh(true);
    await tick();
    model.setActive(false);
    pending.resolve([{ ...value("2"), usedPercent: 90 }]);
    await tick();
    expect(model.getState().usage["2"].value?.usedPercent).toBe(68);
  });
  it("isolates restored snapshots by session and revalidates moved-window data", async () => {
    const snapshot = {
      sessionId: "a",
      table: table(),
      usage: { "2": { value: value("2"), loading: true, stale: false } },
      fetchedAt: 100_000,
    };
    const pending = deferred<MountTable>();
    const api = {
      list: vi.fn(() => pending.promise),
      usage: vi.fn(async () => [value("2")]),
    };
    const moved = new RemoteMounts("a", api, snapshot, () => 100_000);
    expect(moved.getState().usage["2"].loading).toBe(false);
    expect(moved.getState().usage["2"].stale).toBe(true);
    const other = new RemoteMounts("b", api, snapshot);
    expect(other.getState().table).toBeUndefined();
    moved.setActive(true);
    await tick();
    expect(api.list).toHaveBeenCalledWith("a");
    moved.setActive(false);
    pending.resolve(table());
    await tick();
  });
  it("clears a changed mount and does not repeatedly poll unsupported servers", async () => {
    let now = 100_000;
    const api = {
      list: vi.fn(async () => table()),
      usage: vi.fn(async () => {
        throw { kind: "mountChanged", message: "已卸载" };
      }),
    };
    const model = new RemoteMounts("a", api, undefined, () => now);
    model.setVisible(["/data"]);
    model.setActive(true);
    await tick();
    expect(model.getState().table).toBeUndefined();
    expect(model.getState().error).toBe("已卸载");
    api.list.mockResolvedValue({
      version: "",
      status: "unsupported",
      mounts: [],
    });
    now += 30_000;
    await model.refresh();
    now += 30_000;
    await model.refresh();
    expect(api.list).toHaveBeenCalledTimes(2);
    model.setActive(false);
  });
});
