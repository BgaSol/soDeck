import { useSystemSettings, browserPlatform } from "./useSystemSettings";
import {
  defaultPreferences,
  emptyClipboardState,
} from "./terminalClipboard/types";
import { shortcutLabels } from "./terminalClipboard/policy";
import {
  lazy,
  Suspense,
  useEffect,
  useCallback,
  useImperativeHandle,
  useRef,
  useState,
} from "react";
import {
  Copy,
  ClipboardPaste,
  ArrowLeft,
  Check,
  CircleAlert,
  Eraser,
  History,
  Search,
} from "lucide-react";
import {
  LiveTerminalPane,
  type TerminalController,
  type TerminalPaneProps,
} from "./TerminalPane";
const TerminalHistoryView = lazy(() =>
  import("./TerminalHistoryView").then(({ TerminalHistoryView }) => ({
    default: TerminalHistoryView,
  })),
);
import {
  terminalHistoryApi,
  type HistoryBinding,
  type HistoryPosition,
  type HistorySession,
} from "./terminalHistoryApi";
import { errorOf, type TerminalEvent } from "./types";
import "./terminal-history.css";

export interface TerminalChromeState {
  mode: "live" | "history";
  changed: boolean;
  cleared: boolean;
  recordError: string;
  recordLabel: string;
  clipboard: Pick<
    typeof emptyClipboardState,
    "hasSelection" | "canPaste" | "busy" | "unavailableReason"
  >;
  alternate: boolean;
  liveId: string | null;
}

export interface TerminalWorkspaceController extends TerminalController {
  toggleHistory: () => void;
  openHistorySearch: () => void;
}

