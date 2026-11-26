import { core, type Message } from './core';
import { DomRegion } from './dom-update';
import { activityContextKey, type ActivityContext } from './activity-presentation';
import { messagesFrame, messageTools, messageStatus, messageRows, messageDetail, visibleMessages } from './messages-views';

export interface MessagesState { messages: Message[]; unread: boolean; selectedId: string; loaded: boolean; reading: boolean; writing: boolean; pendingIds: Set<string>; error: string; writeError: string; feedback: string }
export class MessagesPage {
  readonly state: MessagesState={messages:[],unread:false,selectedId:'',loaded:false,reading:false,writing:false,pendingIds:new Set(),error:'',writeError:'',feedback:''};
  private host?: HTMLElement;
  private region?: DomRegion;
  private events?: AbortController;
  private context: ActivityContext={storageErrors:[]};
  private contextKey='';
  private active=false;
  private disposed=false;
  private revision=0;
  private inFlight=false;
  private readAgain=false;
  private writes: (string|null)[]=[];
  private failedWrite: string|null|undefined;
  private openId?:string;
  private pane:'list'|'detail'='list';
  private listKey='';
  private scroll={list:0,detail:0};
  constructor(private locatePlugin:(id:string)=>void, private changed:()=>void, private api={messages:core.messages,markRead:core.markRead}){}
  mount(host:HTMLElement|null){
    if(this.host===(host??undefined))return;
    if(this.host)this.scroll={list:this.host.querySelector('.message-list')!.scrollTop,detail:this.host.querySelector('.message-detail')!.scrollTop};
    this.setActive(false);this.region?.dispose();this.events?.abort();this.host=host??undefined;this.region=undefined;this.listKey='';
    if(!host||this.disposed)return;
    host.innerHTML=messagesFrame();this.events=new AbortController();this.region=new DomRegion(host);
    host.addEventListener('click',event=>{
      const button=(event.target as Element).closest<HTMLButtonElement>('[data-message-action]');
      if(!button||button.disabled)return;event.stopPropagation();this.action(button.dataset.messageAction!,button.dataset.id);
    },{signal:this.events.signal});
    this.render();host.querySelector('.message-list')!.scrollTop=this.scroll.list;host.querySelector('.message-detail')!.scrollTop=this.scroll.detail;
  }
  setContext(context:ActivityContext){const key=activityContextKey(context);this.context=context;if(key!==this.contextKey){this.contextKey=key;this.render();}}
  setActive(active:boolean){if(this.disposed||active===this.active)return;this.active=active;++this.revision;this.readAgain=false;this.state.reading=false;if(active)void this.refresh();else this.openId=undefined;}
  open(id:string){this.openId=id;this.state.unread=false;this.state.feedback='';if(this.active)void this.refresh();}
  async refresh(){
    if(this.disposed||!this.active||!this.host||this.state.writing)return;
    if(this.inFlight){this.readAgain=true;return;}
    this.inFlight=true;this.state.reading=true;const revision=this.revision;
    try{
      const messages=await this.api.messages();
      if(this.disposed||!this.active||revision!==this.revision)return;
      this.state.messages=messages;this.state.error='';this.state.loaded=true;this.render();
      if(this.openId){const id=this.openId;this.openId=undefined;if(messages.some(message=>message.id===id))this.select(id);else{this.state.selectedId=id;this.state.feedback='此消息已不在保存范围';this.render();}}
    }catch(error){if(!this.disposed&&this.active&&revision===this.revision){this.state.error=`读取失败：${String(error)}${this.state.loaded?' · 保留上次结果':''}`;this.render();}}
    finally{this.inFlight=false;if(revision===this.revision)this.state.reading=false;if(!this.disposed&&this.active&&this.readAgain){this.readAgain=false;void this.refresh();}}
  }
  private queueRead(id:string|null){
    if(id===null){if(this.state.writing)return;this.writes=[null];}
    else if(this.state.pendingIds.has(id)||this.writes.includes(null))return;
    else this.writes.push(id);
    if(id)this.state.pendingIds.add(id);
    this.state.writeError='';this.failedWrite=undefined;++this.revision;this.readAgain=false;void this.write();
  }
  private async write(){
    if(this.state.writing||this.disposed)return;
    this.state.writing=true;this.render();
    while(this.writes.length&&!this.disposed){
      const id=this.writes.shift()!;
      try{await this.api.markRead(id);if(!this.disposed)this.changed();}
      catch(error){if(!this.disposed){this.state.writeError=`已读保存失败：${String(error)}`;this.failedWrite=id;}}
      finally{if(id)this.state.pendingIds.delete(id);}
    }
    this.state.writing=false;
    if(!this.disposed){this.render();if(this.active)void this.refresh();}
  }
  private select(id:string){
    const message=this.state.messages.find(message=>message.id===id);if(!message)return;
    this.state.selectedId=id;this.pane='detail';this.state.feedback='';this.render();
    if(this.host?.querySelector('.activity-back')?.getBoundingClientRect().width)this.host.querySelector<HTMLElement>('[data-detail-focus]')?.focus();
    if(!message.read)this.queueRead(id);
  }
  private action(action:string,id?:string){
    if(action==='select'&&id){this.select(id);return;}
    if(action==='all'||action==='unread'){this.state.unread=action==='unread';this.render();return;}
    if(action==='read-all'){this.queueRead(null);return;}
    if(action==='retry-read'){if(this.failedWrite!==undefined)this.queueRead(this.failedWrite);return;}
    if(action==='retry'){void this.refresh();return;}
    if(action==='back'){this.pane='list';this.render();(this.host?.querySelector<HTMLElement>(`[data-message-action="select"][data-id="${CSS.escape(this.state.selectedId)}"]`)??this.host?.querySelector<HTMLElement>('#messages-all'))?.focus();return;}
    if(action==='plugin'&&id&&this.context.plugins?.some(plugin=>plugin.manifest.id===id))this.locatePlugin(id);
  }
  private part(name:string,html:string){const target=this.host?.querySelector<HTMLElement>(`[data-message-part="${name}"]`);if(target)this.region?.update(target,html,false);}
  private render(){
    if(!this.host||!this.region)return;
    const list=this.host.querySelector<HTMLElement>('.message-list')!,detail=this.host.querySelector<HTMLElement>('.message-detail')!;
    const focused=document.activeElement instanceof HTMLElement?document.activeElement.closest<HTMLElement>('.message-row'):null;
    const focusedId=focused?.dataset.id,oldIds=[...list.querySelectorAll<HTMLElement>('.message-row')].map(row=>row.dataset.id);
    this.part('tools',messageTools(this.state));this.part('status',messageStatus(this.state,this.context));
    const key=JSON.stringify([this.state.messages,this.state.unread,this.state.selectedId,this.contextKey,this.state.loaded,this.state.error]);
    if(key!==this.listKey){this.region.update(list,messageRows(this.state,this.context),false);this.listKey=key;}
    const previous=detail.querySelector('article')?.getAttribute('data-dom-key');
    this.region.update(detail,messageDetail(this.state,this.context),false);
    if(previous!==detail.querySelector('article')?.getAttribute('data-dom-key'))detail.scrollTop=0;
    if(focusedId&&!visibleMessages(this.state).some(message=>message.id===focusedId)){
      const rows=[...list.querySelectorAll<HTMLElement>('.message-row')];
      (rows[Math.min(Math.max(0,oldIds.indexOf(focusedId)),rows.length-1)]??this.host.querySelector<HTMLElement>(this.state.unread?'#messages-unread':'#messages-all'))?.focus({preventScroll:true});
    }
    this.host.querySelector<HTMLElement>('.activity-split')!.dataset.pane=this.pane;
  }
  dispose(){this.mount(null);this.disposed=true;++this.revision;this.writes=[];}
}
