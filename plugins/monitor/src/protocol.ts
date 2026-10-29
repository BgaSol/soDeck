export type Quality = 'warming' | 'fresh' | 'stale' | 'unsupported' | 'error';
export interface Metric<T> {quality: Quality; value: T | null; updatedAt: number | null; ageMs: number | null; message?: string}
export interface Memory {total: string; used: string; available: string; percent: number; estimated: boolean; swapTotal: string; swapUsed: string}
export interface Cpu {percent: number | null; iowait: number | null; cores: number | null; load: number[]}
export interface Counter {id: string; name: string; first: string; second: string; weakIdentity?: boolean; virtual?: boolean; partition?: boolean}
export interface Rate {id: string; name: string; first: number | null; second: number | null; virtual?: boolean; partition?: boolean; weakIdentity?: boolean}
export interface Mount {id: string; identity: string; rawPath: string; path: string; device: string; fsType: string; source: string; shared: boolean; network: boolean; temporary: boolean}
export interface Capacity {total: string; used: string; available: string; denominator: string; percent: number | null; source: 'stat' | 'df'}
export interface MountRow extends Omit<Mount, 'rawPath'> {capacity: Metric<Capacity>; nextReadMs: number; readAllowed: boolean}
export interface Inventory {os: string; host: string; kernel: string; boot: string; cores: number | null; mounts: Mount[]; interfaces: Record<string, {id: string; virtual: boolean}>; devices: Record<string, {id: string; partition: boolean}>; partial: boolean}
export interface FastData {cpu: bigint[] | null; memory: Memory | null; load: number[]; uptime: number | null; net: Counter[]; disk: Counter[]; errors: Record<string, string>; partial: boolean}
export interface Point {time: number; value: number | null}
export interface Options {refreshSeconds: 2 | 5 | 10; rangeSeconds: 60 | 300 | 600; paused: boolean; showLoopback: boolean; showVirtual: boolean}
export interface Selection {network: string | null; disk: string | null; mount: string | null; mountPage: number; mountExpanded: boolean; networkPage: number; diskPage: number}
export interface Snapshot {
  protocolVersion: 1; revision: string; viewId: string; serverId: string; serverName: string; workspaceGeneration: string;
  connection: 'waiting' | 'connecting' | 'connected' | 'disconnected'; status: string; error?: string; diagnosticRef?:import('@sodeck/sdk').DiagnosticRef; visible: boolean; cleanupPending: boolean; limited: boolean;
  options: Options; selection: Selection; system: {host: string; kernel: string; os: string; uptime: number | null};
  cpu: Metric<Cpu>; memory: Metric<Memory>; network: Metric<Rate[]>; disk: Metric<Rate[]>;
  mounts: MountRow[]; selectedMount: MountRow|null; mountCount: number; networkCount: number; diskCount: number; partial: boolean; historyReleased: boolean;
  trends: Record<string, Point[]>; capturedAt: number; timelineMs: number; diagnostics: {activeJobs: number; cacheBytes: number};
}
export type SnapshotResult = Snapshot | {unchanged: true; revision: string};
export const DEFAULT_OPTIONS: Options = {refreshSeconds: 2, rangeSeconds: 60, paused: false, showLoopback: false, showVirtual: false};
export const emptyMetric = <T>(message = '等待采集'): Metric<T> => ({quality:'warming',value:null,updatedAt:null,ageMs:null,message});
