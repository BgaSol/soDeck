import type {ObservationClient} from './observation';
import {networkOperation} from '@sodeck/sdk/network-client';
import {Rules,requestHash,type Rule} from './rules';
import {draft,edit,ruleFromDraft,type Draft} from './editor-model';
import {fault} from './protocol/address';
import type {Manager} from './manager';
type Action='save'|'start'|'restart';
type Saved={ruleId:string;revision:number;action:Action};
interface Session {token:string;viewId:string;generation:string;serverId:string;requestId:string;requestHash:string;original?:Rule;automatic:boolean;type:Rule['type'];submission?:{id:string;hash:string;action:Action};result?:Saved}
export class Editors {
  private sessions=new Map<string,Session>();
  constructor(private rules:Rules){}
  open(m:Manager,p:Record<string,unknown>){
    const requestId=String(p.requestId??'');if(!requestId||requestId.length>128)throw fault('INVALID_PARAMS','缺少编辑操作标识');
    const hash=requestHash([p.id??null,p.type??'network-proxy']);
    const existing=[...this.sessions.values()].find(s=>s.viewId===m.viewId&&s.requestId===requestId);if(existing){if(existing.requestHash!==hash)throw fault('OPERATION_CONFLICT','编辑标识已用于另一条规则');return {editorToken:existing.token};}
    // Each view owns one editor. A remounted main surface can replace its stale draft.
    this.closeView(m.viewId);
    if(this.sessions.size>=16)throw fault('CAPACITY','编辑会话已满');
    const original=typeof p.id==='string'?this.rules.get(m.serverId,p.id):undefined;
    const type=original?.type??p.type??'network-proxy';if(type!=='network-proxy'&&type!=='port-forward')throw fault('INVALID_PARAMS','用途无效');
    const token=networkOperation().id;
    this.sessions.set(token,{token,viewId:m.viewId,generation:m.scope.workspaceGeneration,serverId:m.serverId,requestId,requestHash:hash,original,automatic:original?this.rules.automatic(original.id):true,type});
    return {editorToken:token};
  }
  private session(m:Manager,token:unknown){const s=this.sessions.get(String(token));if(!s||s.viewId!==m.viewId||s.generation!==m.scope.workspaceGeneration||m.closed)throw fault('EDITOR_EXPIRED','编辑会话已结束');return s;}
  get(m:Manager,token:unknown){const s=this.session(m,token);return {original:s.original,automatic:s.automatic,value:s.original?edit(s.original,s.automatic):draft(s.type),running:!!s.original&&!!m.runs.get(s.original.id)&&!['stopped','failed'].includes(m.runs.get(s.original.id)!.status)};}
  async save(m:Manager,p:Record<string,unknown>,diagnostic?:ObservationClient){
    const s=this.session(m,p.editorToken),action=p.action as Action;if(!['save','start','restart'].includes(action)||typeof p.submissionId!=='string'||!p.submissionId||p.submissionId.length>128)throw fault('INVALID_PARAMS','提交操作无效');
    const input=p.draft as Draft,rule=ruleFromDraft(input,m.serverId,s.original);if(!s.original)rule.id='';
    const nameMode=input.name.trim()?'manual':'auto';
    const hash=requestHash([m.serverId,s.original?.revision??0,rule,nameMode,action]);
    if(s.submission&&(s.submission.id!==p.submissionId||s.submission.hash!==hash))throw fault('OPERATION_CONFLICT','保存结果待核实，请先完成当前提交');
    s.submission={id:p.submissionId,hash,action};
    try{
      const r=await this.rules.saveDraft(m.serverId,rule,s.original?.revision??0,nameMode,p.submissionId,hash,diagnostic);
      s.result={ruleId:r.ruleId,revision:r.revision,action};return s.result;
    }catch(error){if((error as {code?:string}).code!=='RESULT_UNKNOWN')s.submission=undefined;throw error;}
  }
  async result(m:Manager,token:unknown){const s=this.session(m,token);if(s.result)return {status:'committed',result:s.result};if(!s.submission)return {status:'unknown'};const r=await this.rules.result(s.submission.id);if(!r)return {status:'unknown'};s.result={ruleId:r.ruleId,revision:r.revision,action:s.submission.action};return {status:'committed',result:s.result};}
  close(m:Manager,token:unknown){const s=this.session(m,token);this.sessions.delete(s.token);return {closed:true};}
  closeView(viewId:string){for(const [token,s] of this.sessions)if(s.viewId===viewId)this.sessions.delete(token);}
}
