import type { CreateContainerSpec } from "../dockerApi";
export const restartPolicies = [
  ["no", "不自动重启"],
  ["unless-stopped", "除非手动停止"],
  ["always", "始终重启"],
  ["on-failure", "仅失败时重启"],
] as const;
export interface CreateContainerDraft {
  name: string;
  start: boolean;
  ports: Array<{
    rowId: string;
    hostIp: string;
    hostPort: string;
    containerPort: string;
    protocol: string;
  }>;
  mounts: Array<{
    rowId: string;
    kind: string;
    source: string;
    destination: string;
    readOnly: boolean;
  }>;
  environmentText: string;
  commandText: string;
  network: string;
  restartPolicy: string;
  maximumRetryCount: string;
  workingDirectory: string;
}
export type FieldIssue = { field: string; message: string; line?: number };
export function createInitialDraft(): CreateContainerDraft {
  return {
    name: "",
    start: true,
    ports: [],
    mounts: [],
    environmentText: "",
    commandText: "",
    network: "",
    restartPolicy: "unless-stopped",
    maximumRetryCount: "0",
    workingDirectory: "",
  };
}
export function draftEquals(a: CreateContainerDraft, b: CreateContainerDraft) {
  const clean = (v: CreateContainerDraft) => ({
    ...v,
    ports: v.ports.map(({ rowId: _, ...r }) => r),
    mounts: v.mounts.map(({ rowId: _, ...r }) => r),
  });
  return JSON.stringify(clean(a)) === JSON.stringify(clean(b));
}
export function validateRestartPolicy(
  policy: string,
  count: string,
): string | null {
  if (!restartPolicies.some(([p]) => p === policy))
    return "请选择有效的重启策略。";
  if (
    policy === "on-failure" &&
    (!/^\d+$/.test(count) || Number(count) > 100000)
  )
    return "重试次数须为 0–100000 的整数，0 表示不限次数。";
  return null;
}
export const validContainerName = (name: string) =>
  /^[A-Za-z0-9][A-Za-z0-9_.-]{0,254}$/.test(name);
