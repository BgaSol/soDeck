import {
  useCallback,
  useLayoutEffect,
  useImperativeHandle,
  useRef,
  useState,
} from "react";
import { Plug, TerminalSquare, Unplug, X } from "lucide-react";
import { TerminalPane } from "./TerminalPane";
import type {
  TerminalController,
  TerminalSnapshot,
  TerminalCwd,
} from "./TerminalPane";
import type { Ref } from "react";
import type { FilePanelSnapshot } from "./FilePanel";
import type { DockerPanelSnapshot } from "./DockerPanel";
import { FilePanel } from "./FilePanel";
import { DockerPanel } from "./DockerPanel";
import {
  SessionConnectionDetails,
  type ConnectionPreferences,
} from "./SessionConnectionDetails";
import { monitorWindowApi } from "./monitorWindowApi";
import { MonitorProvider } from "./MonitorProvider";
import {
  ResourcePanel,
  ResourceStatusBar,
  normalizeResourcePreferences,
  type ResourcePreferences,
} from "./ResourcePanel";
import { FingerprintDialog, LoginDialog } from "./ConnectionDialogs";
import { ServicePanel } from "./ServicePanel";
import { WorkspaceLayout } from "./WorkspaceLayout";
import {
  normalizeWorkspaceView,
  type WorkspaceLayoutSize,
  type WorkspaceViewState,
  type WorkspaceVisibility,
} from "./workspaceSizing";
import type { ResourceMetric } from "./resourcePresentation";
import "./workspace-layout.css";
import { useConnection } from "./useConnection";
import { addressOf } from "./types";
import type { AppError } from "./types";
import type { WorkspaceTab } from "./workspaces";

