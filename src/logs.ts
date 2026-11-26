import { core, type Filter, type Log } from './core';
import { DomRegion } from './dom-update';
import { parseLocalTime } from './datetime';
import { activityContextKey, type ActivityContext } from './activity-presentation';
import { logsFrame, logsHeading, logsTools, logsStatus, logRows, logDetail } from './logs-views';

export interface LogsState {
  filter: Filter; resultFilter?: Filter; sinceInput: string; untilInput: string;
  records: Log[]; selectedId: string; selectedSnapshot?: Log;
  paused: boolean; reading: boolean; loaded: boolean; error: string; validation: string; feedback: string;
  following: boolean; rangeMoved: boolean;
}
interface LogAnchor { id: string; offset: number; top: number; time: number; follow: boolean }
export class LogsPage {
  readonly state: LogsState = {filter:{pluginId:'',level:'',query:''},sinceInput:'',untilInput:'',records:[],selectedId:'',paused:false,reading:false,loaded:false,error:'',validation:'',feedback:'',following:true,rangeMoved:false};
  private host?: HTMLElement;
  private region?: DomRegion;
  private events?: AbortController;
  private context: ActivityContext = {storageErrors:[]};
  private contextKey = '';
  private active = false;
  private disposed = false;
  private revision = 0;
  private inFlight = false;
  private readAgain = false;
  private composing = false;
  private queryTimer?: ReturnType<typeof setTimeout>;
  private listKey = '';
  private anchor?: LogAnchor;
  private pane: 'list'|'detail' = 'list';
  constructor(private api = {logs:core.logs,copy:(text:string)=>navigator.clipboard.writeText(text)}) {}
  mount(host: HTMLElement | null) {
    if (this.host === (host ?? undefined)) return;
    this.anchor = this.captureAnchor() ?? this.anchor;
    this.setActive(false); this.region?.dispose(); this.events?.abort(); this.host = host ?? undefined;
    this.region = undefined; this.listKey = ''; this.composing = false;
    clearTimeout(this.queryTimer); this.queryTimer = undefined;
    if (!host || this.disposed) return;
    host.innerHTML = logsFrame(); this.events = new AbortController();
    this.region = new DomRegion(host,()=>this.restoreAnchor());
    const signal = this.events.signal;
    host.addEventListener('click',event=>{
      const button=(event.target as Element).closest<HTMLButtonElement>('[data-log-action]');
      if (!button || button.disabled) return;
      event.stopPropagation(); void this.action(button.dataset.logAction!,button.dataset.id);
    },{signal});
    host.addEventListener('input',event=>{
      const input=event.target as HTMLInputElement;
      if(input.id!=='log-query')return;
      this.state.filter={...this.state.filter,query:input.value};this.invalidate();
      if (!this.composing) this.debounce();
    },{signal});
    host.addEventListener('compositionstart',event=>{if((event.target as Element).id==='log-query'){this.composing=true;this.invalidate();clearTimeout(this.queryTimer);this.queryTimer=undefined;}},{signal});
    host.addEventListener('compositionend',event=>{if((event.target as Element).id==='log-query'){this.composing=false;this.state.filter={...this.state.filter,query:(event.target as HTMLInputElement).value};this.debounce();}},{signal});
    host.addEventListener('change',event=>this.change(event.target as HTMLInputElement),{signal});
    host.querySelector('#log-results')!.addEventListener('scroll',()=>{
      const list=this.results!; this.state.following=list.scrollHeight-list.clientHeight-list.scrollTop<=12;
      this.anchor=this.captureAnchor();this.renderStatus();
    },{signal,passive:true});
    this.render();
  }
  setContext(context: ActivityContext) { const key=activityContextKey(context);this.context=context;if(key!==this.contextKey){this.contextKey=key;this.render();} }
  setActive(active: boolean) {
    if(this.disposed || active===this.active)return;
    this.active=active;this.invalidate();
    if(active)void this.refresh();
  }
  openSource(pluginId: string) { this.invalidate();clearTimeout(this.queryTimer);this.queryTimer=undefined;this.state.filter={...this.state.filter,pluginId};this.state.paused=false;this.state.feedback='';this.render();void this.refresh(); }
  private invalidate(){++this.revision;this.readAgain=false;this.state.reading=false;}
  private debounce(){clearTimeout(this.queryTimer);this.queryTimer=setTimeout(()=>{this.queryTimer=undefined;void this.refresh();},250);this.renderStatus();}
  private change(input: HTMLInputElement) {
    if(input.id==='log-source')this.state.filter={...this.state.filter,pluginId:input.value};
    else if(input.id==='log-level')this.state.filter={...this.state.filter,level:input.value};
    else if(input.id==='log-since'||input.id==='log-until'){
      const since=input.id==='log-since';this.state[since?'sinceInput':'untilInput']=input.value;
      this.state.filter={...this.state.filter,[since?'since':'until']:parseLocalTime(input.value)};
    }else return;
    this.invalidate();this.validate();this.render();void this.refresh();
  }
  private validate() {
    const {filter,sinceInput,untilInput}=this.state;
    this.state.validation=(sinceInput&&filter.since===undefined)||(untilInput&&filter.until===undefined)?'请输入有效的本地日期与时间':
      (filter.since!==undefined&&filter.since<0)||(filter.until!==undefined&&filter.until<0)?'时间不能早于 1970 年 UTC 起点':
      filter.since!==undefined&&filter.until!==undefined&&filter.since>filter.until?'开始时间不能晚于结束时间':'';
  }
  async refresh() {
    if(this.disposed||!this.active||!this.host||this.state.paused||this.composing||this.queryTimer||this.state.validation)return;
    if(this.inFlight){this.readAgain=true;return;}
    this.inFlight=true;this.state.reading=true;this.renderStatus();
    const revision=this.revision,filter={...this.state.filter};
    try{
      const records=await this.api.logs(filter);
      if(this.disposed||!this.active||revision!==this.revision)return;
      const changedFilter=this.state.resultFilter&&JSON.stringify(filter)!==JSON.stringify(this.state.resultFilter);
      const selected=this.state.records.find(record=>record.id===this.state.selectedId)??this.state.selectedSnapshot;
      if(changedFilter){this.state.selectedId='';this.state.selectedSnapshot=undefined;this.pane='list';this.anchor=undefined;this.state.following=true;}
      else this.state.selectedSnapshot=selected&&!records.some(record=>record.id===selected.id)?selected:undefined;
      this.state.records=records;this.state.resultFilter=filter;this.state.error='';this.state.loaded=true;
      this.render();
    }catch(error){if(!this.disposed&&this.active&&revision===this.revision){this.state.error=`读取失败：${String(error)}${this.state.loaded?' · 显示上次结果':''}`;this.render();}}
    finally{
      this.inFlight=false;if(revision===this.revision)this.state.reading=false;
      if(!this.disposed&&this.active){this.renderStatus();if(this.readAgain){this.readAgain=false;void this.refresh();}}
    }
  }
  private get results(){return this.host?.querySelector<HTMLElement>('#log-results');}
  private captureAnchor(): LogAnchor|undefined {
    const list=this.results;if(!list)return;
    const bounds=list.getBoundingClientRect();
    const row=[...list.querySelectorAll<HTMLElement>('[data-log-id]')].find(row=>row.getBoundingClientRect().bottom>bounds.top+30);
    if(!row)return;
    const id=row.dataset.logId!,record=this.state.records.find(record=>record.id===id);
    const selection=getSelection();const selecting=!!selection&&!selection.isCollapsed&&!!selection.anchorNode&&list.contains(selection.anchorNode);
    return {id,offset:row.getBoundingClientRect().top-bounds.top,top:list.scrollTop,time:record?.receivedAt??0,follow:this.state.following&&!selecting};
  }
  private restoreAnchor(){
    const list=this.results;if(!list)return;
    if(!this.anchor||this.anchor.follow){list.scrollTop=list.scrollHeight;return;}
    let row=list.querySelector<HTMLElement>(`[data-log-id="${CSS.escape(this.anchor.id)}"]`);
    if(!row&&this.state.records.length){const record=this.state.records.find(record=>record.receivedAt>=this.anchor!.time)??this.state.records[0];row=list.querySelector(`[data-log-id="${CSS.escape(record.id)}"]`);this.state.rangeMoved=true;}
    if(row)list.scrollTop+=row.getBoundingClientRect().top-list.getBoundingClientRect().top-this.anchor.offset;
    else list.scrollTop=this.anchor.top;
  }
  private part(name:string,html:string){const target=this.host?.querySelector<HTMLElement>(`[data-log-part="${name}"]`);if(target)this.region?.update(target,html,false);}
  private renderStatus(){this.part('status',logsStatus(this.state,this.context));}
  private render(){
    if(!this.host||!this.region)return;
    this.part('heading',logsHeading(this.state));if(!this.composing)this.part('tools',logsTools(this.state,this.context));this.renderStatus();
    const key=JSON.stringify([this.state.records.map(record=>record.id),this.state.selectedId,this.contextKey,this.state.loaded,!this.state.records.length&&this.state.error]);
    if(key!==this.listKey){this.anchor=this.captureAnchor()??this.anchor;this.region.update(this.results!,logRows(this.state.records,this.state.selectedId,this.context,this.state.loaded,this.state.error),false);this.listKey=key;this.restoreAnchor();}
    this.region.update(this.host.querySelector('.log-detail')!,logDetail(this.state,this.context),false);
    this.host.querySelector<HTMLElement>('.activity-split')!.dataset.pane=this.pane;
  }
  private async action(action:string,id?:string){
    const s=this.state;s.feedback='';
    if(action==='select'&&id){s.selectedId=id;s.selectedSnapshot=undefined;this.pane='detail';this.render();if(this.host!.querySelector('.activity-back')!.getBoundingClientRect().width)this.host!.querySelector<HTMLElement>('[data-detail-focus]')?.focus();return;}
    if(action==='back'){this.pane='list';this.render();this.host?.querySelector<HTMLElement>(`[data-log-action="select"][data-id="${CSS.escape(s.selectedId)}"]`)?.focus();return;}
    if(action==='pause'){this.invalidate();s.paused=!s.paused;this.render();if(!s.paused)void this.refresh();return;}
    if(action==='clear'){this.invalidate();clearTimeout(this.queryTimer);this.queryTimer=undefined;s.filter={pluginId:'',level:'',query:''};s.sinceInput=s.untilInput=s.validation='';this.render();this.host?.querySelector<HTMLElement>('#clear-log-filters')?.focus();void this.refresh();return;}
    if(action==='latest'){s.following=true;s.rangeMoved=false;this.anchor=undefined;this.restoreAnchor();this.renderStatus();return;}
    if(action==='retry'){void this.refresh();return;}
    const records=action==='copy-one'?[s.records.find(record=>record.id===s.selectedId)??s.selectedSnapshot].filter((record):record is Log=>!!record):s.records;
    if(action==='copy'||action==='copy-one'){
      const revision=this.revision;
      try{await this.api.copy(records.map(record=>JSON.stringify(record)).join('\n'));if(revision===this.revision)s.feedback='已复制';}
      catch(error){if(revision===this.revision)s.feedback=`复制失败：${String(error)}，可重试复制。`;}
      if(!this.disposed&&this.active)this.renderStatus();
    }
  }
  dispose(){this.mount(null);this.disposed=true;this.invalidate();}
}