const bytes = (s: string) => new TextEncoder().encode(s).length;
function validIp(s: string) {
  if (/^\d+\.\d+\.\d+\.\d+$/.test(s))
    return s
      .split(".")
      .every((v) => /^(0|[1-9]\d{0,2})$/.test(v) && Number(v) <= 255);
  if (!s.includes(":")) return false;
  try {
    return new URL("http://[" + s + "]/").hostname.startsWith("[");
  } catch {
    return false;
  }
}
export function validateCreateDraft(
  d: CreateContainerDraft,
  imageId: string,
):
  | { ok: true; specification: CreateContainerSpec }
  | { ok: false; issues: FieldIssue[] } {
  const issues: FieldIssue[] = [];
  const fail = (field: string, message: string, line?: number) =>
    issues.push({ field, message, line });
  if (!validContainerName(d.name))
    fail(
      "name",
      "名称须以字母或数字开头，仅包含字母、数字、下划线、点和连字符，最多 255 个字符。",
    );
  if (d.ports.length > 64) fail("ports", "发布端口最多 64 项。");
  if (d.mounts.length > 32) fail("mounts", "挂载最多 32 项。");
  const bindings = new Set<string>();
  for (const p of d.ports) {
    if (!validIp(p.hostIp))
      fail("ports." + p.rowId + ".hostIp", "请输入有效的 IPv4 或 IPv6 地址。");
    for (const key of ["hostPort", "containerPort"] as const)
      if (!/^\d+$/.test(p[key]) || Number(p[key]) < 1 || Number(p[key]) > 65535)
        fail("ports." + p.rowId + "." + key, "端口须为 1–65535 的整数。");
    if (!["tcp", "udp"].includes(p.protocol))
      fail("ports." + p.rowId + ".protocol", "请选择 TCP 或 UDP。");
    const key = p.hostIp + ":" + Number(p.hostPort) + "/" + p.protocol;
    if (bindings.has(key))
      fail(
        "ports." + p.rowId + ".hostPort",
        "该地址和协议的宿主端口已被使用。",
      );
    bindings.add(key);
  }
  const environment: CreateContainerSpec["environment"] = [],
    keys = new Set<string>();
  d.environmentText.split(/\r?\n/).forEach((line, i) => {
    if (!line.trim()) return;
    const at = line.indexOf("=");
    const key = line.slice(0, at).trim(),
      value = line.slice(at + 1);
    if (at < 1 || !/^[_A-Za-z][_A-Za-z0-9]*$/.test(key) || bytes(key) > 255)
      fail(
        "environmentText",
        "第 " + (i + 1) + " 行须为有效的 KEY=value。",
        i + 1,
      );
    else if (keys.has(key))
      fail("environmentText", "第 " + (i + 1) + " 行变量名重复：" + key, i + 1);
    if (/[\r\0]/.test(value))
      fail("environmentText", "第 " + (i + 1) + " 行不能含回车或 NUL。", i + 1);
    keys.add(key);
    environment.push({ key, value });
  });
  if (
    environment.length > 200 ||
    environment.reduce((n, e) => n + bytes(e.key) + bytes(e.value), 0) > 128000
  )
    fail("environmentText", "环境变量最多 200 项，总大小不得超过 128 KB。");
  const destinations = new Set<string>();
  for (const m of d.mounts) {
    if (!["bind", "volume"].includes(m.kind))
      fail("mounts." + m.rowId + ".kind", "请选择 Bind 或 Volume。");
    if (
      !m.source ||
      (m.kind === "bind" && !m.source.startsWith("/")) ||
      bytes(m.source) > 4096 ||
      /[,"\x00-\x1f\x7f-\x9f]/.test(m.source)
    )
      fail(
        "mounts." + m.rowId + ".source",
        "Bind 来源须为绝对路径；来源不能含逗号、引号或控制字符。",
      );
    if (
      !m.destination.startsWith("/") ||
      bytes(m.destination) > 4096 ||
      /[,"\x00-\x1f\x7f-\x9f]/.test(m.destination) ||
      destinations.has(m.destination)
    )
      fail(
        "mounts." + m.rowId + ".destination",
        "请输入唯一的绝对容器路径，不能含逗号、引号或控制字符。",
      );
    destinations.add(m.destination);
  }
  const policyError = validateRestartPolicy(
    d.restartPolicy,
    d.maximumRetryCount,
  );
  if (policyError) fail("maximumRetryCount", policyError);
  if (
    d.network &&
    (!/^[A-Za-z0-9_.-]+$/.test(d.network) || bytes(d.network) > 255)
  )
    fail(
      "network",
      "网络名称只能包含字母、数字、下划线、点和连字符，最多 255 字节。",
    );
  if (
    d.workingDirectory &&
    (!d.workingDirectory.startsWith("/") ||
      bytes(d.workingDirectory) > 4096 ||
      /[\x00-\x1f\x7f-\x9f]/.test(d.workingDirectory))
  )
    fail("workingDirectory", "工作目录须为有效的绝对路径。");
  const command = d.commandText.split(/\r?\n/).filter(Boolean);
  if (
    command.length > 128 ||
    command.some((v) => bytes(v) > 8192 || v.includes("\0"))
  )
    fail(
      "commandText",
      "命令最多 128 个参数，每个参数不超过 8192 字节且不能含 NUL。",
    );
  if (issues.length) return { ok: false, issues };
  return {
    ok: true,
    specification: {
      imageId,
      name: d.name,
      start: d.start,
      ports: d.ports.map(({ rowId: _, ...p }) => ({
        ...p,
        hostPort: Number(p.hostPort),
        containerPort: Number(p.containerPort),
      })),
      mounts: d.mounts.map(({ rowId: _, ...m }) => m),
      environment,
      command,
      network: d.network || null,
      restartPolicy: d.restartPolicy,
      maximumRetryCount:
        d.restartPolicy === "on-failure" ? Number(d.maximumRetryCount) : 0,
      workingDirectory: d.workingDirectory,
    },
  };
}
