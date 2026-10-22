import {
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
  type Ref,
} from "react";
import { createPortal } from "react-dom";
import { ArrowLeft, X } from "lucide-react";
import {
  TerminalPane,
  type TerminalCwd,
  type TerminalSnapshot,
} from "./TerminalPane";
import {
  type TerminalChromeState,
  type TerminalWorkspaceController,
} from "./TerminalWorkspace";
import { useConnection } from "./useConnection";
import { LoginDialog, FingerprintDialog } from "./ConnectionDialogs";
import { DiagnosticErrorInfo } from "./DiagnosticErrorInfo";
import {
  SessionConnectionDetails,
  type ConnectionPreferences,
} from "./SessionConnectionDetails";
import { ResourceStatusBar, type ResourcePreferences } from "./ResourcePanel";
import type { ResourceMetric } from "./resourcePresentation";
import type { RecoverySnapshot } from "./connectionRecovery";
import {
  terminalStateLabel,
  type WorkspaceTab,
  type TerminalTab,
} from "./workspaces";
import { workspaceApi } from "./workspaceApi";
import { invokeDiagnosed as invoke } from "./diagnostics";
import type { AppError, ConnectionRequest, ConnectionView } from "./types";
import type { WorkspaceConnectionQueue } from "./workspaceConnectionQueue";

export interface TerminalSessionSnapshot {
  connection?: ConnectionView | null;
  id: string;
  terminal: TerminalSnapshot;
  recovery: RecoverySnapshot;
}
export interface TerminalSessionController extends TerminalWorkspaceController {
  snapshot: () => Promise<TerminalSessionSnapshot>;
  reconnect: () => void;
  disconnect: () => void;
  resumeRecovery: () => void;
  presentChallenge: () => void;
}
export interface TerminalHeaderState {
  status: string;
  sessionId: string | null;
  automatic: boolean;
  phase: string;
  seconds: number;
  attempts: number;
  prompt: boolean;
  error: AppError | null;
  chrome?: TerminalChromeState;
}
export interface TerminalDetails {
  sessionId: string | null;
  cwd: TerminalCwd | null;
  view: ConnectionView | null;
  prompt: boolean;
}

