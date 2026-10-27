import {automaticName,validateRule,type Rule} from './rules';
export type Draft={type:Rule['type'];name:string;port:string;auto:boolean;autoReconnect:boolean;host:string;targetPort:string};
export const draft=(type:Rule['type']):Draft=>({type,name:'',port:'',auto:true,autoReconnect:false,host:'',targetPort:''});
export const edit=(rule:Rule,automatic=false):Draft=>({type:rule.type,name:automatic?'':rule.name,port:String(rule.port||''),auto:rule.bindMode==='auto',autoReconnect:rule.autoReconnect,host:rule.type==='port-forward'?rule.targetHost:'',targetPort:rule.type==='port-forward'?String(rule.targetPort):''});
export function ruleFromDraft(value:Draft,serverId:string,old?:Rule):Rule {
  if(!value||typeof value!=='object'||Object.keys(value).some(k=>!['type','name','port','auto','autoReconnect','host','targetPort'].includes(k))||typeof value.name!=='string'||typeof value.port!=='string'||typeof value.host!=='string'||typeof value.targetPort!=='string'||typeof value.auto!=='boolean'||typeof value.autoReconnect!=='boolean')throw Error('表单内容无效');
  if((!value.auto&&!/^\d+$/.test(value.port))||(value.type==='port-forward'&&!/^\d+$/.test(value.targetPort)))throw Error('端口必须是 1–65535 的整数');
  const r={id:old?.id??'draft',revision:old?.revision??1,name:value.name.trim(),serverId,type:value.type,bindMode:value.auto?'auto' as const:'fixed' as const,port:value.auto?0:Number(value.port),autoReconnect:value.autoReconnect,...(value.type==='port-forward'?{targetHost:value.host.trim(),targetPort:Number(value.targetPort)}:{})};
  return validateRule({...r,name:r.name||automaticName(r)});
}
