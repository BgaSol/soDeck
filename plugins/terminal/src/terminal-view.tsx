import {openConnectionDiagnostics,type DiagnosticRef,isConnectionAuthenticationError} from '@sodeck/sdk';
import {settingsPageId,defaultFontSize,terminalPreferences} from './settings';
import {validateSettings,type SettingsSnapshot} from '@sodeck/sdk/settings';
import {changeSetting} from './ui-observation';
import {Button,Input} from '@sodeck/ui';
import { Notice,StatusBar,StatusDot,Checkbox,foundation } from '@sodeck/ui';
import { onThemeError,retryTheme } from '@sodeck/sdk';
import { useCallback, useEffect, useRef, useState } from "react";

import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { terminalGeometry } from './terminal-geometry';
import { observeState, presentation } from '@sodeck/sdk';
import { SerializeAddon } from "@xterm/addon-serialize";
import { core, connectionCommandTimeout, context, CoreError, decodeBytes, encodeBytes, errorMessage, onCoreNotification, showMenu, subscribeTheme, currentTheme, type ThemeResult } from "@sodeck/sdk";
import { TerminalClipboard } from "./clipboard";
const statusLabel=(s:string)=>({running:"已连接",active:"已连接",connected:"已连接",connecting:"正在连接",waitingAuth:"等待认证",canceled:"已取消连接",ended:"会话已结束",failed:"连接失败",disconnected:"连接已断开"}[s]??s);
import { snapshotChunks } from "./snapshot";

declare const __TERMINAL_VERSION__: string;
interface Binding { sessionId: string; ptyId: string; generation: number; status: string; error?: string; operationId?: string; diagnosticRef?:DiagnosticRef; integrationAvailable?:boolean }
interface ReadResult { frames: { sequence: number; bytes: string }[]; lastSequence: number; state: string; reason?: string }
interface Snapshot { schema: 1; screen: string; cursor: number; inputSeq: number; fontSize: number; viewportY: number; cols: number; rows: number; inputPaused?: boolean; selection?: { start: { x: number; y: number }; end: { x: number; y: number } } }
const defaultFont = defaultFontSize;


