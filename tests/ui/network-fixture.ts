import {expect,type Page} from '@playwright/test';
import {presentationFixture} from './presentation-fixture';

// Separate public surfaces here; native tests exercise the owned window.
export async function setupNetwork(page:Page,dialog=true){
  await presentationFixture(page);
  await page.addInitScript(dialog=>{
    const f={items:[] as any[],requests:[] as any[],saveError:'',committed:false,clipboard:''};(window as any).__NETWORK_TEST__=f;
    const scope={workspaceId:'server',workspaceGeneration:'g'};
    (window as any).__SODECK_CONTEXT__={viewId:'network-view',...scope,serverId:'server',serverName:'隔离测试主机',...(dialog?{dialog:{sessionId:'dialog',dialogId:'network.rule-editor',title:'创建网络隧道',width:640,height:600,initialState:{editorToken:'editor'}}}:{})};
    const snapshot=(p:any={})=>({items:f.items,total:f.items.length,start:0,runningCount:f.items.filter(i=>i.run?.status==='ready').length,selected:f.items.find(i=>i.rule.id===p.selected)});
    const saved={ruleId:'saved-rule',revision:1,action:'save'};
    (window as any).ipc={postMessage(encoded:string){
      const r=JSON.parse(encoded);f.requests.push(r);const appearance=(window as any).__PRESENTATION__.handle(r);
      if(appearance){queueMicrotask(()=>(window as any).__sodeckReceive(appearance));return;}
      const p=r.params;let result:any={accepted:true},error:any;
      if(r.method==='view.initialize')result=snapshot();
      if(r.method==='view.ready')result={ready:true};
      if(r.method==='views.presentation')result={scope,viewId:'network-view',visible:true,focused:true,revision:1};
      if(r.method==='clipboard.writeText'){f.clipboard=p.text;result={written:true};}
      if(r.method==='plugin.invoke'){
        if(p.command==='network.snapshot')result=snapshot(p.args);
        if(p.command==='network.editor.get')result={automatic:true,running:false,value:{type:'network-proxy',name:'',port:'',auto:true,autoReconnect:false,host:'',targetPort:''}};
        if(p.command==='network.editor.save'){if(f.saveError)error={code:f.saveError,message:'测试：保存回执暂不可用'};else result={...saved,action:p.args.action};}
        if(p.command==='network.editor.result')result=f.committed?{status:'committed',result:saved}:{status:'unknown'};
        if(p.command==='network.diagnostics')result={items:[],total:0};
      }
      queueMicrotask(()=>(window as any).__sodeckReceive({requestId:r.requestId,...(error?{error}:{result})}));
    }};
  },dialog);
  await page.goto('http://127.0.0.1:1434');
  await expect(page.getByText(dialog?'创建网络隧道':'暂无网络隧道',{exact:true})).toBeVisible();
}
