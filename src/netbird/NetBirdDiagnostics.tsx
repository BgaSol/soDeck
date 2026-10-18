import { useEffect, useRef, useState } from "react";
import { netbirdApi, type Diagnostic, type Snapshot } from "./netbirdApi";
import { at, statusLabel } from "./netbirdPresentation";
import { CopyValue } from "./NetBirdCommon";
import { errorOf } from "../types";
export function NetBirdDiagnostics({
  snapshot,
  sessionId,
  onRefresh,
}: {
  snapshot: Snapshot;
  sessionId: string | null;
  onRefresh: () => Promise<void>;
}) {
  const [data, setData] = useState<Diagnostic | null>(null),
    [busy, setBusy] = useState(""),
    [error, setError] = useState(""),
    [path, setPath] = useState(snapshot.binding?.cliPath ?? "");
  const epoch = useRef(0),
    pending = useRef(false);
  useEffect(() => {
    ++epoch.current;
    setData(null);
    return () => {
      ++epoch.current;
    };
  }, [sessionId, snapshot.binding?.token]);
  async function read(kind: "raw" | "logs") {
    if (!sessionId || !snapshot.binding || pending.current) return;
    pending.current = true;
    const current = epoch.current;
    setBusy(kind);
    setError("");
    setData(null);
    try {
      const next = await netbirdApi[kind](sessionId, snapshot.binding.token);
      if (current === epoch.current) setData(next);
    } catch (e) {
      if (current === epoch.current) setError(errorOf(e).message);
    } finally {
      pending.current = false;
      if (current === epoch.current) setBusy("");
    }
  }
  async function savePath(reset: boolean) {
    if (!sessionId || pending.current) return;
    pending.current = true;
    const current = epoch.current;
    setBusy("path");
    setError("");
    try {
      await netbirdApi.path(sessionId, reset ? null : path.trim());
      if (current === epoch.current) {
        setData(null);
        if (reset) setPath("");
        await onRefresh();
      }
    } catch (e) {
      if (current === epoch.current) setError(errorOf(e).message);
    } finally {
      pending.current = false;
      if (current === epoch.current) setBusy("");
    }
  }
  const summary = `NetBird 诊断摘要\n采样：${at(snapshot.updatedAt)}\n客户端：${snapshot.binding?.clientVersion ?? "未知"}\n服务：${statusLabel(snapshot.service)}\n网络：${statusLabel(snapshot.connection)}\nManagement：${statusLabel(snapshot.management.state)}\nSignal：${statusLabel(snapshot.signal.state)}\nRelay：${statusLabel(snapshot.relay.state)}\nDNS：${statusLabel(snapshot.dns.state)}\n可见节点：${snapshot.peers.length}，已连接：${snapshot.peers.filter((p) => p.state === "connected").length}`;
  return (
    <>
      <p className="nb-note">
        基于当前客户端快照；连接分项正常并不保证所有业务端口可达。
      </p>
      {(
        [
          [
            "客户端服务",
            {
              state: snapshot.service,
              address: null,
              message: snapshot.binding?.capabilities.reason ?? null,
            },
          ],
          ["Management", snapshot.management],
          ["Signal", snapshot.signal],
          ["Relay / STUN", snapshot.relay],
          ["DNS", snapshot.dns],
        ] as const
      ).map(([name, c]) => (
        <details className="nb-check" key={name}>
          <summary>
            <span>{name}</span>
            <span>{statusLabel(c.state)}</span>
          </summary>
          <p>
            {c.message ||
              (c.state === "unknown"
                ? "当前版本或环境没有提供足够信息。"
                : "来自客户端状态报告。")}
          </p>
          {c.address && <p className="nb-selectable">{c.address}</p>}
          <p className="nb-secondary">采样：{at(snapshot.updatedAt)}</p>
        </details>
      ))}
      <section className="nb-section">
        <h4>状态与日志</h4>
        <div className="nb-actions">
          <button
            disabled={!sessionId || !snapshot.binding || !!busy}
            onClick={() => void read("logs")}
          >
            {busy === "logs" ? "正在读取…" : "最近日志"}
          </button>
          <button
            disabled={!sessionId || !snapshot.binding || !!busy}
            onClick={() => void read("raw")}
          >
            {busy === "raw" ? "正在读取…" : "状态原文"}
          </button>
          <span className="nb-inline-copy">
            脱敏摘要 <CopyValue value={summary} />
          </span>
        </div>
        <p className="nb-note">
          摘要不包含名称、IP 或服务地址。原文仅在此页按需读取。
        </p>
        {data && (
          <div className="nb-output">
            <div className="nb-output-head">
              <span>{data.source}</span>
              <CopyValue value={data.text} />
            </div>
            <pre tabIndex={0}>{data.text || "没有可读取的记录"}</pre>
            <p className="nb-secondary">
              {at(data.checkedAt)}
              {data.truncated ? " · 仅显示最近记录" : ""}
            </p>
          </div>
        )}
      </section>
      <details className="nb-path">
        <summary>客户端路径</summary>
        <p className="nb-note">
          默认从远端 PATH 检测。自定义路径需要指向 Linux 原生 NetBird
          可执行文件。
        </p>
        <label>
          CLI 绝对路径
          <input
            aria-label="NetBird CLI 绝对路径"
            placeholder="/usr/bin/netbird"
            value={path}
            onChange={(e) => setPath(e.target.value)}
          />
        </label>
        <div className="nb-actions">
          <button
            disabled={!sessionId || !!busy || !path.trim().startsWith("/")}
            onClick={() => void savePath(false)}
          >
            保存并检测
          </button>
          <button
            disabled={!sessionId || !!busy}
            onClick={() => void savePath(true)}
          >
            自动检测
          </button>
        </div>
      </details>
      {error && (
        <p className="nb-error" role="alert">
          {error}
        </p>
      )}
    </>
  );
}
