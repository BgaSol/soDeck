import { invokeDiagnosed as invoke } from "./diagnostics";
import { projectWorkspace, type ServerWorkspaceView } from "./workspaces";
import { workspaceArgs } from "./workspaceApi";
import {
  useCallback,
  useEffect,
  useImperativeHandle,
  useReducer,
  useRef,
  useState,
  type Ref,
} from "react";
import type { TerminalController } from "./TerminalPane";
import {
  TerminalSession,
  type TerminalDetails,
  type TerminalSessionController,
  type TerminalSessionSnapshot,
  type TerminalHeaderState,
} from "./TerminalSession";
import { TerminalTabs, type TerminalHeaderAction } from "./TerminalTabs";
import {
  WorkspaceToolConnection,
  type WorkspaceToolController,
  type WorkspaceToolView,
} from "./WorkspaceToolConnection";
import { WorkspaceConnectionQueue } from "./workspaceConnectionQueue";
import { WorkspaceCloseDialog } from "./WorkspaceCloseDialog";
import { FilePanel, type FilePanelSnapshot } from "./FilePanel";
import { DockerPanel, type DockerPanelSnapshot } from "./DockerPanel";
import {
  NetBirdPanel,
  type NetBirdPanelSnapshot,
} from "./netbird/NetBirdPanel";
import { MonitorProvider } from "./MonitorProvider";
import { DockerManagementProvider } from "./DockerManagementProvider";
import { NetBirdProvider } from "./netbird/NetBirdProvider";
import { ResourcePanel, type ResourcePreferences } from "./ResourcePanel";
import type { ResourceMetric } from "./resourcePresentation";
import { monitorWindowApi } from "./monitorWindowApi";
import type { ConnectionPreferences } from "./SessionConnectionDetails";
import { ServicePanel } from "./ServicePanel";
import { WorkspaceLayout } from "./WorkspaceLayout";
import { useWorkspaceLayout } from "./useWorkspaceLayout";
import { useWorkspaceFocus } from "./useWorkspaceFocus";
import { WorkspaceInteractionContext } from "./WorkspaceInteractionContext";
import {
  normalizeWorkspacePresentation,
  workspacePresentationReducer,
  type WorkspaceToolState,
  type ToolSide,
} from "./workspaceTools";
import type { WorkspaceLayoutSize } from "./workspaceSizing";
import { terminalTitle, type WorkspaceTab } from "./workspaces";
import { workspaceApi } from "./workspaceApi";
import { errorOf, type AppError } from "./types";
import "./workspace-layout.css";