export function TerminalWorkspace(
  props: TerminalPaneProps & {
    integrated?: boolean;
    onChromeState?: (state: TerminalChromeState) => void;
  },
) {
  const live = useRef<TerminalController>(null);
  const history = useRef<HTMLDivElement>(null);
  const moving = useRef(false);
  const [clipboardState, setClipboardState] = useState(emptyClipboardState);
  const settings = useSystemSettings();
  const labels = shortcutLabels(
    settings?.platform ?? browserPlatform,
    settings?.terminalClipboard ?? defaultPreferences,
  );
  const [binding, setBinding] = useState<HistoryBinding | undefined>(
    props.initial?.historyBinding,
  );
  const [mode, setMode] = useState<"live" | "history">(
    props.initial?.historyMode ?? "live",
  );
  const modeRef = useRef(mode);
  modeRef.current = mode;
  const position = useRef<HistoryPosition | undefined>(
    props.initial?.historyPosition,
  );
  const [status, setStatus] = useState<HistorySession | null>(null);
  const [liveId, setLiveId] = useState(props.initial?.sessionId ?? null);
  const [alternate, setAlternate] = useState(false);
  const [changed, setChanged] = useState(false);
  const [cleared, setCleared] = useState(false);
  const [statusError, setStatusError] = useState("");
  const restoreLiveFocus = useRef(false);
  const toggleHistory = useCallback(() => {
    restoreLiveFocus.current = modeRef.current === "history";
    setMode((current) => (current === "history" ? "live" : "history"));
    setChanged(false);
  }, []);
  useEffect(() => {
    if (!restoreLiveFocus.current || mode !== "live") return;
    restoreLiveFocus.current = false;
    if ((props.active ?? true) && (props.interactive ?? true))
      live.current?.focus();
  }, [mode, props.active, props.interactive]);
  const openHistorySearch = useCallback(() => {
    void terminalHistoryApi.open().catch((e) => props.onError(errorOf(e)));
  }, [props.onError]);
  useImperativeHandle(
    props.controllerRef,
    () => ({
      focus: () => {
        if (
          !(props.active ?? true) ||
          !(props.interactive ?? true) ||
          moving.current ||
          document.querySelector("dialog[open]")
        )
          return;
        if (mode === "history") history.current?.focus({ preventScroll: true });
        else live.current?.focus();
      },
      clipboard: {
        copySelection: async () => {
          await live.current?.clipboard.copySelection();
        },
        requestPaste: async (request) => {
          await live.current?.clipboard.requestPaste(request);
        },
        selectAllBuffer: () => live.current?.clipboard.selectAllBuffer(),
        sendControl: async (key) => {
          await live.current?.clipboard.sendControl(key);
        },
      },
      prepareMove: async () => {
        if (!live.current) throw new Error("终端尚未准备好");
        moving.current = true;
        let snapshot;
        try {
          snapshot = await live.current.prepareMove();
        } catch (e) {
          moving.current = false;
          throw e;
        }
        return {
          ...snapshot,
          historyBinding: binding,
          historyMode: mode,
          historyPosition: position.current,
        };
      },
      cancelMove: async () => {
        moving.current = false;
        await live.current?.cancelMove();
      },
      clearDisplay: async () => live.current?.clearDisplay(),
      toggleHistory,
      openHistorySearch,
    }),
    [
      binding,
      mode,
      props.active,
      props.interactive,
      toggleHistory,
      openHistorySearch,
    ],
  );
  useEffect(() => {
    if (!binding) return;
    let cancelled = false,
      pending = false;
    const check = async () => {
      if (pending) return;
      pending = true;
      try {
        const result = await terminalHistoryApi.status(binding.sessionId);
        if (!cancelled) {
          setStatus(result);
          setStatusError("");
        }
      } catch (e) {
        if (!cancelled) setStatusError(errorOf(e).message);
      } finally {
        pending = false;
      }
    };
    void check();
    const timer = setInterval(() => void check(), 2000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [binding]);
  function receive(event: TerminalEvent, attempt: number) {
    if (event.type === "history") {
      setBinding({
        sessionId: event.sessionId,
        sourceId: event.sourceId,
        workspaceId: event.workspaceId,
      });
      setStatus(null);
    }
    if (event.type === "data" && modeRef.current === "history")
      setChanged(true);
    props.onConnectionEvent?.(event, attempt);
  }
  async function clear() {
    try {
      await live.current?.clearDisplay();
    } catch (e) {
      props.onError(errorOf(e));
    }
  }
  const recordError = status?.error || statusError;
  const recordLabel = recordError
    ? `历史记录异常 · ${recordError}`
    : status
      ? status.state === "recording"
        ? "正在保存历史"
        : "历史已保存"
      : "";
  useEffect(() => {
    props.onChromeState?.({
      mode,
      changed,
      cleared,
      recordError,
      recordLabel,
      clipboard: {
        hasSelection: clipboardState.hasSelection,
        canPaste: clipboardState.canPaste,
        busy: clipboardState.busy,
        unavailableReason: clipboardState.unavailableReason,
      },
      alternate,
      liveId,
    });
  }, [
    props.onChromeState,
    mode,
    changed,
    cleared,
    recordError,
    recordLabel,
    clipboardState.hasSelection,
    clipboardState.canPaste,
    clipboardState.busy,
    clipboardState.unavailableReason,
    alternate,
    liveId,
  ]);
  return (
    <div className="terminal-workspace">
      {!props.integrated && (
        <div className="terminal-history-toolbar">
          <div className="terminal-tools" role="group" aria-label="终端工具">
            <button
              className="terminal-tool history-toggle"
              aria-pressed={mode === "history"}
              aria-label={mode === "history" ? "返回实时终端" : "历史"}
              title={mode === "history" ? "返回实时终端" : "查看终端历史"}
              onClick={toggleHistory}
            >
              {mode === "history" ? (
                <ArrowLeft size={15} aria-hidden="true" />
              ) : (
                <History size={15} aria-hidden="true" />
              )}
              <span>{mode === "history" ? "实时终端" : "历史"}</span>
            </button>
            <button
              className="terminal-tool"
              aria-label="全局历史搜索"
              title="全局历史搜索"
              onClick={openHistorySearch}
            >
              <Search size={15} aria-hidden="true" />
              <span className="terminal-tool-label">全局历史搜索</span>
            </button>
            {mode === "live" && (
              <>
                <button
                  className="terminal-tool"
                  aria-label="复制终端选区"
                  title={"复制选区 · " + labels.copy}
                  disabled={!clipboardState.hasSelection}
                  onClick={() => void live.current?.clipboard.copySelection()}
                >
                  <Copy size={15} />
                  <span className="terminal-tool-label">复制</span>
                </button>
                <button
                  className="terminal-tool"
                  aria-label="粘贴到终端"
                  title={
                    clipboardState.unavailableReason ?? "粘贴 · " + labels.paste
                  }
                  disabled={!clipboardState.canPaste || clipboardState.busy}
                  onClick={() => void live.current?.clipboard.requestPaste()}
                >
                  <ClipboardPaste size={15} />
                  <span className="terminal-tool-label">粘贴</span>
                </button>
              </>
            )}
            <span className="terminal-tools-divider" aria-hidden="true" />
            <button
              className="terminal-tool"
              aria-label="清理显示"
              disabled={!liveId || alternate || mode === "history"}
              title={
                alternate
                  ? "屏幕交互程序运行期间由程序处理清屏"
                  : "清理显示，保留当前输入行和全部历史"
              }
              onClick={() => void clear()}
            >
              <Eraser size={15} aria-hidden="true" />
              <span className="terminal-tool-label">清理显示</span>
            </button>
          </div>
          <div className="terminal-history-feedback">
            {cleared && mode === "live" && (
              <button
                className="terminal-tool history-cleared"
                title="已清理显示，查看此前记录"
                onClick={() => setMode("history")}
              >
                已清理 · 查看此前记录
              </button>
            )}
            {changed && mode === "history" && (
              <span
                className="history-new-output"
                role="status"
                title="实时终端有新输出"
              >
                实时终端有新输出
              </span>
            )}
            {recordLabel && (
              <span
                className={`history-record-state${recordError ? " history-warning" : ""}`}
                role="status"
                title={recordLabel}
              >
                {recordError ? (
                  <CircleAlert size={13} aria-hidden="true" />
                ) : status?.state === "recording" ? (
                  <span className="history-record-dot" aria-hidden="true" />
                ) : (
                  <Check size={13} aria-hidden="true" />
                )}
                <span>{recordLabel}</span>
              </span>
            )}
          </div>
        </div>
      )}
      <div className="terminal-workspace-body">
        <div
          className={`terminal-live-layer${mode === "history" ? " history-covered" : ""}`}
          aria-hidden={mode === "history"}
        >
          <LiveTerminalPane
            {...props}
            controllerRef={live}
            interactive={
              mode === "live" &&
              (props.active ?? true) &&
              (props.interactive ?? true)
            }
            onClipboardState={setClipboardState}
            onAlternate={setAlternate}
            onCleared={() => setCleared(true)}
            onSession={(id) => {
              setLiveId(id);
              props.onSession(id);
            }}
            onConnectionEvent={receive}
          />
        </div>
        {mode === "history" && (
          <div
            className="terminal-history-layer"
            ref={history}
            tabIndex={-1}
            aria-label="终端历史显示"
          >
            <Suspense fallback={<div role="status">正在打开历史…</div>}>
              <TerminalHistoryView
                binding={binding}
                initial={position.current}
                onPosition={(next) => {
                  position.current = next;
                }}
              />
            </Suspense>
          </div>
        )}
      </div>
    </div>
  );
}
