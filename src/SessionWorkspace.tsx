import {
  useCallback,
  useEffect,
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
import { FingerprintDialog, LoginDialog } from "./ConnectionDialogs";
import { api } from "./api";
import { addressOf, automaticCredentials } from "./types";
import type { AppError, ConnectionRequest, Credentials, Server } from "./types";
import type { WorkspaceTab } from "./workspaces";

export interface WorkspaceSnapshot {
  tab: WorkspaceTab;
  terminal: TerminalSnapshot;
  files?: FilePanelSnapshot;
  docker?: DockerPanelSnapshot;
  resourceWidth: number;
  fileRatio: number;
}
export interface WorkspaceController {
  prepareMove: () => Promise<WorkspaceSnapshot>;
  cancelMove: () => Promise<void>;
}
export function SessionWorkspace({
  tab,
  active,
  controllerRef,
  initial,
  onReady,
  latestServer,
  onChange,
  onServerSaved,
}: {
  tab: WorkspaceTab;
  controllerRef?: Ref<WorkspaceController>;
  initial?: WorkspaceSnapshot;
  onReady?: () => void;
  active: boolean;
  latestServer?: Server;
  onChange: (
    id: string,
    patch: Partial<Pick<WorkspaceTab, "server" | "state">>,
  ) => void;
  onServerSaved: (server: Server) => void;
}) {
  const [request, setRequest] = useState<ConnectionRequest | undefined>(() => {
    if (initial) return undefined;
    const credentials = automaticCredentials(tab.server);
    return credentials
      ? { server: tab.server, credentials, attempt: 0 }
      : undefined;
  });
  const [login, setLogin] = useState<Server | null>(() =>
    initial || automaticCredentials(tab.server) ? null : tab.server,
  );
  const [sessionId, setSessionId] = useState<string | null>(
      initial?.terminal.sessionId || null,
    ),
    [error, setError] = useState<AppError | null>(null);
  const [terminalCwd, setTerminalCwd] = useState<TerminalCwd | null>(() =>
    initial?.terminal.sessionId && initial.terminal.cwd
      ? { sessionId: initial.terminal.sessionId, ...initial.terminal.cwd }
      : null,
  );
  const [resourceWidth, setResourceWidth] = useState(
      initial?.resourceWidth || 320,
    ),
    [fileRatio, setFileRatio] = useState(initial?.fileRatio || 49);
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
        if (tab.state === "连接中" || login || error?.kind === "hostKey")
          throw new Error("请完成连接或关闭认证提示后再移动会话");
        if (!terminalController.current) throw new Error("终端尚未准备好");
        const terminal = await terminalController.current.prepareMove();
        return {
          tab: structuredClone(tab),
          terminal,
          files: filesSnapshot.current,
          docker: dockerSnapshot.current,
          resourceWidth,
          fileRatio,
        };
      },
      cancelMove: async () => {
        await terminalController.current?.cancelMove();
      },
    }),
    [tab, login, error, resourceWidth, fileRatio],
  );
  const resourcePane = useRef<HTMLElement>(null);
  const page = useRef<HTMLElement>(null);
  const widthDrag = useRef<{ x: number; width: number } | null>(null);
  const splitDrag = useRef<{ y: number; ratio: number; height: number } | null>(
    null,
  );
  const [paneMetrics, setPaneMetrics] = useState({
    width: 320,
    ratio: 49,
    maxWidth: 520,
    minRatio: 28,
    maxRatio: 72,
  });
  useEffect(() => {
    const measure = () => {
      if (!resourcePane.current?.clientWidth || !page.current) return;
      const height = resourcePane.current.clientHeight - 10;
      if (height <= 0) return;
      const fileHeight =
        resourcePane.current
          .querySelector<HTMLElement>(".file-panel")
          ?.getBoundingClientRect().height || 0;
      const next = {
        width: resourcePane.current.clientWidth,
        ratio: Math.round((fileHeight / height) * 100),
        maxWidth: Math.min(
          520,
          page.current.clientWidth -
            (window.innerWidth <= 1000 ? 250 : 280) -
            10,
        ),
        minRatio: Math.round(Math.max(28, (190 / height) * 100)),
        maxRatio: Math.round(Math.min(72, ((height - 150) / height) * 100)),
      };
      setPaneMetrics((current) =>
        Object.keys(next).every(
          (key) =>
            next[key as keyof typeof next] ===
            current[key as keyof typeof next],
        )
          ? current
          : next,
      );
    };
    const observer = new ResizeObserver(measure);
    observer.observe(page.current!);
    observer.observe(resourcePane.current!);
    observer.observe(resourcePane.current!.querySelector(".file-panel")!);
    return () => observer.disconnect();
  }, []);
  function begin(server: Server, credentials: Credentials) {
    setRestoredTerminal(undefined);
    setLogin(null);
    setError(null);
    onChange(tab.id, { server: structuredClone(server), state: "连接中" });
    setRequest({
      server: structuredClone(server),
      credentials,
      attempt: Date.now(),
    });
  }
  function reconnect() {
    if (!latestServer) {
      setError({
        kind: "notFound",
        message: "服务器配置已删除，请重新添加服务器后建立连接。",
      });
      return;
    }
    if (!latestServer.username) {
      setError({
        kind: "validation",
        message: "请先在右侧编辑服务器并填写 SSH 用户名。",
      });
      return;
    }
    const credentials = automaticCredentials(latestServer);
    if (credentials && error?.kind !== "authentication")
      begin(latestServer, credentials);
    else setLogin(structuredClone(latestServer));
  }
  function disconnect() {
    setRestoredTerminal(undefined);
    setRequest(undefined);
    setSessionId(null);
    setError(null);
    onChange(tab.id, { state: "已断开" });
  }
  function changeWidth(width: number) {
    const terminalMinimum = window.innerWidth <= 1000 ? 250 : 280;
    const maximum = Math.min(
      520,
      (page.current?.clientWidth || 850) - terminalMinimum - 10,
    );
    setResourceWidth(Math.min(maximum, Math.max(260, width)));
  }
  function splitMetrics() {
    const height = Math.max(
      1,
      (resourcePane.current?.clientHeight || 600) - 10,
    );
    const fileHeight =
      resourcePane.current?.querySelector<HTMLElement>(
        ".file-panel",
      )?.clientHeight;
    return {
      height,
      ratio: fileHeight ? ((fileHeight + 2) / height) * 100 : fileRatio,
    };
  }
  function changeSplit(ratio: number) {
    const { height } = splitMetrics();
    const minimum = Math.max(28, (190 / height) * 100);
    const maximum = Math.min(72, ((height - 150) / height) * 100);
    setFileRatio(Math.min(maximum, Math.max(minimum, ratio)));
  }
  return (
    <section
      className="session-page"
      ref={page}
      role="tabpanel"
      id={`panel-${tab.id}`}
      aria-labelledby={`tab-${tab.id}`}
      hidden={!active}
      style={
        { "--resource-width": `${resourceWidth}px` } as React.CSSProperties
      }
    >
      <aside
        className="resource-panel"
        ref={resourcePane}
        style={{
          gridTemplateRows: `minmax(190px,${fileRatio}fr) 10px minmax(150px,${100 - fileRatio}fr)`,
        }}
      >
        <FilePanel
          sessionId={sessionId}
          serverName={tab.server.name}
          active={active}
          initialState={initial?.files}
          onStateChange={captureFiles}
          terminalCwd={terminalCwd ?? undefined}
        />
        <div
          className="horizontal-resizer"
          onPointerDown={(event) => {
            if (event.button !== 0) return;
            event.currentTarget.setPointerCapture(event.pointerId);
            splitDrag.current = { y: event.clientY, ...splitMetrics() };
          }}
          onPointerMove={(event) => {
            const drag = splitDrag.current;
            if (drag)
              changeSplit(
                drag.ratio + ((event.clientY - drag.y) / drag.height) * 100,
              );
          }}
          onPointerUp={(event) => {
            splitDrag.current = null;
            if (event.currentTarget.hasPointerCapture(event.pointerId))
              event.currentTarget.releasePointerCapture(event.pointerId);
          }}
          onLostPointerCapture={() => {
            splitDrag.current = null;
          }}
          onKeyDown={(event) => {
            if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
            event.preventDefault();
            changeSplit(
              splitMetrics().ratio + (event.key === "ArrowDown" ? 2 : -2),
            );
          }}
          role="separator"
          tabIndex={0}
          aria-valuemin={paneMetrics.minRatio}
          aria-valuemax={paneMetrics.maxRatio}
          aria-valuenow={paneMetrics.ratio}
          aria-orientation="horizontal"
          aria-label="调整文件和服务区域"
        />
        <DockerPanel
          sessionId={sessionId}
          serverName={tab.server.name}
          active={active}
          initialState={initial?.docker}
          onStateChange={captureDocker}
        />
      </aside>
      <div
        className="vertical-resizer"
        onPointerDown={(event) => {
          if (event.button !== 0) return;
          event.currentTarget.setPointerCapture(event.pointerId);
          widthDrag.current = {
            x: event.clientX,
            width: resourcePane.current?.clientWidth || resourceWidth,
          };
        }}
        onPointerMove={(event) => {
          if (widthDrag.current)
            changeWidth(
              widthDrag.current.width + event.clientX - widthDrag.current.x,
            );
        }}
        onPointerUp={(event) => {
          widthDrag.current = null;
          if (event.currentTarget.hasPointerCapture(event.pointerId))
            event.currentTarget.releasePointerCapture(event.pointerId);
        }}
        onLostPointerCapture={() => {
          widthDrag.current = null;
        }}
        onKeyDown={(event) => {
          if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
          event.preventDefault();
          changeWidth(
            (resourcePane.current?.clientWidth || resourceWidth) +
              (event.key === "ArrowRight" ? 10 : -10),
          );
        }}
        role="separator"
        tabIndex={0}
        aria-valuemin={260}
        aria-valuemax={paneMetrics.maxWidth}
        aria-valuenow={paneMetrics.width}
        aria-orientation="vertical"
        aria-label="调整文件和服务区域宽度"
      />
      <main className="terminal-panel">
        <header className="terminal-toolbar">
          <TerminalSquare size={16} />
          <strong>{tab.server.name}</strong>
          <span
            className={`connection-state ${sessionId ? "running" : tab.state === "连接失败" ? "failed" : ""}`}
          >
            {tab.state}
          </span>
          {sessionId ? (
            <button className="quiet" onClick={disconnect}>
              <Unplug size={14} />
              断开
            </button>
          ) : (
            <button
              className="primary"
              disabled={tab.state === "连接中"}
              onClick={reconnect}
            >
              <Plug size={14} />
              {tab.state === "未连接" ? "连接" : "重新连接"}
            </button>
          )}
        </header>
        <div className="terminal-address">
          {tab.server.username}@{addressOf(tab.server)}
        </div>
        {error && error.kind !== "hostKey" && (
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
        {error?.kind === "hostKey" && !active && (
          <span className="sr-only">等待核对服务器指纹</span>
        )}
        <div className="terminal-host">
          <TerminalPane
            controllerRef={terminalController}
            initial={restoredTerminal}
            onReady={onReady}
            request={request}
            active={active}
            onSession={setSessionId}
            onCwdChange={setTerminalCwd}
            onState={(state) => onChange(tab.id, { state })}
            onError={setError}
          />
        </div>
        <footer className="workspace-status">
          <span>{sessionId ? "SSH 会话已建立" : "SSH"}</span>
          <span>UTF-8</span>
        </footer>
      </main>
      {active && login && (
        <LoginDialog
          server={login}
          onSubmit={async (credentials) => {
            if (login.authType === "password") {
              const saved = await api.savePassword(
                login.id,
                credentials.password,
              );
              onServerSaved(saved);
              begin(saved, credentials);
            } else begin(login, credentials);
          }}
          onClose={() => setLogin(null)}
        />
      )}
      {active && error?.kind === "hostKey" && (
        <FingerprintDialog
          error={error}
          onClose={() => {
            setError(null);
            setRequest(undefined);
            onChange(tab.id, { state: "未连接" });
          }}
          onTrust={() => {
            if (request) {
              setRequest({
                ...request,
                trustFingerprint: error.fingerprint,
                attempt: Date.now(),
              });
              setError(null);
            }
          }}
        />
      )}
    </section>
  );
}
