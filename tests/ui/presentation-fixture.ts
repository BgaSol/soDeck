import type { Page } from '@playwright/test';
import { readFileSync } from 'node:fs';
export async function presentationFixture(page: Page) {
  const palettes=Object.fromEntries(['light','dark','dopamine'].map(mode=>[mode,JSON.parse(readFileSync(`themes/${mode==='dopamine'?mode:'soft-'+mode}.json`,'utf8'))]));
  await page.addInitScript(palettes=>{
    const menus=new Map<string,any>(),feeds=new Map<string,{sequence:number}>();let revision=1;
    const f:any={theme:{...palettes.dark,revision:'1',highContrast:false,testOverride:false},menu:null,failure:false,hold:false,legacyPlugins:[],
    preference(){return {mode:this.theme.mode,themeId:this.theme.id,revision:this.theme.revision,highContrast:this.theme.highContrast,testOverride:false,legacyPlugins:this.legacyPlugins};},
    catalog(){return {revision:this.theme.revision,selectedThemeId:this.theme.id,effectiveThemeId:this.theme.id,defaultThemeId:'dopamine',highContrast:this.theme.highContrast,testOverride:false,legacyPlugins:this.legacyPlugins,partialStylePlugins:[],themes:Object.entries(palettes).map(([mode,theme]:any)=>({id:theme.id,label:mode==='dopamine'?'多巴胺':mode==='light'?'浅色':'深色',mode:theme.mode,preview:{...theme,revision:'1',highContrast:false,testOverride:false}}))};},
    snapshot(){return {scope:{appGeneration:'app-test'},objectId:'theme',revision:String(revision),value:this.theme};},
    change(mode:string){this.theme={...palettes[mode],revision:String(++revision),highContrast:false,testOverride:false};for(const [id,feed] of feeds)(window as any).__sodeckNotify({type:'state.update',payload:{subscriptionId:id,feedEpoch:'epoch',sequence:String(++feed.sequence),snapshot:this.snapshot()}});},
    choose(id:string|null){const current=this.menu;if(!current)return;const result={menuId:current.menuId,status:'closed',selectedId:id};menus.set(current.menuId,result);this.menu=null;(window as any).__sodeckNotify({type:'ui.menu.closed',payload:result});},handle(request:any){
      const p=request.params;let result;
      if(request.method==='ui.theme.get')result=this.theme;
      else if(request.method==='ui.theme.preference.get')result=this.preference();
      else if(request.method==='ui.theme.catalog.get')result=this.catalog();
      else if(request.method==='ui.theme.select'||request.method==='ui.theme.choose'){
        if(p.expectedRevision!==this.theme.revision)return {requestId:request.requestId,error:{code:'REVISION_CONFLICT',message:'外观已在其他窗口更改，请重新选择。'}};
        if(this.failure)return {requestId:request.requestId,error:{code:'PERSISTENCE_FAILED',message:'外观未更改：测试写入失败'}};
        const mode=p.themeId?p.themeId.replace('soft-',''):p.mode;if(palettes[mode].id!==this.theme.id)this.change(mode);result=request.method==='ui.theme.choose'?this.catalog():this.preference();
      }
      else if(request.method==='state.open'&&p.contract==='app.theme.v1'){const id=crypto.randomUUID();feeds.set(id,{sequence:0});result={subscriptionId:id,feedEpoch:'epoch',sequence:'0',snapshot:this.snapshot()};}
      else if(['state.ack','state.close'].includes(request.method)&&feeds.has(p.subscriptionId)){if(request.method==='state.close')feeds.delete(p.subscriptionId);result={accepted:true};}
      else if(request.method==='workbench.dock.configure')result={width:p.width??320};
      else if(request.method==='ui.menu.open'){this.menu=p;menus.set(p.menuId,{menuId:p.menuId,status:'open',selectedId:null});result={menuId:p.menuId};}
      else if(request.method==='ui.menu.result')result=menus.get(p.menuId);
      else if(request.method==='ui.menu.close'){this.choose(null);result={closed:true};}
      else return null;
      return {requestId:request.requestId,result};
    }};(window as any).__PRESENTATION__=f;
    // Existing focused feature fixtures keep their workbench model. This adapter supplies
    // the additive connection-home protocol when a fixture has no implementation of it.
    let bridge:any;Object.defineProperty(window,'__TAURI_INTERNALS__',{configurable:true,get:()=>bridge,set(value){
      const original=value.invoke;let selected:boolean|undefined,opened:boolean|undefined,nav=0,revision='1';
      const receipts=new Map<string,any>(),contexts=new Map<string,any>(),catalogFeeds=new Map<string,number>();
      const raw=async(method:string,params:any={})=>original('rpc',{request:{apiVersion:1,requestId:crypto.randomUUID(),method,params}});
      const catalogChanged=()=>{revision=String(Number(revision)+1);for(const [id,sequence] of catalogFeeds){catalogFeeds.set(id,sequence+1);(window as any).__sodeckNotify?.({type:'state.update',payload:{subscriptionId:id,feedEpoch:'fixture-catalog',sequence:String(sequence+1),snapshot:{scope:{appGeneration:'fixture'},objectId:'catalog',revision,value:{catalogRevision:revision}}}});}};
      const item=(server:any)=>({server:{id:server.id,name:server.name,host:server.host,port:server.port,username:server.username,authType:server.authType,revision:server.revision??1,hasSecret:server.hasSecret??false},accessPath:server.accessPath??{kind:'direct'},groupId:server.groupId??null,membershipRevision:server.membershipRevision??'0',postCommitPending:false});
      bridge={...value,invoke:async(command:string,args:any)=>{
        if(command==='application_menu_open'){f.menu={menuId:args.menuId,anchor:args.anchor,items:[{id:'appearance'},{id:'plugins'},{id:'resources'},{id:'calls'},{id:'recall'},{id:'layoutRetry'},{id:'merge'},{id:'exit'}]};menus.set(args.menuId,{menuId:args.menuId,status:'open',selectedId:null});return;}
        const r=args?.request;if(!r||(window as any).__CONNECTION_HOME_TEST__)return original(command,args);
        let response=await original(command,args);const p=r.params??{},m=r.method,result=response?.result;
        const ok=(result:any)=>({requestId:r.requestId,result});
        if(m==='ui.connectionHome.snapshot'&&!result?.workbench){const old=await raw('workbench.snapshot');if(old.error)return old;const w=old.result;selected??=!w.activeWorkspaceId;opened??=selected;return ok({workbench:{...w,activeWorkspaceId:selected?null:w.activeWorkspaceId,focusedViewId:selected?null:w.focusedViewId??null},page:{open:opened,selected,generation:'fixture-home'},navigationRevision:String(nav+(w.revision??0))});}
        if(m==='ui.connectionHome.open'&&!result?.generation){selected=true;opened=true;nav++;(window as any).__sodeckNotify?.({type:'connectionHome.navigate',payload:{intent:p.intent,generation:'fixture-home',navigationRevision:String(nav)}});return ok({windowId:'main',generation:'fixture-home',navigationRevision:String(nav)});}
        if(m==='ui.connectionHome.session.attach'&&!result?.pageSession)return ok({pageSession:'fixture-home-session',replacedDirty:false});
        if(m==='ui.connectionHome.session.report'&&!result?.accepted)return ok({accepted:true});
        if(m==='ui.connectionHome.close.request'){selected=false;opened=false;nav++;return ok({accepted:true});}
        if(m==='servers.catalog.list'&&!result?.items){const old=await raw('servers.list');if(old.error)return {requestId:r.requestId,error:old.error};return ok({items:(old.result?.servers??[]).map(item),nextCursor:null,catalogRevision:revision});}
        if(m==='serverGroups.list'){if(result?.groups){revision=result.catalogRevision;return response;}return ok({groups:[],nextCursor:null,catalogRevision:revision});}
        if(m==='proxies.catalog.list'&&!result?.items)return ok({items:[],nextCursor:null,catalogRevision:revision});
        if(m==='state.open'&&p.contract==='servers.catalog.v1'&&!result?.subscriptionId){const id=crypto.randomUUID();catalogFeeds.set(id,0);return ok({subscriptionId:id,feedEpoch:'fixture-catalog',sequence:'0',snapshot:{scope:{appGeneration:'fixture'},objectId:'catalog',revision,value:{catalogRevision:revision}}});}
        if((m==='state.close'||m==='state.ack')&&catalogFeeds.has(p.subscriptionId)){if(m==='state.close')catalogFeeds.delete(p.subscriptionId);return ok({accepted:true});}
        if(m==='servers.catalog.save'&&!result?.server&&!response?.error){if(receipts.has(p.operationId))return ok(receipts.get(p.operationId));const saved=await raw(p.serverId?'servers.update':'servers.create',{...p.configuration,...(p.serverId?{serverId:p.serverId}:{} )});if(saved.error)return saved;const value=item(saved.result);receipts.set(p.operationId,value);catalogChanged();return ok(value);}
        if(m==='servers.catalog.operation'&&!result?.status)return ok(receipts.has(p.operationId)?{status:'committed',result:receipts.get(p.operationId)}:{status:'unknown'});
        if(m==='credentials.configuration.prepare'&&!result?.contextToken){const id=crypto.randomUUID();contexts.set(id,p);return ok({contextToken:id,remembered:false,expiresInMs:300000});}
        if(m==='credentials.configuration.set'&&!result?.stored){const prepare=contexts.get(p.contextToken);const value=await raw('auth.ssh.set',{serverId:prepare.target.id,password:p.password,privateKeyPath:p.privateKeyPath,passphrase:p.passphrase,remember:p.remember});if(value.error)return value;return ok({stored:true,remembered:p.remember});}
        if(['workspaces.open','workspaces.activate','views.activate'].includes(m)&&!response.error){selected=false;nav++;}
        return response;
      }};
    }});
  },palettes);
}
export async function chooseMenu(page:Page,id:string|null) {
  await page.waitForFunction(()=>(window as any).__PRESENTATION__.menu!==null);
  await page.evaluate(id=>(window as any).__PRESENTATION__.choose(id),id);
}
