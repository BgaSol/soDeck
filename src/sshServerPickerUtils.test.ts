import { describe, expect, it } from "vitest";
import { searchSSHServers, sshServerSummary } from "./sshServerPickerUtils";
import { emptyServer, type Server } from "./types";

const jump: Server = {
  ...emptyServer(),
  id: "vpn",
  name: "VPN 入口",
  host: "localhost",
  port: 2222,
  username: "gateway",
};
const site: Server = {
  ...emptyServer(),
  id: "site",
  name: "现场节点",
  host: "192.168.1.186",
  username: "ops",
  jumpServerId: jump.id,
  tags: ["集群", "生产"],
};

describe("SSH access node presentation", () => {
  it("keeps a descriptive name, connection identity and jump role separate", () => {
    expect(sshServerSummary(site, jump)).toEqual({
      title: "现场节点",
      identity: "192.168.1.186:22 · ops",
      route: "localhost:2222（跳板） → 此节点访问目标",
    });
    expect(sshServerSummary(jump).route).toBe("直连 SSH");
  });

  it("shows an address only once when the name is the host or endpoint", () => {
    for (const name of ["", "192.168.1.186", " 192.168.1.186:22 "]) {
      expect(sshServerSummary({ ...site, name })).toMatchObject({
        title: "192.168.1.186:22",
        identity: "ops",
      });
    }
    expect(
      sshServerSummary({ ...site, name: "SSH.INTERNAL", host: "ssh.internal" }),
    ).toMatchObject({ title: "ssh.internal:22", identity: "ops" });
  });

  it("preserves IPv6 endpoint brackets and does not claim a missing jump is direct", () => {
    expect(
      sshServerSummary({ ...site, name: "2001:db8::1", host: "2001:db8::1" }),
    ).toMatchObject({
      title: "[2001:db8::1]:22",
      route: "跳板节点不可用，请检查服务器配置",
    });
  });
});

describe("SSH access node search", () => {
  const servers = [jump, site];

  it("matches connection fields, tags and jump information using all query words", () => {
    for (const query of [
      "192.168.1.186:22",
      "OPS 生产",
      "现场 localhost",
      "集群 VPN",
      "现场 2222",
      "现场 localhost:2222",
    ])
      expect(
        searchSSHServers(servers, query).map((server) => server.id),
      ).toEqual(["site"]);
  });

  it("preserves source order and handles blank queries, no results and missing jumps", () => {
    expect(searchSSHServers(servers, "  ")).toBe(servers);
    expect(searchSSHServers(servers, "localhost")).toEqual(servers);
    expect(searchSSHServers(servers, "不存在")).toEqual([]);
    expect(searchSSHServers([site], "VPN")).toEqual([]);
    expect(searchSSHServers([site], "现场")).toEqual([site]);
  });
});
