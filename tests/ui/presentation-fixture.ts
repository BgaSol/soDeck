import type { Page } from '@playwright/test';
import { readFileSync } from 'node:fs';
export async function presentationFixture(page: Page) {
  const theme={...JSON.parse(readFileSync('themes/graphite.json','utf8')),revision:1};
  await page.addInitScript(theme=>{
    const menus=new Map<string,any>();
    (window as any).__PRESENTATION__={theme,menu:null,choose(id:string|null){const current=this.menu;if(!current)return;const result={menuId:current.menuId,status:'closed',selectedId:id};menus.set(current.menuId,result);this.menu=null;(window as any).__sodeckNotify({type:'ui.menu.closed',payload:result});},handle(request:any){
      const p=request.params;let result;
      if(request.method==='ui.theme.get')result=theme;
      else if(request.method==='ui.menu.open'){this.menu=p;menus.set(p.menuId,{menuId:p.menuId,status:'open',selectedId:null});result={menuId:p.menuId};}
      else if(request.method==='ui.menu.result')result=menus.get(p.menuId);
      else if(request.method==='ui.menu.close'){this.choose(null);result={closed:true};}
      else return null;
      return {requestId:request.requestId,result};
    }};
  },theme);
}
export async function chooseMenu(page:Page,id:string|null) {
  await page.waitForFunction(()=>(window as any).__PRESENTATION__.menu!==null);
  await page.evaluate(id=>(window as any).__PRESENTATION__.choose(id),id);
}
