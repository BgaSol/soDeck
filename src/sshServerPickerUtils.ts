import { addressOf, type Server } from "./types";

export function sshServerSummary(server: Server, jump?: Server) {
  const address = addressOf(server);
  const name = server.name.trim();
  const hasName =
    !!name &&
    ![server.host, address, `[${server.host}]`].some(
      (value) => value.toLocaleLowerCase() === name.toLocaleLowerCase(),
    );
  return {
    title: hasName ? name : address,
    identity: [hasName ? address : "", server.username]
      .filter(Boolean)
      .join(" · "),
    route: server.jumpServerId
      ? jump
        ? `${addressOf(jump)}（跳板） → 此节点访问目标`
        : "跳板节点不可用，请检查服务器配置"
      : "直连 SSH",
  };
}

export function searchSSHServers(servers: Server[], query: string) {
  const words = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return servers;
  const byId = new Map(servers.map((server) => [server.id, server]));
  return servers.filter((server) => {
    const jump = server.jumpServerId
      ? byId.get(server.jumpServerId)
      : undefined;
    const text = [
      server.name,
      server.host,
      addressOf(server),
      server.port,
      server.username,
      ...server.tags,
      jump?.name,
      jump?.host,
      jump?.port,
      jump && addressOf(jump),
    ]
      .join(" ")
      .toLocaleLowerCase();
    return words.every((word) => text.includes(word));
  });
}
