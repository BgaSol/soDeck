import {CapacityCache,type CacheValue} from './capacity-cache';
import {RowLayout} from './row-layout';
import {
  Remote,
  type Attributes,
  type Bridge,
  type WorkspaceScope,
  type Lease,
  message,
  fault,
} from "./remote";
import {
  root,
  path,
  pathText,
  basename,
  join,
  parent,
  ancestors,
  decode,
  utf8,
  type RemotePath,
} from "./remote-path";
import { matching } from "./mounts";
import { mounts, mountCapacity, type MountSnapshot } from "./linux-collector";
import type { Capacity } from "./format";
export interface Node {
  path: RemotePath;
  name: string;
  attributes: Attributes;
  children: string[];
  expanded: boolean;
  loaded: boolean;
  loading: boolean;
  error?: string;
  directoryId?: string;
  cursor: number;
  eof: boolean;
  observedAt?: number;
}
export interface Row {
  units:number;unitOffset:number;capacity?:CacheValue<Capacity>;
  id: string;
  path: RemotePath;
  name: string;
  depth: number;
  type: string;
  size: string | null;
  expanded: boolean;
  selected: boolean;
  mount: boolean;
  readOnly: boolean;
  loading: boolean;
  error?: string;
  action?: "more" | "retry" | "empty";
}
export interface Snapshot {
  layoutRevision:number;startUnit:number;nextUnit:number;totalUnits:number;selectedUnit:number;viewportUnit:number;selectedPath:RemotePath;
  follow?:import('./follow').FollowSnapshot;
  revision: number;
  state: string;
  message?: string;
  showHidden: boolean;
  selected: string;
  selectedIndex: number;
  rows: Row[];
  total: number;
  scrollTop: number;
  mountError?: string;
  loading: boolean;
  path: string;
  version: string;
}
const unknown: Attributes = {
  type: "directory",
  size: null,
  mtime: null,
  atime: null,
  permissions: null,
  uid: null,
  gid: null,
};
const node = (p: RemotePath, attributes = unknown): Node => ({
  path: p,
  name: basename(p),
  attributes,
  children: [],
  expanded: false,
  loaded: false,
  loading: false,
  cursor: 0,
  eof: false,
});
export class Explorer {
  followSnapshot?:()=>import('./follow').FollowSnapshot;
  private followVisible=new Set<string>();
  snapshotTiming = { flatMs: 0, encodeMs: 0 };
  remote: Remote;
  nodes = new Map<string, Node>();
  selected = root.bytesBase64;
  revision = 1;
  state = "waitingConnection";
  error?: string;
  visible = false;
  disposed = false;
  showHidden = true;
  scrollTop = 0;
  private detailRemote?: Remote;
  mountSnapshot?: MountSnapshot;
  mountError?: string;
  private serial = 0;
  private selection = 0;
  private presentationRevision = -1;
  private observing = false;
  private observeTimer?: ReturnType<typeof setTimeout>;
  private jobs = new Set<Promise<unknown>>();
  private rows: Row[] = [];
  private rowRevision = 0;
  private collecting = false;
  private layout=new RowLayout([]);private layoutRevision=0;private mountEpoch=0;
  readonly capacities=new CapacityCache<Capacity>(key=>{
    const [generation,connection,mountEpoch,id]=JSON.parse(key) as [string,number,number,string];
    const snapshot=this.mountSnapshot,n=this.nodes.get(id),remote=this.remote.fork();
    const valid=generation===this.scope.workspaceGeneration&&connection===this.remote.lease?.generation&&mountEpoch===this.mountEpoch&&this.visible&&!this.disposed&&snapshot&&n;
    return {promise:valid?mountCapacity(remote,n.path,snapshot):Promise.reject(fault('CANCELED','容量读取已失效')),cancel:()=>remote.cancel()};
  },()=>{if(!this.disposed)this.changed();});
  private capacityKey(id:string){return JSON.stringify([this.scope.workspaceGeneration,this.remote.lease?.generation??0,this.mountEpoch,id]);}
  private connectionRevision?: number;
  constructor(
    readonly bridge: Bridge,
    readonly scope: WorkspaceScope,
    readonly serverId: string,
    readonly viewId: string,
    readonly budget: () => number,
  ) {
    this.remote = new Remote(bridge, scope);
    this.nodes.set(root.bytesBase64, node(root));
  }
  changed() {
    this.revision++;
  }
  private background(work: () => Promise<unknown>) {
    const serial=this.serial;
    const task = work()
      .catch((e) => {
        if (!this.disposed && serial===this.serial && (e as { code?: string }).code !== "CANCELED") {
          this.error = message(e);
          if(this.state==='loading'){this.state='partialError';const rootNode=this.nodes.get(root.bytesBase64);if(rootNode)rootNode.error=this.error;}
          this.changed();
        }
      })
      .finally(() => {
        this.jobs.delete(task);
        this.changed();
      });
    this.jobs.add(task);
    this.changed();
  }
  async presentation(visible: boolean, revision: number) {
    if (revision < this.presentationRevision) return;
    this.presentationRevision = revision;
    if (this.visible === visible) return;
    this.visible = visible;
    if (!visible) {
      this.capacities.visible([]);
      clearTimeout(this.observeTimer);
      this.serial++;
      this.selection++;
      await this.detailRemote?.cancel();
      await this.remote.cancel();
      for (const n of this.nodes.values()) {
        n.directoryId = undefined;
        n.loading = false;
      }
      this.changed();
      return;
    }
    if (this.remote.lease) {
      this.background(async () => {
        await this.verifyConnection();
        if (this.remote.lease && !this.nodes.get(root.bytesBase64)!.loaded)
          await this.load(root.bytesBase64);
        if (this.remote.lease) await this.select(this.selected);
      });
    } else void this.observe();
  }
  private async verifyConnection() {
    const capabilities=await this.bridge.call<{capabilities:string[]}>('capabilities.list');
    if(capabilities?.capabilities?.includes('connections.route.v1')){
      const status=await this.bridge.call<{running:{connectionId:string;generation:number;state:string}[]}>('connections.route.get',{serverId:this.serverId,workspaceId:this.scope.workspaceId});
      const lease=this.remote.lease;
      // Visibility and window moves validate the held snapshot. A saved route
      // edit cannot silently replace or dispose a still-live old connection.
      if(status.running.some(r=>r.connectionId===lease?.connectionId&&r.generation===lease?.generation&&r.state==='active'))return;
      await this.bridge.call('views.bindResources',{viewId:this.viewId,resources:[]});
      await this.remote.dispose();this.state='disconnected';this.error='连接已结束，请重新连接并读取';this.changed();return;
    }
    const server = await this.bridge.call<{ revision: number }>("servers.get", {
      serverId: this.serverId,
    });
    const state = await this.bridge.call<{
      state: string;
      connection: { generation: number } | null;
    }>("connections.observe", {
      scope: this.scope,
      serverId: this.serverId,
      configRevision: server.revision,
      waitMs: 0,
    });
    if (
      server.revision !== this.connectionRevision ||
      state.state !== "active" ||
      state.connection?.generation !== this.remote.lease?.generation
    ) {
      await this.bridge.call('views.bindResources',{viewId:this.viewId,resources:[]});
      await this.remote.dispose();
      this.state = "disconnected";
      this.error = "连接或服务器配置已变化，请重新连接并读取";
      this.changed();
    }
  }
  private async observe() {
    if (this.observing || !this.visible || this.disposed || this.remote.lease)
      return;
    this.observing = true;
    try {
      const server = await this.bridge.call<{ revision: number }>(
        "servers.get",
        { serverId: this.serverId },
      );
      const state = await this.bridge.call<{ state: string }>(
        "connections.observe",
        {
          scope: this.scope,
          serverId: this.serverId,
          configRevision: server.revision,
          waitMs: 1000,
        },
      );
      if (!this.visible || this.disposed) return;
      if (state.state === "active") {
        const lease = await this.bridge.call<Lease>(
          "connections.acquireExisting",
          {
            scope: this.scope,
            serverId: this.serverId,
            configRevision: server.revision,
            operationId: `files-auto-${this.viewId}-${Date.now()}`,
          },
        );
        await this.connected(lease, server.revision);
        return;
      }
      this.state = state.state === "connecting" ? "waitingConnection" : "idle";
      this.changed();
    } catch (e) {
      if (!this.disposed) {
        this.state = "idle";
        this.error = message(e);
        this.changed();
      }
    } finally {
      this.observing = false;
      if (this.visible && !this.disposed && !this.remote.lease)
        this.observeTimer = setTimeout(
          () => void this.observe(),
          this.state === "waitingConnection" ? 500 : 5000,
        );
    }
  }
  async connect(operationId: string) {
    if (this.disposed) throw fault("CANCELED", "文件视图已关闭");
    this.state = "loading";
    this.error = undefined;
    this.changed();
    try {
      const server = await this.bridge.call<{ revision: number }>(
        "servers.get",
        { serverId: this.serverId },
      );
      const lease = await this.bridge.call<Lease>("connections.acquire", {
        serverId: this.serverId,
        workspaceId: this.scope.workspaceId,
        operationId,
      });
      await this.connected(lease, server.revision);
    } catch (e) {
      this.state = "idle";
      this.error = message(e);
      this.changed();
      throw e;
    }
  }
  private async connected(lease: Lease, revision: number) {
    if (this.disposed) {
      await this.bridge.call("connections.release", { leaseId: lease.leaseId });
      return;
    }
    if (this.remote.lease) {
      await this.bridge.call("connections.release", { leaseId: lease.leaseId });
      return;
    }
    this.serial++;
    this.selection++;
    this.remote = new Remote(this.bridge, this.scope);
    this.remote.lease = lease;
    this.capacities.clear();this.mountEpoch++;
    this.connectionRevision = revision;
    this.state = "loading";
    this.error = undefined;
    this.nodes.clear();
    this.nodes.set(root.bytesBase64, node(root));
    this.selected = root.bytesBase64;
    this.mountSnapshot = undefined;
    this.scrollTop = 0;
    try {
      await this.bridge.call("views.bindResources", {
        viewId: this.viewId,
        resources: [{ kind: "lease", id: lease.leaseId }],
      });
    } catch (e) {
      await this.remote.dispose();
      throw e;
    }
    if (!this.visible) return;
    const serial=this.serial,remote=this.remote;
    this.background(async () => {
      const n = this.nodes.get(root.bytesBase64)!;
      n.expanded = true;
      const attributes = await remote.stat(root);
      if(serial!==this.serial||this.disposed)return;
      n.attributes=attributes;
      await this.load(root.bytesBase64);
      if(serial===this.serial&&!this.disposed)this.state = n.error?'partialError':'ready';
    });
    this.background(() => this.collect());
  }
  async collect() {
    if (this.collecting || !this.visible || !this.remote.lease) return;
    this.collecting = true;
    const serial = this.serial;
    try {
      const snapshot = await mounts(this.remote);
      if (this.disposed || serial !== this.serial) return;
      this.capacities.clear();this.mountEpoch++;
      this.mountSnapshot = snapshot;
      this.mountError = undefined;
      this.changed();
      await this.select(this.selected);
    } catch (e) {
      if (serial === this.serial && !this.disposed) {
        this.mountError = message(e);
        this.capacities.clear();this.mountEpoch++;
        this.mountSnapshot = undefined;
        this.changed();
        await this.select(this.selected);
      }
    } finally {
      this.collecting = false;
    }
  }
  private admit(count: number, bytes: number) {
    if (
      this.nodes.size + count <= 20000 &&
      this.cacheBytes() + bytes <= 12 * 1024 * 1024 &&
      this.budget() + bytes <= 32 * 1024 * 1024
    )
      return;
    const pinned = new Set(
      ancestors(this.nodes.get(this.selected)?.path ?? root).map(
        (p) => p.bytesBase64,
      ),
    );
    for (const [key, n] of this.nodes) {
      if (!n.expanded && !pinned.has(key) && n.children.length) {
        this.evictChildren(n);
        n.loaded = false;
        n.eof = false;
        n.error = "缓存已回收，展开后重新读取";
        if (
          this.nodes.size + count <= 20000 &&
          this.cacheBytes() + bytes <= 12 * 1024 * 1024 &&
          this.budget() + bytes <= 32 * 1024 * 1024
        )
          return;
      }
    }
    throw fault(
      "LIMIT_EXCEEDED",
      "文件列表已达到缓存上限；收起其他目录后刷新。当前列表尚未完整加载",
    );
  }
  cacheBytes() {
    let bytes = 0;
    for (const [id, n] of this.nodes)
      bytes +=
        2 *
        (id.length +
          n.name.length +
          n.children.reduce((v, k) => v + k.length, 0) +
          256);
    return bytes + (this.mountSnapshot?.raw.length ?? 0) * 3;
  }
  private evictChildren(n: Node) {
    for (const key of n.children) {
      const child = this.nodes.get(key);
      if (child) {
        this.evictChildren(child);
        if (child.directoryId)
          void this.remote.close(child.directoryId).catch(() => {});
      }
      this.nodes.delete(key);
    }
    n.children = [];
  }
  async load(key: string, refresh = false) {
    const n = this.nodes.get(key);
    if (
      !n ||
      n.attributes.type !== "directory" ||
      n.loading ||
      !this.visible ||
      this.disposed
    )
      return;
    const serial = this.serial;
    n.loading = true;
    n.error = undefined;
    this.changed();
    try {
      if (refresh) {
        if (n.directoryId) await this.remote.close(n.directoryId);
        n.directoryId = undefined;
        n.cursor = 0;
      }
      if (!n.directoryId) {
        if (n.loaded && !refresh && !n.eof)
          throw fault(
            "CURSOR_EXPIRED",
            "目录游标已关闭或过期，请刷新此目录后继续",
          );
        const open = await this.remote.openDirectory(n.path);
        n.directoryId = open.directoryId;
        n.cursor = 0;
      }
      const page = await this.remote.page(n.directoryId, n.cursor);
      if (serial !== this.serial || this.disposed) {
        if (n.directoryId) await this.remote.close(n.directoryId);
        n.directoryId = undefined;
        return;
      }
      this.admit(
        page.entries.length,
        page.entries.reduce(
          (v, e) => v + (key.length + e.name.bytesBase64.length + 300) * 2,
          0,
        ),
      );
      const keys = refresh || !n.loaded ? [] : [...n.children];
      for (const entry of page.entries) {
        const p = join(n.path, entry.name),
          id = p.bytesBase64;
        const old = this.nodes.get(id);
        this.nodes.set(
          id,
          old
            ? { ...old, attributes: entry.attributes }
            : node(p, entry.attributes),
        );
        if (!keys.includes(id)) keys.push(id);
      }
      if (refresh) {
        // A first page cannot prove an old child disappeared. Preserve the
        // selected branch only after checking it, without scanning its siblings.
        const selectedPath = this.nodes.get(this.selected)?.path;
        const chain = selectedPath
          ? ancestors(selectedPath).map((p) => p.bytesBase64)
          : [];
        for (const old of n.children) {
          if (keys.includes(old)) continue;
          const child = this.nodes.get(old);
          if (!child) continue;
          if (chain.includes(old)) {
            try {
              child.attributes = await this.remote.stat(child.path);
              if (serial !== this.serial) return;
              keys.push(old);
              continue;
            } catch (e) {
              if ((e as { code?: string }).code !== "NOT_FOUND") {
                keys.push(old);
                continue;
              }
            }
            await this.select(key);
          }
          this.evictChildren(child);
          if (child.directoryId) await this.remote.close(child.directoryId);
          this.nodes.delete(old);
        }
      }
      n.children = keys.sort((a, b) => {
        const x = this.nodes.get(a)!,
          y = this.nodes.get(b)!;
        return (
          Number(y.attributes.type === "directory") -
            Number(x.attributes.type === "directory") ||
          x.name.localeCompare(y.name) ||
          a.localeCompare(b)
        );
      });
      n.cursor = page.nextCursor;
      n.eof = page.eof;
      n.loaded = true;
      n.observedAt = Date.now();
      if (page.eof) n.directoryId = undefined;
    } catch (e) {
      if (serial === this.serial) {
        n.error = message(e);
        if (n.directoryId)
          await this.remote.close(n.directoryId).catch(() => {});
        n.directoryId = undefined;
      }
    } finally {
      n.loading = false;
      this.changed();
    }
  }
  toggle(key: string) {
    const n = this.nodes.get(key);
    if (!n || n.attributes.type !== "directory") return;
    n.expanded = !n.expanded;
    this.changed();
    if (n.expanded && !n.loaded) this.background(() => this.load(key));
    if (!n.expanded && n.directoryId) {
      const id = n.directoryId;
      n.directoryId = undefined;
      void this.remote.close(id).catch(() => {});
    }
  }
  async select(key: string) {
    const n=this.nodes.get(key);if(!n)return;
    this.selected=key;this.selection++;this.changed();
    await this.detailRemote?.cancel();this.detailRemote=undefined;
  }
  async linkTarget(key:string){
    const n=this.nodes.get(key);if(!n||n.attributes.type!=='symlink')throw fault('INVALID_PARAMS','此节点不是符号链接');
    if(!this.visible||!this.remote.lease||this.disposed)throw fault('CANCELED','文件面板当前不可用');
    await this.detailRemote?.cancel();const remote=this.remote.fork();this.detailRemote=remote;
    const serial=this.serial;
    const target=await remote.call<{path:RemotePath}>('files.realPath',{path:n.path});
    const attributes=await remote.stat(target.path);
    if(serial!==this.serial||!this.visible||this.disposed)throw fault('CANCELED','链接读取已取消');
    return {path:target.path,type:attributes.type};
  }
  async locate(target: RemotePath) {
    const serial = this.serial;
    const chain = ancestors(target);
    const targetAttributes = await this.remote.stat(target);
    if (serial !== this.serial || this.disposed) return;
    this.admit(chain.length, chain.length * 512);
    for (let i = 0; i < chain.length; i++) {
      const p = chain[i],
        key = p.bytesBase64;
      if (!this.nodes.has(key))
        this.nodes.set(
          key,
          node(
            p,
            i === chain.length - 1
              ? targetAttributes
              : await this.remote.stat(p),
          ),
        );
      if (serial !== this.serial) return;
      if (i) {
        const ancestor = this.nodes.get(chain[i - 1].bytesBase64)!;
        ancestor.expanded = true;
        if (!ancestor.children.includes(key)) ancestor.children.push(key);
      }
    }
    this.nodes.get(target.bytesBase64)!.attributes = targetAttributes;
    this.changed();
    await this.select(target.bytesBase64);
  }
  async locateFollow(target:RemotePath,remote:Remote,valid:()=>boolean){
    const chain=ancestors(target),prepared:Node[]=[];
    const cached=this.nodes.get(target.bytesBase64);
    if(cached?.loaded&&cached.eof&&!cached.error&&chain.every(p=>this.nodes.get(p.bytesBase64)?.attributes.type==='directory')){
      if(!valid())return;
      this.followVisible=new Set(chain.map(p=>p.bytesBase64));
      for(let i=0;i<chain.length;i++){
        const n=this.nodes.get(chain[i].bytesBase64)!;n.expanded=true;
        if(i){const parent=this.nodes.get(chain[i-1].bytesBase64)!;if(!parent.children.includes(n.path.bytesBase64))parent.children.push(n.path.bytesBase64);}
      }
      void this.select(target.bytesBase64).catch(e=>{if(valid()){this.error=message(e);this.changed();}});
      return;
    }
    for(const p of chain){
      const attributes=await remote.stat(p,true);
      if(!valid())return;
      if(attributes.type!=='directory')throw fault('NOT_DIRECTORY','终端路径不是可浏览目录');
      prepared.push(node(p,attributes));
    }
    const opened=await remote.openDirectory(target);
    const page=await remote.page(opened.directoryId,0);
    if(!valid()){await remote.close(opened.directoryId);return;}
    const previous=this.nodes.get(target.bytesBase64)?.directoryId;
    if(previous)await this.remote.close(previous);
    if(!valid()){await remote.close(opened.directoryId);return;}
    this.admit(prepared.length+page.entries.length,(prepared.length+page.entries.length)*512);
    if(!valid()){await remote.close(opened.directoryId);return;}
    // All navigation-critical I/O succeeded. Commit path/selection together.
    this.followVisible=new Set(chain.map(p=>p.bytesBase64));
    for(let i=0;i<prepared.length;i++){
      const n=prepared[i],key=n.path.bytesBase64,existing=this.nodes.get(key);
      if(existing)existing.attributes=n.attributes;else this.nodes.set(key,n);
      this.nodes.get(key)!.expanded=true;
      if(i){const parent=this.nodes.get(prepared[i-1].path.bytesBase64)!;if(!parent.children.includes(key))parent.children.push(key);}
    }
    const targetNode=this.nodes.get(target.bytesBase64)!;
    for(const entry of page.entries){
      const p=join(target,entry.name),key=p.bytesBase64;
      if(!this.nodes.has(key))this.nodes.set(key,node(p,entry.attributes));
      if(!targetNode.children.includes(key))targetNode.children.push(key);
    }
    targetNode.directoryId=page.eof?undefined:opened.directoryId;remote.transfer(opened.directoryId,this.remote);
    targetNode.cursor=page.nextCursor;targetNode.eof=page.eof;targetNode.loaded=true;targetNode.loading=false;targetNode.error=undefined;
    this.changed();
    // Selection commits synchronously before optional details loading.
    void this.select(target.bytesBase64).catch(e=>{if(valid()){this.error=message(e);this.changed();}});
  }
  manualNavigation(){this.followVisible.clear();this.changed();}
  refresh(key?: string) {
    if (!this.remote.lease || !this.visible) return;
    this.background(async () => {
      await this.verifyConnection();if(!this.remote.lease)return;
      const serial=this.serial;this.error=undefined;
      const targets = key
        ? [key]
        : this.flat()
            .filter((r) => r.type === "directory" && r.expanded && !r.action)
            .map((r) => r.id);
      for (const id of targets) {
        if (!this.visible||serial!==this.serial) return;
        await this.load(id, true);
      }
      await this.collect();
      if(serial===this.serial)this.state=[...this.nodes.values()].some(n=>n.error)?'partialError':'ready';
    });
  }
  schedule(action: () => Promise<unknown>) {
    this.background(action);
  }
  async cancel() {
    this.serial++;
    this.selection++;this.capacities.visible([]);
    await Promise.all([this.detailRemote?.cancel(), this.remote.cancel()]);
    for (const n of this.nodes.values()) {
      n.directoryId = undefined;
      n.loading = false;
    }
    this.changed();
  }
  flat(): Row[] {
    if (this.rowRevision === this.revision) return this.rows;
    const rows: Row[] = [];
    const visit = (id: string, depth: number) => {
      const n = this.nodes.get(id);
      if (
        !n
      )
        return;
      const match =
        n.attributes.type !== "symlink" && this.mountSnapshot
          ? matching(this.mountSnapshot.mounts, n.path)
          : { exact: false, candidates: [] };
      rows.push({
        units:1,unitOffset:0,
        id,
        path: n.path,
        name: n.name,
        depth,
        type: n.attributes.type,
        size: n.attributes.size,
        expanded: n.expanded,
        selected: id === this.selected,
        mount: match.exact,
        readOnly: match.candidates.length === 1 && match.candidates[0].readOnly,
        loading: n.loading,
        error: n.error,
      });
      if (n.expanded) {
        for (const child of n.children) visit(child, depth + 1);
        if (
          n.error ||
          (!n.eof && n.loaded) ||
          (n.loaded && !n.children.length && n.eof)
        )
          rows.push({
            units:1,unitOffset:0,
            id: id + ":action",
            path: n.path,
            name: n.error ?? (n.eof ? "空目录" : "尚有条目未加载 · 继续加载"),
            depth: depth + 1,
            type: "action",
            size: null,
            expanded: false,
            selected: false,
            mount: false,
            readOnly: false,
            loading: n.loading,
            action: n.error ? "retry" : n.eof ? "empty" : "more",
          });
      }
    };
    visit(root.bytesBase64, 0);
    if(rows.length!==this.rows.length||rows.some((r,i)=>r.id!==this.rows[i].id||r.units!==this.rows[i].units))this.layoutRevision++;
    this.layout=new RowLayout(rows);rows.forEach((row,i)=>{row.unitOffset=this.layout.offsets[i];});
    this.rows = rows;
    this.rowRevision = this.revision;
    return rows;
  }
  snapshot(startUnit=0,countUnits=100,overscanUnits=0,anchorId?:string,anchorOffset=0,continuation=false):Snapshot {
    const began=Date.now(),rows=this.flat(),flattened=Date.now();
    startUnit=Math.max(0,Number.isFinite(startUnit)?startUnit:0);countUnits=Math.min(300,Math.max(1,countUnits));overscanUnits=Math.min(40,Math.max(0,overscanUnits));
    const anchor=anchorId?rows.find(r=>r.id===anchorId):undefined;
    if(anchor)startUnit=anchor.unitOffset+Math.max(0,Math.min(anchor.units,anchorOffset));
    const visible=this.layout.range(startUnit,countUnits),range=this.layout.range(startUnit,countUnits,overscanUnits);
    const capacityKeys=rows.slice(visible.first,visible.end).filter(r=>r.mount&&r.unitOffset<startUnit+countUnits&&r.unitOffset+r.units>startUnit).map(r=>this.capacityKey(r.id));
    if(!continuation)this.capacities.visible(this.visible&&!!this.remote.lease?capacityKeys:[]);
    const selectedIndex=rows.findIndex(r=>r.id===this.selected);
    const snapshot:Snapshot={revision:this.revision,layoutRevision:this.layoutRevision,state:this.state,message:this.error,showHidden:true,selected:this.selected,selectedIndex,selectedUnit:selectedIndex<0?-1:this.layout.offsets[selectedIndex],selectedPath:this.nodes.get(this.selected)?.path??root,rows:[],total:rows.length,startUnit:this.layout.offsets[range.first]??0,nextUnit:this.layout.offsets[range.first]??0,totalUnits:this.layout.total,viewportUnit:startUnit,scrollTop:this.scrollTop,mountError:this.mountError,loading:this.jobs.size>0,path:pathText(this.nodes.get(this.selected)?.path??root),version:__FILES_VERSION__,follow:this.followSnapshot?.()};
    let bytes=JSON.stringify(snapshot).length*3;
    for(const original of rows.slice(range.first,range.end)){
      const row={...original,...(original.mount?{capacity:this.capacities.get(this.capacityKey(original.id))??{state:'loading' as const}}:{})};
      const size=JSON.stringify(row).length*3+1;
      if(bytes+size>32*1024){if(!snapshot.rows.length)throw fault('CAPACITY','文件行超过单次32 KiB预算');break;}
      snapshot.rows.push(row);bytes+=size;snapshot.nextUnit=row.unitOffset+row.units;
    }
    this.snapshotTiming={flatMs:flattened-began,encodeMs:Date.now()-flattened};return snapshot;
  }
  rowAt(index:number){const rows=this.flat();return rows[Math.max(0,Math.min(rows.length-1,Math.floor(index)))];}
  rowAtUnit(unit:number){const rows=this.flat();return rows[this.layout.at(unit)];}
  async dispose() {
    this.disposed = true;this.capacities.clear();
    this.visible = false;
    this.serial++;
    this.selection++;
    clearTimeout(this.observeTimer);
    await this.detailRemote?.cancel();
    await this.remote.dispose();
    this.nodes.clear();
    this.mountSnapshot = undefined;
  }
}
declare const __FILES_VERSION__: string;