export function TerminalSession({
  workspace,
  tab,
  active,
  interactive,
  promptActive,
  initial,
  queue,
  controllerRef,
  onDetails,
  onReady,
  onConfigurationChanged,
  onPromptChange,
  onConnectionStatus,
  onHeaderState,
  resources,
  preferences,
  onPreferences,
  onMetric,
}: {
  workspace: WorkspaceTab;
  tab: TerminalTab;
  active: boolean;
  interactive: boolean;
  promptActive: boolean;
  initial?: TerminalSessionSnapshot;
  queue: WorkspaceConnectionQueue;
  controllerRef: Ref<TerminalSessionController>;
  onDetails: (id: string, details: TerminalDetails) => void;
  onReady: (id: string) => void;
  onConfigurationChanged?: () => void;
  onPromptChange?: (id: string, visible: boolean) => void;
  onHeaderState?: (id: string, state: TerminalHeaderState) => void;
  onConnectionStatus?: (
    id: string,
    attempt: number,
    state: string,
    error?: AppError,
  ) => void;
  resources: ResourcePreferences;
  preferences: ConnectionPreferences;
  onPreferences: (p: ConnectionPreferences) => void;
  onMetric: (m: ResourceMetric) => void;
}) {
  const [restored, setRestored] = useState(initial?.terminal);
  const [sessionId, setSessionId] = useState(
    initial?.terminal.sessionId ?? null,
  );
  const [cwd, setCwd] = useState<TerminalCwd | null>(
    initial?.terminal.sessionId && initial.terminal.cwd
      ? { sessionId: initial.terminal.sessionId, ...initial.terminal.cwd }
      : null,
  );
  const [error, setError] = useState<AppError | null>(null);
  const [state, setState] = useState(terminalStateLabel(tab.state));
  const live = useRef<TerminalWorkspaceController>(null);
  const [chrome, setChrome] = useState<TerminalChromeState>();
  const release = useRef<(() => void) | null>(null);
  const restoredConnection = useRef(
    !!initial || tab.state !== "queued",
  ).current;
  const connection = useConnection(workspace.server.id, {
    restored: restoredConnection,
    connected: !!initial?.terminal.sessionId,
    recovery: initial?.recovery,
    initialAttempt: tab.attempt,
    onAttempt: () => {
      setRestored(undefined);
      setError(null);
      setSessionId(null);
      setCwd(null);
      setState("连接中");
    },
  });
  const [admitted, setAdmitted] = useState<ConnectionRequest>();
  useEffect(() => {
    const abort = new AbortController();
    setAdmitted(undefined);
    if (connection.request) {
      const request = connection.request;
      void queue.acquire(abort.signal).then((done) => {
        if (abort.signal.aborted) {
          done();
          return;
        }
        release.current = done;
        setAdmitted(request);
      });
    }
    return () => {
      abort.abort();
      release.current = null;
    };
  }, [connection.request, queue]);
  const status = connection.prompt
    ? "等待验证"
    : connection.recovery.phase === "waiting"
      ? "等待重连"
      : connection.recovery.phase === "stopped"
        ? "已断开"
        : connection.recovery.automatic &&
            connection.recovery.phase === "connecting"
          ? "正在重连"
          : connection.request && admitted !== connection.request
            ? "等待连接"
            : state;
  useEffect(() => {
    onHeaderState?.(tab.id, {
      status,
      sessionId,
      automatic: connection.recovery.automatic,
      phase: connection.recovery.phase,
      seconds: connection.seconds,
      attempts: connection.recovery.attempts,
      prompt: !!connection.prompt,
      error,
      chrome,
    });
  }, [
    onHeaderState,
    tab.id,
    status,
    sessionId,
    connection.recovery.automatic,
    connection.recovery.phase,
    connection.recovery.attempts,
    connection.seconds,
    !!connection.prompt,
    error,
    chrome,
  ]);
  useEffect(() => {
    const map: Record<string, TerminalTab["state"]> = {
      等待连接: "queued",
      连接中: "connecting",
      等待验证: "authenticating",
      等待重连: "waiting",
      已连接: "connected",
      已断开: "disconnected",
      连接失败: "failed",
    };
    const next = map[status];
    if (next && next !== tab.state && workspace.lifecycle === "active")
      void workspaceApi
        .update(workspace, {
          type: "report",
          id: tab.id,
          attempt: connection.request?.attempt ?? tab.attempt,
          state: next,
        })
        .catch(() => {});
  }, [
    status,
    tab.state,
    tab.id,
    tab.attempt,
    workspace.id,
    workspace.lifecycle,
    connection.request?.attempt,
  ]);
  useEffect(() => {
    onDetails(tab.id, {
      sessionId,
      cwd,
      view: connection.view ?? initial?.connection ?? null,
      prompt: !!connection.prompt,
    });
  }, [tab.id, sessionId, cwd, connection.view, !!connection.prompt, onDetails]);
  useLayoutEffect(() => {
    onPromptChange?.(workspace.id, promptActive && !!connection.prompt);
    return () => onPromptChange?.(workspace.id, false);
  }, [workspace.id, promptActive, !!connection.prompt, onPromptChange]);
  useEffect(() => {
    if (promptActive && connection.prompt && connection.connectionId)
      void invoke("present_connection_challenge", {
        connectionId: connection.connectionId,
        challengeId: connection.prompt.challenge.id,
      }).catch(() => {});
  }, [promptActive, connection.prompt, connection.connectionId]);
  const disconnect = () => {
    setRestored(undefined);
    connection.disconnect();
    release.current?.();
    setSessionId(null);
    setCwd(null);
    setState("已断开");
  };
  useImperativeHandle(controllerRef, () => ({
    focus: () => live.current?.focus(),
    clipboard: {
      copySelection: async () => live.current?.clipboard.copySelection(),
      requestPaste: async (r) => live.current?.clipboard.requestPaste(r),
      selectAllBuffer: () => live.current?.clipboard.selectAllBuffer(),
      sendControl: async (k) => live.current?.clipboard.sendControl(k),
    },
    clearDisplay: async () => live.current?.clearDisplay(),
    toggleHistory: () => live.current?.toggleHistory(),
    openHistorySearch: () => live.current?.openHistorySearch(),
    prepareMove: async () => {
      if (!live.current) throw new Error("终端尚未准备好");
      return live.current.prepareMove();
    },
    snapshot: async () => {
      if (
        ["connecting", "authenticating", "queued"].includes(tab.state) ||
        connection.prompt
      )
        throw new Error("请完成或取消终端连接后再移动工作区");
      if (!live.current) throw new Error("终端尚未准备好");
      const recovery = connection.pauseRecovery();
      try {
        return {
          id: tab.id,
          terminal: await live.current.prepareMove(),
          recovery,
          connection: connection.view ?? initial?.connection,
        };
      } catch (e) {
        connection.resumeRecovery();
        throw e;
      }
    },
    cancelMove: async () => {
      try {
        await live.current?.cancelMove();
      } finally {
        connection.resumeRecovery();
      }
    },
    resumeRecovery: connection.resumeRecovery,
    reconnect: connection.reconnect,
    disconnect,
    presentChallenge: () => {
      if (connection.prompt && connection.connectionId)
        void invoke("present_connection_challenge", {
          connectionId: connection.connectionId,
          challengeId: connection.prompt.challenge.id,
        }).catch(() => {});
    },
  }));
  const prompt = connection.prompt;
  return (
    <div
      className="terminal-session"
      hidden={!active}
      inert={!active || !interactive}
      role="tabpanel"
      id={`terminal-panel-${tab.id}`}
      aria-labelledby={`terminal-tab-${tab.id}`}
    >
      {chrome?.mode === "history" ? (
        <div className="terminal-context-line" role="status">
          <span>
            正在查看历史{chrome.changed ? " · 实时终端有新输出" : " · 只读"}
          </span>
          <button
            className="quiet"
            onClick={() => live.current?.toggleHistory()}
          >
            <ArrowLeft size={14} />
            返回实时终端
          </button>
        </div>
      ) : connection.recovery.automatic ? (
        <div className="terminal-context-line" role="status">
          <span>
            {connection.recovery.phase === "waiting"
              ? `连接已断开，${connection.seconds} 秒后重试 · 第 ${connection.recovery.attempts + 1} 次`
              : prompt
                ? "重连需要身份验证"
                : `正在重新连接 · 第 ${connection.recovery.attempts} 次`}
            {error ? ` · ${error.message}` : ""}
          </span>
          <button className="quiet" onClick={disconnect}>
            停止重连
          </button>
        </div>
      ) : error ? (
        <div className="terminal-context-line" role="alert">
          <span>
            {error.message}
            <DiagnosticErrorInfo error={error} />
          </span>
          <button
            className="icon-button"
            onClick={() => setError(null)}
            aria-label="关闭错误提示"
          >
            <X size={14} />
          </button>
        </div>
      ) : null}
      <div className="terminal-host">
        <TerminalPane
          source={{
            serverName: workspace.server.name,
            host: workspace.server.host,
            port: workspace.server.port,
            username: workspace.server.username,
          }}
          workspaceId={workspace.id}
          terminalId={tab.id}
          ownerEpoch={workspace.owner.ownerEpoch}
          focusPolicy="managed"
          controllerRef={live}
          initial={restored}
          request={admitted}
          active={active}
          interactive={interactive}
          integrated
          onChromeState={setChrome}
          onReady={() => {
            release.current?.();
            onReady(tab.id);
          }}
          onSession={setSessionId}
          onCwdChange={setCwd}
          onState={(s) => {
            setState(s);
            if (s === "已断开" || s === "连接失败") release.current?.();
            onConnectionStatus?.(
              workspace.id,
              connection.request?.attempt ?? tab.attempt,
              s,
            );
          }}
          onError={(failure) => {
            setError(failure);
            release.current?.();
            onConnectionStatus?.(
              workspace.id,
              connection.request?.attempt ?? tab.attempt,
              "error",
              failure,
            );
          }}
          onConnectionId={connection.setId}
          onConnectionEvent={(event, attempt) => {
            connection.receive(event, attempt);
            if (event.type === "ready") {
              release.current?.();
              onConfigurationChanged?.();
            }
            if (event.type === "closed") release.current?.();
          }}
        />
      </div>
      <footer className="workspace-status session-statusbar">
        {chrome?.recordError && (
          <span
            className="terminal-record-warning"
            role="status"
            title={chrome.recordLabel}
          >
            历史保存异常
          </span>
        )}
        {chrome?.cleared && chrome.mode === "live" && (
          <button
            className="quiet terminal-cleared-link"
            onClick={() => live.current?.toggleHistory()}
          >
            已清理 · 查看此前记录
          </button>
        )}
        <SessionConnectionDetails
          active={active}
          status={status}
          progress={connection.progress || status}
          route={
            connection.view
              ? `本机 → ${connection.view.hops.map((h) => h.server.name).join(" → ")}`
              : ""
          }
          preferences={preferences}
          onChange={onPreferences}
        />
        <ResourceStatusBar
          networkInterface={resources.networkInterface}
          active={active}
          onOpen={onMetric}
        />
      </footer>
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
    </div>
  );
}