export interface WorkspaceSnapshot {
  version: 2;
  transferId: string;
  tab: WorkspaceTab;
  terminals: TerminalSessionSnapshot[];
  files?: FilePanelSnapshot;
  docker?: DockerPanelSnapshot;
  netbird?: NetBirdPanelSnapshot;
  layout?: WorkspaceLayoutSize;
  resources?: ResourcePreferences;
  tools?: WorkspaceToolState;
  connectionPreferences?: ConnectionPreferences;
}
export interface WorkspaceController {
  prepareMove: (
    onPrepared?: (transferId: string) => void,
  ) => Promise<WorkspaceSnapshot>;
  cancelMove: (expectedTransferId?: string) => Promise<void>;
  reconnect: () => void;
  resumeRecovery: () => void;
  runToolConnection: () => void;
}
export function ServerWorkspace({
  tab,
  active,
  promptActive = active,
  controllerRef,
  initial,
  onReady,
  onChange: _onChange,
  onConfigurationChanged,
  onConnectionStatus,
  onPromptChange,
  onToolState,
}: {
  tab: WorkspaceTab;
  active: boolean;
  promptActive?: boolean;
  controllerRef?: Ref<WorkspaceController>;
  initial?: WorkspaceSnapshot;
  onReady?: () => void;
  onChange: (
    id: string,
    patch: Partial<Pick<WorkspaceTab, "server" | "state">>,
  ) => void;
  onConfigurationChanged?: () => void;
  onConnectionStatus?: (
    id: string,
    attempt: number,
    state: string,
    error?: AppError,
  ) => void;
  onPromptChange?: (id: string, visible: boolean) => void;
  onToolState?: (id: string, state: WorkspaceToolView) => void;
}) {
  const [presentation, dispatch] = useReducer(
    workspacePresentationReducer,
    initial,
    normalizeWorkspacePresentation,
  );
  const { resources, tools, layout } = presentation;
  const { root, ruler, metrics, resolved } = useWorkspaceLayout(tools, layout);
  const visible = resolved.visible;
  const handles = useRef(new Map<string, TerminalSessionController>());
  const toolController = useRef<WorkspaceToolController>(null);
  const [toolView, setToolView] = useState<WorkspaceToolView>({
    busy: false,
    error: "",
    promptPending: false,
  });
  const [headers, setHeaders] = useState<Record<string, TerminalHeaderState>>(
    {},
  );
  const captureHeader = useCallback(
    (id: string, value: TerminalHeaderState) => {
      setHeaders((previous) => ({ ...previous, [id]: value }));
    },
    [],
  );
  useEffect(() => {
    onToolState?.(tab.id, toolView);
  }, [onToolState, tab.id, toolView]);
  const [selected, setSelected] = useState(tab.activeTerminalId);
  const mru = useRef<string[]>([]);
  const activeId = tab.terminals.some((t) => t.id === selected)
    ? selected
    : tab.activeTerminalId;
  const activeController = useRef<TerminalController | null>(null);
  activeController.current = activeId
    ? (handles.current.get(activeId) ?? null)
    : null;
  const focus = useWorkspaceFocus(
    root,
    active,
    visible,
    resources.tab,
    activeController,
  );
  const queue = useRef(new WorkspaceConnectionQueue()).current;
  const [suspended, setSuspended] = useState(!!initial);
  const moving = suspended || tab.lifecycle !== "active";
  const contextId = tab.tools.context?.id ?? null;
  useEffect(() => {
    if (tab.lifecycle === "active")
      void invoke("mount_server_workspace", workspaceArgs(tab)).catch((e) =>
        setError(errorOf(e).message),
      );
  }, [tab.id, tab.owner.ownerEpoch, tab.lifecycle]);
  const [error, setError] = useState("");
  const [closingTerminal, setClosingTerminal] = useState<string | null>(null);
  const [details, setDetails] = useState<Record<string, TerminalDetails>>({});
  const captureDetails = useCallback(
    (id: string, value: TerminalDetails) =>
      setDetails((previous) => ({ ...previous, [id]: value })),
    [],
  );
  const current = activeId ? details[activeId] : undefined;
  const identityMatches =
    !!current?.view && current.view.identity === tab.tools.connection?.identity;
  const terminalCwd =
    identityMatches && current?.sessionId === current?.cwd?.sessionId
      ? (current?.cwd ?? undefined)
      : undefined;
  const [preferences, setPreferences] = useState<ConnectionPreferences>(
    initial?.connectionPreferences ?? {
      minutes: 5,
      scope: initial?.resources?.trafficScope ?? "total",
    },
  );
  const [revealRequest, setRevealRequest] = useState<{
    path: string;
    sequence: number;
  }>();
  const [metricNavigation, setMetricNavigation] = useState(0);
  const snapshots = useRef({
    files: initial?.files,
    docker: initial?.docker,
    netbird: initial?.netbird,
  });
  const captureFiles = useCallback((files: FilePanelSnapshot) => {
    snapshots.current.files = files;
  }, []);
  const captureDocker = useCallback((docker: DockerPanelSnapshot) => {
    snapshots.current.docker = docker;
  }, []);
  const captureNetbird = useCallback((netbird: NetBirdPanelSnapshot) => {
    snapshots.current.netbird = netbird;
  }, []);
  const transfer = useRef<string | null>(initial?.transferId ?? null);
  const restored = useRef(new Set<string>());
  const announced = useRef(false);
  const readyCallback = useRef(onReady);
  readyCallback.current = onReady;
  const terminalReady = useCallback(
    (id: string) => {
      restored.current.add(id);
      if (!initial) {
        focus.terminalReady();
        return;
      }
      if (
        !announced.current &&
        initial.terminals.every((t) => restored.current.has(t.id))
      ) {
        announced.current = true;
        readyCallback.current?.();
      }
    },
    [initial, focus.terminalReady],
  );
  useEffect(() => {
    if (initial && !initial.terminals.length && !announced.current) {
      announced.current = true;
      readyCallback.current?.();
    }
  }, [initial]);
  useEffect(() => {
    setSelected(tab.activeTerminalId);
  }, [tab.activeTerminalId]);
  function select(id: string) {
    setSelected(id);
    mru.current = [id, ...mru.current.filter((v) => v !== id)];
    void workspaceApi
      .update(tab, { type: "select", id })
      .catch((e) => setError(errorOf(e).message));
  }
  function openMetric(metric: ResourceMetric) {
    setMetricNavigation((n) => n + 1);
    dispatch({ type: "open-metric", metric });
  }
  function revealMount(path: string) {
    dispatch({ type: "reveal-file" });
    setRevealRequest((p) => ({ path, sequence: (p?.sequence ?? 0) + 1 }));
  }
  function closeSide(side: ToolSide) {
    focus.requestFocus(side);
    dispatch({ type: "close-side", side });
  }
  function terminalAction(id: string, action: TerminalHeaderAction) {
    if (
      moving ||
      !active ||
      tab.lifecycle !== "active" ||
      !tab.terminals.some((t) => t.id === id)
    )
      return;
    const controller = handles.current.get(id);
    if (!controller) return;
    if (action === "disconnect") {
      controller.disconnect();
      return;
    }
    if (action === "reconnect") {
      controller.reconnect();
      return;
    }
    if (id !== activeId) return;
    if (action === "history") controller.toggleHistory();
    if (action === "search") controller.openHistorySearch();
    if (action === "challenge") controller.presentChallenge();
    if (action === "details")
      root.current
        ?.querySelector<HTMLButtonElement>(
          ".terminal-session:not([hidden]) [aria-label='通信详情']",
        )
        ?.click();
    if (action === "copy") void controller.clipboard.copySelection();
    if (action === "paste") {
      controller.focus();
      void controller.clipboard.requestPaste();
    }
    if (action === "clear") {
      controller.focus();
      void controller.clearDisplay().catch((e) => setError(errorOf(e).message));
    }
  }
  async function createTerminal() {
    try {
      await workspaceApi.createTerminal(tab);
    } catch (e) {
      setError(errorOf(e).message);
    }
  }
  async function closeTerminal(id: string) {
    const next = mru.current.find(
      (candidate) =>
        candidate !== id && tab.terminals.some((t) => t.id === candidate),
    );
    await workspaceApi.closeTerminal(tab, id);
    setHeaders((previous) => {
      const copy = { ...previous };
      delete copy[id];
      return copy;
    });
    if (activeId === id && next) select(next);
    setDetails((previous) => {
      const copy = { ...previous };
      delete copy[id];
      return copy;
    });
  }
  useImperativeHandle(controllerRef, () => ({
    runToolConnection: () => {
      if (!moving) toolController.current?.run();
    },
    prepareMove: async (onPrepared) => {
      if (
        document.querySelector("dialog[open]") ||
        tab.tools.state === "verifying" ||
        toolView.busy
      )
        throw new Error("请先完成或取消当前对话框及工具验证，再移动工作区");
      if (handles.current.size !== tab.terminals.length)
        throw new Error("终端尚未准备好");
      const prepared = await invoke<{
        transferId: string;
        workspace: ServerWorkspaceView;
      }>("prepare_workspace_move", workspaceArgs(tab));
      transfer.current = prepared.transferId;
      onPrepared?.(prepared.transferId);
      const shared = { ...snapshots.current };
      setSuspended(true);
      const result = await Promise.allSettled(
        tab.terminals.map((t) => handles.current.get(t.id)!.snapshot()),
      );
      const failed = result.find(
        (r): r is PromiseRejectedResult => r.status === "rejected",
      );
      if (failed) {
        await invoke("cancel_workspace_move", { transferId: transfer.current });
        transfer.current = null;
        await Promise.allSettled(
          [...handles.current.values()].map((h) => h.cancelMove()),
        );
        setSuspended(false);
        throw failed.reason;
      }
      return {
        version: 2,
        transferId: prepared.transferId,
        tab: structuredClone({
          ...projectWorkspace(prepared.workspace),
          activeTerminalId: activeId,
        }),
        terminals: result.map(
          (r) => (r as PromiseFulfilledResult<TerminalSessionSnapshot>).value,
        ),
        ...shared,
        layout,
        resources,
        tools,
        connectionPreferences: preferences,
      };
    },
    cancelMove: async (expectedTransferId) => {
      const transferId = transfer.current;
      if (expectedTransferId && transferId !== expectedTransferId) return;
      if (transferId) await invoke("cancel_workspace_move", { transferId });
      if (transfer.current !== transferId) return;
      transfer.current = null;
      await Promise.allSettled(
        [...handles.current.values()].map((h) => h.cancelMove()),
      );
      setSuspended(false);
    },
    resumeRecovery: () => {
      transfer.current = null;
      for (const h of handles.current.values()) h.resumeRecovery();
      setSuspended(false);
    },
    reconnect: () => {
      if (activeId) handles.current.get(activeId)?.reconnect();
    },
  }));
  return (
    <MonitorProvider
      contextId={moving ? null : contextId}
      active={active && !moving}
    >
      <DockerManagementProvider contextId={moving ? null : contextId}>
        <NetBirdProvider contextId={contextId} suspended={moving}>
          <WorkspaceInteractionContext value={focus.interaction}>
            <WorkspaceToolConnection
              workspace={tab}
              active={promptActive && !moving}
              queue={queue}
              controllerRef={toolController}
              onState={setToolView}
            />
            <div
              className="session-workspace-host"
              hidden={!active}
              inert={moving}
            >
              <WorkspaceLayout
                id={`panel-${tab.id}`}
                labelledBy={`tab-${tab.id}`}
                active={active}
                root={root}
                ruler={ruler}
                resolved={resolved}
                metrics={metrics}
                selected={resources.tab}
                onFiles={() =>
                  dispatch({ type: "toggle-files", visible: visible.files })
                }
                onService={(tool) =>
                  dispatch({
                    type: "toggle-service",
                    tool,
                    visible: visible.services,
                  })
                }
                onClose={closeSide}
                onReturn={() => {
                  focus.requestFocus("terminal");
                  dispatch({ type: "return-to-terminal" });
                }}
                onResize={(side, width) =>
                  dispatch({ type: "set-size", side, width })
                }
                files={
                  <FilePanel
                    contextId={contextId}
                    contextIdentity={tab.tools.connection?.identity}
                    serverName={tab.server.name}
                    active={active && visible.files && !moving}
                    onClose={() => closeSide("files")}
                    revealRequest={revealRequest}
                    initialState={initial?.files}
                    onStateChange={captureFiles}
                    terminalCwd={terminalCwd}
                  />
                }
                services={
                  <ServicePanel
                    preferences={resources}
                    onClose={() => closeSide("services")}
                    monitor={
                      <ResourcePanel
                        serverName={tab.server.name}
                        active={
                          active &&
                          visible.services &&
                          resources.tab === "monitor" &&
                          !moving
                        }
                        navigationSequence={metricNavigation}
                        onOpenMetric={openMetric}
                        onRevealMount={revealMount}
                        onOpenWindow={async () => {
                          if (contextId)
                            await monitorWindowApi.open(contextId, resources);
                        }}
                        preferences={resources}
                        onChange={(next) =>
                          dispatch({
                            type: "replace-resource-preferences",
                            next,
                          })
                        }
                      />
                    }
                    netbird={
                      <NetBirdPanel
                        workspaceActive={active && !moving}
                        contextId={contextId}
                        serverName={tab.server.name}
                        active={
                          active &&
                          visible.services &&
                          resources.tab === "netbird" &&
                          !moving
                        }
                        initialState={initial?.netbird}
                        onStateChange={captureNetbird}
                      />
                    }
                    docker={
                      <DockerPanel
                        serverId={tab.server.id}
                        onRevealPath={revealMount}
                        contextId={moving ? null : contextId}
                        serverName={tab.server.name}
                        active={
                          active &&
                          visible.services &&
                          resources.tab === "docker" &&
                          !moving
                        }
                        initialState={initial?.docker}
                        onStateChange={captureDocker}
                      />
                    }
                  />
                }
                terminal={
                  <main className="terminal-panel">
                    <TerminalTabs
                      tabs={tab.terminals}
                      activeId={activeId}
                      disabled={moving}
                      active={active && visible.terminal}
                      header={activeId ? headers[activeId] : undefined}
                      onAction={terminalAction}
                      onSelect={select}
                      onCreate={() => void createTerminal()}
                      onClose={(id) => {
                        const t = tab.terminals.find((t) => t.id === id);
                        if (t && !["disconnected", "failed"].includes(t.state))
                          setClosingTerminal(id);
                        else
                          void closeTerminal(id).catch((e) =>
                            setError(errorOf(e).message),
                          );
                      }}
                      onRename={(id, title) => {
                        void workspaceApi
                          .update(tab, { type: "rename", id, title })
                          .catch((e) => setError(errorOf(e).message));
                      }}
                      onReorder={(ids) => {
                        void workspaceApi
                          .update(tab, { type: "reorder", ids })
                          .catch((e) => setError(errorOf(e).message));
                      }}
                    />
                    {error && (
                      <div role="alert" className="connection-error">
                        {error}
                        <button onClick={() => setError("")}>关闭提示</button>
                      </div>
                    )}
                    <div className="terminal-sessions">
                      {tab.terminals.map((terminal) => (
                        <TerminalSession
                          key={terminal.id}
                          workspace={tab}
                          tab={terminal}
                          active={
                            active &&
                            visible.terminal &&
                            activeId === terminal.id
                          }
                          interactive={!moving && activeId === terminal.id}
                          promptActive={
                            promptActive && activeId === terminal.id && !moving
                          }
                          queue={queue}
                          initial={initial?.terminals.find(
                            (t) => t.id === terminal.id,
                          )}
                          controllerRef={(value) => {
                            if (value) handles.current.set(terminal.id, value);
                            else handles.current.delete(terminal.id);
                            if (terminal.id === activeId)
                              activeController.current = value;
                          }}
                          onDetails={captureDetails}
                          onHeaderState={captureHeader}
                          onReady={terminalReady}
                          onConfigurationChanged={onConfigurationChanged}
                          onPromptChange={onPromptChange}
                          onConnectionStatus={onConnectionStatus}
                          resources={resources}
                          preferences={preferences}
                          onPreferences={setPreferences}
                          onMetric={openMetric}
                        />
                      ))}
                      {!tab.terminals.length && (
                        <div className="terminal-empty">
                          <span>此服务器暂无终端，文件和工具仍可使用。</span>
                          <button
                            className="primary"
                            onClick={() => void createTerminal()}
                          >
                            新建终端
                          </button>
                        </div>
                      )}
                    </div>
                  </main>
                }
              />
            </div>
            {closingTerminal &&
              tab.terminals.some((t) => t.id === closingTerminal) && (
                <WorkspaceCloseDialog
                  title={`关闭${terminalTitle(tab.terminals.find((t) => t.id === closingTerminal)!)}`}
                  details="该终端的连接将结束，尚在运行的前台命令可能中断。其他终端和服务器工具继续保留。"
                  onClose={() => setClosingTerminal(null)}
                  onConfirm={() => closeTerminal(closingTerminal)}
                />
              )}
          </WorkspaceInteractionContext>
        </NetBirdProvider>
      </DockerManagementProvider>
    </MonitorProvider>
  );
}
