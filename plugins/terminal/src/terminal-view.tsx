import { useCallback, useEffect, useRef, useState } from "react";

import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SerializeAddon } from "@xterm/addon-serialize";
import { core, context, CoreError, decodeBytes, encodeBytes, errorMessage, onCoreNotification, showMenu, type ThemeResult } from "@sodeck/sdk";
import { TerminalClipboard } from "./clipboard";
const statusLabel=(s:string)=>({running:"已连接",active:"已连接",connected:"已连接",connecting:"正在连接",ended:"会话已结束",failed:"连接失败",disconnected:"连接已断开"}[s]??s);
import { snapshotChunks } from "./snapshot";

declare const __TERMINAL_VERSION__: string;
interface Binding { sessionId: string; ptyId: string; generation: number; status: string }
interface ReadResult { frames: { sequence: number; bytes: string }[]; lastSequence: number; state: string; reason?: string }
interface Snapshot { schema: 1; screen: string; cursor: number; inputSeq: number; fontSize: number; viewportY: number; cols: number; rows: number; inputPaused?: boolean; selection?: { start: { x: number; y: number }; end: { x: number; y: number } } }
const defaultFont = 14;


export function TerminalView({theme}:{theme:ThemeResult}) {
  const host = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitRef = useRef<(() => void) | null>(null);
  const bindingRef = useRef<Binding | null>(null);
  const sendRef = useRef<((text: string) => void) | null>(null);
  const [fontSize, setFontSize] = useState(defaultFont);
  const [status, setStatus] = useState("connecting");
  const [error, setError] = useState("");
  const clipboardRef=useRef<TerminalClipboard|null>(null);
  const menuRef=useRef<(x:number,y:number)=>Promise<void>>(async()=>{});
  const [reload, setReload] = useState(0);
  const [moving, setMoving] = useState(false);
  const [inputPaused, setInputPaused] = useState(false);
  const copy=useCallback(()=>clipboardRef.current?.copy(),[]);
  const paste=useCallback(()=>clipboardRef.current?.paste(),[]);
  useEffect(() => {
    if (!host.current) return;
    let alive = true, frozen = false, committed = false, composing = false, mounted = false, inputBlocked = false, readRunning = false;
    let cursor = 0, inputSeq = 0, queuedBytes = 0;
    let pollTimer: ReturnType<typeof setTimeout> | undefined;
    let readFlight = Promise.resolve();
    let inputFlight = Promise.resolve();
    let binding: Binding | null = null;
    const term = new Terminal({ fontFamily: '"Cascadia Mono", Consolas, "Noto Sans Mono", monospace', fontSize: defaultFont, lineHeight: 1.12, cursorBlink: true, scrollback: 5000, allowProposedApi: true,
      theme: theme.terminal });
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
          if (alive) { setInputPaused(inputBlocked); setError(`${errorMessage(e)}${inputBlocked ? " 部分输入可能已发送，请核对远端状态。输入已暂停，可重新连接；不会重发此输入。" : " 剩余输入未发送，请等待后再操作。"}`); }
        }
        finally { queuedBytes -= bytes.length; }
      });
    };
    sendRef.current = send;
    const onData = term.onData(send);
    let selectionVersion=0;
    const selectionChange=term.onSelectionChange(()=>{selectionVersion++;});
    const clipboard=new TerminalClipboard(()=>({key:context().viewId+":"+context().mountGeneration+":"+binding?.ptyId+":"+binding?.generation,active:document.hasFocus()&&Boolean(host.current?.contains(document.activeElement)),canCopy:alive&&!frozen,canPaste:alive&&!frozen&&!committed&&!inputBlocked&&mounted&&Boolean(binding&&["active","running","connected"].includes(binding.status)),selection:term.getSelection(),selectionVersion}),
      async()=>{const result=await core.call<string|{text:string}>("clipboard.readText");return typeof result==="string"?result:result.text;},
      text=>core.call("clipboard.writeText",{text}),text=>term.paste(text),()=>term.clearSelection(),e=>{if(alive)setError(errorMessage(e));});
    clipboardRef.current=clipboard;
    const blur=()=>clipboard.invalidate();window.addEventListener("blur",blur);document.addEventListener("focusout",blur);
    term.attachCustomKeyEventHandler(event=>{
      if(event.isComposing||composing||event.type!=="keydown")return true;
      const key=event.key.toLowerCase();
      const copyKey=event.ctrlKey&&!event.altKey&&!event.metaKey&&key==="c"&&(event.shiftKey||term.hasSelection());
      const pasteKey=!event.altKey&&!event.metaKey&&((event.ctrlKey&&key==="v")||(!event.ctrlKey&&event.shiftKey&&key==="insert"));
      const menuKey=(event.shiftKey&&!event.ctrlKey&&key==="f10")||key==="contextmenu";
      if(copyKey||pasteKey||menuKey){event.preventDefault();event.stopPropagation();if(!event.repeat){if(copyKey)void clipboard.copy();if(pasteKey)void clipboard.paste();if(menuKey){const r=host.current!.getBoundingClientRect();void menuRef.current(r.left+12,r.top+16);}}return false;}
      return true;
    });
    const pasteEvent=(event:ClipboardEvent)=>{event.preventDefault();event.stopImmediatePropagation();if(!composing)void clipboard.paste();};
    const contextMenu=(event:MouseEvent)=>{if(term.modes.mouseTrackingMode!=="none"&&!event.shiftKey)return;event.preventDefault();event.stopImmediatePropagation();void menuRef.current(event.clientX,event.clientY);};
    host.current.addEventListener("paste",pasteEvent,true);host.current.addEventListener("contextmenu",contextMenu,true);
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
        if (binding.status !== output.state) { void core.call("plugin.invoke", {command:"sessions.status",args:{sessionId:binding.sessionId,ptyId:binding.ptyId,status:output.state}}).catch(e=>{if(alive)setError(errorMessage(e));}); }
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
        clipboard.invalidate(); frozen = true; setMoving(true); if (pollTimer) clearTimeout(pollTimer);
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
      if (notification.type === "view.moveCommitted") { clipboard.invalidate(); committed = true; frozen = true; setMoving(true); }
      if (notification.type === "view.focus") term.focus();
      if (notification.type === "view.resourcesChanged" && mounted && !frozen) {
        void core.call<{session:Binding}>("view.initialize").then(data=>{
          if(alive && data.session.ptyId!==binding?.ptyId)setReload(value=>value+1);
        }).catch(e=>{if(alive)setError(errorMessage(e));});
      }
    });
    const initialize = async () => {
      try {
        const ctx = context();
        const settings = await core.call<{ fontSize: number; session: Binding }>("view.initialize");
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
        binding = settings.session;
        if (!alive) return;
        clipboard.invalidate(); bindingRef.current = binding; setStatus(binding.status); fitTerminal();
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
      clipboard.invalidate();clipboardRef.current=null;selectionChange.dispose();window.removeEventListener("blur",blur);document.removeEventListener("focusout",blur);element.removeEventListener("paste",pasteEvent,true);element.removeEventListener("contextmenu",contextMenu,true);
      alive = false; off(); observer.disconnect(); onData.dispose(); if (pollTimer) clearTimeout(pollTimer);
      element.removeEventListener("compositionstart", compositionStart); element.removeEventListener("compositionend", compositionEnd);
      term.dispose(); terminalRef.current = null; fitRef.current = null; sendRef.current = null; bindingRef.current = null;
    };
  }, [copy, paste, reload, theme]);
  const changeFont = async (size: number, reset = false) => {
    try {
      const settings = await core.call<{ fontSize: number }>("plugin.invoke", { command: reset ? "settings.reset" : "settings.set", args: { fontSize: Math.max(10, Math.min(24, size)) } });
      const next = settings.fontSize; setFontSize(next); if (terminalRef.current) terminalRef.current.options.fontSize = next;
      requestAnimationFrame(() => fitRef.current?.());
    } catch (e) { setError(errorMessage(e)); }
  };
  const reconnect = async () => {
    clipboardRef.current?.invalidate();setStatus("connecting"); setError("");
    try {
      await core.call<Binding>("commands.execute", { commandId: "terminal.reconnect", viewId: context().viewId, operationId: crypto.randomUUID() }, 130_000);
      setReload(value => value + 1);
    } catch (e) { setError(errorMessage(e)); setStatus("failed"); }
  };
  menuRef.current=async(x,y)=>{
    const term=terminalRef.current;if(!term||moving)return;
    const active=["active","running","connected"].includes(status)&&!inputPaused;
    try {
      const selected=await showMenu([{id:"copy",label:"复制",enabled:term.hasSelection(),shortcut:"Ctrl+C"},{id:"paste",label:"粘贴",enabled:active,shortcut:"Ctrl+V"},{id:"smaller",label:"缩小字号",enabled:fontSize>10,separator:true},{id:"larger",label:"增大字号",enabled:fontSize<24},{id:"reset",label:"恢复默认字号",enabled:fontSize!==14},...(!active?[{id:"reconnect",label:"重新连接",enabled:!moving,separator:true}]:[])],{x,y});
      if(!terminalRef.current||term!==terminalRef.current)return;
      if(document.hasFocus())term.focus();
      if(selected==="copy")await copy();else if(selected==="paste"){await core.call("views.activate",{viewId:context().viewId});term.focus();await paste();}else if(selected==="smaller")await changeFont(fontSize-1);else if(selected==="larger")await changeFont(fontSize+1);else if(selected==="reset")await changeFont(defaultFont,true);else if(selected==="reconnect")await reconnect();
    }catch(e){setError(errorMessage(e));}
  };
  return <section className="terminal-view" aria-label={(context().serverName ?? "服务器")+" 终端"}>
    {error && <div className="notice error" role="alert"><span>{error}</span><button className="dismiss" aria-label="收起提示" onClick={()=>setError("")}>×</button></div>}
    <div className="terminal-content" ref={host}/>
    {(inputPaused||!["active","running","connected","connecting"].includes(status))&&<div className="ended"><span>{inputPaused?"输入已暂停，请先核对远端执行结果":statusLabel(status)+" · 本次屏幕已保留"}</span><button onClick={()=>void reconnect()}>重新连接</button></div>}
    <footer className="terminal-status"><span><i className={"status-dot "+status}/>{moving?"正在移动":statusLabel(status)}</span><span>字号 {fontSize}</span></footer>
  </section>;
}
