import { useEffect, useRef, useState, useImperativeHandle } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { Terminal } from "@xterm/xterm";
import { SerializeAddon } from "@xterm/addon-serialize";
import type { Ref } from "react";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { errorOf } from "./types";
import type { AppError, ConnectionRequest, TerminalEvent } from "./types";

export interface TerminalSnapshot {
  sessionId: string | null;
  content: string;
  cols: number;
  rows: number;
  viewportY: number;
}
export interface TerminalController {
  prepareMove: () => Promise<TerminalSnapshot>;
  cancelMove: () => Promise<void>;
}
interface Props {
  controllerRef?: Ref<TerminalController>;
  initial?: TerminalSnapshot;
  onReady?: () => void;
  request?: ConnectionRequest;
  container?: boolean;
  active?: boolean;
  onSession: (id: string | null) => void;
  onError: (error: AppError) => void;
  onState: (state: string) => void;
}
export function TerminalPane({
  request,
  controllerRef,
  initial,
  onReady,
  container = false,
  active = true,
  onSession,
  onError,
  onState,
}: Props) {
  const element = useRef<HTMLDivElement>(null),
    terminal = useRef<Terminal | null>(null),
    fit = useRef<FitAddon | null>(null),
    session = useRef<string | null>(null);
  const callbacks = useRef({ onSession, onError, onState });
  callbacks.current = { onSession, onError, onState };
  const isActive = useRef(active);
  isActive.current = active;
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
      prepareMove: async () => {
        if (transferring.current) throw new Error("会话正在移动");
        transferring.current = true;
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
            content: serializer.current?.serialize() || "",
            cols: term?.cols || 100,
            rows: term?.rows || 30,
            viewportY: term?.buffer.active.viewportY || 0,
          };
        } catch (e) {
          transferring.current = false;
          if (id)
            await invoke("terminal_resume", { sessionId: id }).catch(() => {});
          throw e;
        }
      },
      cancelMove: async () => {
        transferring.current = false;
        if (session.current)
          await invoke("terminal_resume", { sessionId: session.current }).catch(
            () => {},
          );
      },
    }),
    [],
  );
  const hadRequest = useRef(false);
  const [ended, setEnded] = useState(false);
  useEffect(() => {
    const motionPreference = matchMedia("(prefers-reduced-motion: reduce)");
    const term = new Terminal({
      fontFamily:
        '"Cascadia Code", "SFMono-Regular", Consolas, "Liberation Mono", monospace',
      fontSize: 14,
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
      if (session.current && !transferring.current)
        void invoke("terminal_input", {
          sessionId: session.current,
          data,
        }).catch((error) => callbacks.current.onError(errorOf(error)));
    });
    if (!container && !initial)
      term.writeln("\x1b[38;2;142;160;183m建立连接，开始终端会话。\x1b[0m");
    return () => {
      observer.disconnect();
      motionPreference.removeEventListener("change", updateMotion);
      input.dispose();
      term.dispose();
      terminal.current = null;
      fit.current = null;
    };
  }, []);
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
    hadRequest.current = true;
    let cancelled = false,
      closed = false,
      opened: string | null = null;
    setEnded(false);
    if (!initial) {
      callbacks.current.onSession(null);
      callbacks.current.onState("连接中");
    }
    if (!initial)
      terminal.current?.writeln(
        "\r\n\x1b[38;2;142;160;183m正在建立安全连接…\x1b[0m",
      );
    const output = new Channel<TerminalEvent>();
    output.onmessage = (event) => {
      if (cancelled) return;
      if (event.type === "frozen") {
        terminal.current?.write("", () => {
          if (frozen.current?.token === event.token) {
            frozen.current.resolve();
            frozen.current = null;
          }
        });
      } else if (event.type === "data") {
        const bytes = Uint8Array.from(atob(event.data), (c) => c.charCodeAt(0));
        terminal.current?.write(bytes);
      } else {
        closed = true;
        session.current = null;
        callbacks.current.onSession(null);
        callbacks.current.onState("已断开");
        setEnded(true);
        terminal.current?.writeln(
          `\r\n\x1b[38;2;142;160;183m${event.message}\x1b[0m`,
        );
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
        : invoke<string>("connect_terminal", {
            serverId: request!.server.id,
            credentials: request!.credentials,
            trustFingerprint: request!.trustFingerprint || null,
            output,
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
            void invoke("terminal_close", { sessionId: id }).catch(() => {});
          return;
        }
        session.current = id;
        callbacks.current.onSession(id);
        if (id) callbacks.current.onState("已连接");
        if (isActive.current) {
          fit.current?.fit();
          terminal.current?.focus();
        }
        if (id)
          void invoke("terminal_resize", {
            sessionId: id,
            cols: terminal.current?.cols || 100,
            rows: terminal.current?.rows || 30,
          }).catch(() => {});
        readyCallback.current?.();
      })
      .catch((error) => {
        if (!cancelled) {
          callbacks.current.onState("连接失败");
          callbacks.current.onError(errorOf(error));
          setEnded(true);
        }
      });
    return () => {
      cancelled = true;
      session.current = null;
      callbacks.current.onSession(null);
      if (opened && !transferring.current)
        void invoke("terminal_close", { sessionId: opened }).catch(() => {});
    };
  }, [request, container, initial]);
  return (
    <div
      className={`terminal-surface${ended ? " terminal-ended" : ""}`}
      ref={element}
    />
  );
}
