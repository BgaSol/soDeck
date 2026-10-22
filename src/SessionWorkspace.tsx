import { DiagnosticErrorInfo } from "./DiagnosticErrorInfo";
import { createPortal } from "react-dom";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useImperativeHandle,
  useRef,
  useState,
  useReducer,
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
import { NetBirdProvider } from "./netbird/NetBirdProvider";
import {
  NetBirdPanel,
  type NetBirdPanelSnapshot,
} from "./netbird/NetBirdPanel";
import { DockerManagementProvider } from "./DockerManagementProvider";
import {
  ResourcePanel,
  ResourceStatusBar,
  type ResourcePreferences,
} from "./ResourcePanel";
import { FingerprintDialog, LoginDialog } from "./ConnectionDialogs";
import { ServicePanel } from "./ServicePanel";
import { WorkspaceLayout } from "./WorkspaceLayout";
import {
  type WorkspaceLayoutSize,
  type WorkspaceViewState,
} from "./workspaceSizing";
import {
  normalizeWorkspacePresentation,
  workspacePresentationReducer,
  type WorkspaceToolState,
  type ToolSide,
} from "./workspaceTools";
import { useWorkspaceLayout } from "./useWorkspaceLayout";
import { useWorkspaceFocus } from "./useWorkspaceFocus";
import { WorkspaceInteractionContext } from "./WorkspaceInteractionContext";
import type { ResourceMetric } from "./resourcePresentation";
import "./workspace-layout.css";
import { useConnection } from "./useConnection";
import type { RecoverySnapshot } from "./connectionRecovery";
import { addressOf } from "./types";
import type { AppError } from "./types";
import type { WorkspaceTab } from "./workspaces";
import { PortForwardButton } from "./PortForwardButton";
import { errorOf } from "./types";

