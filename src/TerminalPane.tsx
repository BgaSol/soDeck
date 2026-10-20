import { useTerminalClipboard } from "./terminalClipboard/useTerminalClipboard";
import {
  byteLength,
  finalPasteSize,
  INPUT_LIMIT,
} from "./terminalClipboard/policy";
import type {
  ClipboardActions,
  ClipboardViewState,
  PasteTarget,
  TerminalSourceLabel,
} from "./terminalClipboard/types";
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useImperativeHandle,
} from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { Terminal } from "@xterm/xterm";
import { SerializeAddon } from "@xterm/addon-serialize";
import type { Ref } from "react";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { errorOf } from "./types";
import type { AppError, ConnectionRequest, TerminalEvent } from "./types";
import type { HistoryBinding, HistoryPosition } from "./terminalHistoryApi";

export interface TerminalSnapshot {
  source?: TerminalSourceLabel;
  sessionId: string | null;
  content: string;
  cols: number;
  rows: number;
  viewportY: number;
  cwd?: { path: string; home: string | null; sequence: number };
  historyBinding?: HistoryBinding;
  historyMode?: "live" | "history";
  historyPosition?: HistoryPosition;
}
export interface TerminalCwd {
  sessionId: string;
  path: string;
  home: string | null;
  sequence: number;
}
export interface TerminalController {
  clipboard: ClipboardActions;
  prepareMove: () => Promise<TerminalSnapshot>;
  cancelMove: () => Promise<void>;
  clearDisplay: () => Promise<void>;
}
export interface TerminalPaneProps {
  source?: TerminalSourceLabel;
  onClipboardState?: (state: ClipboardViewState) => void;
  controllerRef?: Ref<TerminalController>;
  initial?: TerminalSnapshot;
  onReady?: () => void;
  request?: ConnectionRequest;
  container?: boolean;
  active?: boolean;
  interactive?: boolean;
  workspaceId?: string;
  onAlternate?: (alternate: boolean) => void;
  onCleared?: () => void;
  onSession: (id: string | null) => void;
  onError: (error: AppError) => void;
  onState: (state: string) => void;
  onCwdChange?: (cwd: TerminalCwd | null) => void;
  onConnectionId?: (id: string | null) => void;
  onConnectionEvent?: (event: TerminalEvent) => void;
}
export function LiveTerminalPane({
  source,
  onClipboardState,
  request,
  controllerRef,
  initial,
  onReady,
  container = false,
  active = true,
  interactive = true,
  workspaceId,
  onAlternate,
  onCleared,
  onSession,
  onError,
  onState,
  onCwdChange,
  onConnectionId,
  onConnectionEvent,
}: TerminalPaneProps) {
  const element = useRef<HTMLDivElement>(null),
    terminal = useRef<Terminal | null>(null),
    fit = useRef<FitAddon | null>(null),
    session = useRef<string | null>(null);
  const callbacks = useRef({
    onSession,
    onError,
    onState,
    onCwdChange,
    onConnectionId,
    onConnectionEvent,
  });
  callbacks.current = {
    onSession,
    onError,
    onState,
    onCwdChange,
    onConnectionId,
    onConnectionEvent,
  };
  const sourceProp = useRef(source);
  sourceProp.current = source;
  const connectionSource = useRef(initial?.source ?? source);
  const inputEpoch = useRef(0);
  const clipboard = useTerminalClipboard(onClipboardState);
  const pasteCapture = useRef<string[] | null>(null);
  const latestCwd = useRef(initial?.cwd);
  const isActive = useRef(active);
  isActive.current = active;
  const isInteractive = useRef(interactive);
  isInteractive.current = interactive;
  const alternateCallback = useRef(onAlternate);
  alternateCallback.current = onAlternate;
  const clearedCallback = useRef(onCleared);
  clearedCallback.current = onCleared;
  const serializer = useRef<SerializeAddon | null>(null);
  const transferring = useRef(false);
  const frozen = useRef<{
    token: string;
    resolve: () => void;
    reject: (error: unknown) => void;
  } | null>(null);
  const readyCallback = useRef(onReady);
  readyCallback.current = onReady;
  useImperativeHandle(
    controllerRef,
    () => ({
      clipboard: {
        copySelection: async () => {
          await clipboard.controller.current?.copySelection();
        },
        requestPaste: async (request) => {
          await clipboard.controller.current?.requestPaste(request);
        },
        selectAllBuffer: () => clipboard.controller.current?.selectAllBuffer(),
        sendControl: async (key) => {
          await clipboard.controller.current?.sendControl(key);
        },
      },
      prepareMove: async () => {
        if (transferring.current) throw new Error("会话正在移动");
        transferring.current = true;
        clipboard.invalidate();
        const id = session.current;
        try {
          if (id) {
            const token = crypto.randomUUID();
            await new Promise<void>((resolve, reject) => {
              const timer = setTimeout(() => {
                frozen.current = null;
                reject(new Error("无法暂停终端输出，请重试"));
              }, 8000);
              frozen.current = {
                token,
                resolve: () => {
                  clearTimeout(timer);
                  resolve();
                },
                reject: (e) => {
                  clearTimeout(timer);
                  reject(e);
                },
              };
              void invoke("terminal_freeze", { sessionId: id, token }).catch(
                (e) => frozen.current?.reject(e),
              );
            });
          }
          const term = terminal.current;
          return {
            sessionId: session.current,
            source: connectionSource.current,
            content: serializer.current?.serialize() || "",
            cols: term?.cols || 100,
            rows: term?.rows || 30,
            viewportY: term?.buffer.active.viewportY || 0,
            cwd: latestCwd.current,
          };
        } catch (e) {
          transferring.current = false;
          clipboard.controller.current?.refresh();
          if (id)
            await invoke("terminal_resume", { sessionId: id }).catch(() => {});
          throw e;
        }
      },
      cancelMove: async () => {
        transferring.current = false;
        clipboard.controller.current?.refresh();
        if (session.current)
          await invoke("terminal_resume", { sessionId: session.current }).catch(
            () => {},
          );
      },
      clearDisplay: async () => {
        if (transferring.current || !session.current)
          throw new Error("终端当前不能清理显示");
        if (terminal.current?.buffer.active.type === "alternate")
          throw new Error("请先退出屏幕交互程序");
        await invoke("terminal_clear_display", { sessionId: session.current });
      },
    }),
    [],
  );
  useEffect(() => {
    if (!connectionSource.current && source) {
      connectionSource.current = { ...source };
      clipboard.controller.current?.refresh();
    } else if (
      container &&
      source?.containerName &&
      connectionSource.current &&
      !connectionSource.current.containerName
    ) {
      connectionSource.current = {
        ...connectionSource.current,
        containerName: source.containerName,
      };
      clipboard.controller.current?.refresh();
    }
  }, [source, container]);
  const hadRequest = useRef(false);
  const [ended, setEnded] = useState(false);
  useEffect(() => {
    const motionPreference = matchMedia("(prefers-reduced-motion: reduce)");
    const term = new Terminal({
      fontFamily:
        '"Cascadia Code", "SFMono-Regular", Consolas, "Liberation Mono", monospace',
      fontSize: 14,
      lineHeight: 1.2,
      scrollback: 5000,
      cursorBlink: !motionPreference.matches,
      allowProposedApi: false,
      theme: {
        background: "#14171e",
        foreground: "#dce3ef",
        cursor: "#b7c9e5",
        selectionBackground: "#344761",
        black: "#15202e",
        red: "#f08c91",
        green: "#8bccad",
        yellow: "#e3c981",
        blue: "#91b4ff",
        magenta: "#c6a5ea",
        cyan: "#87c9d6",
        white: "#d9e3f0",
        brightBlack: "#8c95a3",
        brightRed: "#ff929b",
        brightGreen: "#a3d6ad",
        brightYellow: "#efd28f",
        brightBlue: "#a4c2f4",
        brightMagenta: "#d4b5ed",
        brightCyan: "#9cd8de",
        brightWhite: "#f0f2f6",
      },
    });
    const addon = new FitAddon();
    term.loadAddon(addon);
    const serialize = new SerializeAddon();
    term.loadAddon(serialize);
    serializer.current = serialize;
    term.open(element.current!);
    terminal.current = term;
    fit.current = addon;
    const updateMotion = () => {
      term.options.cursorBlink = !motionPreference.matches;
    };
    motionPreference.addEventListener("change", updateMotion);
    const observer = new ResizeObserver(() => {
      if (element.current?.clientWidth && element.current?.clientHeight) {
        addon.fit();
        if (session.current && !transferring.current)
          void invoke("terminal_resize", {
            sessionId: session.current,
            cols: term.cols,
            rows: term.rows,
          }).catch(() => {});
      }
    });
    observer.observe(element.current!);
    const input = term.onData((data) => {
      if (pasteCapture.current) {
        pasteCapture.current.push(data);
        return;
      }
      if (session.current && !transferring.current)
        void invoke("terminal_input", {
          sessionId: session.current,
          data,
        }).catch((error) => callbacks.current.onError(errorOf(error)));
    });
    const target = (): PasteTarget | null => {
      const origin = connectionSource.current;
      return session.current &&
        origin &&
        isActive.current &&
        isInteractive.current &&
        !transferring.current
        ? {
            sessionId: session.current,
            epoch: inputEpoch.current,
            source: { ...origin },
          }
        : null;
    };
    const assertTarget = (expected: PasteTarget) => {
      const current = target();
      if (
        !current ||
        current.sessionId !== expected.sessionId ||
        current.epoch !== expected.epoch
      )
        throw new Error("目标终端已改变");
    };
    const unbindClipboard = clipboard.attach(term, {
      target,
      reason: () =>
        !session.current
          ? "终端尚未连接"
          : transferring.current
            ? "会话正在移动"
            : !connectionSource.current
              ? "正在读取会话来源"
              : "请先返回当前实时终端",
      invalidate: () => {
        inputEpoch.current++;
      },
      canFocus: () =>
        isActive.current && isInteractive.current && !transferring.current,
      paste: async (expected, text) => {
        assertTarget(expected);
        if (finalPasteSize(text, term.modes.bracketedPasteMode) > INPUT_LIMIT)
          throw new Error("输入过大");
        const emissions: string[] = [];
        pasteCapture.current = emissions;
        try {
          term.paste(text);
        } finally {
          pasteCapture.current = null;
        }
        if (emissions.length !== 1 || byteLength(emissions[0]) > INPUT_LIMIT)
          throw new Error("终端粘贴编码异常");
        assertTarget(expected);
        await invoke("terminal_input", {
          sessionId: expected.sessionId,
          data: emissions[0],
        });
      },
      control: async (expected, key) => {
        assertTarget(expected);
        await invoke("terminal_input", {
          sessionId: expected.sessionId,
          data: key === "C" ? "\x03" : "\x16",
        });
      },
    });
    const parsed = term.onWriteParsed(() =>
      alternateCallback.current?.(term.buffer.active.type === "alternate"),
    );
    if (!container && !initial)
      term.writeln("\x1b[38;2;142;160;183m建立连接，开始终端会话。\x1b[0m");
    return () => {
      observer.disconnect();
      motionPreference.removeEventListener("change", updateMotion);
      unbindClipboard();
      input.dispose();
      parsed.dispose();
      term.dispose();
      terminal.current = null;
      fit.current = null;
    };
  }, []);
  useLayoutEffect(() => {
    clipboard.invalidate();
    if (interactive && active) terminal.current?.focus();
    else terminal.current?.blur();
  }, [interactive, active]);
  useEffect(() => {
    if (!active) return;
    const frame = requestAnimationFrame(() => {
      if (element.current?.clientWidth && element.current.clientHeight)
        fit.current?.fit();
    });
    return () => cancelAnimationFrame(frame);
  }, [active]);
  useEffect(() => {
    if (!request && !container && !initial) {
      if (hadRequest.current)
        terminal.current?.writeln(
          "\r\n\x1b[38;2;142;160;183m连接已断开。\x1b[0m",
        );
      hadRequest.current = false;
      return;
    }
    clipboard.invalidate();
    connectionSource.current = initial?.source ?? sourceProp.current;
    hadRequest.current = true;
    latestCwd.current = initial?.cwd;
    let cancelled = false,
      closed = false,
      ready = !!initial,
      opened: string | null = null;
    setEnded(false);
    if (!initial) {
      callbacks.current.onSession(null);
      callbacks.current.onCwdChange?.(null);
      callbacks.current.onState("连接中");
    }
    if (!initial)
      terminal.current?.writeln(
        "\r\n\x1b[38;2;142;160;183m正在建立安全连接…\x1b[0m",
      );
    function markReady() {
      if (!ready || !opened || closed || cancelled) return;
      session.current = opened;
      clipboard.controller.current?.refresh();
      callbacks.current.onSession(opened);
      if (opened && latestCwd.current)
        callbacks.current.onCwdChange?.({
          sessionId: opened,
          ...latestCwd.current,
        });
      if (opened) callbacks.current.onState("已连接");
      if (isActive.current) {
        fit.current?.fit();
        if (isInteractive.current) terminal.current?.focus();
      }
      if (opened)
        void invoke("terminal_resize", {
          sessionId: opened,
          cols: terminal.current?.cols || 100,
          rows: terminal.current?.rows || 30,
        }).catch(() => {});
      readyCallback.current?.();
    }
    const output = new Channel<TerminalEvent>();
    // Preserve stream order across a local clear and a migration barrier. Ordinary writes
    // still use xterm's batching; only barrier operations hold later writes briefly.
    const actions: Array<(done: () => void) => boolean> = [];
    let waiting = false;
    function drain() {
      while (!waiting && actions.length && !cancelled) {
        const action = actions.shift()!;
        waiting = true;
        if (
          !action(() => {
            waiting = false;
            drain();
          })
        )
          waiting = false;
      }
    }
    function write(data: string | Uint8Array) {
      actions.push(() => {
        terminal.current?.write(data);
        return false;
      });
      drain();
    }
    function barrier(action: () => void) {
      actions.push((done) => {
        const term = terminal.current;
        if (!term) {
          if (!cancelled) action();
          return false;
        }
        term.write("", () => {
          if (!cancelled) action();
          done();
        });
        return true;
      });
      drain();
    }
    output.onmessage = (event) => {
      if (cancelled) return;
      callbacks.current.onConnectionEvent?.(event);
      if (event.type === "ready") {
        const connected = event.connection.target;
        const previous = connectionSource.current;
        connectionSource.current = {
          serverName: connected.name,
          host: connected.host ?? previous?.host ?? "",
          port: connected.port ?? previous?.port ?? 22,
          username: connected.username ?? previous?.username ?? "",
          containerName: previous?.containerName,
        };
        ready = true;
        markReady();
      } else if (event.type === "frozen") {
        barrier(() => {
          if (frozen.current?.token === event.token) {
            frozen.current.resolve();
            frozen.current = null;
          }
        });
      } else if (event.type === "data") {
        const bytes = Uint8Array.from(atob(event.data), (c) => c.charCodeAt(0));
        write(bytes);
      } else if (event.type === "displayCleared") {
        barrier(() => {
          if (terminal.current?.buffer.active.type === "alternate") {
            callbacks.current.onError({
              kind: "terminal",
              message: "已进入屏幕交互程序，本次清理显示已跳过",
            });
            return;
          }
          terminal.current?.clear();
          clearedCallback.current?.();
        });
      } else if (event.type === "cwd") {
        if (!latestCwd.current || event.sequence > latestCwd.current.sequence) {
          latestCwd.current = {
            path: event.path,
            home: event.home,
            sequence: event.sequence,
          };
          if (opened)
            callbacks.current.onCwdChange?.({
              sessionId: opened,
              ...latestCwd.current,
            });
        }
      } else if (event.type === "closed") {
        closed = true;
        session.current = null;
        clipboard.invalidate();
        callbacks.current.onSession(null);
        callbacks.current.onCwdChange?.(null);
        callbacks.current.onConnectionId?.(null);
        callbacks.current.onState(
          event.reason === "failed" ? "连接失败" : "已断开",
        );
        if (event.error) callbacks.current.onError(event.error);
        setEnded(true);
        write(`\r\n\x1b[38;2;142;160;183m${event.message}\x1b[0m\r\n`);
      }
    };
    const restore = initial
      ? new Promise<void>((resolve) => {
          const term = terminal.current;
          if (!term) return resolve();
          term.resize(initial.cols, initial.rows);
          term.write(initial.content, () => {
            term.scrollToLine(initial.viewportY);
            resolve();
          });
        })
      : Promise.resolve();
    const task = initial
      ? restore.then(async () => {
          if (initial.sessionId) {
            await invoke("terminal_attach", {
              sessionId: initial.sessionId,
              output,
            });
            return initial.sessionId;
          }
          return null;
        })
      : container
        ? invoke<string>("connect_container", { output })
        : invoke<string>("open_connection", {
            serverId: request!.serverId,
            workspaceId: workspaceId ?? null,
            events: output,
          });
    void task
      .then((id) => {
        opened = id;
        if (closed && !cancelled && initial) {
          readyCallback.current?.();
          return;
        }
        if (cancelled || closed) {
          if (id && !transferring.current)
            void invoke("close_connection", { connectionId: id }).catch(
              () => {},
            );
          return;
        }
        callbacks.current.onConnectionId?.(id);
        markReady();
        if (initial && !id) readyCallback.current?.();
      })
      .catch((error) => {
        if (!cancelled) {
          callbacks.current.onConnectionId?.(null);
          callbacks.current.onState("连接失败");
          callbacks.current.onError(errorOf(error));
          setEnded(true);
        }
      });
    return () => {
      cancelled = true;
      session.current = null;
      clipboard.invalidate();
      callbacks.current.onSession(null);
      callbacks.current.onConnectionId?.(null);
      if (opened && !transferring.current)
        void invoke("close_connection", { connectionId: opened }).catch(
          () => {},
        );
    };
  }, [request, container, initial]);
  return (
    <>
      <div
        className={`terminal-surface${ended ? " terminal-ended" : ""}`}
        ref={element}
        inert={!interactive || !active}
      />
      {clipboard.ui}
    </>
  );
}
export { TerminalWorkspace as TerminalPane } from "./TerminalWorkspace";
