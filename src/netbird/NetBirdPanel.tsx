import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { ArrowLeft, MoreHorizontal, RefreshCw, X } from "lucide-react";
import { useNetBird } from "./NetBirdProvider";
import { netbirdApi, type Action, type Preview } from "./netbirdApi";
import { activeOperation, actionLabels, at } from "./netbirdPresentation";
import { NetBirdOverview } from "./NetBirdOverview";
import { NetBirdPeers, type PeerFilters } from "./NetBirdPeers";
import { NetBirdPeerDetail } from "./NetBirdPeerDetail";
import { NetBirdDiagnostics } from "./NetBirdDiagnostics";
import { NetBirdOperationDialog } from "./NetBirdOperationDialog";
import { errorOf } from "../types";
import "./netbird.css";
export interface NetBirdPanelSnapshot {
  page: "overview" | "peers" | "peer" | "diagnostics";
  selectedPeerId: string | null;
  peerOrigin: "overview" | "peers";
  filters: PeerFilters;
  scrollTop: Record<string, number>;
}
export const initialPanel = (): NetBirdPanelSnapshot => ({
  page: "overview",
  selectedPeerId: null,
  peerOrigin: "peers",
  filters: { query: "", state: "", mode: "" },
  scrollTop: {},
});
export function NetBirdPanel({
  sessionId,
  serverName,
  active,
  workspaceActive = active,
  initialState,
  onStateChange,
}: {
  sessionId: string | null;
  serverName: string;
  active: boolean;
  workspaceActive?: boolean;
  initialState?: NetBirdPanelSnapshot;
  onStateChange?: (snapshot: NetBirdPanelSnapshot) => void;
}) {
  const { store, state } = useNetBird();
  const snapshot = state.snapshot;
  const [view, setView] = useState<NetBirdPanelSnapshot>(
    () => initialState ?? initialPanel(),
  );
  const [preview, setPreview] = useState<Preview | null>(null),
    [error, setError] = useState(""),
    [pending, setPending] = useState("");
  const epoch = useRef(0),
    task = useRef(false),
    scroll = useRef<HTMLDivElement>(null),
    menu = useRef<HTMLDetailsElement>(null),
    back = useRef<HTMLButtonElement>(null),
    firstPage = useRef(true);
  const saved = useRef(view),
    previousView = useRef(view);
  if (previousView.current !== view) {
    saved.current = view;
    previousView.current = view;
  }
  const changed = useRef(onStateChange);
  changed.current = onStateChange;
  const working = state.operations.some((op) => activeOperation(op.phase));
  const token = snapshot.binding?.token;
  useEffect(() => {
    const update = () =>
      store.setInterest(
        active && sessionId && !document.hidden ? "visible" : "none",
      );
    update();
    document.addEventListener("visibilitychange", update);
    return () => {
      document.removeEventListener("visibilitychange", update);
      store.setInterest("none");
    };
  }, [store, active, sessionId]);
  useEffect(() => {
    ++epoch.current;
    setPreview(null);
    setPending("");
    task.current = false;
    return () => {
      ++epoch.current;
    };
  }, [sessionId, token]);
  useEffect(() => {
    if (!active && menu.current) menu.current.open = false;
  }, [active]);
  useLayoutEffect(() => {
    if (scroll.current)
      scroll.current.scrollTop = saved.current.scrollTop[view.page] ?? 0;
    if (!firstPage.current && active) back.current?.focus();
    firstPage.current = false;
  }, [view.page]);
  useEffect(() => {
    changed.current?.(view);
  }, [view]);
  function navigate(page: NetBirdPanelSnapshot["page"]) {
    change((v) => ({ ...v, page }));
    setError("");
  }
  function openPeer(id: string) {
    change((v) => ({
      ...v,
      page: "peer",
      selectedPeerId: id,
      peerOrigin: v.page === "overview" ? "overview" : "peers",
    }));
  }
  function capture(top: number) {
    saved.current = {
      ...saved.current,
      scrollTop: { ...saved.current.scrollTop, [view.page]: top },
    };
    changed.current?.(saved.current);
  }
  // Scroll values live in a ref so every scroll event does not rerender the workspace.
  function change(next: (v: NetBirdPanelSnapshot) => NetBirdPanelSnapshot) {
    setView(next(saved.current));
  }
  async function run(label: string, fn: () => Promise<unknown>) {
    if (task.current) return;
    task.current = true;
    const current = epoch.current;
    setPending(label);
    setError("");
    try {
      await fn();
    } catch (e) {
      if (current === epoch.current) setError(errorOf(e).message);
    } finally {
      if (current === epoch.current) {
        task.current = false;
        setPending("");
      }
    }
  }
  async function prepare(action: Action) {
    if (!active || !sessionId || !token) return;
    if (menu.current) {
      menu.current.open = false;
      menu.current.querySelector("summary")?.focus();
    }
    const current = epoch.current;
    await run("正在检查 SSH 路径…", async () => {
      const result = await netbirdApi.prepare(sessionId, token, action);
      if (current === epoch.current) setPreview(result);
    });
  }
  const title = {
    overview: "NetBird",
    peers: "节点",
    peer: "节点详情",
    diagnostics: "检测与日志",
  }[view.page];
  const stale =
    !sessionId ||
    ["stale", "disconnected", "unavailable"].includes(
      snapshot.collectionStatus,
    );
  return (
    <section className="nb-panel" aria-label="NetBird 客户端">
      <header className="nb-toolbar">
        {view.page !== "overview" && (
          <button
            ref={back}
            className="icon-button"
            aria-label="返回 NetBird 上一页"
            title="返回"
            onClick={() =>
              change((v) => ({
                ...v,
                page: v.page === "peer" ? v.peerOrigin : "overview",
              }))
            }
          >
            <ArrowLeft size={16} />
          </button>
        )}
        <h2>{title}</h2>
        <button
          className="icon-button"
          aria-label="刷新 NetBird"
          title="刷新 NetBird"
          disabled={!active || !sessionId || state.refreshing || !!pending}
          onClick={() => void run("刷新中…", () => store.refresh())}
        >
          <RefreshCw
            size={15}
            className={state.refreshing ? "nb-spinning" : ""}
          />
        </button>
        {view.page === "overview" && (
          <details className="nb-more" ref={menu}>
            <summary aria-label="NetBird 更多操作" title="更多操作">
              <MoreHorizontal size={18} />
            </summary>
            <div className="nb-popover">
              {(["down", "serviceStart", "serviceRestart"] as const).map(
                (action) => (
                  <button
                    key={action}
                    disabled={
                      !active ||
                      !sessionId ||
                      !token ||
                      working ||
                      !!pending ||
                      !snapshot.binding?.capabilities[action]
                    }
                    onClick={() => void prepare(action)}
                  >
                    {actionLabels[action]}
                  </button>
                ),
              )}
              {snapshot.binding?.capabilities.reason && (
                <p>{snapshot.binding.capabilities.reason}</p>
              )}
              <button
                onClick={() => {
                  if (menu.current) menu.current.open = false;
                  navigate("diagnostics");
                }}
              >
                查看原始状态与日志
              </button>
            </div>
          </details>
        )}
      </header>
      {(stale || snapshot.error) && (
        <div className="nb-banner" role="status">
          {!sessionId
            ? "SSH 已断开，请重新连接服务器。"
            : snapshot.error || "正在显示上次成功的数据。"}
          {snapshot.updatedAt && (
            <small>上次成功更新 {at(snapshot.updatedAt)}</small>
          )}
        </div>
      )}
      {(pending || error) && (
        <div
          className={error ? "nb-banner nb-error" : "nb-banner"}
          role={error ? "alert" : "status"}
        >
          {error || pending}
        </div>
      )}
      <div className="nb-operations" aria-live="polite">
        {state.operations
          .filter((op) => !op.acknowledged)
          .slice(0, 3)
          .map((op) => (
            <div
              className={`nb-operation ${op.phase === "uncertain" ? "nb-warning" : ""}`}
              key={op.id}
            >
              <div>
                <strong>
                  {actionLabels[op.action]} ·{" "}
                  {
                    (
                      {
                        queued: "准备中",
                        running: "执行中",
                        verifying: "正在核对",
                        succeeded: "已完成",
                        failed: "失败",
                        uncertain: "结果待核对",
                      } as Record<string, string>
                    )[op.phase]
                  }
                </strong>
                <p>{op.message}</p>
                {op.phase === "uncertain" && (
                  <button
                    disabled={!active || !sessionId || !!pending || working}
                    onClick={() =>
                      void run("正在检查当前状态…", () =>
                        netbirdApi.reconcile(sessionId!, op.id),
                      )
                    }
                  >
                    检查当前状态
                  </button>
                )}
              </div>
              {!activeOperation(op.phase) && (
                <button
                  className="icon-button"
                  disabled={!active || !sessionId || !!pending}
                  aria-label="收起操作记录"
                  onClick={() =>
                    void run("", () =>
                      netbirdApi.acknowledge(sessionId!, op.id),
                    )
                  }
                >
                  <X size={12} />
                </button>
              )}
            </div>
          ))}
      </div>
      {view.page === "peers" ? (
        <NetBirdPeers
          snapshot={snapshot}
          filters={view.filters}
          onChange={(filters) =>
            change((v) => ({
              ...v,
              filters,
              scrollTop: { ...v.scrollTop, peers: 0 },
            }))
          }
          onPeer={(id) =>
            change((v) => ({
              ...v,
              page: "peer",
              selectedPeerId: id,
              peerOrigin: "peers",
            }))
          }
          scrollTop={view.scrollTop.peers ?? 0}
          onScroll={capture}
        />
      ) : (
        <div
          className="nb-scroll"
          ref={scroll}
          onScroll={(e) => capture(e.currentTarget.scrollTop)}
        >
          {view.page === "overview" && (
            <NetBirdOverview
              snapshot={snapshot}
              connected={!!sessionId}
              busy={!active || working || !!pending}
              onAction={(action) => void prepare(action)}
              onPeers={() => change((v) => ({ ...v, page: "peers" }))}
              onPeer={openPeer}
              onDiagnostics={() =>
                change((v) => ({ ...v, page: "diagnostics" }))
              }
            />
          )}
          {view.page === "peer" && (
            <NetBirdPeerDetail
              key={`${view.selectedPeerId}-${token}`}
              peer={snapshot.peers.find((p) => p.id === view.selectedPeerId)}
              sessionId={sessionId}
              token={token ?? null}
              enabled={
                active &&
                !!sessionId &&
                !!token &&
                snapshot.collectionStatus === "ready"
              }
            />
          )}
          {view.page === "diagnostics" && (
            <NetBirdDiagnostics
              key={token}
              snapshot={snapshot}
              active={active}
              sessionId={sessionId}
              onRefresh={() => store.refresh()}
            />
          )}
        </div>
      )}
      {preview && sessionId && (
        <NetBirdOperationDialog
          preview={preview}
          visible={active}
          workspaceActive={workspaceActive}
          serverName={serverName}
          onConfirm={async (request) => {
            await netbirdApi.submit(sessionId, request);
          }}
          onClose={() => setPreview(null)}
        />
      )}
    </section>
  );
}