export interface WorkspaceSnapshot {
  tab: WorkspaceTab;
  terminal: TerminalSnapshot;
  recovery?: RecoverySnapshot;
  files?: FilePanelSnapshot;
  docker?: DockerPanelSnapshot;
  netbird?: NetBirdPanelSnapshot;
  layout?: WorkspaceLayoutSize;
  resources?: ResourcePreferences;
  view?: WorkspaceViewState;
  tools?: WorkspaceToolState;
  connectionPreferences?: ConnectionPreferences;
  resourceWidth?: number; // Read legacy snapshots from the former stacked layout.
  fileRatio?: number;
}
export interface WorkspaceController {
  prepareMove: () => Promise<WorkspaceSnapshot>;
  cancelMove: () => Promise<void>;
  reconnect: () => void;
  resumeRecovery: () => void;
}
export function SessionWorkspace({
  tab,
  active,
  promptActive = active,
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
  promptActive?: boolean;
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
  const [presentation, dispatch] = useReducer(
    workspacePresentationReducer,
    initial,
    normalizeWorkspacePresentation,
  );
  const { resources, tools, layout } = presentation;
  const { root, ruler, metrics, resolved } = useWorkspaceLayout(tools, layout);
  const visible = resolved.visible;
  const setResources = (
    next:
      ResourcePreferences | ((p: ResourcePreferences) => ResourcePreferences),
  ) => dispatch({ type: "replace-resource-preferences", next });
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
  const [metricNavigation, setMetricNavigation] = useState(0);
  function openMetric(metric: ResourceMetric) {
    setMetricNavigation((n) => n + 1);
    dispatch({ type: "open-metric", metric });
  }
  function revealMount(path: string) {
    dispatch({ type: "reveal-file" });
    setRevealRequest((p) => ({ path, sequence: (p?.sequence ?? 0) + 1 }));
  }
  const connection = useConnection(tab.server.id, {
    restored: !!initial,
    connected: !!initial?.terminal.sessionId,
    recovery: initial?.recovery,
    onAttempt: () => {
      setNetbirdSuspended(false);
      setRestoredTerminal(undefined);
      setError(null);
      onChange(tab.id, { state: "连接中" });
    },
  });
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
  useLayoutEffect(() => {
    onPromptChange?.(tab.id, promptActive && !!prompt);
    return () => onPromptChange?.(tab.id, false);
  }, [tab.id, promptActive, !!prompt, onPromptChange]);
  function reportState(state: string, failure?: AppError) {
    onConnectionStatus?.(tab.id, request?.attempt ?? 0, state, failure);
  }
  const terminalController = useRef<TerminalController>(null);
  const focus = useWorkspaceFocus(
    root,
    active,
    visible,
    resources.tab,
    terminalController,
  );
  function closeSide(side: ToolSide) {
    focus.requestFocus(side);
    dispatch({ type: "close-side", side });
  }
  const [restoredTerminal, setRestoredTerminal] = useState(initial?.terminal);
  const filesSnapshot = useRef(initial?.files);
  const dockerSnapshot = useRef(initial?.docker);
  const netbirdSnapshot = useRef(initial?.netbird);
  const [netbirdSuspended, setNetbirdSuspended] = useState(false);
  const captureNetBird = useCallback((s: NetBirdPanelSnapshot) => {
    netbirdSnapshot.current = s;
  }, []);
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
        if (
          connection.recovery.phase === "connecting" ||
          tab.state === "连接中" ||
          !!prompt
        )
          throw new Error("请完成连接或关闭认证提示后再移动会话");
        if (!terminalController.current) throw new Error("终端尚未准备好");
        const recovery = connection.pauseRecovery();
        const terminal = await terminalController.current.prepareMove();
        setNetbirdSuspended(true);
        return {
          tab: structuredClone(tab),
          terminal,
          recovery,
          files: filesSnapshot.current,
          docker: dockerSnapshot.current,
          netbird: netbirdSnapshot.current,
          layout,
          resources,
          connectionPreferences,
          tools,
        };
      },
      reconnect,
      resumeRecovery: connection.resumeRecovery,
      cancelMove: async () => {
        try {
          await terminalController.current?.cancelMove();
        } finally {
          setNetbirdSuspended(false);
          connection.resumeRecovery();
        }
      },
    }),
    [tab, prompt, layout, resources, connectionPreferences, tools, reconnect],
  );
  function reconnect() {
    connection.reconnect();
  }
  function disconnect() {
    setNetbirdSuspended(false);
    setRestoredTerminal(undefined);
    connection.disconnect();
    setSessionId(null);
    setError(null);
    onChange(tab.id, { state: "已断开" });
    reportState("已断开");
  }
  const recoveryActive = connection.recovery.automatic;
  const connectionState = prompt
    ? "等待验证"
    : connection.recovery.phase === "waiting"
      ? "等待重连"
      : connection.recovery.phase === "connecting"
        ? recoveryActive
          ? "正在重连"
          : "连接中"
        : connection.recovery.phase === "stopped"
          ? "已断开"
          : tab.state;
  useEffect(() => {
    if (connectionState !== tab.state)
      onChange(tab.id, { state: connectionState });
  }, [connectionState, tab.id, tab.state, onChange]);
  return (
    <MonitorProvider sessionId={sessionId} active={active}>
      <DockerManagementProvider sessionId={sessionId}>
        <NetBirdProvider sessionId={sessionId} suspended={netbirdSuspended}>
          <WorkspaceInteractionContext value={focus.interaction}>
            <div className="session-workspace-host" hidden={!active}>
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
                    sessionId={sessionId}
                    serverName={tab.server.name}
                    active={active && visible.files}
                    onClose={() => closeSide("files")}
                    revealRequest={revealRequest}
                    initialState={initial?.files}
                    onStateChange={captureFiles}
                    terminalCwd={terminalCwd ?? undefined}
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
                          resources.tab === "monitor"
                        }
                        navigationSequence={metricNavigation}
                        onOpenMetric={openMetric}
                        onRevealMount={revealMount}
                        onOpenWindow={() =>
                          monitorWindowApi.open(sessionId!, resources)
                        }
                        preferences={resources}
                        onChange={setResources}
                      />
                    }
                    netbird={
                      <NetBirdPanel
                        workspaceActive={active && !netbirdSuspended}
                        sessionId={sessionId}
                        serverName={tab.server.name}
                        active={
                          active &&
                          !netbirdSuspended &&
                          visible.services &&
                          resources.tab === "netbird"
                        }
                        initialState={initial?.netbird}
                        onStateChange={captureNetBird}
                      />
                    }
                    docker={
                      <DockerPanel
                        serverId={tab.server.id}
                        onRevealPath={revealMount}
                        sessionId={sessionId}
                        serverName={tab.server.name}
                        active={
                          active &&
                          visible.services &&
                          resources.tab === "docker"
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
                        <strong title={tab.server.name}>
                          {tab.server.name}
                        </strong>
                        <span
                          className="terminal-address"
                          title={`${tab.server.username}@${addressOf(tab.server)}`}
                        >
                          {tab.server.username}@{addressOf(tab.server)}
                        </span>
                      </div>
                      <span
                        className={`connection-state ${sessionId ? "running" : tab.state === "连接失败" ? "failed" : tab.state === "连接中" || recoveryActive ? "connecting" : ""}`}
                        role="status"
                        title={connectionState}
                        aria-label={connectionState}
                      >
                        <span
                          className="connection-state-dot"
                          aria-hidden="true"
                        />
                        <span className="connection-state-label">
                          {connectionState}
                        </span>
                      </span>
                      <PortForwardButton
                        serverId={tab.server.id}
                        onError={(e) => setError(errorOf(e))}
                      />
                      {recoveryActive ? (
                        <>
                          <button
                            className="quiet"
                            onClick={reconnect}
                            disabled={connection.recovery.phase !== "waiting"}
                          >
                            立即重连
                          </button>
                          <button className="quiet" onClick={disconnect}>
                            停止重连
                          </button>
                        </>
                      ) : sessionId || tab.state === "连接中" || prompt ? (
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
                    {recoveryActive && (
                      <div className="connection-recovery" role="status">
                        {connection.recovery.phase === "waiting"
                          ? "连接已断开，" +
                            connection.seconds +
                            " 秒后重试 · 第 " +
                            (connection.recovery.attempts + 1) +
                            " 次"
                          : prompt
                            ? "重连需要身份验证"
                            : "正在重新连接 · 第 " +
                              connection.recovery.attempts +
                              " 次"}
                        {error && (
                          <span className="connection-recovery-detail">
                            {error.message}
                          </span>
                        )}
                      </div>
                    )}
                    {error && !recoveryActive && (
                      <div className="connection-error" role="alert">
                        <div>
                          {error.message}
                          <DiagnosticErrorInfo error={error} />
                        </div>
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
                        source={{
                          serverName: tab.server.name,
                          host: tab.server.host,
                          port: tab.server.port,
                          username: tab.server.username,
                        }}
                        workspaceId={tab.id}
                        focusPolicy="managed"
                        controllerRef={terminalController}
                        initial={restoredTerminal}
                        onReady={() => {
                          focus.terminalReady();
                          onReady?.();
                        }}
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
                        onConnectionEvent={(event, attempt) => {
                          connection.receive(event, attempt);
                          if (event.type === "ready") {
                            onChange(tab.id, {
                              server: event.connection.target,
                            });
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
            {promptActive &&
              prompt?.type === "authRequired" &&
              createPortal(
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
                />,
                document.body,
              )}
            {promptActive &&
              prompt?.type === "hostKeyRequired" &&
              createPortal(
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
                />,
                document.body,
              )}
          </WorkspaceInteractionContext>
        </NetBirdProvider>
      </DockerManagementProvider>
    </MonitorProvider>
  );
}
