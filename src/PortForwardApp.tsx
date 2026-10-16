import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { LockKeyhole, Network, Plus, Search, X } from "lucide-react";
import { WindowTitlebar } from "./WindowTitlebar";
import { LoginDialog, FingerprintDialog } from "./ConnectionDialogs";
import { PortForwardEditor } from "./PortForwardEditor";
import { ForwardStatus, PortForwardDetail } from "./PortForwardDetail";
import { usePortForwards } from "./usePortForwards";
import {
  emptyForward,
  forwardActive,
  forwardApi,
  type ForwardRule,
  type ForwardView,
} from "./portForwardApi";
import { forwardPresentation } from "./portForwardPresentation";
import type { ForwardSaveIntent } from "./portForwardDraft";
import { errorOf, type ConnectionAnswer } from "./types";
import { useForwardConfirm } from "./PortForwardConfirm";
import "./port-forwarding.css";

export function PortForwardApp() {
  const data = usePortForwards();
  const confirmation = useForwardConfirm();
  const [selected, setSelected] = useState(""),
    [query, setQuery] = useState("");
  const [scope, setScope] = useState<string | null>(null);
  const [editor, setEditor] = useState<ForwardRule | null>(null);
  const [dirty, setDirty] = useState(false),
    [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState<{
    rule: ForwardRule;
    message: string;
  } | null>(null);
  const search = useRef<HTMLInputElement>(null);
  const dirtyRef = useRef(false),
    busyRef = useRef(false);
  const latest = useRef(data);
  latest.current = data;
  dirtyRef.current = dirty;
  const appliedRequest = useRef("");
  const remembered = useRef(new Map<string | null, string>());
  const visible = data.rules.filter((view) => {
    const p = forwardPresentation(view, data.servers);
    return (
      (!scope || p.settings.serverId === scope) &&
      p.search.includes(query.trim().toLowerCase())
    );
  });
  const current = visible.find((v) => v.rule.id === selected) || visible[0];
  const generation = data.generation ?? 0;
  const currentId = current?.rule.id || "";
  useEffect(() => {
    if (currentId) remembered.current.set(scope, currentId);
    setSelected(currentId);
  }, [currentId, scope]);
  useEffect(() => {
    const request = data.openRequest;
    // Keep an incoming navigation request until the editor is closed.
    if (!request || request.id === appliedRequest.current || editor) return;
    appliedRequest.current = request.id;
    setScope(request.serverId);
    setQuery("");
    const rememberedId = remembered.current.get(request.serverId);
    if (rememberedId) setSelected(rememberedId);
  }, [data.openRequest, editor]);
  useEffect(() => {
    let disposed = false,
      cleanup: (() => void) | undefined;
    void getCurrentWindow()
      .onCloseRequested(async (event) => {
        if (busyRef.current) {
          event.preventDefault();
          return;
        }
        if (dirtyRef.current) {
          event.preventDefault();
          if (
            await confirmation.confirm({
              title: "放弃修改并关闭？",
              message:
                "尚未保存的转发设置将被丢弃。已启动的转发继续在后台运行。",
              confirmLabel: "放弃并关闭",
              danger: true,
            })
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
  }, [confirmation.confirm]);
  function newRule() {
    if (
      data.generation !== null &&
      !editor &&
      !busyRef.current &&
      data.servers.length
    )
      setEditor(emptyForward(scope || data.servers[0]?.id));
  }
  const shortcuts = useRef({ newRule, editor });
  shortcuts.current = { newRule, editor };
  useEffect(() => {
    function keydown(e: KeyboardEvent) {
      if (
        !(e.metaKey || e.ctrlKey) ||
        e.altKey ||
        e.shiftKey ||
        document.querySelector("dialog[open]")
      )
        return;
      if (e.key.toLowerCase() === "f") {
        e.preventDefault();
        search.current?.focus();
        search.current?.select();
      }
      if (e.key.toLowerCase() === "n") {
        e.preventDefault();
        shortcuts.current.newRule();
      }
    }
    window.addEventListener("keydown", keydown);
    return () => window.removeEventListener("keydown", keydown);
  }, []);
  async function perform(action: () => Promise<unknown>) {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError("");
    try {
      await action();
    } catch (e) {
      setError(errorOf(e).message);
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }
  async function interruption(view: ForwardView, title: string) {
    const count = view.runtime?.activeConnections || 0;
    return (
      count === 0 ||
      (await confirmation.confirm({
        title,
        message: `这会断开当前 ${count} 条业务连接，客户端需要重新连接。`,
        confirmLabel: title,
      }))
    );
  }
  function closeEditor() {
    setEditor(null);
    setDirty(false);
  }
  async function save(rule: ForwardRule, intent: ForwardSaveIntent) {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError("");
    try {
      const live = latest.current.rules.find((v) => v.rule.id === rule.id);
      if (
        intent === "restart" &&
        live &&
        forwardActive(live) &&
        !(await interruption(live, "保存并重启"))
      )
        return;
      const saved = await forwardApi.save(generation, rule);
      setSelected(saved.id);
      setQuery("");
      setRetry(null);
      // The selected rule must remain visible if its saved server was changed.
      if (
        scope &&
        saved.settings.serverId !== scope &&
        (!live || !forwardActive(live))
      )
        setScope(null);
      closeEditor();
      try {
        if (intent === "start") await forwardApi.start(generation, saved);
        if (intent === "restart") {
          if (live && forwardActive(live))
            await forwardApi.apply(generation, { ...live, rule: saved });
          else await forwardApi.start(generation, saved);
          if (scope && saved.settings.serverId !== scope) setScope(null);
        }
      } catch (e) {
        setRetry({
          rule: saved,
          message: `配置已保存，但${intent === "restart" ? "重新启动" : "启动"}未完成：${errorOf(e).message}`,
        });
      }
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }
  const promptView = data.rules.find((v) => v.runtime?.prompt);
  const prompt = promptView?.runtime?.prompt;
  async function answer(value: ConnectionAnswer | null) {
    if (prompt && promptView)
      await forwardApi.answer(generation, promptView, prompt, value);
  }
  const scopeName =
    data.servers.find((s) => s.id === scope)?.name || "所选服务器";
  const readyCount = data.rules.filter(
    (v) => v.runtime?.phase === "ready",
  ).length;
  const attentionCount = data.rules.filter(
    (v) =>
      !!v.serverError ||
      ["failed", "attention", "authenticating"].includes(
        v.runtime?.phase || "",
      ) ||
      (v.runtime?.phase === "ready" &&
        v.runtime.targetResult?.success === false),
  ).length;
  return (
    <div className="forward-app">
      <WindowTitlebar
        title="端口转发"
        leading={<Network size={19} />}
        onError={(e) => setError(errorOf(e).message)}
        actions={
          <button
            disabled={data.generation === null || busy || !data.servers.length}
            onClick={newRule}
          >
            <Plus size={15} />
            新建转发
          </button>
        }
      />
      {(error || data.error) && (
        <div role="alert" className="connection-error">
          <span>{error || data.error}</span>
          {error && (
            <button
              className="icon-button"
              aria-label="关闭错误提示"
              onClick={() => setError("")}
            >
              <X size={14} />
            </button>
          )}
        </div>
      )}
      {retry && (
        <div role="alert" className="forward-save-error">
          <span>{retry.message}</span>
          <button
            disabled={busy}
            onClick={() =>
              void perform(async () => {
                const live = latest.current.rules.find(
                  (v) => v.rule.id === retry.rule.id,
                );
                if (live && forwardActive(live)) {
                  if (!(await interruption(live, "重新启动"))) return;
                  await forwardApi.apply(generation, live);
                } else
                  await forwardApi.start(generation, live?.rule || retry.rule);
                setRetry(null);
              })
            }
          >
            重试启动
          </button>
          <button
            className="icon-button"
            aria-label="关闭启动提示"
            onClick={() => setRetry(null)}
          >
            <X size={14} />
          </button>
        </div>
      )}
      <div className="forward-layout">
        <aside className="forward-sidebar" aria-label="转发规则">
          <label className="forward-search">
            <Search size={15} />
            <input
              ref={search}
              aria-label="搜索转发规则"
              placeholder="搜索名称、地址或服务器"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
            {query && (
              <button
                className="icon-button"
                aria-label="清除搜索"
                onClick={() => {
                  setQuery("");
                  search.current?.focus();
                }}
              >
                <X size={12} />
              </button>
            )}
          </label>
          {scope && (
            <div className="forward-scope">
              <span title={scopeName}>{scopeName}</span>
              <button
                className="icon-button"
                aria-label="清除服务器筛选"
                onClick={() => setScope(null)}
              >
                <X size={13} />
              </button>
            </div>
          )}
          <div className="forward-list-heading">
            <span>转发规则</span>
            <span>{visible.length}</span>
          </div>
          <div className="forward-list">
            {!data.loaded ? (
              <p className="forward-empty">正在读取转发规则…</p>
            ) : !visible.length ? (
              <p className="forward-empty">
                {query
                  ? "没有匹配的转发"
                  : scope
                    ? "此服务器暂无转发"
                    : "暂无转发规则"}
              </p>
            ) : (
              visible.map((view) => {
                const p = forwardPresentation(view, data.servers);
                const description = [
                  p.serverName,
                  forwardActive(view)
                    ? `${view.runtime!.activeConnections} 条连接`
                    : "",
                  view.pendingChanges ? "待应用" : "",
                ]
                  .filter(Boolean)
                  .join(" · ");
                return (
                  <button
                    key={view.rule.id}
                    className="forward-row"
                    aria-pressed={currentId === view.rule.id}
                    title={`${view.rule.name}\n${p.local}\n${description}`}
                    onClick={() => setSelected(view.rule.id)}
                  >
                    <span className="forward-row-title">
                      <strong>{view.rule.name}</strong>
                      <ForwardStatus phase={view.runtime?.phase || "stopped"} />
                    </span>
                    <code>{p.local}</code>
                    <small>{description}</small>
                  </button>
                );
              })
            )}
          </div>
          <div className="forward-sidebar-foot">
            <LockKeyhole size={14} />
            仅本机可访问
          </div>
        </aside>
        <main className="forward-detail">
          {current ? (
            <PortForwardDetail
              key={current.rule.id}
              view={current}
              servers={data.servers}
              busy={busy}
              onEdit={() => setEditor(current.rule)}
              onStart={() =>
                void perform(() => forwardApi.start(generation, current.rule))
              }
              onStop={() =>
                void perform(async () => {
                  if (await interruption(current, "停止转发"))
                    await forwardApi.stop(generation, current);
                })
              }
              onResume={() =>
                void perform(() => forwardApi.resume(generation, current))
              }
              onApply={() =>
                void perform(async () => {
                  if (await interruption(current, "重新启动"))
                    await forwardApi.apply(generation, current);
                })
              }
              onProbe={() =>
                void perform(() =>
                  forwardApi.probe(
                    generation,
                    current,
                    current.runtime?.probeId || null,
                  ),
                )
              }
              onOpenWeb={() =>
                void perform(() => forwardApi.openWeb(generation, current))
              }
              onDelete={() =>
                void perform(async () => {
                  const count = current.runtime?.activeConnections || 0;
                  if (
                    await confirmation.confirm({
                      title: `删除“${current.rule.name}”？`,
                      message: forwardActive(current)
                        ? `此转发将停止${count ? `，并断开 ${count} 条业务连接` : ""}。删除后无法恢复。`
                        : "删除后无法恢复。",
                      confirmLabel: "删除转发",
                      danger: true,
                    })
                  ) {
                    if (forwardActive(current))
                      await forwardApi.apply(generation, current, true);
                    else await forwardApi.remove(generation, current.rule);
                    if (retry?.rule.id === current.rule.id) setRetry(null);
                  }
                })
              }
            />
          ) : (
            <div className="forward-welcome">
              <Network size={32} />
              <h2>
                {!data.loaded
                  ? "正在读取转发"
                  : query
                    ? "没有匹配的转发"
                    : !data.servers.length
                      ? "先添加一台服务器"
                      : scope
                        ? "此服务器暂无转发"
                        : "创建第一条转发"}
              </h2>
              <p>
                {query
                  ? "试试其他名称、地址或服务器。"
                  : !data.servers.length
                    ? "在主窗口添加 SSH 服务器后，即可配置端口转发。"
                    : "将远程 TCP 服务连接到本机，在熟悉的客户端中使用。"}
              </p>
              {query ? (
                <button onClick={() => setQuery("")}>清除搜索</button>
              ) : (
                data.loaded &&
                !!data.servers.length && (
                  <button className="primary" onClick={newRule}>
                    <Plus size={14} />
                    {scope ? "为此服务器新建转发" : "创建转发"}
                  </button>
                )
              )}
            </div>
          )}
        </main>
      </div>
      <footer className="forward-statusbar">
        <span>
          {data.rules.length} 条转发 · {readyCount} 条已启动
          {attentionCount > 0 ? ` · ${attentionCount} 条需处理` : ""}
        </span>
        <span>关闭窗口后，转发继续运行</span>
      </footer>
      {editor && (
        <PortForwardEditor
          key={editor.id || "new"}
          initial={editor}
          servers={data.servers}
          view={data.rules.find((v) => v.rule.id === editor.id)}
          onDirty={setDirty}
          onClose={closeEditor}
          confirm={confirmation.confirm}
          onSave={save}
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
