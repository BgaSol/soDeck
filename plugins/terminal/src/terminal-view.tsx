import { useCallback, useEffect, useRef, useState } from "react";
import { Copy, ClipboardPaste, Minus, Plus, RotateCcw, MoreHorizontal, ArrowUpRight, PanelBottom, PanelsTopLeft } from "lucide-react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SerializeAddon } from "@xterm/addon-serialize";
import { core, context, CoreError, decodeBytes, encodeBytes, errorMessage, onCoreNotification } from "@sodeck/sdk";
import { statusLabel } from "./session-list";
import { snapshotChunks } from "./snapshot";

declare const __TERMINAL_VERSION__: string;
interface Binding { sessionId: string; ptyId: string; generation: number; status: string }
interface ReadResult { frames: { sequence: number; bytes: string }[]; lastSequence: number; state: string; reason?: string }
interface Snapshot { schema: 1; screen: string; cursor: number; inputSeq: number; fontSize: number; viewportY: number; cols: number; rows: number; inputPaused?: boolean; selection?: { start: { x: number; y: number }; end: { x: number; y: number } } }
const defaultFont = 14;
const hasReset = __TERMINAL_VERSION__ !== "0.1.0";

export function TerminalView() {
  const host = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitRef = useRef<(() => void) | null>(null);
  const bindingRef = useRef<Binding | null>(null);
  const sendRef = useRef<((text: string) => void) | null>(null);
  const [fontSize, setFontSize] = useState(defaultFont);
  const [status, setStatus] = useState("connecting");
  const [error, setError] = useState("");
  const [menu, setMenu] = useState(false);
  const [reload, setReload] = useState(0);
  const [moving, setMoving] = useState(false);
  const [inputPaused, setInputPaused] = useState(false);
  const copy = useCallback(async () => {
    const selection = terminalRef.current?.getSelection();
    if (!selection) return;
    try { await core.call("clipboard.writeText", { text: selection }); } catch (e) { setError(errorMessage(e)); }
  }, []);
  const paste = useCallback(async () => {
    try {
      const result = await core.call<string | { text: string }>("clipboard.readText");
      terminalRef.current?.paste(typeof result === "string" ? result : result.text);
    } catch (e) { setError(errorMessage(e)); }
  }, []);
  useEffect(() => {
    if (!host.current) return;
    let alive = true, frozen = false, committed = false, composing = false, mounted = false, inputBlocked = false, readRunning = false;
    let cursor = 0, inputSeq = 0, queuedBytes = 0;
    let pollTimer: ReturnType<typeof setTimeout> | undefined;
    let readFlight = Promise.resolve();
    let inputFlight = Promise.resolve();
    let binding: Binding | null = null;
    const term = new Terminal({ fontFamily: '"Cascadia Mono", Consolas, "Noto Sans Mono", monospace', fontSize: defaultFont, lineHeight: 1.12, cursorBlink: true, scrollback: 5000, allowProposedApi: true,
      theme: { background: "#15191f", foreground: "#dde3ec", cursor: "#91b9ff", selectionBackground: "#39506e", black: "#252c35", red: "#f08d88", green: "#a1d59a", yellow: "#eac28b", blue: "#83b4ec", magenta: "#c0a3e6", cyan: "#83cfce", white: "#d8dfe9", brightBlack: "#778496", brightWhite: "#ffffff" } });
    const fit = new FitAddon(), serialize = new SerializeAddon();
    term.loadAddon(fit); term.loadAddon(serialize); term.open(host.current);
    terminalRef.current = term;
    const handle = () => ({ ptyId: binding!.ptyId, generation: binding!.generation });
    let lastSize = "";
    const fitTerminal = () => {
      if (!alive || frozen || !host.current || host.current.clientWidth < 30 || host.current.clientHeight < 30) return;
      fit.fit();
      if (!binding || !mounted || term.cols < 2 || term.rows < 1) return;
      const size = `${term.cols}:${term.rows}`;
      if (size === lastSize) return;
      lastSize = size;
      void core.call("pty.resize", { ...handle(), cols: term.cols, rows: term.rows }).catch(e => { lastSize = ""; if (alive) setError(errorMessage(e)); });
    };
    fitRef.current = fitTerminal;
    const observer = new ResizeObserver(fitTerminal); observer.observe(host.current);
    const writeOutput = (bytes: Uint8Array | string) => new Promise<void>(resolve => term.write(bytes, resolve));
    const send = (text: string) => {
      if (!alive || frozen || inputBlocked || !mounted || !binding || !["active", "running", "connected"].includes(binding.status)) return;
      const bytes = new TextEncoder().encode(text);
      if (queuedBytes + bytes.length > 256 * 1024) { setError("输入队列已满，请等待已提交的输入完成后再粘贴。"); return; }
      queuedBytes += bytes.length;
      inputFlight = inputFlight.then(async () => {
        try {
          for (let offset = 0; offset < bytes.length; offset += 65536) {
            if (!alive || committed || inputBlocked || !binding) break;
            const accepted = await core.call<{ inputSeq: number }>("pty.write", { ...handle(), inputSeq: inputSeq + 1, bytes: encodeBytes(bytes.subarray(offset, offset + 65536)) });
            inputSeq = accepted.inputSeq;
          }
        } catch (e) {
          if (!(e instanceof CoreError) || !["BACKPRESSURE", "CAPACITY", "LIMIT_EXCEEDED"].includes(e.code)) inputBlocked = true;
          if (alive) { setInputPaused(inputBlocked); setError(`${errorMessage(e)}${inputBlocked ? " 输入已暂停，核对远端状态后可重新连接；不会重发此输入。" : " 剩余输入未发送，请等待后再操作。"}`); }
        }
        finally { queuedBytes -= bytes.length; }
      });
    };
    sendRef.current = send;
    const onData = term.onData(send);
    term.attachCustomKeyEventHandler(event => {
      if (event.isComposing || composing || event.type !== "keydown") return true;
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "c") { void copy(); return false; }
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "v") { void paste(); return false; }
      return true;
    });
    const compositionStart = () => { composing = true; };
    const compositionEnd = () => { composing = false; };
    host.current.addEventListener("compositionstart", compositionStart);
    host.current.addEventListener("compositionend", compositionEnd);
    const readOnce = async (): Promise<void> => {
      if (readRunning) return readFlight;
      if (!alive || frozen || !binding) return;
      readRunning = true;
      try {
        const output = await core.call<ReadResult>("pty.read", { ...handle(), after: cursor });
        if (!alive || committed) return;
        for (const chunk of output.frames) await writeOutput(decodeBytes(chunk.bytes));
        // lastSequence may include frames beyond this bounded read. Only confirm
        // bytes whose xterm write callback has completed.
        cursor = output.frames.at(-1)?.sequence ?? cursor;
        if (output.frames.length) await core.call("pty.ack", { ...handle(), sequence: cursor });
        binding.status = output.state; setStatus(output.state);
        if (output.reason) setError(({ "shell-exited": "远端 shell 已结束。", "connection-closed": "SSH 连接已结束。", "closed-by-user": "终端已关闭。" } as Record<string, string>)[output.reason] ?? output.reason);
        if (!output.frames.length && !["active", "running", "connected", "connecting"].includes(output.state)) return;
        if (!frozen && alive) pollTimer = setTimeout(() => { readFlight = readOnce(); }, output.frames.length ? 0 : 25);
      } catch (e) { if (alive) { setError(errorMessage(e)); setStatus("disconnected"); if (binding) binding.status = "disconnected"; } }
      finally { readRunning = false; }
    };
    const off = onCoreNotification(notification => {
      const payload = notification.payload as { operationId?: string } | undefined;
      if (notification.type === "view.prepareMove") {
        if (composing) { void core.call("view.moveRejected", { operationId: payload?.operationId, reason: "请完成当前输入法组合后再移动终端。" }).catch(e => setError(errorMessage(e))); return; }
        frozen = true; setMoving(true); if (pollTimer) clearTimeout(pollTimer);
        void Promise.all([readFlight, inputFlight]).then(async () => {
          if (!alive || committed) return;
          const selection = term.getSelectionPosition();
          const snapshot: Snapshot = { schema: 1, screen: serialize.serialize(), cursor, inputSeq, inputPaused: inputBlocked, fontSize: term.options.fontSize ?? defaultFont, viewportY: term.buffer.active.viewportY, cols: term.cols, rows: term.rows, ...(selection ? { selection } : {}) };
          const chunks = snapshotChunks(snapshot);
          for (let index = 0; index < chunks.length; index++) {
            if (!alive || committed) return;
            await core.call("view.snapshotChunk", { operationId: payload?.operationId, index, total: chunks.length, data: chunks[index] });
          }
        }).catch(e => {
          if (!alive) return;
          setError(errorMessage(e));
          // A final-chunk response can be lost after commit. Only the native
          // cancellation notification may reactivate the old input surface.
          void core.call("view.moveRejected", { operationId: payload?.operationId, reason: errorMessage(e) }).catch(() => {});
        });
      }
      if (notification.type === "view.moveCancelled") { frozen = false; setMoving(false); readFlight = readOnce(); fitTerminal(); term.focus(); }
      if (notification.type === "view.moveCommitted") { committed = true; frozen = true; setMoving(true); }
      if (notification.type === "view.focus") term.focus();
    });
    const initialize = async () => {
      try {
        const ctx = context();
        const settings = await core.call<{ fontSize: number }>("plugin.invoke", { command: "settings.get", args: {} });
        if (!alive) return;
        const state = (reload === 0 ? ctx.snapshot : undefined) as (Snapshot & { recovered?: boolean; acknowledged?: number; lastInputSeq?: number }) | undefined;
        const snapshot = state?.recovered ? undefined : state;
        if (state?.recovered) { cursor = state.acknowledged ?? 0; inputSeq = state.lastInputSeq ?? 0; setError("终端显示已重建，旧屏幕无法恢复。SSH 会话仍在继续，接下来显示新的输出。"); }
        inputBlocked = snapshot?.inputPaused ?? false; setInputPaused(inputBlocked);
        const size = Math.min(24, Math.max(10, snapshot?.fontSize ?? settings.fontSize ?? defaultFont));
        term.options.fontSize = size; setFontSize(size);
        if (snapshot) {
          if (snapshot.schema !== 1) throw new Error("终端快照版本不兼容，未恢复此视图。");
          cursor = snapshot.cursor; inputSeq = snapshot.inputSeq;
          if (!Number.isInteger(snapshot.cols) || snapshot.cols < 2 || snapshot.cols > 1000 || !Number.isInteger(snapshot.rows) || snapshot.rows < 1 || snapshot.rows > 1000) throw new Error("终端快照尺寸无效，未恢复此视图。");
          term.resize(snapshot.cols, snapshot.rows);
          await writeOutput(snapshot.screen); term.scrollToLine(snapshot.viewportY);
          if (snapshot.selection) { const { start, end } = snapshot.selection; term.select(start.x, start.y, (end.y - start.y) * term.cols + end.x - start.x); }
        }
        binding = await core.call<Binding>("plugin.invoke", { command: "sessions.get", args: { sessionId: ctx.sessionId, viewId: ctx.viewId } });
        if (!alive) return;
        bindingRef.current = binding; setStatus(binding.status); fitTerminal();
        const ready = await core.call<{ ready: boolean; lastInputSeq?: number; acknowledged?: number; screenGap?: boolean }>("view.ready", { cursor });
        if (!alive) return;
        // A recovered renderer starts from the host's authoritative cursors;
        // output already consumed by the lost surface cannot be reconstructed.
        if (ready.lastInputSeq !== undefined) inputSeq = ready.lastInputSeq;
        if (!snapshot && ready.acknowledged !== undefined) cursor = ready.acknowledged;
        if (ready.screenGap) setError("终端显示已重建，旧屏幕无法恢复。SSH 会话仍在继续，接下来显示新的输出。");
        mounted = true; fitTerminal(); term.focus(); readFlight = readOnce();
      } catch (e) { if (alive) { setError(errorMessage(e)); setStatus("failed"); } }
    };
    void initialize();
    const element = host.current;
    return () => {
      alive = false; off(); observer.disconnect(); onData.dispose(); if (pollTimer) clearTimeout(pollTimer);
      element.removeEventListener("compositionstart", compositionStart); element.removeEventListener("compositionend", compositionEnd);
      term.dispose(); terminalRef.current = null; fitRef.current = null; sendRef.current = null; bindingRef.current = null;
    };
  }, [copy, paste, reload]);
  const changeFont = async (size: number, reset = false) => {
    try {
      const settings = await core.call<{ fontSize: number }>("plugin.invoke", { command: reset ? "settings.reset" : "settings.set", args: { fontSize: Math.max(10, Math.min(24, size)) } });
      const next = settings.fontSize; setFontSize(next); if (terminalRef.current) terminalRef.current.options.fontSize = next;
      requestAnimationFrame(() => fitRef.current?.());
    } catch (e) { setError(errorMessage(e)); }
  };
  const move = async (placement: string) => {
    setMenu(false);
    try { await core.call("workbench.move", { viewId: context().viewId, placement }); } catch (e) { setError(errorMessage(e)); }
  };
  const reconnect = async () => {
    setStatus("connecting"); setError("");
    try {
      await core.call<Binding>("plugin.invoke", { command: "sessions.reconnect", args: { sessionId: bindingRef.current?.sessionId ?? context().sessionId } }, 130_000);
      setReload(value => value + 1);
    } catch (e) { setError(errorMessage(e)); setStatus("failed"); }
  };
  return <section className="terminal-view" aria-label={`${context().serverName ?? "服务器"} 终端`}>
    <header className="terminal-toolbar"><div className="connection-state"><i className={`status-dot ${status}`} />{moving ? "正在移动…" : statusLabel(status)}</div>
      <div className="toolbar-actions"><button className="icon" title="复制选区 (Ctrl+Shift+C)" aria-label="复制选区" onClick={() => void copy()}><Copy size={14} /></button><button className="icon" title="粘贴 (Ctrl+Shift+V)" aria-label="粘贴" onClick={() => void paste()} disabled={moving}><ClipboardPaste size={14} /></button><span className="toolbar-divider" />
        <button className="icon" aria-label="缩小字号" disabled={fontSize <= 10} onClick={() => void changeFont(fontSize - 1)}><Minus size={14} /></button><span className="font-size">{fontSize}</span><button className="icon" aria-label="增大字号" disabled={fontSize >= 24} onClick={() => void changeFont(fontSize + 1)}><Plus size={14} /></button>
        {hasReset && <button className="icon" aria-label="恢复默认字号" title="恢复默认字号" onClick={() => void changeFont(defaultFont, true)}><RotateCcw size={14} /></button>}
        <div className="menu-anchor"><button className="icon" aria-label="终端视图操作" aria-expanded={menu} onClick={() => setMenu(!menu)}><MoreHorizontal size={18} /></button>{menu && <div className="menu" role="menu" onKeyDown={e => { if (e.key === "Escape") setMenu(false); }}><button role="menuitem" onClick={() => void move("center")}><PanelsTopLeft size={14} />移到中央</button><button role="menuitem" onClick={() => void move("bottom")}><PanelBottom size={14} />移到底部</button><button role="menuitem" onClick={() => void move("detached")}><ArrowUpRight size={14} />独立窗口</button></div>}</div>
      </div></header>
    {error && <div className="notice error" role="alert"><span>{error}</span><button className="dismiss" aria-label="收起提示" onClick={() => setError("")}>×</button></div>}
    <div className="terminal-content" ref={host} />
    {(inputPaused || !["active", "running", "connected", "connecting"].includes(status)) && <div className="ended"><span>{inputPaused ? "输入已暂停，请先核对远端执行结果" : `${statusLabel(status)} · 本次屏幕已保留`}</span><button onClick={() => void reconnect()}>重新连接</button></div>}
  </section>;
}
