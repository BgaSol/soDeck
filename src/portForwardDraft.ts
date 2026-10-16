import type { ForwardRule, ForwardSettings } from "./portForwardApi";

export interface ForwardDraft {
  name: string;
  serverId: string;
  targetHost: string;
  targetPort: string;
  portMode: "fixed" | "auto";
  localPort: string;
  localPortEdited: boolean;
  listenAddress: string;
  autoReconnect: boolean;
  webScheme: ForwardRule["webScheme"];
}
export type ForwardSaveIntent = "save" | "start" | "restart";
export type DraftErrors = Partial<
  Record<
    "name" | "serverId" | "targetHost" | "targetPort" | "localPort",
    string
  >
>;
export function forwardDraft(rule: ForwardRule): ForwardDraft {
  return {
    name: rule.name,
    serverId: rule.settings.serverId,
    targetHost: rule.settings.targetHost,
    targetPort: rule.settings.targetPort
      ? String(rule.settings.targetPort)
      : "",
    portMode: rule.id && rule.settings.listenPort === null ? "auto" : "fixed",
    localPort:
      rule.settings.listenPort === null ? "" : String(rule.settings.listenPort),
    localPortEdited: !!rule.id,
    listenAddress: rule.settings.listenAddress,
    autoReconnect: rule.settings.autoReconnect,
    webScheme: rule.webScheme,
  };
}
export function updateTargetPort(
  draft: ForwardDraft,
  value: string,
): ForwardDraft {
  return {
    ...draft,
    targetPort: value,
    localPort: draft.localPortEdited ? draft.localPort : value,
  };
}
export function validateForwardDraft(
  draft: ForwardDraft,
  serverIds: string[],
): DraftErrors {
  const errors: DraftErrors = {};
  const name = draft.name.trim(),
    host = draft.targetHost.trim();
  if (!name) errors.name = "请输入名称";
  else if (Array.from(name).length > 128 || /[\u0000-\u001f\u007f]/.test(name))
    errors.name = "名称不能包含控制字符，且不超过 128 个字符";
  if (!serverIds.includes(draft.serverId))
    errors.serverId = "请选择可用的 SSH 服务器";
  if (!host) errors.targetHost = "请输入目标地址";
  else if (host.length > 253 || /[\s/\\@?#\u0000-\u001f\u007f]/.test(host))
    errors.targetHost = "请填写主机名或 IP，不含协议和路径";
  else if (host.includes(":")) {
    try {
      const bracketed = host.startsWith("[");
      if (bracketed && !host.endsWith("]")) throw new Error("embedded port");
      const parsed = new URL(`http://${bracketed ? host : `[${host}]`}/`);
      if (!parsed.hostname.startsWith("[") || parsed.port)
        throw new Error("invalid host");
    } catch {
      errors.targetHost = "请输入有效的 IPv6 地址；端口请单独填写";
    }
  }
  const portValid = (v: string) =>
    /^\d+$/.test(v) && Number(v) >= 1 && Number(v) <= 65535;
  if (!portValid(draft.targetPort))
    errors.targetPort = "请输入 1–65535 之间的端口";
  if (draft.portMode === "fixed" && !portValid(draft.localPort))
    errors.localPort = "请输入 1–65535 之间的端口";
  return errors;
}
export function draftSettings(draft: ForwardDraft): ForwardSettings {
  return {
    serverId: draft.serverId,
    listenAddress: draft.listenAddress,
    listenPort: draft.portMode === "auto" ? null : Number(draft.localPort),
    targetHost: draft.targetHost.trim().replace(/^\[([^\]]+)\]$/, "$1"),
    targetPort: Number(draft.targetPort),
    autoReconnect: draft.autoReconnect,
  };
}