export interface WorkspaceSnapshot {
  tab: WorkspaceTab;
  terminal: TerminalSnapshot;
  files?: FilePanelSnapshot;
  docker?: DockerPanelSnapshot;
  layout?: WorkspaceLayoutSize;
  resources?: ResourcePreferences;
  view?: WorkspaceViewState;
  connectionPreferences?: ConnectionPreferences;
  resourceWidth?: number; // Read legacy snapshots from the former stacked layout.
  fileRatio?: number;
}
export interface WorkspaceController {
  prepareMove: () => Promise<WorkspaceSnapshot>;
  cancelMove: () => Promise<void>;
  reconnect: () => void;
}
export function SessionWorkspace({
  tab,
  active,
  controllerRef,
  initial,
  onReady,
  onChange,
  onConfigurationChanged,
  onConnectionStatus,
  onPromptChange,
}: {
  tab: WorkspaceTab;
  controllerRef?: Ref<WorkspaceController>;
  initial?: WorkspaceSnapshot;
  onReady?: () => void;
  active: boolean;
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
}) {
  const [resources, setResources] = useState<ResourcePreferences>(() =>
    normalizeResourcePreferences(initial?.resources),
  );
  const [view, setView] = useState(() => normalizeWorkspaceView(initial?.view));
  const [visible, setVisible] = useState<WorkspaceVisibility>({
    files: true,
    terminal: true,
    services: true,
  });
  const [connectionPreferences, setConnectionPreferences] =
    useState<ConnectionPreferences>(
      initial?.connectionPreferences ?? {
        minutes: 5,
        scope: initial?.resources?.trafficScope ?? "total",
      },
    );
  const [revealRequest, setRevealRequest] = useState<{
    path: string;
    sequence: number;
  }>();
  function openMetric(metric: ResourceMetric) {
    setResources((p) => ({
      ...p,
      tab: "monitor",
      selectedMetric: metric,
      detailOpen: true,
    }));
    setView({ pane: "services" });
  }
  function revealMount(path: string) {
    setView({ pane: "files" });
    setRevealRequest((p) => ({ path, sequence: (p?.sequence ?? 0) + 1 }));
  }
  const connection = useConnection(tab.server.id, !!initial);
  const { request, prompt } = connection;
  const [sessionId, setSessionId] = useState<string | null>(
      initial?.terminal.sessionId || null,
    ),
    [error, setError] = useState<AppError | null>(null);
  const [terminalCwd, setTerminalCwd] = useState<TerminalCwd | null>(() =>
    initial?.terminal.sessionId && initial.terminal.cwd
      ? { sessionId: initial.terminal.sessionId, ...initial.terminal.cwd }
      : null,
  );
  const [layout, setLayout] = useState<WorkspaceLayoutSize | undefined>(
    initial?.layout ??
      (initial?.resourceWidth
        ? { fileWidth: initial.resourceWidth, serviceWidth: 320 }
        : undefined),
  );
  useLayoutEffect(() => {
    onPromptChange?.(tab.id, active && !!prompt);
    return () => onPromptChange?.(tab.id, false);
  }, [tab.id, active, !!prompt, onPromptChange]);
  function reportState(state: string, failure?: AppError) {
    onConnectionStatus?.(tab.id, request?.attempt ?? 0, state, failure);
  }
  const terminalController = useRef<TerminalController>(null);
  const [restoredTerminal, setRestoredTerminal] = useState(initial?.terminal);
  const filesSnapshot = useRef(initial?.files);
  const dockerSnapshot = useRef(initial?.docker);
  const captureFiles = useCallback((s: FilePanelSnapshot) => {
    filesSnapshot.current = s;
  }, []);
  const captureDocker = useCallback((s: DockerPanelSnapshot) => {
    dockerSnapshot.current = s;
  }, []);
  useImperativeHandle(
    controllerRef,
    () => ({
      prepareMove: async () => {
        if (tab.state === "连接中" || !!prompt)
          throw new Error("请完成连接或关闭认证提示后再移动会话");
        if (!terminalController.current) throw new Error("终端尚未准备好");
        const terminal = await terminalController.current.prepareMove();
        return {
          tab: structuredClone(tab),
          terminal,
          files: filesSnapshot.current,
          docker: dockerSnapshot.current,
          layout,
          resources,
          connectionPreferences,
          view,
        };
      },
      reconnect,
      cancelMove: async () => {
        await terminalController.current?.cancelMove();
      },
    }),
    [tab, prompt, layout, resources, connectionPreferences, view, reconnect],
  );
  function reconnect() {
    setRestoredTerminal(undefined);
    setError(null);
    onChange(tab.id, { state: "连接中" });
    connection.reconnect();
  }
  function disconnect() {
    setRestoredTerminal(undefined);
    connection.disconnect();
    setSessionId(null);
    setError(null);
    onChange(tab.id, { state: "已断开" });
    reportState("已断开");
  }
  return (
    <MonitorProvider sessionId={sessionId} active={active}>
      <div className="session-workspace-host" hidden={!active}>
        <WorkspaceLayout
          id={`panel-${tab.id}`}
          labelledBy={`tab-${tab.id}`}
          active={active}
          value={layout}
          onChange={setLayout}
          view={view}
          onViewChange={setView}
          onVisibilityChange={setVisible}
          files={
            <FilePanel
              sessionId={sessionId}
              serverName={tab.server.name}
              active={active && visible.files}
              revealRequest={revealRequest}
              initialState={initial?.files}
              onStateChange={captureFiles}
              terminalCwd={terminalCwd ?? undefined}
            />
          }
          services={
            <ServicePanel
              preferences={resources}
              onChange={setResources}
              monitor={
                <ResourcePanel
                  serverName={tab.server.name}
                  active={
                    active && visible.services && resources.tab === "monitor"
                  }
                  onOpenMetric={openMetric}
                  onRevealMount={revealMount}
                  onOpenWindow={() =>
                    monitorWindowApi.open(sessionId!, resources)
                  }
                  preferences={resources}
                  onChange={setResources}
                />
              }
              docker={
                <DockerPanel
                  sessionId={sessionId}
                  serverName={tab.server.name}
                  active={
                    active && visible.services && resources.tab === "docker"
                  }
                  initialState={initial?.docker}
                  onStateChange={captureDocker}
                />
              }
            />
          }
          terminal={
            <main className="terminal-panel">
              <header className="terminal-toolbar">
                <span className="terminal-symbol" aria-hidden="true">
                  <TerminalSquare size={18} />
                </span>
                <div className="terminal-identity">
                  <strong title={tab.server.name}>{tab.server.name}</strong>
                  <span
                    className="terminal-address"
                    title={`${tab.server.username}@${addressOf(tab.server)}`}
                  >
                    {tab.server.username}@{addressOf(tab.server)}
                  </span>
                </div>
                <span
                  className={`connection-state ${sessionId ? "running" : tab.state === "连接失败" ? "failed" : tab.state === "连接中" ? "connecting" : ""}`}
                  role="status"
                  title={tab.state}
                  aria-label={tab.state}
                >
                  <span className="connection-state-dot" aria-hidden="true" />
                  <span className="connection-state-label">{tab.state}</span>
                </span>
                {sessionId || tab.state === "连接中" ? (
                  <button
                    className="quiet terminal-connection-action"
                    onClick={disconnect}
                    aria-label={sessionId ? "断开" : "取消连接"}
                    title={sessionId ? "断开连接" : "取消连接"}
                  >
                    <Unplug size={15} aria-hidden="true" />
                    <span>{sessionId ? "断开" : "取消连接"}</span>
                  </button>
                ) : (
                  <button
                    className="primary"
                    disabled={tab.state === "连接中"}
                    onClick={reconnect}
                  >
                    <Plug size={14} aria-hidden="true" />
                    {tab.state === "未连接" ? "连接" : "重新连接"}
                  </button>
                )}
              </header>
              {error && (
                <div className="connection-error" role="alert">
                  <span>{error.message}</span>
                  <button
                    className="icon-button"
                    onClick={() => setError(null)}
                    aria-label="关闭错误提示"
                  >
                    <X size={14} />
                  </button>
                </div>
              )}
              {prompt && !active && (
                <span className="sr-only">等待核对服务器指纹</span>
              )}
              <div className="terminal-host">
                <TerminalPane
                  workspaceId={tab.id}
                  controllerRef={terminalController}
                  initial={restoredTerminal}
                  onReady={onReady}
                  request={request}
                  active={active && visible.terminal}
                  interactive={visible.terminal}
                  onSession={setSessionId}
                  onCwdChange={setTerminalCwd}
                  onState={(state) => {
                    onChange(tab.id, { state });
                    reportState(state);
                  }}
                  onError={(failure) => {
                    setError(failure);
                    reportState("error", failure);
                  }}
                  onConnectionId={connection.setId}
                  onConnectionEvent={(event) => {
                    connection.receive(event);
                    if (event.type === "ready") {
                      onChange(tab.id, { server: event.connection.target });
                      onConfigurationChanged?.();
                    }
                  }}
                />
              </div>
              <footer className="workspace-status session-statusbar">
                <SessionConnectionDetails
                  active={active && visible.terminal}
                  status={sessionId ? "已连接" : tab.state}
                  progress={
                    connection.progress ||
                    (sessionId ? "SSH 会话已建立" : tab.state)
                  }
                  route={
                    connection.view
                      ? `本机 → ${connection.view.hops.map((hop) => hop.server.name).join(" → ")}`
                      : ""
                  }
                  preferences={connectionPreferences}
                  onChange={setConnectionPreferences}
                />
                <ResourceStatusBar
                  networkInterface={resources.networkInterface}
                  active={active && visible.terminal}
                  onOpen={openMetric}
                />
              </footer>
            </main>
          }
        />
      </div>
      {active && prompt?.type === "authRequired" && (
        <LoginDialog
          key={prompt.challenge.id}
          challenge={prompt.challenge}
          disabled={!connection.connectionId}
          onSubmit={(credentials) =>
            connection.answer(prompt.challenge.id, {
              type: "credentials",
              ...credentials,
            })
          }
          onClose={disconnect}
        />
      )}
      {active && prompt?.type === "hostKeyRequired" && (
        <FingerprintDialog
          key={prompt.challenge.id}
          challenge={prompt.challenge}
          disabled={!connection.connectionId}
          onTrust={() =>
            connection.answer(prompt.challenge.id, {
              type: "trust",
              accept: true,
            })
          }
          onClose={disconnect}
        />
      )}
    </MonitorProvider>
  );
}
