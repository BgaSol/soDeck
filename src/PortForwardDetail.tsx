import { useEffect, useRef, useState } from "react";
import {
  ArrowDown,
  ArrowUp,
  Check,
  ChevronDown,
  ChevronRight,
  Copy,
  ExternalLink,
  MoreHorizontal,
  Network,
  Play,
  Square,
} from "lucide-react";
import {
  endpoint,
  forwardActive,
  forwardPhases,
  type ForwardView,
} from "./portForwardApi";
import {
  forwardPresentation,
  localDescription,
  runningTime,
} from "./portForwardPresentation";
import { PortForwardPath } from "./PortForwardPath";
import { PortForwardMenu } from "./PortForwardMenu";
import { formatMountBytes } from "./remoteMounts";
import { rate } from "./resourceMonitoring";
import { errorOf, type Server } from "./types";

export function ForwardStatus({ phase }: { phase: string }) {
  return (
    <span className="forward-status" data-phase={phase}>
      <i aria-hidden="true" />
      {forwardPhases[phase] || phase}
    </span>
  );
}

export function PortForwardDetail({
  view,
  servers,
  busy,
  onStart,
  onStop,
  onResume,
  onEdit,
  onDelete,
  onApply,
  onProbe,
  onOpenWeb,
}: {
  view: ForwardView;
  servers: Server[];
  busy: boolean;
  onStart: () => void;
  onStop: () => void;
  onResume: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onApply: () => void;
  onProbe: () => void;
  onOpenWeb: () => void;
}) {
  const runtime = view.runtime;
  const active = forwardActive(view),
    ready = runtime?.phase === "ready";
  const presentation = forwardPresentation(view, servers);
  const savedPresentation = forwardPresentation(
    { ...view, runtime: null },
    servers,
  );
  const { settings, hops, local, address, copyable, target } = presentation;
  const routeDescription = (route: Server[]) =>
    route.map((s) => `${s.name} (${endpoint(s.host, s.port)})`).join(" → ") ||
    "未配置服务器";
  const [copied, setCopied] = useState(false),
    [copyError, setCopyError] = useState("");
  const copyTimer = useRef<ReturnType<typeof setTimeout> | undefined>(
    undefined,
  );
  useEffect(() => () => clearTimeout(copyTimer.current), []);
  async function copy(value: string) {
    setCopyError("");
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      clearTimeout(copyTimer.current);
      copyTimer.current = setTimeout(() => setCopied(false), 1800);
    } catch (e) {
      setCopyError("无法复制：" + errorOf(e).message);
    }
  }
  const targetResult = runtime?.targetResult;
  const targetFailed = ready && targetResult && !targetResult.success;
  const diagnosticSummary = runtime?.probeId
    ? "正在测试目标…"
    : targetResult
      ? `${targetResult.source === "probe" ? "上次测试" : "最近连接"}${targetResult.success ? "成功" : "失败"} · ${new Date(targetResult.at).toLocaleTimeString()}`
      : "目标尚未测试";
  return (
    <>
      <div className="forward-detail-head">
        <div className="forward-heading">
          <div className="forward-heading-icon">
            <Network size={18} aria-hidden="true" />
          </div>
          <div>
            <h2>{view.rule.name}</h2>
            <div className="forward-subtitle">
              <ForwardStatus phase={runtime?.phase || "stopped"} />
              {active && runtime && (
                <span>{runningTime(runtime.startedAt)}</span>
              )}
            </div>
          </div>
        </div>
        <div className="forward-actions">
          {runtime?.phase === "attention" && (
            <button className="primary" disabled={busy} onClick={onResume}>
              {runtime.error?.kind === "auth" ? "继续认证" : "重试连接"}
            </button>
          )}
          {active ? (
            <button
              disabled={busy || runtime?.phase === "stopping"}
              onClick={onStop}
            >
              <Square size={14} />
              {runtime?.phase === "starting" ? "取消启动" : "停止"}
            </button>
          ) : (
            <button
              className="primary"
              disabled={busy || !!view.serverError}
              onClick={onStart}
            >
              <Play size={14} />
              {runtime?.phase === "failed" ? "重试" : "启动"}
            </button>
          )}
          <PortForwardMenu
            label="更多操作"
            disabled={busy}
            items={[
              { label: "编辑规则", run: onEdit },
              { label: "删除转发", run: onDelete, danger: true },
            ]}
          >
            <MoreHorizontal size={17} />
          </PortForwardMenu>
        </div>
      </div>
      {view.serverError && (
        <div className="forward-error" role="alert">
          <span>{view.serverError}；请编辑规则并重新选择服务器。</span>
          <button onClick={onEdit} disabled={busy}>
            编辑规则
          </button>
        </div>
      )}
      {runtime?.error && (
        <div className="forward-error" role="status">
          {runtime.error.message}
        </div>
      )}
      {runtime?.phase === "reconnecting" && (
        <p className="field-help">
          已尝试 {runtime.attempts} 次
          {runtime.retryAt
            ? `，下次尝试 ${new Date(runtime.retryAt).toLocaleTimeString()}`
            : "，正在建立连接"}
          。恢复后需由客户端重新连接。
        </p>
      )}
      {view.pendingChanges && (
        <div className="forward-pending">
          <div className="forward-pending-head">
            <span>配置已保存，下次启动时生效</span>
            <button disabled={busy} onClick={onApply}>
              重新启动
            </button>
          </div>
          <details>
            <summary>查看配置变化</summary>
            <dl className="forward-comparison">
              <dt>当前运行</dt>
              <dd>
                {local} → {routeDescription(hops)} → {target}
                <small>
                  {settings.autoReconnect ? "自动重连" : "不自动重连"}
                </small>
              </dd>
              <dt>下次启动</dt>
              <dd>
                {localDescription(view.rule.settings)} →{" "}
                {routeDescription(savedPresentation.hops)} →{" "}
                {savedPresentation.target}
                <small>
                  {view.rule.settings.autoReconnect ? "自动重连" : "不自动重连"}
                </small>
              </dd>
            </dl>
            <p className="field-help">
              服务器地址、认证或跳板的更改也会在重新启动后使用。
            </p>
          </details>
        </div>
      )}
      <section className="forward-endpoint" aria-label="本地访问">
        <span>
          {ready
            ? "本地访问地址"
            : address
              ? "已保留的本地端口 · 暂不可用"
              : "本地监听配置"}
        </span>
        <div className="forward-endpoint-main">
          <code>{local}</code>
          <div className="forward-actions">
            {view.rule.webScheme && (
              <button
                className="primary"
                disabled={!copyable || busy}
                onClick={onOpenWeb}
              >
                <ExternalLink size={14} />
                打开网页
              </button>
            )}
            <div
              className={`forward-copy${!view.rule.webScheme && ready ? " forward-copy-primary" : ""}`}
            >
              <button
                disabled={!copyable}
                aria-label={copied ? "已复制" : "复制地址"}
                onClick={() => void copy(address!)}
              >
                {copied ? <Check size={14} /> : <Copy size={14} />}
                <span aria-live="polite">{copied ? "已复制" : "复制地址"}</span>
              </button>
              <PortForwardMenu
                label="更多复制选项"
                disabled={!copyable}
                items={[
                  {
                    label: "复制主机",
                    run: () => void copy(settings.listenAddress),
                  },
                  {
                    label: "复制端口",
                    run: () =>
                      void copy(address!.slice(address!.lastIndexOf(":") + 1)),
                  },
                ]}
              >
                <ChevronDown size={14} />
              </PortForwardMenu>
            </div>
          </div>
        </div>
        <p className="field-help">
          {ready
            ? "在本机客户端中使用此地址。"
            : active
              ? "SSH 路径就绪后即可使用本地地址。"
              : settings.listenPort === null
                ? "启动后显示实际地址；再次启动可能分配不同端口。"
                : "启动后，在本机客户端中使用此地址。"}
        </p>
        {copyError && (
          <p className="forward-field-error" role="alert">
            {copyError}
          </p>
        )}
      </section>
      <section>
        <div className="forward-section-head">
          <h3>转发路径</h3>
          <span>SSH 加密连接</span>
        </div>
        <PortForwardPath hops={hops} local={local} target={target} />
      </section>
      {runtime && (
        <section>
          <div className="forward-section-head">
            <h3>本次运行</h3>
            <span>业务流量</span>
          </div>
          <div className="forward-metrics">
            <div>
              <span>活动连接</span>
              <strong>
                {runtime.activeConnections}
                <small> 条</small>
              </strong>
              <small>{active ? "正在使用此隧道" : "转发已停止"}</small>
            </div>
            <div>
              <span>
                <ArrowUp size={14} />
                发送
              </span>
              <strong>{rate(runtime.sentPerSecond)}</strong>
              <small>累计 {formatMountBytes(runtime.sentBytes)}</small>
            </div>
            <div>
              <span>
                <ArrowDown size={14} />
                接收
              </span>
              <strong>{rate(runtime.receivedPerSecond)}</strong>
              <small>累计 {formatMountBytes(runtime.receivedBytes)}</small>
            </div>
          </div>
        </section>
      )}
      {targetFailed && (
        <div className="forward-error" role="alert">
          <div>
            <strong>目标连接失败</strong>
            <p>{targetResult.message}</p>
            <small>
              {new Date(targetResult.at).toLocaleTimeString()} · SSH
              隧道仍已启动
            </small>
          </div>
          <button disabled={busy || !!runtime?.probeId} onClick={onProbe}>
            重新测试
          </button>
        </div>
      )}
      <details className="forward-diagnostics">
        <summary>
          <ChevronRight size={14} />
          <span>连接检查与事件</span>
          <small>{diagnosticSummary}</small>
        </summary>
        <div className="forward-diagnostics-body">
          <div className="forward-probe">
            <div>
              <strong>
                {runtime?.probeId
                  ? "正在测试目标连接…"
                  : targetResult
                    ? targetResult.success
                      ? "目标连接成功"
                      : "目标连接失败"
                    : "目标尚未测试"}
              </strong>
              {targetResult && (
                <p>
                  {targetResult.message}
                  <br />
                  <time>
                    {new Date(targetResult.at).toLocaleString()}
                  </time> ·{" "}
                  {targetResult.source === "probe"
                    ? "主动 TCP 测试"
                    : "业务连接结果"}
                </p>
              )}
            </div>
            <button disabled={!ready || busy} onClick={onProbe}>
              {runtime?.probeId ? "取消测试" : "测试目标连接"}
            </button>
          </div>
          <p className="field-help">
            目标由最终 SSH 服务器访问和解析。测试只建立并关闭 TCP
            连接，不发送业务请求；结果代表测试时刻，服务端可能记录这次连接。
          </p>
          {runtime && (
            <p className="field-help">
              本次运行开始于 {new Date(runtime.startedAt).toLocaleString()}
              ；业务流量不含 SSH 协议开销。
            </p>
          )}
          {view.rule.webScheme && (
            <p className="field-help">
              网页访问不改写 Host、SNI 或重定向；HTTPS 仍由浏览器检查证书。
            </p>
          )}
          <h3>近期事件</h3>
          {runtime?.notes.length ? (
            <ol className="forward-events">
              {runtime.notes
                .slice()
                .reverse()
                .map((n, i) => (
                  <li key={n.at + ":" + i}>
                    <time>{new Date(n.at).toLocaleTimeString()}</time>
                    <span>
                      {n.message}
                      {(n.count || 0) > 1 ? `（重复 ${n.count} 次）` : ""}
                    </span>
                  </li>
                ))}
            </ol>
          ) : (
            <p className="field-help">暂无事件</p>
          )}
        </div>
      </details>
    </>
  );
}
