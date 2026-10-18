import { expect, it } from "vitest";
import { emptyFrame } from "./netbirdStore";
import {
  bytes,
  filterPeers,
  latency,
  overall,
  statusLabel,
} from "./netbirdPresentation";
import { normalizeResourcePreferences } from "../resourcePresentation";
import type { Peer } from "./netbirdApi";
it("distinguishes limited control plane and stale snapshots from healthy connections", () => {
  const s = emptyFrame().snapshot;
  s.collectionStatus = "ready";
  s.connection = "connected";
  s.updatedAt = 1;
  expect(overall(s, true).label).toBe("已连接");
  s.management.state = "disconnected";
  expect(overall(s, true).label).toBe("连接受限");
  s.collectionStatus = "stale";
  expect(overall(s, true).label).toBe("数据待更新");
  expect(overall(s, false).label).toBe("SSH 已断开");
});
it("does not present relay or idle as errors, or missing measurements as zero", () => {
  expect(statusLabel("relayed")).toBe("中继");
  expect(statusLabel("idle")).toBe("空闲");
  expect(latency(null)).toBe("—");
  expect(latency(0)).toBe("—");
  expect(bytes(null)).toBe("—");
  expect(bytes("0")).toBe("0 B");
  expect(bytes("9007199254740993")).toContain("TiB");
});
it("matches IPv6, names and combined state/mode filters", () => {
  const peers = [
    {
      name: "DB-Prod",
      fqdn: null,
      ipv4: null,
      ipv6: "fd00::2",
      state: "idle",
      connectionMode: "relayed",
    },
  ] as Peer[];
  expect(filterPeers(peers, "db-", "idle", "relayed")).toHaveLength(1);
  expect(filterPeers(peers, "fd00::", "", "")).toHaveLength(1);
  expect(filterPeers(peers, "db", "connected", "")).toHaveLength(0);
});
it("restores the third service tab without changing legacy defaults", () => {
  expect(normalizeResourcePreferences().tab).toBe("monitor");
  expect(normalizeResourcePreferences({ tab: "docker" }).tab).toBe("docker");
  expect(normalizeResourcePreferences({ tab: "netbird" }).tab).toBe("netbird");
});
it("keeps an intentional disconnect neutral even when control-plane connections are down", () => {
  const s = emptyFrame().snapshot;
  s.collectionStatus = "ready";
  s.connection = "disconnected";
  s.management.state = "disconnected";
  s.signal.state = "disconnected";
  expect(overall(s, true)).toEqual({ label: "已断开", tone: "muted" });
});
