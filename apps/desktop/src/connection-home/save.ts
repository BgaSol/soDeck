import {core,CoreError,errorMessage,type CatalogItem,type CatalogSave,type OperationResult,type ProxySave,type ProxySaved,type ProxyOperationResult,type CredentialPrepared,type CredentialSet,type CredentialStored,type CredentialOperationResult} from '@sodeck/sdk';
import {accessPath,dependencies,generatedName,locked,type Catalog,type Draft,type GroupEdit} from './model';
import type {Group} from '@sodeck/sdk';
import {validate} from './validation';
import {Drafts} from './drafts';
type Request={kind:'ssh';value:CatalogSave}|{kind:'proxy';value:ProxySave};
type Pending={request:Request;started:number;configuration?:CatalogItem|ProxySaved;credential?:CredentialSet;credentialUnknown?:boolean;unknown?:boolean;openId?:string;connect:boolean;current:()=>boolean};
const unknown=(e:unknown)=>['RESULT_UNKNOWN','TIMEOUT','TRANSPORT_ERROR'].includes((e as {code?:string})?.code??'');
export class Saves {
  readonly pending=new Map<string,Pending>();
  private active=new Set<string>();
  constructor(private store:Drafts,private catalog:()=>Catalog,private refresh:()=>Promise<void>,private open:(id:string,operationId:string)=>Promise<void>){}
  operations(){return [...this.pending].flatMap(([id,p])=>{const d=this.store.items.get(id);return d&&(locked(d)||this.active.has(id))?[p.request.value.operationId,...(p.credential?[p.credential.operationId]:[])]:[];}).concat(this.store.groupOperations());}
  async saveGroup(g:GroupEdit){
    if(g.busy||this.store.frozen&&!g.operationId)return;
    g.busy=true;g.error='';g.operationId??=crypto.randomUUID();this.store.changed();
    try{
      const result=await core.call<{group:Group}>('serverGroups.'+(g.group?'update':'create'),{operationId:g.operationId,name:g.name.trim(),...(g.group?{groupId:g.group.id,expectedRevision:g.group.revision}:{})});
      g.operationId=undefined;g.unknown=false;
      const parent=[...this.store.items.values()].find(d=>d.groupEdit===g);
      if(parent){parent.groupEdit=undefined;parent.fields.groupId=result.group.id;}
      else if(this.store.groupEdit===g)this.store.groupEdit=undefined;
      await this.refresh();
    }catch(e){g.unknown=unknown(e);if(!g.unknown)g.operationId=undefined;g.error=errorMessage(e);}
    finally{g.busy=false;this.store.changed();}
  }
  async save(id:string,connect=false,current:()=>boolean=()=>false){
    const d=this.store.items.get(id);if(!d||this.active.has(id)||this.store.frozen&&!this.pending.has(id)||locked(d)&&d.phase!=='reconciling')return;
    let pending=this.pending.get(id);
    if(!pending){
      d.errors=validate(d,this.catalog());
      const secret=this.store.secrets.get(id);
      if(d.kind==='proxy'&&new TextEncoder().encode(secret.password).length>255)d.errors.password='代理密码最多 255 字节';
      if(d.fields.authType==='key'&&secret.passphrase&&!secret.privateKeyPath)d.errors.privateKeyPath='请输入私钥文件路径，或清空口令后连接时再提供';
      if(d.groupEdit?.busy||d.groupEdit?.unknown)d.errors.groupId='请先核对分组保存结果';
      if(Object.keys(d.errors).length){d.error='请检查标出的字段';this.store.changed();return;}
      const f=d.fields;
      const configuration={name:f.name.trim()||generatedName(d),host:f.host.trim(),port:Number(f.port),username:f.authType==='none'?'':f.username.trim(),authType:f.authType};
      const request:Request=d.kind==='ssh'?{kind:'ssh',value:{operationId:crypto.randomUUID(),configuration:{...configuration,authType:f.authType as 'key'|'password',accessPath:accessPath(f)},groupId:f.groupId,expectedDependencies:f.dependencies,...(d.server?{serverId:d.server.server.id,expectedServerRevision:d.server.server.revision,expectedMembershipRevision:d.server.membershipRevision}:{})}}:{kind:'proxy',value:{operationId:crypto.randomUUID(),configuration:{...configuration,authType:f.authType as 'none'|'usernamePassword'},...(d.proxy?{proxyId:d.proxy.id,expectedRevision:d.proxy.revision}:{})}};
      pending={request,started:Date.now(),connect:connect&&!d.server,current};this.pending.set(id,pending);
    }
    this.active.add(id);d.error='';this.store.changed();
    try{
      if(!pending.configuration){d.phase=pending.unknown?'reconciling':'savingConfiguration';this.store.changed();
        if(pending.unknown){
          const result=pending.request.kind==='ssh'?await core.call<OperationResult>('servers.catalog.operation',{operationId:pending.request.value.operationId}):await core.call<ProxyOperationResult>('proxies.catalog.operation',{operationId:pending.request.value.operationId});
          if(result.status==='committed')pending.configuration=result.result as CatalogItem|ProxySaved;
          if(!pending.configuration&&Date.now()-pending.started>=86400000)throw new CoreError({code:'RESULT_UNKNOWN',message:'保存回执已超过核对期限，请从目录核实配置，不能自动重新创建'});
        }
        if(!pending.configuration||'postCommitPending' in pending.configuration&&pending.configuration.postCommitPending){pending.configuration=pending.request.kind==='ssh'?await core.call<CatalogItem>('servers.catalog.save',pending.request.value):await core.call<ProxySaved>('proxies.catalog.save',pending.request.value);}
        pending.unknown=false;
        if('server' in pending.configuration)d.server=pending.configuration;else d.proxy=pending.configuration.proxy;
      }
      const secret=this.store.secrets.get(id);
      if(pending.credential||secret.password||secret.privateKeyPath){
        d.phase='savingCredential';this.store.changed();
        if(!pending.credential){
          const target=d.kind==='ssh'?{kind:'ssh' as const,id:d.server!.server.id}:{kind:'socks5' as const,id:d.proxy!.id};
          const prepared=await core.call<CredentialPrepared>('credentials.configuration.prepare',{target,expectedOwnerRevision:d.kind==='ssh'?d.server!.server.revision:d.proxy!.revision,expectedDependencies:d.kind==='ssh'?d.fields.dependencies:[]});
          pending.credential={operationId:crypto.randomUUID(),contextToken:prepared.contextToken,remember:d.fields.remember,...(d.fields.authType==='key'?{privateKeyPath:secret.privateKeyPath,passphrase:secret.passphrase}:{password:secret.password})};
        }
        let stored:CredentialStored|undefined;
        if(pending.credentialUnknown){
          const result=await core.call<CredentialOperationResult>('credentials.configuration.operation',{operationId:pending.credential.operationId});
          if(result.status==='stored')stored=result.result;
          else if(result.status==='failed'){pending.credential=undefined;pending.credentialUnknown=false;throw new CoreError(result.error);}
          else if(result.status==='running')throw new CoreError({code:'RESULT_UNKNOWN',message:'凭据仍在保存，请继续核对'});
          else if(Date.now()-pending.started>=86400000)throw new CoreError({code:'RESULT_UNKNOWN',message:'凭据保存结果无法自动确认，请核对后重新输入'});
        }
        stored??=await core.call<CredentialStored>('credentials.configuration.set',pending.credential);
        if(d.server)d.server={...d.server,server:{...d.server.server,hasSecret:stored.remembered}};
        if(d.proxy)d.proxy={...d.proxy,hasPreset:stored.remembered};
      }
      this.store.secrets.clear(id);pending.credential=undefined;
      d.baseline=JSON.stringify(d.fields);d.phase='saved';d.message=d.kind==='ssh'?'连接已保存':'代理已保存，可供其他连接使用';d.errors={};this.store.changed();await this.refresh();
      if(d.inlineTarget){
        const parent=this.store.items.get(d.inlineTarget.parentDraftId);
        if(parent?.inlineId===id){
          const step=parent.fields.steps.find(s=>s.stepId===d.inlineTarget!.targetStepId);
          if(step?.kind==='socks5'){
            step.proxyId=d.proxy!.id;
            parent.fields.dependencies=dependencies(parent.fields,{...this.catalog(),proxies:[...this.catalog().proxies.filter(p=>p.proxy.id!==d.proxy!.id),{proxy:d.proxy!,referenceCount:0}]});
            parent.message=d.message;
          }
          this.store.remove(id);
        }
      }else if(pending.connect&&pending.current()){
        pending.openId??=crypto.randomUUID();d.phase='opening';this.store.changed();await this.open(d.server!.server.id,pending.openId);d.phase='saved';
      }else if(d.kind==='proxy'&&pending.current())this.store.select({kind:'proxies'});
      this.pending.delete(id);this.store.changed();
    }catch(e){
      if(unknown(e)){if(d.phase==='savingCredential')pending.credentialUnknown=true;else if(!pending.configuration)pending.unknown=true;d.phase='reconciling';}
      else if(pending.configuration){d.phase='credentialError';pending.credential=undefined;pending.credentialUnknown=false;}
      else {d.phase=(e as {code?:string}).code==='REVISION_CONFLICT'?'conflict':'configurationError';this.pending.delete(id);}
      d.error=(pending.configuration?'配置已保存，后续步骤未完成。':'')+errorMessage(e);this.store.changed();
    }finally{this.active.delete(id);this.store.changed();}
  }
  forget(id:string){this.pending.delete(id);}
}