export function TerminalView({theme}:{theme:ThemeResult}) {
  const [themeError,setThemeError]=useState<string>();
  useEffect(()=>onThemeError(setThemeError),[]);
  const host = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitRef = useRef<(() => void) | null>(null);
  const bindingRef = useRef<Binding | null>(null);
  const sendRef = useRef<((text: string) => void) | null>(null);
  const settingsRef=useRef<SettingsSnapshot|undefined>(undefined);
  const settingBusy=useRef(false);
  const [settingsBusy,setSettingsBusy]=useState(false);
  const [fontSize, setFontSize] = useState<number>(defaultFont);
  const [shellIntegration,setShellIntegration]=useState(true);
  const [integrationAvailable,setIntegrationAvailable]=useState<boolean>();
  const [settingsAvailable,setSettingsAvailable]=useState(false);
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
    // A replaced PTY can report its closure while the reconnect is completing.
    // The new binding reports its own errors; do not retain the old PTY error.
    if (reload > 0) setError("");
    let alive = true, frozen = false, committed = false, composing = false, mounted = false, inputBlocked = false, readRunning = false, presentationVisible = true;
    let cursor = 0, inputSeq = 0, queuedBytes = 0;
    let pollTimer: ReturnType<typeof setTimeout> | undefined;
    let settingsFeed:Awaited<ReturnType<typeof observeState>>|undefined;
    let presentationFeed:Awaited<ReturnType<typeof observeState>>|undefined;
    let readFlight = Promise.resolve();
    let inputFlight = Promise.resolve();
    let binding: Binding | null = null;
    const term = new Terminal({ fontFamily: foundation['font-mono'], fontSize: defaultFont, lineHeight: foundation['terminal-line-height'], cursorBlink: true, scrollback: 5000, allowProposedApi: true,
      theme: (currentTheme()??theme).terminal });
    const fit = new FitAddon(), serialize = new SerializeAddon();
    term.loadAddon(fit); term.loadAddon(serialize); term.open(host.current);
    const directoryOsc=term.parser.registerOscHandler(633,data=>data.startsWith('SoDeck;1;'));
    terminalRef.current = term;
    const handle = () => ({ ptyId: binding!.ptyId, generation: binding!.generation });
    const geometry = terminalGeometry({element:host.current,term,fit,
      enabled:()=>alive&&!frozen&&presentationVisible,
      binding:()=>mounted&&binding?.ptyId?binding:null,
      resize:size=>core.call('pty.resize',size),error:e=>{if(alive)setError(errorMessage(e));}});
    const fitTerminal = geometry.schedule;
    fitRef.current = fitTerminal;
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
      if (!alive || frozen || !binding?.ptyId) return;
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
      if (notification.type === 'view.presentation') { presentationVisible=(notification.payload as {visible?:boolean})?.visible!==false;if(presentationVisible)fitTerminal(); }
      if (notification.type === "view.moveCommitted") { clipboard.invalidate(); committed = true; frozen = true; setMoving(true); }
      if (notification.type === "view.focus") term.focus();
      if (["view.resourcesChanged", "workbench.changed"].includes(notification.type) && mounted && !frozen) {
        void core.call<{session:Binding}>("view.initialize").then(data=>{
          if(!alive)return;
          if(data.session.ptyId!==binding?.ptyId)setReload(value=>value+1);
          else { const changed=binding?.status!==data.session.status; binding=data.session; bindingRef.current=binding; setStatus(binding.status);setIntegrationAvailable(binding.integrationAvailable); if(binding.error||changed&&!binding.ptyId)setError(binding.error??""); }
        }).catch(e=>{if(alive)setError(errorMessage(e));});
      }
    });
    const initialize = async () => {
      try {
        const ctx = context();
        const settings = await core.call<{ fontSize: number; shellIntegration?:boolean; session: Binding }>("view.initialize");
        if (!alive) return;
        setShellIntegration(settings.shellIntegration!==false);
        const state = (reload === 0 ? ctx.snapshot : undefined) as (Snapshot & { recovered?: boolean; acknowledged?: number; lastInputSeq?: number }) | undefined;
        const snapshot = state?.recovered ? undefined : state;
        if (state?.recovered) { cursor = state.acknowledged ?? 0; inputSeq = state.lastInputSeq ?? 0; setError("终端显示已重建，旧屏幕无法恢复。SSH 会话仍在继续，接下来显示新的输出。"); }
        inputBlocked = snapshot?.inputPaused ?? false; setInputPaused(inputBlocked);
        const size = Math.min(24, Math.max(10, settings.fontSize ?? defaultFont));
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
        clipboard.invalidate(); bindingRef.current = binding; setStatus(binding.status);setIntegrationAvailable(binding.integrationAvailable); if(binding.error)setError(binding.error); fitTerminal();
        const ready = await core.call<{ ready: boolean; lastInputSeq?: number; acknowledged?: number; screenGap?: boolean }>("view.ready", { cursor });
        if (!alive) return;
        // A recovered renderer starts from the host's authoritative cursors;
        // output already consumed by the lost surface cannot be reconstructed.
        try{settingsFeed=await observeState({call:core.call,onNotification:onCoreNotification},{contract:'settings.page.v1',target:{pageId:settingsPageId}},value=>{const error=validateSettings('x-results','storage.preferences.read',value);if(error)throw Error(error);return value as SettingsSnapshot;},snapshot=>{
          if(!alive)return;setSettingsAvailable(snapshot.syncState!=='unavailable');if(snapshot.syncState!=='current')setError('配置已保存，终端设置同步待恢复');if(snapshot.syncState==='unavailable')return;settingsRef.current=snapshot;const next=terminalPreferences(snapshot);setShellIntegration(next.shellIntegration);setFontSize(next.fontSize);
          if(term.options.fontSize!==next.fontSize){term.options.fontSize=next.fontSize;fitTerminal();}
        },{onError:e=>{if(alive)setError(errorMessage(e));}});
        }catch(e){if(alive){setSettingsAvailable(false);setError('终端设置暂不可用：'+errorMessage(e));}}
        if(!alive){await settingsFeed?.dispose();return;}
        if (ready.lastInputSeq !== undefined) inputSeq = ready.lastInputSeq;
        if (!snapshot && ready.acknowledged !== undefined) cursor = ready.acknowledged;
        if (ready.screenGap) setError("终端显示已重建，旧屏幕无法恢复。SSH 会话仍在继续，接下来显示新的输出。");
        mounted = true; fitTerminal(); if(document.hasFocus()&&document.visibilityState==="visible")term.focus(); readFlight = readOnce();
        const feed=await observeState({call:(method,params)=>core.call(method,params),onNotification:onCoreNotification},
          {contract:'view.presentation.v1',target:{scope:{workspaceId:ctx.workspaceId!,workspaceGeneration:ctx.workspaceGeneration!},viewId:ctx.viewId!}},presentation,
          value=>{presentationVisible=value.visible;if(value.visible)fitTerminal();},{onError:e=>{if(alive)setError(errorMessage(e));}});
        if(!alive){await feed.dispose();return;}presentationFeed=feed;
        const latest=await core.call<{session:Binding}>("view.initialize");
        if(alive) {
          if(latest.session.ptyId!==binding.ptyId)setReload(value=>value+1);
          else {binding=latest.session;bindingRef.current=binding;setStatus(binding.status);setIntegrationAvailable(binding.integrationAvailable);if(binding.error)setError(binding.error);}
        }
      } catch (e) { if (alive) { setError(errorMessage(e)); setStatus("failed"); } }
    };
    void initialize();
    const element = host.current;
    return () => {
      clipboard.invalidate();clipboardRef.current=null;selectionChange.dispose();window.removeEventListener("blur",blur);document.removeEventListener("focusout",blur);element.removeEventListener("paste",pasteEvent,true);element.removeEventListener("contextmenu",contextMenu,true);
      alive = false; off(); geometry.dispose(); onData.dispose(); if (pollTimer) clearTimeout(pollTimer);
      void presentationFeed?.dispose();void settingsFeed?.dispose();settingsRef.current=undefined;
      element.removeEventListener("compositionstart", compositionStart); element.removeEventListener("compositionend", compositionEnd);
      directoryOsc.dispose();term.dispose(); terminalRef.current = null; fitRef.current = null; sendRef.current = null; bindingRef.current = null;
    };
  }, [copy, paste, reload]);
  useEffect(()=>subscribeTheme(value=>{if(terminalRef.current)terminalRef.current.options.theme=value.terminal;}),[]);
  const saveSetting=async(command:string,args:Record<string,unknown>)=>{
    if(!settingsAvailable||!settingsRef.current){setError('设置暂不可用，请在设置窗口重新同步');return;}if(settingBusy.current)return;settingBusy.current=true;setSettingsBusy(true);
    try{await changeSetting(command,args,settingsRef.current);}catch(e){setError(errorMessage(e));}finally{settingBusy.current=false;setSettingsBusy(false);}
  };
  const changeFont=(size:number,reset=false)=>saveSetting(reset?'settings.reset':'settings.set',{fontSize:Math.max(10,Math.min(24,size))});
  const reconnect = async () => {
    if(status==="connecting")return;
    clipboardRef.current?.invalidate();setStatus("connecting"); setError("");
    try {
      await core.call<Binding>("commands.execute", { commandId: "terminal.reconnect", viewId: context().viewId, operationId: crypto.randomUUID() }, await connectionCommandTimeout(core,context().serverId,context().workspaceId));
      setReload(value => value + 1);
    } catch (e) { setError(errorMessage(e)); setStatus(e instanceof CoreError && e.code==="CANCELED"?"canceled":e instanceof CoreError && isConnectionAuthenticationError(e.code)?"waitingAuth":"failed"); }
  };
  menuRef.current=async(x,y)=>{
    const term=terminalRef.current;if(!term||moving)return;
    const active=["active","running","connected"].includes(status)&&!inputPaused;
    try {
      const selected=await showMenu([{id:"copy",label:"复制",enabled:term.hasSelection(),shortcut:"Ctrl+C"},{id:"paste",label:"粘贴",enabled:active,shortcut:"Ctrl+V"},{id:"smaller",label:"缩小字号",enabled:fontSize>10,separator:true},{id:"larger",label:"增大字号",enabled:fontSize<24},{id:"reset",label:"恢复默认字号",enabled:fontSize!==defaultFont},...(!active?[{id:"reconnect",label:"重新连接",enabled:!moving&&status!=="connecting"&&status!=="waitingAuth",separator:true}]:[])],{x,y});
      if(!terminalRef.current||term!==terminalRef.current)return;
      if(document.hasFocus())term.focus();
      if(selected==="copy")await copy();else if(selected==="paste"){await core.call("views.activate",{viewId:context().viewId});term.focus();await paste();}else if(selected==="smaller")await changeFont(fontSize-1);else if(selected==="larger")await changeFont(fontSize+1);else if(selected==="reset")await changeFont(defaultFont,true);else if(selected==="reconnect")await reconnect();
    }catch(e){setError(errorMessage(e));}
  };
  return <section className="terminal-view" aria-label={(context().serverName ?? "服务器")+" 终端"}>
    {themeError&&<Notice kind="error" action={<Button onClick={()=>void retryTheme()}>重试同步</Button>}>{themeError}</Notice>}
    {error && <Notice kind="error" action={error===bindingRef.current?.error&&bindingRef.current?.diagnosticRef&&<Button onClick={()=>void openConnectionDiagnostics(core,bindingRef.current!.diagnosticRef!).catch(e=>setError(errorMessage(e)))}>查看连接详情</Button>} onDismiss={()=>setError("")}>{error}</Notice>}
    <div className="terminal-content"><div className="terminal-grid" ref={host}/></div>
    {(["connecting","waitingAuth"].includes(status))&&<Notice className="terminal-connection"><span>{statusLabel(status)}…</span>{status==="waitingAuth"&&<Button onClick={()=>{const operationId=bindingRef.current?.operationId;if(operationId)void connectionCommandTimeout(core,context().serverId,context().workspaceId).then(timeout=>core.call("commands.retry",{operationId},timeout)).catch(e=>setError(errorMessage(e)));}}>继续认证</Button>}<Button onClick={()=>{const operationId=bindingRef.current?.operationId;if(operationId)void core.call("workbench.operation.cancel",{operationId}).catch(e=>setError(errorMessage(e)));}}>取消连接</Button></Notice>}
    {(inputPaused||!["active","running","connected","connecting","waitingAuth"].includes(status))&&<Notice className="terminal-connection"><span>{inputPaused?"输入已暂停，请先核对远端执行结果":statusLabel(status)+" · 本次屏幕已保留"}</span><Button onClick={()=>void reconnect()}>重新连接</Button></Notice>}
    <StatusBar className="terminal-status"><span><StatusDot tone={["running","active","connected"].includes(status)?"success":["connecting","waitingAuth"].includes(status)?"warning":["failed","disconnected"].includes(status)?"error":"neutral"}/>{moving?"正在移动":statusLabel(status)}</span><Checkbox label="Shell 目录集成" title={integrationAvailable===false?"当前远端未确认支持 Linux/Bash 目录集成，本会话使用普通 SSH Shell。":"用于受支持 Linux/Bash 的目录自动跟随；仅对新建或明确重连生效。"} aria-label="Shell 目录集成" checked={shellIntegration&&integrationAvailable!==false} disabled={settingsBusy||!settingsAvailable||integrationAvailable===false} onChange={e=>void saveSetting('settings.shellIntegration',{enabled:e.target.checked})}/>{integrationAvailable===false&&<span>本会话未启用</span>}<span>字号 {fontSize}</span></StatusBar>
  </section>;
}
