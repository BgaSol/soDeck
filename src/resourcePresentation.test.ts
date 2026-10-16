import { describe, expect, it } from "vitest";
import { chartSampleAt, chartPath } from "./MetricChart";
import {
  normalizeResourcePreferences,
  mountGroup,
} from "./resourcePresentation";
import {
  dockerKey,
  dockerMatches,
  normalizeDockerSnapshot,
  orderDockerItems,
} from "./dockerPresentation";
import type { Container } from "./types";
describe("session presentation state", () => {
  it("reads legacy preferences without copying live Docker data into new snapshots", () => {
    const legacy = {
      rootOpen: true,
      imagesOpen: true,
      containersOpen: false,
      images: [{ id: "x", name: "image", size: "1 MB" }],
    };
    const view = normalizeDockerSnapshot(legacy);
    expect(view.page).toBe("images");
    expect(view).not.toHaveProperty("images");
    expect(
      normalizeDockerSnapshot({ ...legacy, containersOpen: true }).page,
    ).toBe("containers");
    const preferences = normalizeResourcePreferences({
      tab: "docker",
      minutes: 15,
      networkInterface: "eth1",
    });
    expect(preferences).toMatchObject({
      tab: "docker",
      minutes: 15,
      pinnedMounts: ["/"],
    });
    const customized = {
      ...preferences,
      pinnedMounts: ["/", "/data"],
    };
    expect(
      normalizeResourcePreferences(JSON.parse(JSON.stringify(customized))),
    ).toEqual(customized);
    expect(
      normalizeResourcePreferences({
        collapsed: ["traffic"],
        trafficScope: "monitoring",
      }),
    ).not.toHaveProperty("trafficScope");
    expect(normalizeResourcePreferences().minutes).toBe(5);
    expect(
      normalizeDockerSnapshot(
        JSON.parse(
          JSON.stringify({
            ...view,
            selection: { kind: "images", id: "x", name: "image" },
            scrollTop: { containers: 0, images: 123 },
          }),
        ),
      ),
    ).toMatchObject({
      selection: { kind: "images", id: "x", name: "image" },
      scrollTop: { images: 123 },
    });
  });
});
describe("Docker presentation", () => {
  const a: Container = {
    id: "a",
    name: "alpha",
    image: "nginx:stable",
    state: "running",
    status: "Up",
  };
  const z: Container = {
    id: "z",
    name: "zulu",
    image: "postgres:16",
    state: "restarting",
    status: "Restarting",
  };
  it("sorts by name or attention, then preserves positions on background updates", () => {
    expect(orderDockerItems([z, a], "name").map(dockerKey)).toEqual(["a", "z"]);
    expect(orderDockerItems([a, z], "attention").map(dockerKey)).toEqual([
      "z",
      "a",
    ]);
    expect(
      orderDockerItems(
        [
          { ...a, state: "dead" },
          { ...z, state: "running" },
        ],
        "attention",
        ["z", "a"],
      ).map(dockerKey),
    ).toEqual(["z", "a"]);
    expect(
      orderDockerItems(
        [
          { ...a, state: "dead" },
          { ...z, state: "running" },
        ],
        "attention",
      ).map(dockerKey),
    ).toEqual(["a", "z"]);
  });
  it("filters across identity fields without treating paused containers as stopped", () => {
    expect(dockerMatches(a, "NGINX", "running")).toBe(true);
    expect(dockerMatches(z, "z", "other")).toBe(true);
    expect(dockerMatches({ ...a, state: "paused" }, "", "stopped")).toBe(false);
    expect(dockerMatches({ ...a, state: "exited" }, "", "stopped")).toBe(true);
    expect(dockerKey({ id: "sha256:x", name: "x:latest", size: "1" })).not.toBe(
      dockerKey({ id: "sha256:x", name: "x:v1", size: "1" }),
    );
  });
});
describe("mount classification", () => {
  const group = (target: string, fsType: string) =>
    mountGroup({
      key: target,
      target,
      source: fsType,
      fsType,
      fsRoot: "/",
      readOnly: false,
    });
  it("keeps the root and unknown mounts visible and distinguishes system, runtime and memory", () => {
    expect(group("/", "overlay")).toBe("storage");
    expect(group("/unknown", "overlay")).toBe("storage");
    expect(group("/proc", "proc")).toBe("system");
    expect(group("/snap/core/1", "squashfs")).toBe("system");
    expect(group("/run/netns/x", "nsfs")).toBe("runtime");
    expect(group("/var/lib/docker/overlay2/x/merged", "overlay")).toBe(
      "runtime",
    );
    expect(group("/dev/shm", "tmpfs")).toBe("memory");
  });
});
describe("actual sample cursors", () => {
  const points = [
    { sequence: 1, at: 1000, gap: false, value: 1 },
    { sequence: 2, at: 3000, gap: false, value: 2 },
    { sequence: 3, at: 5000, gap: true, value: null },
    { sequence: 4, at: 7000, gap: false, value: 4 },
    { sequence: 5, at: 40000, gap: false, value: 5 },
  ];
  it("never snaps across missing intervals or outside available history", () => {
    expect(chartSampleAt(points, 2100)?.sequence).toBe(2);
    expect(chartSampleAt(points, 4000)).toBeUndefined();
    expect(chartSampleAt(points, 15000)).toBeUndefined();
    expect(chartSampleAt(points, 100)).toBeUndefined();
    expect(chartSampleAt(points, 41000)).toBeUndefined();
    expect(chartSampleAt(points, 5000)?.value).toBeNull();
    expect(chartSampleAt(points, 40000)?.sequence).toBe(5);
    expect(
      chartPath(points, (p) => p.value, 0, 40000, 5).match(/M/g),
    ).toHaveLength(3);
  });
});
