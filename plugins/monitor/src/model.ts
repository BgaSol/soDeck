import {DEFAULT_OPTIONS, emptyMetric, type Cpu, type Memory, type Rate, type Metric, type Inventory, type FastData, type Capacity, type Options, type Selection, type Mount, type Snapshot} from './protocol';
import {History} from './metrics/history';
import {Counters} from './metrics/counters';
import {cpuDifference} from './metrics/parse';

interface CapacityEntry {identity: string; metric: Metric<Capacity>; at: number; next: number; failures: number}
const backoff = (failures: number) => [5000, 10000, 30000][Math.min(2, failures - 1)];
export {backoff};
export class Model {
  options: Options = {...DEFAULT_OPTIONS};
  selection: Selection = {network:null, disk:null, mount:null, mountPage:0, mountExpanded:false, networkPage:0, diskPage:0};
  inventory?: Inventory;
  history = new History();
  cpu = emptyMetric<Cpu>();
  memory = emptyMetric<Memory>();
  network = emptyMetric<Rate[]>();
  disk = emptyMetric<Rate[]>();
  uptime: number | null = null;
  partial = false;
  historyReleased = false;
  private at: Record<string, number> = {};
  private capacities = new Map<string, CapacityEntry>();
  private allowedNetwork = new Set<string>();
  private netCounters = new Counters();
  private diskCounters = new Counters();
  private previousCpu?: {values: bigint[]; time: number; source: string};
  private lastUptime: number | null = null;
  private previousLocalSpan = Infinity;
  reset(now: number, clearHistory = false) {
    this.previousCpu = undefined; this.netCounters.clear(); this.diskCounters.clear(); this.lastUptime = null;
    this.previousLocalSpan = Infinity;
    if (clearHistory) {this.history.clear(); this.capacities.clear(); this.allowedNetwork.clear();}
    else this.history.gap(now);
  }
  installInventory(value: Inventory, now: number) {
    if (this.inventory?.boot && value.boot && this.inventory.boot !== value.boot) this.reset(now, true);
    // Incomplete inventories cannot silently delete previously known objects.
    if (value.partial && this.inventory) {this.partial = true; return;}
    this.inventory = value; this.partial = value.partial;
    for (const [id, entry] of this.capacities) if (!value.mounts.some(m => m.id === id && m.identity === entry.identity)) this.capacities.delete(id);
    for (const id of this.allowedNetwork) if (!value.mounts.some(m => m.identity === id)) this.allowedNetwork.delete(id);
    if (!value.mounts.some(m => m.id === this.selection.mount)) this.selection.mount = value.mounts.find(m => m.path === '/')?.id ?? value.mounts[0]?.id ?? null;
  }
  installFast(data: FastData, start: number, end: number, utc: number) {
    const now = (start + end) / 2, maximumGap = Math.max(10000, this.options.refreshSeconds * 3000);
    const time = data.uptime === null ? now : data.uptime * 1000, source = data.uptime === null ? 'local' : 'remote';
    if (data.uptime !== null && this.lastUptime !== null && data.uptime < this.lastUptime) this.reset(now, true);
    this.lastUptime = data.uptime; this.uptime = data.uptime; this.partial = data.partial || !!this.inventory?.partial;
    const fresh = <T>(key: string, value: T, quality: Metric<T>['quality'] = 'fresh', message?: string): Metric<T> => {
      this.at[key] = now; return {quality, value, ageMs:0, updatedAt:utc, ...(message ? {message} : {})};
    };
    if (data.cpu) {
      const prev = this.previousCpu;
      const valid = prev && prev.source === source && time > prev.time && time - prev.time <= maximumGap && (source === 'remote' || (end - start <= 2000 && this.previousLocalSpan <= 2000));
      const values = valid ? cpuDifference(data.cpu, prev.values) : {percent:null, iowait:null};
      this.cpu = fresh('cpu', {...values, cores:this.inventory?.cores ?? null, load:data.load}, values.percent === null ? 'warming' : 'fresh', values.percent === null ? '计算中，需要连续样本' : undefined);
      this.previousCpu = {values:data.cpu, time, source}; this.history.add('cpu', now, values.percent);
    } else {this.cpu = this.failure(this.cpu, data.errors.cpu); this.previousCpu = undefined; this.history.add('cpu', now, null);}
    if (data.memory) {this.memory = fresh('memory', data.memory); this.history.add('memory', now, data.memory.percent);}
    else {this.memory = this.failure(this.memory, data.errors.memory); this.history.add('memory', now, null);}
    for (const [key, rows, counters] of [['network',data.net,this.netCounters], ['disk',data.disk,this.diskCounters]] as const) {
      const error = data.errors[key === 'network' ? 'net' : 'disk'];
      if (error) {
        this[key] = this.failure(this[key], error); counters.clear();
        for (const row of this[key].value ?? []) for (const part of ['first','second']) this.history.add(`${key}:${row.id}:${part}`, now, null);
        continue;
      }
      if (source === 'local' && (end - start > 2000 || this.previousLocalSpan > 2000)) counters.clear();
      const rates = counters.update(rows, time, source, maximumGap);
      this[key] = fresh(key, rates, rates.some(r => r.first !== null || r.second !== null) ? 'fresh' : 'warming', end - start > 2000 ? '样本传输有延迟' : undefined);
      for (const row of rates) {this.history.add(`${key}:${row.id}:first`, now, row.first); this.history.add(`${key}:${row.id}:second`, now, row.second);}
    }
    this.previousLocalSpan=end-start;
    this.ensureSelection(); this.retainHistory();
  }
  private failure<T>(old: Metric<T>, message = '未取得有效样本'): Metric<T> {
    return {...old, quality:old.value === null ? (message.includes('未提供')?'unsupported':'error') : 'stale', message};
  }
  fail(message: string, now: number) {
    this.cpu = this.failure(this.cpu, message); this.memory = this.failure(this.memory, message);
    this.network = this.failure(this.network, message); this.disk = this.failure(this.disk, message); this.reset(now);
  }
  networks() {
    const rows = this.network.value ?? [];
    const visible = rows.filter(r => (this.options.showLoopback || r.name !== 'lo') && (this.options.showVirtual || !r.virtual));
    // A virtual-machine NIC is often the only useful interface. Preserve a usable
    // default without inventing a total or making its virtual classification vanish.
    return visible.length ? visible : rows.filter(r => r.name !== 'lo').slice(0,1);
  }
  private ensureSelection() {
    const net = this.networks(), disk = this.disk.value ?? [];
    if (!net.some(r => r.id === this.selection.network)) this.selection.network = net[0]?.id ?? null;
    if (!disk.some(r => r.id === this.selection.disk)) this.selection.disk = disk.find(r => !r.partition && !/^(loop|ram)/.test(r.name))?.id ?? disk[0]?.id ?? null;
  }
  private retainHistory() {
    const ids = new Set(['cpu','memory']);
    for (const key of ['network','disk'] as const) for (const row of this[key].value ?? []) for (const part of ['first','second']) ids.add(`${key}:${row.id}:${part}`);
    for (const mount of this.inventory?.mounts ?? []) ids.add('capacity:' + mount.identity);
    this.history.retain(ids);
  }
  allowCapacity(id: string) {const mount = this.inventory?.mounts.find(m => m.id === id); if (mount) this.allowedNetwork.add(mount.identity);}
  capacityAllowed(mount: Mount) {return !mount.network || this.allowedNetwork.has(mount.identity);}
  capacityTargets(now: number) {
    const mounts = this.inventory?.mounts ?? [];
    const visible = this.selection.mountExpanded ? mounts.slice(this.selection.mountPage * 8, this.selection.mountPage * 8 + 8) : [];
    const ids = new Set([this.selection.mount, mounts.find(m => m.path === '/')?.id, ...visible.map(m => m.id)]);
    return mounts.filter(m => ids.has(m.id) && this.capacityAllowed(m) && (this.capacities.get(m.id)?.next ?? 0) <= now)
      .sort((a,b) => (this.capacities.get(a.id)?.next ?? 0) - (this.capacities.get(b.id)?.next ?? 0));
  }
  capacityAttempt(mount: Mount, now: number) {
    const old = this.capacities.get(mount.id);
    this.capacities.set(mount.id, {...(old ?? {identity:mount.identity, metric:emptyMetric<Capacity>(), at:now, failures:0}), next:now + 30000});
  }
  capacityResult(mount: Mount, now: number, utc: number, value?: Capacity, error?: string) {
    if (!this.inventory?.mounts.some(m => m.identity === mount.identity)) return;
    const old = this.capacities.get(mount.id)!;
    this.capacities.set(mount.id, {identity:mount.identity, at:value ? now : old.at, failures:value ? 0 : old.failures + 1,
      next:Math.max(old.next, now + (value ? 30000 : Math.max(30000, backoff(old.failures + 1)))),
      metric:value ? {quality:'fresh', value, ageMs:0, updatedAt:utc} : this.failure(old.metric, error)});
    this.history.add('capacity:' + mount.identity, now, value?.percent ?? null);
  }
  private aged<T>(value: Metric<T>, at: number | undefined, now: number, limit: number): Metric<T> {
    const ageMs = at === undefined ? null : Math.max(0, now - at);
    return {...value, ageMs, quality: value.value !== null && ageMs !== null && ageMs > limit ? 'stale' : value.quality};
  }
  fields(now: number): Pick<Snapshot, 'options'|'selection'|'system'|'cpu'|'memory'|'network'|'disk'|'mounts'|'selectedMount'|'mountCount'|'networkCount'|'diskCount'|'trends'|'partial'|'historyReleased'> {
    this.ensureSelection(); const mounts = this.inventory?.mounts ?? [], selected = mounts.find(m => m.id === this.selection.mount);
    const limit = Math.max(10000, this.options.refreshSeconds * 3000), trends: Snapshot['trends'] = {};
    const ids = {cpu:'cpu', memory:'memory', networkFirst:`network:${this.selection.network}:first`, networkSecond:`network:${this.selection.network}:second`, diskFirst:`disk:${this.selection.disk}:first`, diskSecond:`disk:${this.selection.disk}:second`, capacity:'capacity:' + selected?.identity};
    for (const [name,id] of Object.entries(ids)) trends[name] = this.history.read(id, now, this.options.rangeSeconds);
    const mountRow=({rawPath:_,...mount}:Mount)=>{
      let hash=2166136261;for(let i=0;i<mount.identity.length;i++)hash=Math.imul(hash^mount.identity.charCodeAt(i),16777619);
      const shorten=(text:string)=>text.length>1024?text.slice(0,1023)+'…':text;
      const entry=this.capacities.get(mount.id);return {...mount,identity:mount.id+':'+(hash>>>0).toString(16),path:shorten(mount.path),source:shorten(mount.source),capacity:entry?this.aged(entry.metric,entry.at,now,65000):emptyMetric<Capacity>(mount.network?'网络文件系统，按需读取':'等待读取'),nextReadMs:Math.max(0,(entry?.next??0)-now),readAllowed:this.capacityAllowed({...mount,rawPath:''})};
    };
    const page=(rows:Rate[],number:number,selected:string|null)=>{
      const values=rows.slice(number*31,number*31+31),current=rows.find(r=>r.id===selected);if(current&&!values.includes(current))values.unshift(current);return values;
    };
    const networks=this.networks(),disks=this.disk.value??[];
    return {options:{...this.options}, selection:{...this.selection}, system:{host:this.inventory?.host ?? '', kernel:this.inventory?.kernel ?? '', os:this.inventory?.os ?? '', uptime:this.uptime},
      cpu:this.aged(this.cpu, this.at.cpu, now, limit), memory:this.aged(this.memory, this.at.memory, now, limit), network:this.aged({...this.network,value:page(networks,this.selection.networkPage,this.selection.network)},this.at.network,now,limit), disk:this.aged({...this.disk,value:page(disks,this.selection.diskPage,this.selection.disk)},this.at.disk,now,limit),
      mounts:mounts.slice(this.selection.mountPage*8, this.selection.mountPage*8+8).map(mountRow), selectedMount:selected?mountRow(selected):null, mountCount:mounts.length,networkCount:networks.length,diskCount:disks.length, trends, partial:this.partial, historyReleased:this.historyReleased};
  }
  bytes() {return this.history.bytes() + JSON.stringify({inventory:this.inventory, capacities:[...this.capacities], cpu:this.cpu, memory:this.memory, network:this.network, disk:this.disk}).length*2 + 32768;}
  releaseHistory() {this.history.clear(); this.capacities.clear(); this.inventory=undefined; this.network=emptyMetric(); this.disk=emptyMetric(); this.historyReleased=true;}
}
