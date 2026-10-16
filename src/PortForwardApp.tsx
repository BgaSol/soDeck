import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Network,
  Plus,
  Search,
  Play,
  Square,
  Pencil,
  Trash2,
  Copy,
  ExternalLink,
} from "lucide-react";
import { WindowTitlebar } from "./WindowTitlebar";
import { LoginDialog, FingerprintDialog } from "./ConnectionDialogs";
import { PortForwardEditor } from "./PortForwardEditor";
import { usePortForwards } from "./usePortForwards";
import {
  emptyForward,
  endpoint,
  forwardActive,
  forwardApi,
  forwardPhases,
  type ForwardRule,
  type ForwardView,
} from "./portForwardApi";
import { errorOf, type ConnectionAnswer } from "./types";
import { formatMountBytes } from "./remoteMounts";
import { rate } from "./resourceMonitoring";
import "./port-forwarding.css";
import { useForwardConfirm } from "./PortForwardConfirm";

export function PortForwardApp() {
  const data = usePortForwards();
  const confirmation = useForwardConfirm();
  const [selected, setSelected] = useState(""),
    [query, setQuery] = useState("");
  const [editor, setEditor] = useState<ForwardRule | null>(null);
  const [dirty, setDirty] = useState(false),
    [busy, setBusy] = useState(false);
  const [error, setError] = useState(""),
    [notice, setNotice] = useState("");
  const dirtyRef = useRef(false);
  dirtyRef.current = dirty;
  useEffect(() => {
    let disposed = false,
      cleanup: (() => void) | undefined;
    void getCurrentWindow()
      .onCloseRequested(async (event) => {
        if (dirtyRef.current) {
          event.preventDefault();
          if (
            await confirmation.confirm("放弃尚未保存的规则修改并关闭窗口？")
          ) {
            await getCurrentWindow()
              .destroy()
              .catch((e) => setError(errorOf(e).message));
          }
        }
      })
      .then((unlisten) => {
        if (disposed) unlisten();
        else cleanup = unlisten;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, []);
  useEffect(() => {
    if (data.selectionVersion && data.selectedServer && !editor) {
      setEditor(emptyForward(data.selectedServer));
    }
    // Explicit entry requests preselect once; later frames do not overwrite a draft.
  }, [data.selectionVersion]);
  const visible = data.rules.filter(({ rule }) => {
    const server = data.servers.find((s) => s.id === rule.settings.serverId);
    return [rule.name, rule.settings.targetHost, server?.name, server?.host]
      .join(" ")
      .toLowerCase()
      .includes(query.toLowerCase());
  });
  const current = data.rules.find((v) => v.rule.id === selected) || visible[0];
  const runtime = current?.runtime;
  const active = !!current && forwardActive(current);
  const ready = runtime?.phase === "ready";
  const generation = data.generation ?? 0;
  const prompts = data.rules.filter((v) => v.runtime?.prompt);
  const promptView = prompts[0];
  const prompt = promptView?.runtime?.prompt;
  const address = runtime?.actualAddress;
  async function perform(action: () => Promise<unknown>) {
    if (busy) return;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await action();
    } catch (e) {
      setError(errorOf(e).message);
    } finally {
      setBusy(false);
    }
  }
  async function interruption(view: ForwardView, action: string) {
    const count = view.runtime?.activeConnections || 0;
    return (
      count === 0 ||
      (await confirmation.confirm(
        action + "将关闭当前 " + count + " 条业务连接。继续？",
      ))
    );
  }
  function closeEditor() {
    setEditor(null);
    setDirty(false);
  }
  async function answer(value: ConnectionAnswer | null) {
    if (prompt && promptView)
      await forwardApi.answer(generation, promptView, prompt, value);
  }
  async function copy(value: string) {
    await navigator.clipboard.writeText(value);
    setNotice("已复制 " + value);
  }
  return (
    <div className="forward-app">
      <WindowTitlebar
        title="端口转发"
        leading={<Network size={19} />}
        subtitle="通过 SSH 访问服务器及其内网资源"
        onError={(e) => setError(errorOf(e).message)}
      />
      <div className="forward-banner">
        <span>
          已启动 {data.rules.filter((v) => v.runtime?.phase === "ready").length}{" "}
          条
        </span>
        <span>关闭窗口后，已启动的转发继续运行。</span>
        <button
          className="primary"
          disabled={data.generation === null}
          onClick={() =>
            setEditor(emptyForward(data.selectedServer || data.servers[0]?.id))
          }
        >
          <Plus size={15} />
          新建转发
        </button>
      </div>
      {(error || data.error) && (
        <div role="alert" className="connection-error">
          {error || data.error}
        </div>
      )}
      {notice && (
        <div className="forward-notice" role="status">
          {notice}
        </div>
      )}
      <div className="forward-layout">
        <aside className="forward-sidebar" aria-label="转发规则">
          <label className="forward-search">
            <Search size={16} />
            <input
              aria-label="搜索转发规则"
              placeholder="搜索名称、服务器、目标"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </label>
          {!data.loaded ? (
            <p className="forward-empty">正在读取转发规则…</p>
          ) : !visible.length ? (
            <p className="forward-empty">
              {query
                ? "没有匹配的规则。"
                : "还没有转发规则。新建规则后，本机应用即可通过 SSH 访问内网服务。"}
            </p>
          ) : (
            <div className="forward-list">
              {visible.map((view) => (
                <button
                  key={view.rule.id}
                  className="forward-row"
                  aria-pressed={current?.rule.id === view.rule.id}
                  onClick={() => setSelected(view.rule.id)}
                >
                  <span className="forward-row-title">
                    <strong>{view.rule.name}</strong>
                    <span data-phase={view.runtime?.phase || "stopped"}>
                      {forwardPhases[view.runtime?.phase || "stopped"] ||
                        view.runtime?.phase}
                    </span>
                  </span>
                  <code>
                    {view.runtime?.actualAddress ||
                      endpoint(
                        view.rule.settings.listenAddress,
                        view.rule.settings.listenPort,
                      )}
                  </code>
                  <span>
                    →{" "}
                    {endpoint(
                      view.rule.settings.targetHost,
                      view.rule.settings.targetPort,
                    )}
                  </span>
                  <small>
                    {data.servers.find(
                      (s) => s.id === view.rule.settings.serverId,
                    )?.name || "请选择服务器"}{" "}
                    · {view.runtime?.activeConnections || 0} 条连接
                    {view.pendingChanges ? " · 待应用" : ""}
                  </small>
                </button>
              ))}
            </div>
          )}
        </aside>
        <main className="forward-detail">
          {!current ? (
            <div className="forward-welcome">
              <Network size={40} />
              <h2>把内网服务带到本机</h2>
              <p>
                选择 SSH
                服务器和目标地址，使用分配的本地端口连接数据库、网站或其他 TCP
                服务。
              </p>
              {!data.servers.length && data.loaded && (
                <p>请先在主窗口添加服务器配置。</p>
              )}
            </div>
          ) : (
            <>
              <div className="forward-detail-head">
                <div>
                  <h2>{current.rule.name}</h2>
                  <p data-phase={runtime?.phase || "stopped"}>
                    {forwardPhases[runtime?.phase || "stopped"]}
                  </p>
                </div>
                <div className="forward-actions">
                  <button
                    aria-label="编辑规则"
                    title="编辑规则"
                    disabled={busy}
                    onClick={() => setEditor(current.rule)}
                  >
                    <Pencil size={15} />
                  </button>
                  <button
                    aria-label="删除规则"
                    title="删除规则"
                    disabled={busy}
                    onClick={async () => {
                      if (
                        await confirmation.confirm(
                          active
                            ? "删除将停止此规则并关闭所有业务连接。确定删除？"
                            : "确定删除此转发规则？",
                        )
                      )
                        void perform(() =>
                          active
                            ? forwardApi.apply(generation, current, true)
                            : forwardApi.remove(generation, current.rule),
                        );
                    }}
                  >
                    <Trash2 size={15} />
                  </button>
                  {active ? (
                    <button
                      disabled={busy || runtime?.phase === "stopping"}
                      onClick={async () => {
                        if (await interruption(current, "停止转发"))
                          void perform(() =>
                            forwardApi.stop(generation, current),
                          );
                      }}
                    >
                      <Square size={14} />
                      {runtime?.phase === "starting" ? "取消启动" : "停止"}
                    </button>
                  ) : (
                    <button
                      className="primary"
                      disabled={busy || !!current.serverError}
                      onClick={() =>
                        void perform(() =>
                          forwardApi.start(generation, current.rule),
                        )
                      }
                    >
                      <Play size={14} />
                      {runtime?.phase === "failed" ? "重试" : "启动"}
                    </button>
                  )}
                </div>
              </div>
              {current.serverError && (
                <p className="panel-error">
                  {current.serverError}；请编辑规则并重新选择服务器。
                </p>
              )}
              {runtime?.error && (
                <p className="panel-error" role="status">
                  {runtime.error.message}
                </p>
              )}
              {current.pendingChanges && (
                <div className="forward-notice">
                  <span>有修改待应用；当前转发保持启动时的配置。</span>
                  <button
                    disabled={busy}
                    onClick={async () => {
                      if (await interruption(current, "停止并应用"))
                        void perform(() =>
                          forwardApi.apply(generation, current),
                        );
                    }}
                  >
                    停止并应用
                  </button>
                </div>
              )}
              {runtime?.phase === "attention" && (
                <button
                  className="primary"
                  disabled={busy}
                  onClick={() =>
                    void perform(() => forwardApi.resume(generation, current))
                  }
                >
                  处理认证并重试
                </button>
              )}
              {runtime?.phase === "reconnecting" && (
                <p className="field-help">
                  已尝试 {runtime.attempts} 次
                  {runtime.retryAt
                    ? "，下次尝试 " +
                      new Date(runtime.retryAt).toLocaleTimeString()
                    : "，正在建立连接"}
                  。 旧业务连接已关闭，恢复后需由客户端重新连接。
                </p>
              )}
              <section className="forward-endpoint">
                <span>
                  {ready
                    ? "本地访问地址"
                    : address
                      ? "已保留的本地端口 · 暂不可用"
                      : "本地监听配置"}
                </span>
                <code>
                  {address ||
                    endpoint(
                      current.rule.settings.listenAddress,
                      current.rule.settings.listenPort,
                    )}
                </code>
                <div className="forward-actions">
                  <button
                    disabled={!ready || busy || !address}
                    onClick={() => void perform(() => copy(address!))}
                  >
                    <Copy size={14} />
                    复制地址
                  </button>
                  <button
                    disabled={!ready || busy || !address}
                    onClick={() =>
                      void perform(() => copy(runtime!.settings.listenAddress))
                    }
                  >
                    复制主机
                  </button>
                  <button
                    disabled={!ready || busy || !address}
                    onClick={() =>
                      void perform(() =>
                        copy(address!.slice(address!.lastIndexOf(":") + 1)),
                      )
                    }
                  >
                    复制端口
                  </button>
                  {current.rule.webScheme && (
                    <button
                      disabled={!ready || busy}
                      onClick={() =>
                        void perform(() =>
                          forwardApi.openWeb(generation, current),
                        )
                      }
                    >
                      <ExternalLink size={14} />
                      打开网页
                    </button>
                  )}
                </div>
              </section>
              <section>
                <h3>转发路径</h3>
                <p className="forward-route">
                  本机 →{" "}
                  {runtime?.connection
                    ? runtime.connection.hops
                        .map(
                          (h) =>
                            h.server.name +
                            " (" +
                            endpoint(h.server.host, h.server.port) +
                            ")",
                        )
                        .join(" → ")
                    : (() => {
                        const s = data.servers.find(
                          (s) => s.id === current.rule.settings.serverId,
                        );
                        const jump = data.servers.find(
                          (j) => j.id === s?.jumpServerId,
                        );
                        return (
                          (jump ? jump.name + " → " : "") +
                          (s?.name || "未配置服务器")
                        );
                      })()}
                  {" → "}
                  {endpoint(
                    (active ? runtime?.settings : current.rule.settings)!
                      .targetHost,
                    (active ? runtime?.settings : current.rule.settings)!
                      .targetPort,
                  )}
                </p>
                {current.pendingChanges && (
                  <p className="field-help">
                    已保存目标：
                    {endpoint(
                      current.rule.settings.targetHost,
                      current.rule.settings.targetPort,
                    )}
                    ； 本地：
                    {endpoint(
                      current.rule.settings.listenAddress,
                      current.rule.settings.listenPort,
                    )}
                  </p>
                )}
                <p className="field-help">目标由最终 SSH 服务器访问和解析。</p>
                <div className="forward-actions">
                  <button
                    disabled={!ready || busy}
                    onClick={() =>
                      void perform(() =>
                        forwardApi.probe(
                          generation,
                          current,
                          runtime?.probeId || null,
                        ),
                      )
                    }
                  >
                    {runtime?.probeId ? "取消测试" : "测试目标连接"}
                  </button>
                  {runtime?.targetResult && (
                    <span className="field-help">
                      {runtime.targetResult.message} ·{" "}
                      {new Date(runtime.targetResult.at).toLocaleTimeString()}
                    </span>
                  )}
                </div>
                <p className="field-help">
                  测试仅建立并关闭 TCP
                  连接，不发送业务请求；服务端可能记录这次连接。
                </p>
              </section>
              {runtime && (
                <>
                  <section>
                    <h3>本次运行</h3>
                    <div className="forward-metrics">
                      <div>
                        <span>活动连接</span>
                        <strong>{runtime.activeConnections}</strong>
                      </div>
                      <div>
                        <span>发送业务数据</span>
                        <strong>{rate(runtime.sentPerSecond)}</strong>
                        <small>
                          累计 {formatMountBytes(runtime.sentBytes)}
                        </small>
                      </div>
                      <div>
                        <span>接收业务数据</span>
                        <strong>{rate(runtime.receivedPerSecond)}</strong>
                        <small>
                          累计 {formatMountBytes(runtime.receivedBytes)}
                        </small>
                      </div>
                    </div>
                    <p className="field-help">
                      开始于 {new Date(runtime.startedAt).toLocaleString()} ·
                      业务数据不含 SSH 协议开销
                    </p>
                  </section>
                  <section>
                    <h3>近期事件</h3>
                    <ol className="forward-events">
                      {runtime.notes
                        .slice()
                        .reverse()
                        .map((n, i) => (
                          <li key={n.at + ":" + i}>
                            <time>{new Date(n.at).toLocaleTimeString()}</time>
                            <span>
                              {n.message}
                              {(n.count || 0) > 1
                                ? "（重复 " + n.count + " 次）"
                                : ""}
                            </span>
                          </li>
                        ))}
                    </ol>
                  </section>
                </>
              )}
              {current.rule.webScheme && (
                <p className="field-help">
                  网页访问不改写 Host、SNI
                  或重定向；依赖原内网域名的站点可能需要额外配置，HTTPS
                  仍由浏览器检查证书。
                </p>
              )}
            </>
          )}
        </main>
      </div>
      {editor && (
        <PortForwardEditor
          key={editor.id || "new"}
          initial={editor}
          servers={data.servers}
          active={
            !!data.rules.find(
              (v) => v.rule.id === editor.id && forwardActive(v),
            )
          }
          onDirty={setDirty}
          onClose={closeEditor}
          confirm={confirmation.confirm}
          onSave={async (rule, start) => {
            const saved = await forwardApi.save(generation, rule);
            setSelected(saved.id);
            closeEditor();
            if (start) await perform(() => forwardApi.start(generation, saved));
          }}
        />
      )}
      {confirmation.dialog}
      {prompt?.event.type === "authRequired" && (
        <LoginDialog
          key={prompt.event.challenge.id}
          challenge={prompt.event.challenge}
          onSubmit={(credentials) =>
            answer({ type: "credentials", ...credentials })
          }
          onClose={() => void perform(() => answer(null))}
        />
      )}
      {prompt?.event.type === "hostKeyRequired" && (
        <FingerprintDialog
          key={prompt.event.challenge.id}
          challenge={prompt.event.challenge}
          onTrust={() => answer({ type: "trust", accept: true })}
          onClose={() => void perform(() => answer(null))}
        />
      )}
    </div>
  );
}
