import {
  useCallback,
  useLayoutEffect,
  useImperativeHandle,
  useRef,
  useState,
} from "react";
import {
  LockKeyhole,
  Plug,
  Route,
  TerminalSquare,
  Unplug,
  X,
} from "lucide-react";
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
import { FingerprintDialog, LoginDialog } from "./ConnectionDialogs";
import { WorkspaceLayout } from "./WorkspaceLayout";
import type { WorkspaceLayoutSize } from "./workspaceSizing";
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
        };
      },
      reconnect,
      cancelMove: async () => {
        await terminalController.current?.cancelMove();
      },
    }),
    [tab, prompt, layout, reconnect],
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
    <>
      <WorkspaceLayout
        id={`panel-${tab.id}`}
        labelledBy={`tab-${tab.id}`}
        active={active}
        value={layout}
        onChange={setLayout}
        files={
          <FilePanel
            sessionId={sessionId}
            serverName={tab.server.name}
            active={active}
            initialState={initial?.files}
            onStateChange={captureFiles}
            terminalCwd={terminalCwd ?? undefined}
          />
        }
        services={
          <DockerPanel
            sessionId={sessionId}
            serverName={tab.server.name}
            active={active}
            initialState={initial?.docker}
            onStateChange={captureDocker}
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
                active={active}
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
            <footer className="workspace-status">
              <span
                className="terminal-session-status"
                role="status"
                aria-live="polite"
              >
                <LockKeyhole size={11} aria-hidden="true" />
                <span title={connection.progress || undefined}>
                  {connection.progress ||
                    (sessionId ? "SSH 会话已建立" : "SSH")}
                </span>
              </span>
              {connection.view && connection.view.hops.length > 1 && (
                <span
                  className="connection-route"
                  aria-label="当前连接路径"
                  title={`本机 → ${connection.view.hops.map((hop) => hop.server.name).join(" → ")}`}
                >
                  <Route size={12} aria-hidden="true" />
                  <span>
                    本机 →{" "}
                    {connection.view.hops
                      .map((hop) => hop.server.name)
                      .join(" → ")}
                  </span>
                </span>
              )}
              <span className="terminal-encoding">UTF-8</span>
            </footer>
          </main>
        }
      />
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
    </>
  );
}
