import {
  ChevronRight,
  Network,
  ShieldCheck,
  TriangleAlert,
} from "lucide-react";
import type { Action, Snapshot } from "./netbirdApi";
import { at, overall, previewPeers, statusLabel } from "./netbirdPresentation";
import { Fact, PeerRow } from "./NetBirdCommon";
export function NetBirdOverview({
  snapshot: s,
  connected,
  busy,
  onAction,
  onPeers,
  onPeer,
  onDiagnostics,
}: {
  snapshot: Snapshot;
  connected: boolean;
  busy: boolean;
  onAction: (action: Action) => void;
  onPeers: () => void;
  onPeer: (id: string) => void;
  onDiagnostics: () => void;
}) {
  const status = overall(s, connected),
    binding = s.binding;
  const count = s.peers.filter((p) => p.state === "connected").length;
  return (
    <>
      <section
        className={`nb-summary nb-${status.tone}`}
        aria-label="客户端概览"
      >
        {status.tone === "good" ? (
          <ShieldCheck size={25} />
        ) : status.tone === "warning" ? (
          <TriangleAlert size={25} />
        ) : (
          <Network size={25} />
        )}
        <div>
          <h3>{status.label}</h3>
          <p>
            {s.peersComplete
              ? `${s.peers.length} 个可见节点中 ${count} 个已连接`
              : "当前服务器可见的 NetBird 网络"}
          </p>
        </div>
      </section>
      {s.connection === "needsLogin" && (
        <p className="nb-note">请在服务器终端完成 NetBird 登录，然后刷新。</p>
      )}
      {connected && binding?.capabilities.up && (
        <button
          className="primary nb-context-action"
          disabled={busy}
          onClick={() => onAction("up")}
        >
          连接 NetBird
        </button>
      )}
      {connected && binding?.capabilities.serviceStart && (
        <button
          className="primary nb-context-action"
          disabled={busy}
          onClick={() => onAction("serviceStart")}
        >
          启动服务
        </button>
      )}
      <section className="nb-section">
        <h4>本机</h4>
        <dl className="nb-facts">
          <Fact label="名称" copy={s.localPeer?.fqdn ?? undefined}>
            {s.localPeer?.name ?? "未知"}
          </Fact>
          <Fact label="IPv4" copy={s.localPeer?.ipv4 ?? undefined}>
            {s.localPeer?.ipv4 ?? "—"}
          </Fact>
          {s.localPeer?.ipv6 && (
            <Fact label="IPv6" copy={s.localPeer.ipv6}>
              {s.localPeer.ipv6}
            </Fact>
          )}
          <Fact label="客户端">{binding?.clientVersion ?? "未知"}</Fact>
          <Fact label="Daemon">{binding?.daemonVersion ?? "未知"}</Fact>
          {binding?.profile && <Fact label="Profile">{binding.profile}</Fact>}
        </dl>
      </section>
      <section className="nb-section">
        <h4>连接分项</h4>
        <div className="nb-components">
          {(
            [
              ["Management", s.management],
              ["Signal", s.signal],
              ["Relay / STUN", s.relay],
              ["DNS", s.dns],
            ] as const
          ).map(([name, component]) => (
            <button key={name} className="nb-component" onClick={onDiagnostics}>
              <span>{name}</span>
              <span
                className={
                  component.state === "connected"
                    ? "nb-good"
                    : ["error", "disconnected", "unavailable"].includes(
                          component.state,
                        )
                      ? "nb-warning"
                      : "nb-muted"
                }
              >
                {statusLabel(component.state)}
                <ChevronRight size={12} />
              </span>
            </button>
          ))}
        </div>
      </section>
      <section className="nb-section">
        <div className="nb-section-head">
          <h4>
            可见节点{s.reportedTotal != null ? ` · ${s.reportedTotal}` : ""}
          </h4>
          <button className="quiet" onClick={onPeers}>
            全部 <ChevronRight size={13} />
          </button>
        </div>
        {previewPeers(s.peers).map((peer) => (
          <PeerRow key={peer.id} peer={peer} onOpen={onPeer} />
        ))}
        {!s.peers.length && (
          <p className="nb-empty">
            {s.peersComplete ? "当前没有可见节点" : "尚无完整的节点数据"}
          </p>
        )}
        {!s.peersComplete && s.peers.length > 0 && (
          <p className="nb-note">节点列表不完整，仅展示已读取的节点。</p>
        )}
      </section>
      <button className="nb-navigation" onClick={onDiagnostics}>
        <span>检测与日志</span>
        <ChevronRight size={15} />
      </button>
      <p className="nb-timestamp">
        {s.updatedAt ? `上次成功更新 ${at(s.updatedAt)}` : "尚无成功采样"}
      </p>
    </>
  );
}
