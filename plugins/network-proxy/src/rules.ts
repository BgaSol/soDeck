import {observation,type ObservationClient} from './observation';
import {utf8} from '@sodeck/sdk/bytes';
import {networkOperation,type NetworkRpc} from '@sodeck/sdk/network-client';
import {fault,validHost} from './protocol/address';
import {sha256} from '@noble/hashes/sha2.js';

export interface CommonRule {id:string;revision:number;name:string;serverId:string;bindMode:'fixed'|'auto';port:number;autoReconnect:boolean}
export type Rule=CommonRule&({type:'port-forward';targetHost:string;targetPort:number}|{type:'network-proxy'});
interface Naming {mode:'auto';ruleRevision:number;generatedName:string}
export interface SaveReceipt {submissionId:string;requestHash:string;ruleId:string;revision:number}
interface Collection {schemaVersion:1;rules:Rule[];naming?:Record<string,Naming>;saveReceipts?:SaveReceipt[]}
export const requestHash=(value:unknown)=>Array.from(sha256(utf8(JSON.stringify(value))),b=>b.toString(16).padStart(2,'0')).join('');
export function automaticName(rule:Pick<Rule,'type'>&{targetHost?:string;targetPort?:number}){
  if(rule.type==='network-proxy')return '内网代理';
  const name=`转发至 ${rule.targetHost?.includes(':')?`[${rule.targetHost}]`:rule.targetHost}:${rule.targetPort}`;
  return [...name].length<=128?name:`端口转发 ${rule.targetPort}`;
}
function port(value:unknown){return typeof value==='number'&&Number.isInteger(value)&&value>=1&&value<=65535;}
export function validateRule(value:unknown):Rule {
  if(!value||typeof value!=='object'||Array.isArray(value))throw fault('INVALID_PARAMS','规则格式无效');
  const r=value as Rule;
  const allowed=['id','revision','name','serverId','type','bindMode','port','autoReconnect',...(r.type==='port-forward'?['targetHost','targetPort']:[])];
  if(Object.keys(r).some(k=>!allowed.includes(k))||!['port-forward','network-proxy'].includes(r.type)
    ||typeof r.id!=='string'||!r.id||r.id.length>128||!Number.isSafeInteger(r.revision)||r.revision<1
    ||typeof r.name!=='string'||!r.name.trim()||[...r.name].length>128||/[\x00-\x1f\x7f]/.test(r.name)
    ||typeof r.serverId!=='string'||!r.serverId||r.serverId.length>128||typeof r.autoReconnect!=='boolean'
    ||!['fixed','auto'].includes(r.bindMode)||(r.bindMode==='auto'?r.port!==0:!port(r.port)))throw fault('INVALID_PARAMS','名称、端口或规则身份无效');
  if(r.type==='port-forward'&&(typeof r.targetHost!=='string'||!validHost(r.targetHost)||!port(r.targetPort)))throw fault('INVALID_PARAMS','固定目标主机或端口无效');
  return {...r,name:r.name.trim()};
}
export class Rules {
  private data:Collection={schemaVersion:1,rules:[]};private tail=Promise.resolve();
  private pending?:{next:Collection;receipt:SaveReceipt;diagnostic?:ObservationClient};
  constructor(private rpc:NetworkRpc){}
  private reported=new Set<string>();
  private saved(rule:Rule,diagnostic:ObservationClient|undefined=observation){const key=`${rule.id}:${rule.revision}`;if(this.reported.has(key))return;this.reported.add(key);if(this.reported.size>512)this.reported.delete(this.reported.values().next().value!);void diagnostic?.emit('plugin.sodeck.network-proxy.rule.saved',{ruleId:rule.id,revision:rule.revision,kind:rule.type,portMode:rule.bindMode});}
  async load() {
    const value=await this.rpc.call<Collection|null>('storage.get',{key:'network-rules'});if(value===null)return;
    if(!value||value.schemaVersion!==1||!Array.isArray(value.rules)||value.rules.length>256||utf8(JSON.stringify(value)).length>524288)throw fault('SCHEMA_UNSUPPORTED','网络规则版本或容量不兼容，未修改原数据');
    this.data=this.decode(value);
  }
  private decode(value:Collection):Collection {
    if(!value||value.schemaVersion!==1||!Array.isArray(value.rules)||value.rules.length>256||utf8(JSON.stringify(value)).length>524288)throw fault('SCHEMA_UNSUPPORTED','网络规则版本或容量不兼容，未修改原数据');
    const rules=value.rules.map(validateRule);if(new Set(rules.map(r=>r.id)).size!==rules.length)throw fault('INVALID_PARAMS','规则身份重复');
    if(value.naming!==undefined&&(!value.naming||typeof value.naming!=='object'||Array.isArray(value.naming)))throw fault('INVALID_PARAMS','自动命名元数据格式无效');
    const naming:Record<string,Naming>={};
    for(const [id,n] of Object.entries(value.naming??{})){
      const rule=rules.find(r=>r.id===id);
      if(rule&&n&&n.mode==='auto'&&n.ruleRevision===rule.revision&&n.generatedName===rule.name&&Object.keys(n).every(k=>['mode','ruleRevision','generatedName'].includes(k)))naming[id]=n;
    }
    const receipts=value.saveReceipts??[];
    if(!Array.isArray(receipts)||receipts.length>64||new Set(receipts.map(r=>r?.submissionId)).size!==receipts.length||receipts.some(r=>!r||Object.keys(r).some(k=>!['submissionId','requestHash','ruleId','revision'].includes(k))||typeof r.submissionId!=='string'||!r.submissionId||r.submissionId.length>128||typeof r.requestHash!=='string'||!(/^[a-f0-9]{64}$/).test(r.requestHash)||typeof r.ruleId!=='string'||!r.ruleId||r.ruleId.length>128||!Number.isSafeInteger(r.revision)||r.revision<1))throw fault('INVALID_PARAMS','保存回执格式无效');
    return {schemaVersion:1,rules,naming,saveReceipts:receipts};
  }
  automatic(id:string){return this.data.naming?.[id]?.mode==='auto';}
  all(serverId:string){return this.data.rules.filter(r=>r.serverId===serverId).map(r=>({...r}));}
  get(serverId:string,id:string){const rule=this.data.rules.find(r=>r.id===id&&r.serverId===serverId);if(!rule)throw fault('NOT_FOUND','规则不存在');return {...rule};}
  private serialize<T>(action:()=>Promise<T>):Promise<T> {const next=this.tail.then(action);this.tail=next.then(()=>{},()=>{});return next;}
  save(serverId:string,draft:unknown,expectedRevision:number,diagnostic:ObservationClient|undefined=observation):Promise<Rule> {
    return this.serialize(async()=>{
      if(this.pending)await this.reconcile();
      if(!draft||typeof draft!=='object')throw fault('INVALID_PARAMS','规则格式无效');
      const input=draft as Rule,old=input.id?this.data.rules.find(r=>r.id===input.id):undefined;
      if(old&&(old.serverId!==serverId||old.type!==input.type))throw fault('INVALID_PARAMS','规则类型和所属服务器不能更改');
      if(expectedRevision!==(old?.revision??0))throw fault('REVISION_CONFLICT','规则已被修改，请重新打开编辑');
      if(!old&&this.data.rules.length>=256)throw fault('CAPACITY','最多保存 256 条规则');
      const rule=validateRule({...input,id:old?.id??networkOperation().id,revision:(old?.revision??0)+1,serverId});
      const naming={...this.data.naming};delete naming[rule.id];
      const next:Collection={...this.data,naming,rules:old?this.data.rules.map(r=>r.id===old.id?rule:r):[...this.data.rules,rule]};
      if(utf8(JSON.stringify(next)).length>524288)throw fault('CAPACITY','规则配置超过 512 KiB');
      await (diagnostic??this.rpc).call('storage.set',{key:'network-rules',value:next});this.data=next;this.saved(rule,diagnostic);return {...rule};
    });
  }
  saveDraft(serverId:string,draft:unknown,expectedRevision:number,nameMode:'auto'|'manual',submissionId:string,hash:string,diagnostic:ObservationClient|undefined=observation):Promise<SaveReceipt> {
    return this.serialize(async()=>{
      if(!submissionId||submissionId.length>128||!(/^[a-f0-9]{64}$/).test(hash)||!['auto','manual'].includes(nameMode))throw fault('INVALID_PARAMS','提交身份无效');
      if(this.pending){
        if(this.pending.receipt.submissionId===submissionId){if(this.pending.receipt.requestHash!==hash)throw fault('OPERATION_CONFLICT','同一提交标识不能用于不同草稿');return this.reconcile();}
        await this.reconcile();
      }
      const receipt=this.data.saveReceipts?.find(r=>r.submissionId===submissionId);
      if(receipt){if(receipt.requestHash!==hash)throw fault('OPERATION_CONFLICT','同一提交标识不能用于不同草稿');return {...receipt};}
      if(!draft||typeof draft!=='object')throw fault('INVALID_PARAMS','规则格式无效');
      const input=draft as Rule,old=input.id?this.data.rules.find(r=>r.id===input.id):undefined;
      if(old&&(old.serverId!==serverId||old.type!==input.type))throw fault('INVALID_PARAMS','规则类型和所属服务器不能更改');
      if(expectedRevision!==(old?.revision??0))throw fault('REVISION_CONFLICT','规则已被修改，请重新打开编辑');
      if(!old&&this.data.rules.length>=256)throw fault('CAPACITY','最多保存 256 条规则');
      const rule=validateRule({...input,id:old?.id??networkOperation().id,revision:(old?.revision??0)+1,serverId,name:nameMode==='auto'?automaticName(input):input.name});
      const naming={...this.data.naming};if(nameMode==='auto')naming[rule.id]={mode:'auto',ruleRevision:rule.revision,generatedName:rule.name};else delete naming[rule.id];
      const saved={submissionId,requestHash:hash,ruleId:rule.id,revision:rule.revision};
      const next:Collection={schemaVersion:1,rules:old?this.data.rules.map(r=>r.id===old.id?rule:r):[...this.data.rules,rule],naming,saveReceipts:[...(this.data.saveReceipts??[]).slice(-63),saved]};
      if(utf8(JSON.stringify(next)).length>524288)throw fault('CAPACITY','规则配置超过 512 KiB');
      try{await (diagnostic??this.rpc).call('storage.set',{key:'network-rules',value:next});this.data=next;this.saved(rule,diagnostic);return saved;}
      catch(error){
        if(!['RESULT_UNKNOWN','TIMEOUT'].includes(String((error as {code?:string}).code)))throw error;
        this.pending={next,receipt:saved,diagnostic};return this.reconcile();
      }
    });
  }
  private async reconcile():Promise<SaveReceipt> {
    const pending=this.pending!;
    try{
      const value=this.decode(await this.rpc.call<Collection>('storage.get',{key:'network-rules'}));
      const receipt=value.saveReceipts?.find(r=>r.submissionId===pending.receipt.submissionId&&r.requestHash===pending.receipt.requestHash);
      if(receipt){this.data=value;this.pending=undefined;const rule=value.rules.find(r=>r.id===receipt.ruleId&&r.revision===receipt.revision);if(rule)this.saved(rule,pending.diagnostic);return {...receipt};}
    }catch{/* An unreadable or absent receipt cannot establish that a timed-out write failed. */}
    throw fault('RESULT_UNKNOWN','保存结果待核实，请保持此草稿并重试核对');
  }
  result(submissionId:string):Promise<SaveReceipt|undefined>{return this.serialize(async()=>{if(this.pending){if(this.pending.receipt.submissionId!==submissionId)throw fault('RESULT_UNKNOWN','另一笔保存仍待核实');return this.reconcile();}return this.data.saveReceipts?.find(r=>r.submissionId===submissionId);});}
  remove(serverId:string,id:string,expectedRevision:number,diagnostic:ObservationClient|undefined=observation) {
    return this.serialize(async()=>{
      if(this.pending)await this.reconcile();
      const old=this.get(serverId,id);if(old.revision!==expectedRevision)throw fault('REVISION_CONFLICT','规则已被修改，请刷新后再删除');
      const naming={...this.data.naming};delete naming[id];
      const next:Collection={...this.data,naming,rules:this.data.rules.filter(r=>r.id!==id)};
      await (diagnostic??this.rpc).call('storage.set',{key:'network-rules',value:next});this.data=next;void diagnostic?.emit('plugin.sodeck.network-proxy.rule.deleted',{ruleId:id,revision:old.revision,kind:old.type});
    });
  }
}
