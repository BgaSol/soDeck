import {Input,Button,BusyIndicator} from '@sodeck/ui';
import { Notice,TreeRow,StatusBar,Toolbar,Text,foundation } from '@sodeck/ui';
import { onThemeError,retryTheme } from '@sodeck/sdk';
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  ChevronRight,
  ChevronDown,
  Folder,
  File,
  Link,
  HardDrive,
  RefreshCw,
  ArrowRight,
  X,
  LocateFixed,
  Link2,
} from "lucide-react";
import {
  core,
  connectionCommandTimeout,
  context,
  onCoreNotification,
  errorMessage,
  showMenu,
  observeState,
  presentation,
} from "@sodeck/sdk";
import {MountCapacity} from "./mount-capacity";
import { parent, pathText, encode, utf8, type RemotePath } from "./remote-path";
import { size } from "./format";
import { clipboardPath } from "./clipboard-path";
import type { Snapshot, Row } from "./state";
const ctx = context();

const nodeId = (id: string) => "node-" + encodeURIComponent(id);
export function FilesView() {
  const [rowHeight,setRowHeight]=useState(parseFloat(foundation['tree-row-height'])*parseFloat(foundation['font-size']));
  const densityProbe=useRef<HTMLSpanElement>(null);
  useEffect(()=>{const el=densityProbe.current;if(!el)return;const measure=()=>setRowHeight(el.getBoundingClientRect().height);const observer=new ResizeObserver(measure);observer.observe(el);measure();return()=>observer.disconnect();},[]);
  const [themeError,setThemeError]=useState<string>();
  useEffect(()=>onThemeError(setThemeError),[]);
  const [snapshot, setSnapshot] = useState<Snapshot>(),
    [error, setError] = useState(""),
    [pathInput, setPathInput] = useState("/"),
    [start, setStart] = useState(0),
    [count, setCount] = useState(80);
  const tree = useRef<HTMLDivElement>(null),
    timer = useRef<ReturnType<typeof setTimeout>>(undefined),
    visible = useRef(true),
    mounted = useRef(false),
    fetching = useRef(false),
    rerun = useRef(false),
    range = useRef({ startUnit: 0, countUnits: 80, overscanUnits:20 }),
    current = useRef<Snapshot>(undefined),
    following = useRef<string>(undefined);
  const editing=useRef(false),navigation=useRef(0);
  const pendingScroll=useRef<{unit:number;center:boolean}|undefined>(undefined);
  useLayoutEffect(()=>{
    const target=pendingScroll.current,element=tree.current;
    if(!target||!element)return;
    pendingScroll.current=undefined;
    element.scrollTop=Math.max(0,target.unit*rowHeight-(target.center?element.clientHeight/2:0));
    range.current.startUnit=element.scrollTop/rowHeight;
  },[snapshot,rowHeight]);
  const invoke = useCallback(
    <T,>(command: string, args: Record<string, unknown> = {}) =>
      core.call<T>("plugin.invoke", {
        command,
        args: { ...args, viewId: ctx.viewId },
      }),
    [],
  );
  const poll = useRef<() => Promise<void>>(async () => {});
  poll.current = async () => {
    if (!mounted.current || !visible.current) return;
    if (fetching.current) {rerun.current=true;return;}
    fetching.current = true;
    clearTimeout(timer.current);
    try {
      const requested = {...range.current};
      const topUnits=(tree.current?.scrollTop??0)/rowHeight;
      const anchor=current.current?.rows.find(r=>r.unitOffset<=topUnits&&r.unitOffset+r.units>topUnits);
      const s = await invoke<Snapshot>('files.snapshot',{...requested,...(anchor?{anchorId:anchor.id,anchorOffset:topUnits-anchor.unitOffset}:{})});
      const end=s.viewportUnit+requested.countUnits+requested.overscanUnits;
      while(s.nextUnit<Math.min(s.totalUnits,end)){
        const next=await invoke<Snapshot>('files.snapshot',{startUnit:s.nextUnit,countUnits:end-s.nextUnit,overscanUnits:0,continuation:true});
        if(next.revision!==s.revision){rerun.current=true;return;}
        if(!next.rows.length||next.nextUnit<=s.nextUnit)break;
        s.rows.push(...next.rows);s.nextUnit=next.nextUnit;
      }
      if (!mounted.current) return;
      if(anchor&&current.current&&s.layoutRevision!==current.current.layoutRevision&&tree.current&&range.current.startUnit===requested.startUnit){
        pendingScroll.current={unit:s.viewportUnit,center:false};
      }
      current.current = s;
      setSnapshot(s);
      setStart(s.startUnit);
      if(s.follow&&s.follow.navigation!==navigation.current){
        navigation.current=s.follow.navigation;
        if(!editing.current){following.current=s.selected;setPathInput(s.path);}
      }
      if (following.current===s.selected && tree.current && s.selectedUnit >= 0) {
        following.current = undefined;
        pendingScroll.current={unit:s.selectedUnit,center:true};
        setPathInput(s.path);
      }
      if(!s.loading)following.current=undefined;
      timer.current=setTimeout(()=>void poll.current(),s.state==='idle'?1000:250);
    } catch (e) {
      if (mounted.current) setError(errorMessage(e));
    } finally {
      fetching.current = false;
      if(rerun.current&&mounted.current&&visible.current){rerun.current=false;clearTimeout(timer.current);timer.current=setTimeout(()=>void poll.current(),0);}
    }
  };
  const act = useCallback(
    async (command: string, args: Record<string, unknown> = {}) => {
      setError("");
      try {
        await invoke(command, args);
        await poll.current();
      } catch (e) {
        setError(errorMessage(e));
      }
    },
    [invoke],
  );
  useEffect(() => {
    let disposed = false;
    let presentationFeed:Awaited<ReturnType<typeof observeState>>|undefined;
    const off = onCoreNotification((n) => {
      if (n.type === "view.presentation") {
        visible.current = (n.payload as { visible: boolean }).visible;
        if (visible.current) {
          void invoke("files.presentation").then(() => poll.current());
        } else clearTimeout(timer.current);
      }
      if (n.type === "view.prepareMove") {
        const p = n.payload as { operationId: string };
        visible.current = false;
        clearTimeout(timer.current);
        const data = JSON.stringify({
          version: 2,
          scrollUnit: (tree.current?.scrollTop ?? 0)/rowHeight,
          selected:current.current?.selected,
        });
        void core.call("view.snapshotChunk", {
          operationId: p.operationId,
          index: 0,
          total: 1,
          data,
        });
      }
      if (n.type === "view.moveCancelled") {
        visible.current = true;
        void poll.current();
      }
    });
    void (async () => {
      try {
        const s = await core.call<Snapshot>("view.initialize");
        if (disposed) return;
        setSnapshot(s);
        current.current = s;
        await core.call("view.ready");
        if (disposed) return;
        mounted.current = true;
        const feed=await observeState({call:(method,params)=>core.call(method,params),onNotification:onCoreNotification},
          {contract:'view.presentation.v1',target:{scope:{workspaceId:ctx.workspaceId!,workspaceGeneration:ctx.workspaceGeneration!},viewId:ctx.viewId!}},presentation,value=>{
            visible.current=value.visible;
            if(value.visible)void poll.current();else clearTimeout(timer.current);
          },{onError:e=>{if(!disposed)setError(errorMessage(e));}});
        if(disposed){await feed.dispose();return;}presentationFeed=feed;
        await invoke("files.presentation");
        await poll.current();
        if(tree.current){const saved=ctx.snapshot as {version?:number;scrollUnit?:number}|undefined;tree.current.scrollTop=saved?.version===2?(saved.scrollUnit??0)*rowHeight:saved?.version===1?Math.max(0,s.selectedUnit)*rowHeight:s.scrollTop;}
      } catch (e) {
        if (!disposed) setError(errorMessage(e));
      }
    })();
    return () => {
      disposed = true;
      mounted.current = false;
      off();
      void presentationFeed?.dispose();
      clearTimeout(timer.current);
    };
  }, [invoke]);
  useEffect(() => {
    const el = tree.current;
    if (!el) return;
    const observer = new ResizeObserver(() => {
      const count = Math.min(300, Math.ceil(el.clientHeight / rowHeight));
      setCount(count);
      range.current.countUnits = count;
      void poll.current();
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [Boolean(snapshot),rowHeight]);
  const menu = async (row: Row, x: number, y: number) => {
    await invoke('follow.interaction',{active:true});
    try {
    const id = await showMenu(
      [
        { id: "copy", label: clipboardPath(row.path).label, enabled: true },
        ...(row.type==='symlink'?[{id:'target',label:'定位到链接目标',enabled:true}]:[]),
        {
          id: "refresh",
          label: "刷新此目录",
          enabled: row.type === "directory",
          separator: true,
        },
        ...(row.type === "directory"
          ? [
              {
                id: "toggle",
                label: row.expanded ? "收起目录" : "展开目录",
                enabled: true,
              },
            ]
          : []),
      ],
      { x, y },
    );
    if (id === "copy")
      void core
        .call("clipboard.writeText", { text: clipboardPath(row.path).text })
        .catch((e) => setError(errorMessage(e)));
    else if(id==='target'){const target=await invoke<{path:RemotePath;type:string}>('files.linkTarget',{id:row.id});locate(target.path);}
    else if (id === "refresh") void act("files.refresh", { id: row.id });
    else if (id === "toggle") void act("files.toggle", { id: row.id });
    }catch(e){setError(errorMessage(e));}finally{await invoke('follow.interaction',{active:false});}
  };
  const choose = (r: Row) => {
    if (r.action) {
      if (r.action !== "empty")
        void act(r.action === "retry" ? "files.refresh" : "files.more", {
          id: r.path.bytesBase64,
        });
      return;
    }
    setPathInput(pathText(r.path));
    void act("files.select", { id: r.id });
  };
  const locate = (p: RemotePath) => {
    following.current = p.bytesBase64;
    void act("files.locate", { path: p });
  };
  const key = async (e: React.KeyboardEvent) => {
    const s = current.current;
    if (!s) return;
    const selected = s.rows.find((r) => r.id === s.selected);
    let index = s.selectedIndex;
    if (e.key === "F5") {
      e.preventDefault();
      void act("files.refresh");
      return;
    }
    if (e.shiftKey && e.key === "F10" && selected) {
      e.preventDefault();
      const box = tree.current!.getBoundingClientRect();
      void menu(selected, box.x + 30, box.y + 30);
      return;
    }
    if (e.key === "ArrowDown") index++;
    else if (e.key === "ArrowUp") index--;
    else if (e.key === "Home") index = 0;
    else if (e.key === "End") index = s.total - 1;
    else if(e.key==='PageDown'||e.key==='PageUp'){
      e.preventDefault();const row=await invoke<Row>('files.row',{unit:Math.max(0,s.selectedUnit+(e.key==='PageDown'?1:-1)*(tree.current?.clientHeight??rowHeight)/rowHeight)});
      if(row){choose(row);tree.current!.scrollTop=row.unitOffset*rowHeight;}return;
    }
    else if (
      (e.key === "ArrowRight" || e.key === "Enter") &&
      selected?.type === "directory"
    ) {
      e.preventDefault();
      if (!selected.expanded || e.key === "Enter") {
        void act("files.toggle", { id: selected.id });
        return;
      } else index++;
    } else if (e.key === "ArrowLeft" && selected) {
      e.preventDefault();
      if (selected.expanded) void act("files.toggle", { id: selected.id });
      else void act("files.select", { id: parent(selected.path).bytesBase64 });
      return;
    } else if (e.key === "Enter" && selected) {
      e.preventDefault();
      choose(selected);
      return;
    } else return;
    e.preventDefault();
    index = Math.max(0, Math.min(s.total - 1, index));
    const row=await invoke<Row>('files.row',{index});
    if (row) {
      choose(row);
      const el = tree.current!;
      const top = row.unitOffset * rowHeight,height=row.units*rowHeight;
      if (top < el.scrollTop) el.scrollTop = top;
      else if (top + height > el.scrollTop + el.clientHeight)
        el.scrollTop = top - el.clientHeight + height;
    }
  };
  return (
    <main
      className="files-view"
      onPointerDown={() =>
        void core
          .call("views.requestFocus", { viewId: ctx.viewId, userGesture: true })
          .catch(() => {})
      }
      onKeyDownCapture={(e) => {
        if (e.key === "Tab")
          void core
            .call("views.requestFocus", {
              viewId: ctx.viewId,
              userGesture: true,
            })
            .catch(() => {});
      }}
    >
      <span ref={densityProbe} className="density-probe" aria-hidden="true"/>
      <form
        className="path-bar"
        onSubmit={async (e) => {
          e.preventDefault();
          editing.current=false;await invoke('follow.interaction',{active:false,submitted:true});
          following.current = snapshot?.selectedPath&&pathInput===pathText(snapshot.selectedPath)?snapshot.selectedPath.bytesBase64:encode(utf8(pathInput));
          void act(
            "files.locate",
            snapshot?.selectedPath && pathInput === pathText(snapshot.selectedPath)
              ? { path: snapshot.selectedPath }
              : { text: pathInput },
          );
        }}
      >
        <Input
          aria-label="远端绝对路径"
          value={pathInput}
          onChange={(e) => setPathInput(e.target.value)}
          onFocus={()=>{editing.current=true;void invoke('follow.interaction',{active:true});}}
          onBlur={()=>{editing.current=false;setTimeout(()=>{if(!editing.current)void invoke('follow.interaction',{active:false});},0);}}
          onKeyDown={(e) => {
            if(e.key==='Escape'){editing.current=false;setPathInput(current.current?.path??'/');void invoke('follow.interaction',{active:false});e.currentTarget.blur();}
            if (e.nativeEvent.isComposing && e.key === "Enter")
              e.preventDefault();
          }}
          spellCheck={false}
        />
        <Button variant="quiet" size="compact" type="submit" className="icon" title="定位路径" aria-label="定位路径">
          <ArrowRight size={16} />
        </Button>
      </form>
      <Toolbar className="files-toolbar">
        <span>名称</span>
        <div>
          <Button variant="quiet" size="compact" className="icon" aria-label="跟随终端" aria-pressed={snapshot?.follow?.enabled??true} title="跟随终端目录" onClick={()=>void act('follow.setEnabled',{enabled:!snapshot?.follow?.enabled})}><Link2 size={15}/></Button>
          <Button variant="quiet" size="compact" className="icon" aria-label="定位到终端目录" title="定位到终端目录" onClick={()=>void act('follow.locateOnce')}><LocateFixed size={15}/></Button>
          <Button variant="quiet" size="compact"
            className="icon"
            title="刷新"
            aria-label="刷新目录与挂载"
            onClick={() => void act("files.refresh")}
          >
            <RefreshCw size={15} />
          </Button>
          {(snapshot?.loading||snapshot?.follow?.loading) && (
            <Button variant="quiet" size="compact"
              className="icon"
              aria-label="取消读取"
              onClick={() => void act("files.cancel")}
            >
              <X size={15} />
            </Button>
          )}
          <span>大小</span>
        </div>
      </Toolbar>
      {snapshot?.follow&&<div className="follow-status" role="status"><Text>{snapshot.follow.paused?'跟随已暂停':snapshot.follow.enabled?'跟随':'手动浏览'}{snapshot.follow.source?` · ${snapshot.follow.source}`:''}</Text><Text tone="muted" size="small">{snapshot.follow.loading?'正在定位…':snapshot.follow.message}</Text></div>}
    {themeError&&<Notice kind="error" action={<Button onClick={()=>void retryTheme()}>重试同步</Button>}>{themeError}</Notice>}
      {error && (
        <Notice kind="error" onDismiss={()=>setError("")}>{error}</Notice>
      )}
      {snapshot?.message && (
        <Notice kind="warning">
          {snapshot.message}
        </Notice>
      )}
      {snapshot &&
        ["idle", "waitingConnection", "disconnected"].includes(
          snapshot.state,
        ) && (
          <div className="connect-state">
            <p>
              {snapshot.state === "waitingConnection"
                ? "等待服务器连接就绪"
                : snapshot.state === "disconnected"
                  ? "已读快照可能过期"
                  : "尚未读取远端目录"}
            </p>
            <Button
              onClick={() =>
                void connectionCommandTimeout(core,ctx.serverId,ctx.workspaceId)
                  .then(timeout=>core.call("commands.execute", {
                    pluginId: "sodeck.files",
                    commandId: "files.connect",
                    workspaceId: ctx.workspaceId,
                    viewId: ctx.viewId,
                    operationId: crypto.randomUUID(),
                  },timeout))
                  .then(() => poll.current())
                  .catch((e) => setError(errorMessage(e)))
              }
            >
              连接并读取
            </Button>
          </div>
        )}
      <div
        ref={tree}
        className="file-tree"
        role="tree"
        aria-label="远端文件目录树"
        tabIndex={0}
        aria-activedescendant={
          snapshot?.rows.some((r) => r.id === snapshot.selected)
            ? nodeId(snapshot.selected)
            : undefined
        }
        onKeyDown={(e) => void key(e)}
        onScroll={(e) => {
          const top = e.currentTarget.scrollTop;
          const next = Math.max(0, top / rowHeight);
          range.current = { startUnit: next, countUnits:count,overscanUnits:20 };
          void act("files.scroll", { top });
        }}
      >
        <div
          className="tree-spacer"
          style={{ height: (snapshot?.totalUnits ?? 0) * rowHeight }}
        >
          <div className="tree-rows" style={{ top: start * rowHeight }}>
            {snapshot?.rows.map((r) => (
              <TreeRow
                id={nodeId(r.id)}
                key={r.id}
                level={r.depth}
                selected={r.selected}
                expanded={r.type === "directory" ? r.expanded : undefined}
                capacity={r.mount&&r.capacity?.state==='ready'?r.capacity.value:undefined}
                className={
                  "file-row " +
                  (r.selected ? "selected " : "") +
                  (r.action ? "action-row" : "")
                }
                style={{ height: r.units*rowHeight }}
                aria-busy={r.loading||r.capacity?.state==='loading'}
                title={[r.error??pathText(r.path),r.readOnly?'只读':undefined].filter(Boolean).join('\n')}
                onClick={() => choose(r)}
                onDoubleClick={() => {
                  if (r.type === "directory")
                    void act("files.toggle", { id: r.id });
                }}
                onContextMenu={(e) => {
                  e.preventDefault();
                  void menu(r, e.clientX, e.clientY);
                }}
              >
                <div className="file-row-main" style={{height:rowHeight}}>
                {r.type === "directory" ? (
                  <Button variant="quiet" size="compact"
                    className="expander"
                    tabIndex={-1}
                    aria-label={r.expanded ? "收起目录" : "展开目录"}
                    onClick={(e) => {
                      e.stopPropagation();
                      void act("files.toggle", { id: r.id });
                    }}
                  >
                    {r.expanded ? (
                      <ChevronDown size={14} />
                    ) : (
                      <ChevronRight size={14} />
                    )}
                  </Button>
                ) : (
                  <span className="expander" />
                )}
                {r.loading ? (
                  <BusyIndicator label="正在读取目录"/>
                ) : r.mount ? (
                  <HardDrive size={16} />
                ) : r.type === "directory" ? (
                  <Folder size={16} />
                ) : r.type === "symlink" ? (
                  <Link size={15} />
                ) : r.type !== "action" ? (
                  <File size={15} />
                ) : null}
                <span className="filename">{r.name}</span>
                {r.mount?<MountCapacity value={r.capacity}/>:<Text tone="muted" size="small" numeric className="file-size">
                  {r.type === "file"
                    ? size(r.size)
                    : r.type === "symlink"
                      ? "链接"
                      : r.type === "directory" || r.action
                        ? ""
                        : "—"}
                </Text>}
                </div>
              </TreeRow>
            ))}
          </div>
        </div>
      </div>
      <StatusBar className="files-status">
        <span>
          {snapshot?.loading
            ? "正在读取"
            : snapshot?.state === "ready"
              ? snapshot.total + " 个可见条目"
              : "只读浏览"}
        </span>
        <span>文件 {snapshot?.version ?? "0.1.0"}</span>
      </StatusBar>
    </main>
  );
}
