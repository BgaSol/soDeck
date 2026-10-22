import { useState } from "react";
import type { FormEvent } from "react";
import type { ForwardView, ProxyProbeRequest } from "./portForwardApi";

export function NetworkProxyProbe({
  view,
  ready,
  busy,
  onStart,
  onCancel,
}: {
  view: ForwardView;
  ready: boolean;
  busy: boolean;
  onStart: (request: ProxyProbeRequest) => void;
  onCancel: (probeId: string) => void;
}) {
  const [host, setHost] = useState("127.0.0.1");
  const [port, setPort] = useState("80");
  const [protocol, setProtocol] = useState<ProxyProbeRequest["protocol"]>("httpConnect");
  const [error, setError] = useState("");
  const probe = view.runtime?.proxy?.lastProbe || null;
  const activeProbe = view.runtime?.probeId;
  function submit(event: FormEvent) {
    event.preventDefault();
    const normalizedHost = host.trim();
    const value = Number(port);
    if (!normalizedHost) return setError("请输入目标主机");
    if (!/^\d+$/.test(port) || value < 1 || value > 65535)
      return setError("请输入 1–65535 之间的端口");
    setError("");
    onStart({ protocol, target: { host: normalizedHost, port: value } });
  }
  return (
    <div className="proxy-probe">
      <div className="proxy-probe-head">
        <div>
          <strong>测试现场目标</strong>
          <p>只验证代理握手和目标 TCP 通路，不发送 HTTP 请求。</p>
        </div>
        {activeProbe ? (
          <button
            type="button"
            disabled={busy}
            onClick={() => onCancel(activeProbe)}
          >
            取消测试
          </button>
        ) : (
          <button type="submit" form="proxy-probe-form" disabled={!ready || busy}>
            测试连接
          </button>
        )}
      </div>
      <form id="proxy-probe-form" className="proxy-probe-form" onSubmit={submit}>
        <label className="forward-field">
          目标主机
          <input
            value={host}
            onChange={(event) => setHost(event.target.value)}
            disabled={!!activeProbe || busy}
            spellCheck={false}
          />
        </label>
        <label className="forward-field proxy-probe-port">
          端口
          <input
            inputMode="numeric"
            value={port}
            onChange={(event) => setPort(event.target.value)}
            disabled={!!activeProbe || busy}
          />
        </label>
        <div className="proxy-protocol" role="group" aria-label="测试协议">
          <button
            type="button"
            className={protocol === "httpConnect" ? "selected" : ""}
            aria-pressed={protocol === "httpConnect"}
            disabled={!!activeProbe || busy}
            onClick={() => setProtocol("httpConnect")}
          >
            HTTP CONNECT
          </button>
          <button
            type="button"
            className={protocol === "socks5" ? "selected" : ""}
            aria-pressed={protocol === "socks5"}
            disabled={!!activeProbe || busy}
            onClick={() => setProtocol("socks5")}
          >
            SOCKS5
          </button>
        </div>
      </form>
      {error && <p className="forward-field-error" role="alert">{error}</p>}
      {probe && (
        <p className={`proxy-probe-result ${probe.success ? "success" : "failure"}`} role="status">
          {probe.message} · {new Date(probe.finishedAt).toLocaleTimeString()}
        </p>
      )}
    </div>
  );
}
