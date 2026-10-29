import type {BoundedExecClient, ExecRequest} from '@sodeck/sdk/bounded-exec';
import {parseSections} from './framing';
import {fastScript, inventoryScript, capacityScript, FAST_LIMITS, INVENTORY_LIMITS} from './scripts';
import {parseFast, parseInventory, parseCapacity} from '../metrics/parse';
import type {Inventory, Mount, Selection} from '../protocol';
type Target = Pick<ExecRequest, 'scope' | 'leaseId' | 'connectionGeneration' | 'cancellation'>;
export class LinuxCollector {
  private groups = new Map<string,{failures:number;next:number;error:string}>();
  reset() {this.groups.clear();}
  constructor(private client: BoundedExecClient) {}
  private async collect(target: Target, token: string, stdin: Uint8Array, limits: Record<string, number>, bytes: number, timeout: number) {
    const result = await this.client.run({...target, operationId: token, command: 'sh -s', stdin, maxOutputBytes: bytes, remoteTimeoutMs: timeout, deadlineMs: timeout + 2000});
    if (result.cleanupPending) throw Object.assign(Error('采集回收待确认'), {code: 'CLEANUP_PENDING'});
    if (!result.stdout.length) throw Object.assign(Error(result.endedReason === 'CANCELED' ? '采集已停止' : '未取得远端采集数据'), {code: result.endedReason ?? 'EXEC_FAILED'});
    const sections = parseSections(result.stdout, token, limits);
    sections.partial ||= result.truncated || result.endedReason !== null || result.exitCode !== 0;
    return sections;
  }
  async fast(target: Target, token: string, inventory?: Inventory, now=0, selected?:Selection) {
    const skip=[...this.groups].filter(([,state])=>state.next>now).map(([name])=>name);
    const limits=Object.fromEntries(Object.entries(FAST_LIMITS).filter(([name])=>!skip.includes(name)));
    const sections=await this.collect(target,token,fastScript(token,skip),limits,262144,4000);
    for(const name of skip)sections.errors[name]=this.groups.get(name)!.error;
    const data=parseFast(sections,inventory,selected);
    for(const name of ['cpu','memory','load','net','disk']){
      if(skip.includes(name))continue;
      const error=data.errors[name];
      if(error){const failures=(this.groups.get(name)?.failures??0)+1;this.groups.set(name,{failures,error,next:error.includes('未提供')?Infinity:now+[5000,10000,30000][Math.min(2,failures-1)]});}
      else this.groups.delete(name);
    }
    return data;
  }
  async inventory(target: Target, token: string, selected?:string|null) {
    return parseInventory(await this.collect(target, token, inventoryScript(token), INVENTORY_LIMITS, 524288, 4000),selected);
  }
  async capacity(target: Target, token: string, mount: Mount) {
    return parseCapacity(await this.collect(target, token, capacityScript(token, mount), {before:32768, capacity:16384, after:32768}, 65536, 3000), mount);
  }
}
