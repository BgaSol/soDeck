import { describe, expect, it } from "vitest";
import {
  formatMountBytes,
  mountForFile,
  mountSource,
  usageSeverity,
} from "./remoteMounts";
import type { MountTable } from "./remoteMounts";

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

describe("remote mount presentation", () => {
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
});
