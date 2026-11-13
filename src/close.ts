import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { escape as e } from "./ui";

export type CloseTarget = {kind:"views";ids:string[]} | {kind:"workspace";id:string} | {kind:"host";id:string} | {kind:"group";id:string;workspaceId:string} | {kind:"window";id:string} | {kind:"application";action:"exit"|"restart"|"installAndRestart"};
export interface CloseDraft {id:string;title:string;dirty:boolean;busy:boolean;error?:string}
interface Operation {id:string;window:string;target:CloseTarget;phase:string;failedStage?:string;revision:number;inputRound:number;decisionRevision:number;decision?:string;workspaces:string[];views:string[];inputs:Record<string,unknown|null>;dirty:CloseDraft[];error?:string;adoptedBy?:string}
type Call = <T>(command:string,args?:Record<string,unknown>)=>Promise<T>;
export type HostInputs = (decision:"collect"|"save"|"discard",operationId:string)=>Promise<CloseDraft[]>;

/** The browser synchronizes inputs and displays decisions. Core owns execution. */
export class CloseCoordinator {
  private operations = new Map<string,Operation>();
  private reporting = new Map<string,Promise<void>>();
  private reports = new Map<string,Record<string,unknown>>();
  private recovering?: Promise<void>;
  private listeners:UnlistenFn[]=[];
  private dialog?:HTMLDialogElement;
  private dialogOperation?:string;
  private disposed=false;
  private timer?:ReturnType<typeof setInterval>;
  hostInputs:HostInputs=async()=>[];
  constructor(private call:Call,private windowId:()=>string,private flush:(ids:string[])=>Promise<unknown>,private block:(ids:string[],blocked:boolean,host:boolean)=>void,private notify:(text:string)=>void) {}
  async start() {
    await this.call("close_register_host");
    for(const name of ["core-close-inputs","core-close-changed"]) {
      const unlisten=await listen<Operation>(name,event=>this.receive(event.payload));
      if(this.disposed)unlisten();else this.listeners.push(unlisten);
    }
    if(this.disposed)return;
    await this.recover();
    if(this.disposed)return;
    this.timer=setInterval(()=>void this.recover().catch(()=>{}),500);
  }
  private recover() {
    if(this.recovering)return this.recovering;
    this.recovering=(async()=>{
      const operations=await this.call<Operation[]>("close_operations");
      if(this.disposed)return;
      const present=new Set(operations.map(o=>o.id));
      for(const [id,old] of this.operations)if(!present.has(id)&&!["completed","aborted"].includes(old.phase)) {
        try {
          const current=await this.call<Operation>("close_status",{operationId:id});this.receive(current);continue;
        } catch(error) {if(!String(error).includes("失效"))continue;}
        this.operations.delete(id);
        if(this.dialogOperation===id){this.dialog?.remove();this.dialog=undefined;this.dialogOperation=undefined;}
      }
      for(const op of operations)this.receive(op);
      this.updateBlock();
    })().finally(()=>{this.recovering=undefined;});
    return this.recovering;
  }
  private updateBlock() {
    const active=[...this.operations.values()].filter(o=>!["completed","aborted"].includes(o.phase)&&!o.adoptedBy);
    this.block([...new Set(active.flatMap(o=>o.views))],active.length>0,active.some(o=>(o.phase!=="preparing"||this.reports.has(`${o.id}/${o.inputRound}`))&&(o.target.kind==="application"||o.target.kind==="window")));
  }
  private receive(op:Operation) {
    if(this.disposed||(!(this.windowId() in op.inputs)&&op.window!==this.windowId()))return;
    if((this.operations.get(op.id)?.revision??0)>op.revision)return;
    this.operations.set(op.id,op);
    const reportKey=`${op.id}/${op.inputRound}`;
    for(const key of this.reporting.keys())if(key.startsWith(`${op.id}/`)&&key!==reportKey)this.reporting.delete(key);
    for(const key of this.reports.keys())if(key.startsWith(`${op.id}/`)&&key!==reportKey)this.reports.delete(key);
    const terminal=["completed","aborted"].includes(op.phase)||Boolean(op.adoptedBy);
    this.updateBlock();
    if(terminal) {
      if(this.dialogOperation===op.id){this.dialog?.remove();this.dialog=undefined;this.dialogOperation=undefined;}
      for(const key of this.reporting.keys())if(key.startsWith(`${op.id}/`))this.reporting.delete(key);
      for(const key of this.reports.keys())if(key.startsWith(`${op.id}/`))this.reports.delete(key);
      if(this.operations.size>128)for(const [id,old] of this.operations)if(["completed","aborted"].includes(old.phase)&&id!==op.id){this.operations.delete(id);break;}
      return;
    }
    if(op.inputs[this.windowId()]===null && (op.phase==="preparing"||op.decision))void this.report(op);
    if(op.window===this.windowId() && op.phase!=="preparing")this.paint(op);
  }
  private report(op:Operation) {
    const key=`${op.id}/${op.inputRound}`;
    if(this.reporting.has(key))return this.reporting.get(key)!;
    const job=(async()=>{
      if(!this.reports.has(key)) {
      let error:string|undefined,drafts:CloseDraft[]=[],surfaces:unknown=[];
      try {
        if(!op.decision)surfaces=await this.flush(op.views);
        if(op.target.kind==="application"||op.target.kind==="window")drafts=await this.hostInputs((op.decision??"collect") as "collect"|"save"|"discard",op.id);
      } catch(reason){error=String(reason);}
      const current=this.operations.get(op.id);
      if(this.disposed || current?.inputRound!==op.inputRound || ["completed","aborted"].includes(current?.phase??"") || current?.adoptedBy)return;
      this.reports.set(key,{drafts,error,surfaces});
      this.updateBlock();
      }
      const latest=await this.call<Operation>("close_report_inputs",{operationId:op.id,inputRound:op.inputRound,report:this.reports.get(key)});
      this.receive(latest);
    })().catch(error=>{this.reporting.delete(key);if(!this.disposed)this.notify(String(error));});
    this.reporting.set(key,job);
    return job;
  }
  async request(target:CloseTarget):Promise<boolean> {
    const op=await this.call<Operation>("close_prepare",{requestId:crypto.randomUUID(),target});this.receive(op);
    while(!this.disposed) {
      const current=await this.call<Operation>("close_status",{operationId:op.id});this.receive(current);
      if(current.phase==="completed")return true;
      if(current.phase==="aborted")return false;
      if(current.adoptedBy)return false;
      await new Promise(resolve=>setTimeout(resolve,200));
    }
    return false;
  }
  private paint(op:Operation) {
    if(this.dialogOperation && this.dialogOperation!==op.id)return;
    if(!this.dialog) {
      this.dialog=document.createElement("dialog");this.dialog.className="component-close-dialog";this.dialogOperation=op.id;
      this.dialog.addEventListener("cancel",event=>{event.preventDefault();const current=this.operations.get(op.id)!;if(["preparing","deciding"].includes(current.phase))void this.decide(current,"cancel");});
      this.dialog.addEventListener("click",event=>{
        const action=(event.target as Element).closest<HTMLElement>("[data-decision]")?.dataset.decision;
        const current=this.operations.get(op.id)!;
        if(action==="retry")void this.call<Operation>("close_retry",{operationId:op.id}).then(o=>this.receive(o)).catch(e=>this.notify(String(e)));
        else if(action==="exit")void (this.windowId()!=="main"?this.call("request_main_action",{action:"exit"}):current.target.kind==="application"?this.call<Operation>("close_exit_after_failure",{operationId:op.id}).then(o=>this.receive(o)):this.request({kind:"application",action:"exit"})).catch(e=>this.notify(String(e)));
        else if(action)void this.decide(current,action);
      });
      document.body.append(this.dialog);this.dialog.showModal();
    }
    const renderKey=JSON.stringify([op.phase,op.failedStage,op.decision,op.decisionRevision,op.error,op.dirty]);
    if(this.dialog.dataset.renderKey===renderKey)return;
    this.dialog.dataset.renderKey=renderKey;
    const focus=(document.activeElement as HTMLElement)?.dataset.decision;
    const labels:Record<string,string>={preparing:"正在同步各窗口的输入…",deciding:"请确认未保存内容和运行任务。",cleaning:"正在停止组件并等待资源释放…",finalizing:"正在完成窗口或应用操作…",failed:"关闭尚未完成，已停止的资源将保持停止。"};
    const title = op.target.kind === "application" ? "关闭应用" : op.target.kind === "window" ? "关闭窗口" : op.target.kind === "host" ? "关闭服务器标签" : op.target.kind === "workspace" ? "关闭工作区" : "关闭组件页面";
    this.dialog.innerHTML=`<h2>${title}</h2><p role="status">${labels[op.phase]??"正在关闭…"}</p>${op.error?`<p class="error-box" role="alert">${e(op.error)}</p>`:""}<ul>${op.dirty.map(v=>`<li>${e(v.title)}${v.busy?" · 任务运行中":""}${v.error?` · ${e(v.error)}`:""}</li>`).join("")}</ul><footer>${op.phase==="preparing"||op.phase==="deciding"?`<button data-decision="cancel">取消</button>${op.phase==="deciding"&&!op.decision?`<button data-decision="discard">放弃并关闭</button><button class="primary" data-decision="save" ${op.error?"disabled":""}>保存并关闭</button>`:""}`:op.phase==="failed"?`<button data-decision="retry">继续完成关闭</button>${op.target.kind!=="application"||op.failedStage==="finalizing"?`<button data-decision="exit">退出应用</button>`:""}`:""}</footer>`;
    if(op.phase==="deciding"&&!op.decision&&!op.error&&!op.dirty.length)void this.decide(op,"discard");
    if(focus)this.dialog.querySelector<HTMLButtonElement>(`[data-decision="${CSS.escape(focus)}"]`)?.focus();
  }
  private async decide(op:Operation,decision:string) {
    try {this.receive(await this.call<Operation>("close_decide",{operationId:op.id,decisionRevision:op.decisionRevision,decision}));}
    catch(error){this.notify(String(error));}
  }
  dispose(){this.disposed=true;clearInterval(this.timer);this.listeners.forEach(fn=>fn());this.dialog?.remove();this.reporting.clear();this.reports.clear();this.operations.clear();}
}
