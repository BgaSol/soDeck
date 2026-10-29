import {Secrets,type SecretInput} from './secrets';
import {fields,locked,serverFields,proxyFields,type Catalog,type Draft,type Mode,type GroupEdit} from './model';
import type {CatalogItem,Proxy} from '@sodeck/sdk';
export class Drafts {
  readonly secrets=new Secrets();
  readonly items=new Map<string,Draft>();
  mode:Mode={kind:'guide'};
  directory={query:'',group:'*',scroll:0,showList:false};
  groupEdit?:GroupEdit;
  revision=0;navigation=0;frozen=false;private listeners=new Set<()=>void>();
  subscribe=(listener:()=>void)=>{this.listeners.add(listener);return()=>{this.listeners.delete(listener);};};
  snapshot=()=>this.revision;
  changed=()=>{this.revision++;for(const fn of this.listeners)fn();};
  dirty(d:Draft):boolean{return JSON.stringify(d.fields)!==d.baseline||this.secrets.has(d.id)||Boolean(d.groupEdit?.name)||Boolean(d.inlineId&&this.items.get(d.inlineId))||locked(d)||d.phase==='credentialError';}
  dirtyCount(){return [...this.items.values()].filter(d=>!d.inlineTarget&&this.dirty(d)).length+(this.groupEdit&&(this.groupEdit.name!==(this.groupEdit.group?.name??'')||this.groupEdit.operationId)?1:0);}
  groupOperations(){return [this.groupEdit,...[...this.items.values()].map(d=>d.groupEdit)].flatMap(g=>g?.operationId?[g.operationId]:[]);}
  startGroup(group?:import('@sodeck/sdk').Group){if(this.frozen)return;if(this.groupEdit){if(this.groupEdit.group?.id===group?.id)return;if(this.groupEdit.operationId||this.groupEdit.name!==(this.groupEdit.group?.name??''))throw Error('请先完成或取消当前分组编辑');}this.groupEdit={name:group?.name??'',group};this.changed();}
  editGroup(g:GroupEdit,name:string){if(this.frozen||g.busy||g.unknown)return;const parent=[...this.items.values()].find(d=>d.groupEdit===g);if(parent)this.canDirty(parent);else if(!g.operationId&&g.name===(g.group?.name??''))this.canDirty();if(this.bytes()+(JSON.stringify(name).length-JSON.stringify(g.name).length)*2>512*1024)throw Error('配置草稿已达容量上限，请先完成其他配置');g.name=name;this.changed();}
  canDirty(d?:Draft){if((!d||!this.dirty(d))&&this.dirtyCount()>=16)throw Error('已有 16 份未完成配置，请先完成其中一份');}
  create(kind:Draft['kind'],catalog:Catalog,options:{server?:CatalogItem;proxy?:Proxy;inlineTarget?:Draft['inlineTarget'];groupId?:string|null}={}):Draft{
    if(this.frozen)throw Error('正在核对关闭，请先完成确认');this.navigation++;
    const same=[...this.items.values()].find(d=>d.kind===kind&&!d.inlineTarget&&(options.server?d.server?.server.id===options.server.server.id:options.proxy?d.proxy?.id===options.proxy.id:!d.server&&!d.proxy&&!this.dirty(d)));
    if(same&&!options.inlineTarget){
      if(!this.dirty(same)&&(options.server||options.proxy)){
        same.server=options.server;same.proxy=options.proxy;
        same.fields=options.server?serverFields(options.server,catalog):proxyFields(options.proxy!);
        same.baseline=JSON.stringify(same.fields);
      }
      this.mode={kind:'draft',id:same.id};this.changed();return same;
    }
    if(options.inlineTarget){
      const parent=this.items.get(options.inlineTarget.parentDraftId);
      if(!parent||parent.inlineId||!parent.fields.steps.some(s=>s.stepId===options.inlineTarget!.targetStepId&&s.kind==='socks5'))throw Error('代理节点已失效，请重新选择节点');
    }
    this.canDirty(options.inlineTarget?this.items.get(options.inlineTarget.parentDraftId):undefined);
    const f=options.server?serverFields(options.server,catalog):options.proxy?proxyFields(options.proxy):{...fields(kind),groupId:options.groupId??null};
    const d:Draft={id:crypto.randomUUID(),kind,fields:f,baseline:JSON.stringify(f),server:options.server,proxy:options.proxy,inlineTarget:options.inlineTarget,phase:'editing',error:'',errors:{},scroll:0,message:''};
    if(this.bytes()+JSON.stringify(d).length*2>512*1024)throw Error('配置草稿已达容量上限，请先关闭其他配置');
    this.items.set(d.id,d);if(options.inlineTarget){this.items.get(options.inlineTarget.parentDraftId)!.inlineId=d.id;}else{this.mode={kind:'draft',id:d.id};}
    this.changed();return d;
  }
  bytes(){return JSON.stringify({items:[...this.items.values()],groupEdit:this.groupEdit}).length*2;}
  edit(id:string,patch:Partial<Draft['fields']>){const d=this.items.get(id);if(!d||locked(d)||this.frozen||d.phase==='credentialError'&&Object.keys(patch).some(key=>key!=='remember'))return;
    if(!d.inlineTarget)this.canDirty(d);
    const next={...d.fields,...patch};const bytes=this.bytes()+(JSON.stringify(next).length-JSON.stringify(d.fields).length)*2;
    if(bytes>512*1024)throw Error('配置草稿已达容量上限，请先完成其他配置');
    if(patch.authType&&patch.authType!==d.fields.authType)this.secrets.clear(id);
    d.fields=next;if(d.phase!=='credentialError')d.phase='editing';d.error='';d.message='';for(const key of Object.keys(patch))delete d.errors[key];this.changed();
  }
  secret(id:string,patch:Partial<SecretInput>){const d=this.items.get(id);if(!d||locked(d)||this.frozen)return;if(!d.inlineTarget)this.canDirty(d);this.secrets.set(id,patch);d.error='';this.changed();}
  remove(id:string){const d=this.items.get(id);if(!d)return;if(d.inlineId)this.remove(d.inlineId);if(d.inlineTarget){const parent=this.items.get(d.inlineTarget.parentDraftId);if(parent?.inlineId===id)delete parent.inlineId;}this.items.delete(id);this.secrets.clear(id);if(this.mode.kind==='draft'&&this.mode.id===id)this.mode={kind:'guide'};this.changed();}
  clear(){this.items.clear();this.secrets.clearAll();this.groupEdit=undefined;this.mode={kind:'guide'};this.changed();}
  select(mode:Mode){if(this.frozen)return;this.navigation++;this.mode=mode;this.changed();}
}
