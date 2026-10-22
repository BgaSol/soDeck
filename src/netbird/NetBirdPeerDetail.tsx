import { useEffect, useRef, useState } from "react";
import { netbirdApi, type Peer, type ProbeResult } from "./netbirdApi";
import { at, bytes, latency, statusLabel } from "./netbirdPresentation";
import { Fact } from "./NetBirdCommon";
import { errorOf } from "../types";
export function NetBirdPeerDetail({
  peer,
  contextId,
  token,
  enabled,
}: {
  peer: Peer | undefined;
  contextId: string | null;
  token: string | null;
  enabled: boolean;
}) {
  const [kind, setKind] = useState<"icmp" | "tcp">("icmp"),
    [port, setPort] = useState("443"),
    [busy, setBusy] = useState(false),
    [result, setResult] = useState<ProbeResult | null>(null),
    [error, setError] = useState("");
  const epoch = useRef(0),
    pending = useRef(false);
  useEffect(() => {
    ++epoch.current;
    return () => {
      ++epoch.current;
    };
  }, [peer?.id, token, contextId]);
  if (!peer)
    return (
      <p className="nb-empty">节点已不在当前快照中。可以返回列表继续查看。</p>
    );
  async function probe() {
    if (!enabled || !peer || !contextId || !token || pending.current) return;
    pending.current = true;
    const current = epoch.current;
    setBusy(true);
    setError("");
    setResult(null);
    try {
      const next = await netbirdApi.probe(contextId, token, {
        peerId: peer.id,
        kind,
        port: kind === "tcp" ? Number(port) : null,
      });
      if (current === epoch.current) setResult(next);
    } catch (e) {
      if (current === epoch.current) setError(errorOf(e).message);
    } finally {
      pending.current = false;
      if (current === epoch.current) setBusy(false);
    }
  }
  return (
    <>
      <div className="nb-detail-title">
        <h3>{peer.name}</h3>
        <span className={peer.state === "connected" ? "nb-good" : "nb-muted"}>
          {statusLabel(peer.state)}
        </span>
      </div>
      <dl className="nb-facts">
        <Fact label="IPv4" copy={peer.ipv4 ?? undefined}>
          {peer.ipv4 ?? "—"}
        </Fact>
        {peer.ipv6 && (
          <Fact label="IPv6" copy={peer.ipv6}>
            {peer.ipv6}
          </Fact>
        )}
        <Fact label="FQDN" copy={peer.fqdn ?? undefined}>
          {peer.fqdn ?? "—"}
        </Fact>
        <Fact label="连接方式">{statusLabel(peer.connectionMode)}</Fact>
        <Fact label="延迟">{latency(peer.latencyMs)}</Fact>
        {peer.latencyMeasuredAt && (
          <Fact label="延迟采样">{at(peer.latencyMeasuredAt)}</Fact>
        )}
        <Fact label="最近握手">{at(peer.lastHandshakeAt)}</Fact>
        <Fact label="累计接收">{bytes(peer.receivedBytes)}</Fact>
        <Fact label="累计发送">{bytes(peer.sentBytes)}</Fact>
        <Fact label="状态更新">{at(peer.stateChangedAt)}</Fact>
      </dl>
      {peer.networks.length > 0 && (
        <section className="nb-section">
          <h4>关联网络</h4>
          <ul>
            {peer.networks.map((network, index) => (
              <li key={`${index}-${network}`}>{network}</li>
            ))}
          </ul>
        </section>
      )}
      <section className="nb-section">
        <h4>连通性测试</h4>
        <p className="nb-note">
          从当前服务器发起单次检测，结果独立于隧道状态。
        </p>
        <div className="nb-probe-form">
          <select
            aria-label="检测方式"
            value={kind}
            disabled={busy}
            onChange={(e) => setKind(e.target.value as "icmp" | "tcp")}
          >
            <option value="icmp">Ping</option>
            <option value="tcp">TCP</option>
          </select>
          {kind === "tcp" && (
            <input
              aria-label="TCP 端口"
              inputMode="numeric"
              type="number"
              min={1}
              max={65535}
              value={port}
              onChange={(e) => setPort(e.target.value)}
              disabled={busy}
            />
          )}
          <button
            disabled={
              !enabled ||
              busy ||
              (kind === "tcp" &&
                (!/^\d+$/.test(port) ||
                  Number(port) < 1 ||
                  Number(port) > 65535))
            }
            onClick={() => void probe()}
          >
            {busy ? "检测中…" : "检测"}
          </button>
        </div>
        <div role="status">
          {result && (
            <p className="nb-note">
              {result.message}
              <br />从 {result.source} → {result.address}
              <br />
              {at(result.checkedAt)}
            </p>
          )}
        </div>
        {error && (
          <p className="nb-error" role="alert">
            {error}
          </p>
        )}
      </section>
    </>
  );
}
