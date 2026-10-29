import type {Page} from '@playwright/test';
import {presentationFixture} from './presentation-fixture';
export async function connectionHomeFixture(page:Page){
  await presentationFixture(page);
  await page.addInitScript(()=>{
    const f:any={items:[],groups:[],proxies:[],calls:[],receipts:{},credentials:{},revision:1,home:true,homeOpen:true,session:'page-session',dirty:0,report:null,loseProxy:false,failCredential:false,holdSave:false,feeds:new Map(),nav:1,
      layout:{schemaVersion:4,windowId:'main',windowRole:'main',workspaceIds:[],activeWorkspaceId:null,focusedViewId:null,workspaces:[],views:[],split:'none',splitRatio:.5,dockSlots:[],revision:1,recoveryWarnings:[],pendingOpens:[]}};
    const clone=(v:any)=>JSON.parse(JSON.stringify(v));
    const notify=(type:string,payload:any)=>queueMicrotask(()=>(window as any).__sodeckNotify?.({type,payload}));
    const snapshot=()=>({...clone(f.layout),activeWorkspaceId:f.home?null:f.layout.activeWorkspaceId});
    const changed=()=>{f.revision++;for(const [id,feed] of f.feeds)notify('state.update',{subscriptionId:id,feedEpoch:'catalog',sequence:String(++feed.sequence),snapshot:{scope:{appGeneration:'test'},objectId:'catalog',revision:String(f.revision),value:{catalogRevision:String(f.revision)}}});};
    f.changed=changed;(window as any).__CONNECTION_HOME_TEST__=f;
    (window as any).__TAURI_INTERNALS__={invoke:async(_:string,{request:r}:any)=>{
      f.calls.push(clone({...r,params:r.method==='credentials.configuration.set'?{operationId:r.params.operationId}:r.params}));
      const presentation=(window as any).__PRESENTATION__.handle(r);if(presentation)return presentation;
      const p=r.params;let result:any=null;const failure=(code:string,message:string)=>({requestId:r.requestId,error:{code,message}});
      const paginate=(items:any[])=>{const offset=Number(p.cursor??0),end=offset+(p.limit??100);return {items:items.slice(offset,end),catalogRevision:String(f.revision),nextCursor:end<items.length?String(end):null};};
      switch(r.method){
        case 'capabilities.list':result={capabilities:['servers.catalog.v2','servers.routes.v2','ui.connectionHome.v1']};break;
        case 'app.info':result={version:'fixture'};break;
        case 'workbench.contributions':result={commands:[],views:[]};break;
        case 'workbench.snapshot':result=snapshot();break;
        case 'ui.connectionHome.snapshot':result={workbench:snapshot(),page:{open:f.homeOpen,selected:f.home,generation:'home'},navigationRevision:String(f.nav)};break;
        case 'ui.connectionHome.open':f.home=true;f.homeOpen=true;f.nav++;f.layout.revision++;notify('connectionHome.navigate',{intent:p.intent,generation:'home',navigationRevision:String(f.nav)});result={windowId:'main',generation:'home',navigationRevision:String(f.nav)};break;
        case 'ui.connectionHome.session.attach':result={pageSession:f.session,replacedDirty:false};break;
        case 'ui.connectionHome.session.report':f.report=p;result={accepted:true};break;
        case 'ui.connectionHome.close.request':f.ticket=crypto.randomUUID();notify('connectionHome.closeRequested',{ticket:f.ticket,pageSession:f.session});result={accepted:true};break;
        case 'ui.connectionHome.close.respond':if(p.decision!=='cancel'){f.home=false;f.homeOpen=false;f.nav++;}notify('connectionHome.closeFinished',{ticket:p.ticket,committed:p.decision!=='cancel'});notify('workbench.changed',{revision:++f.layout.revision,windowIds:['main'],workspaceIds:f.layout.workspaceIds,viewIds:[],changeKind:'snapshot'});result={accepted:true};break;
        case 'ui.connectionHome.pickPrivateKey':result={status:'canceled'};break;
        case 'servers.catalog.list':result=paginate(f.items);break;
        case 'serverGroups.list':result={groups:f.groups,catalogRevision:String(f.revision),nextCursor:null};break;
        case 'proxies.catalog.list':result=paginate(f.proxies.map((proxy:any)=>({proxy,referenceCount:f.items.filter((i:any)=>i.accessPath.steps.some((s:any)=>s.kind==='socks5'&&s.proxyId===proxy.id)).length})));break;
        case 'state.open':{const id=crypto.randomUUID();f.feeds.set(id,{sequence:0});result={subscriptionId:id,feedEpoch:'catalog',sequence:'0',snapshot:{scope:{appGeneration:'test'},objectId:'catalog',revision:String(f.revision),value:{catalogRevision:String(f.revision)}}};break;}
        case 'state.close':f.feeds.delete(p.subscriptionId);result={accepted:true};break;
        case 'state.ack':result={accepted:true};break;
        case 'servers.catalog.save':{
          result=f.receipts[p.operationId];if(!result){const {accessPath,...server}=p.configuration;const old=f.items.find((i:any)=>i.server.id===p.serverId);result={server:{...server,id:p.serverId??crypto.randomUUID(),revision:(old?.server.revision??0)+1,hasSecret:false},accessPath,groupId:p.groupId,membershipRevision:'1',postCommitPending:false};if(old)Object.assign(old,result);else f.items.push(result);f.receipts[p.operationId]=clone(result);changed();}
          if(f.holdSave){f.holdSave=false;await new Promise<void>(resolve=>f.releaseSave=resolve);}break;
        }
        case 'proxies.catalog.save':{
          result=f.receipts[p.operationId];if(!result){const old=f.proxies.find((i:any)=>i.id===p.proxyId);const proxy={...p.configuration,id:p.proxyId??crypto.randomUUID(),revision:(old?.revision??0)+1,hasPreset:false};if(old)Object.assign(old,proxy);else f.proxies.push(proxy);changed();result=f.receipts[p.operationId]={proxy,catalogRevision:String(f.revision)};}
          if(f.loseProxy){f.loseProxy=false;return failure('RESULT_UNKNOWN','测试：代理回执丢失');}break;
        }
        case 'servers.catalog.operation':case 'proxies.catalog.operation':result=f.receipts[p.operationId]?{status:'committed',result:f.receipts[p.operationId]}:{status:'unknown'};break;
        case 'credentials.configuration.prepare':result={contextToken:crypto.randomUUID(),remembered:false,expiresInMs:300000};break;
        case 'credentials.configuration.set':if(f.failCredential)return failure('SECRET_STORE_ERROR','测试：凭据存储失败');result={stored:true,remembered:p.remember};f.credentials[p.operationId]=result;break;
        case 'credentials.configuration.operation':result=f.credentials[p.operationId]?{status:'stored',result:f.credentials[p.operationId]}:{status:'unknown'};break;
        case 'serverGroups.create':case 'serverGroups.update':{
          result=f.receipts[p.operationId];if(!result){let group=f.groups.find((g:any)=>g.id===p.groupId);if(group){group.name=p.name;group.revision=String(Number(group.revision)+1);}else{group={id:crypto.randomUUID(),name:p.name,revision:'1',serverCount:0};f.groups.push(group);}changed();result=f.receipts[p.operationId]={group:clone(group),catalogRevision:String(f.revision)};}break;
        }
        case 'workspaces.open':{f.home=false;f.nav++;f.layout.revision++;const exists=f.layout.workspaces.some((w:any)=>w.serverId===p.serverId);if(!exists){f.layout.workspaceIds.push(p.serverId);f.layout.workspaces.push({workspaceId:p.serverId,serverId:p.serverId,generation:'test',ownerWindowId:'main',initializers:[],initializationError:null});}f.layout.activeWorkspaceId=p.serverId;result={workspaceId:p.serverId,ownerWindowId:'main',created:!exists,committed:true,persisted:true};break;}
        case 'workspaces.activate':f.home=false;f.nav++;f.layout.revision++;f.layout.activeWorkspaceId=p.workspaceId;result={workspaceId:p.workspaceId,ownerWindowId:'main',committed:true,persisted:true};break;
        case 'workbench.modal':case 'windows.chrome':case 'workbench.surface':result=null;break;
        default:return failure('UNSUPPORTED_API',`浏览器夹具未实现 ${r.method}`);
      }
      return {requestId:r.requestId,result:clone(result)};
    }};
  });
}
