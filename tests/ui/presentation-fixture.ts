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
    let bridge:any;Object.defineProperty(window,'__TAURI_INTERNALS__',{configurable:true,get:()=>bridge,set(value){const original=value.invoke;bridge={...value,invoke:async(command:string,args:any)=>{if(command==='application_menu_open'){f.menu={menuId:args.menuId,anchor:args.anchor,items:[{id:'appearance'},{id:'plugins'},{id:'resources'},{id:'calls'},{id:'recall'},{id:'layoutRetry'},{id:'merge'},{id:'exit'}]};menus.set(args.menuId,{menuId:args.menuId,status:'open',selectedId:null});return;}return original(command,args);}};}});
  },palettes);
}
export async function chooseMenu(page:Page,id:string|null) {
  await page.waitForFunction(()=>(window as any).__PRESENTATION__.menu!==null);
  await page.evaluate(id=>(window as any).__PRESENTATION__.choose(id),id);
}
