use super::{descriptor,model,schema::Partition,Observability};
use crate::{Caller,Core,CoreError,Result};
use serde_json::{json,Value};
use std::{cell::RefCell,collections::HashMap};
use rusqlite::{params,OptionalExtension};
thread_local!{static CURRENT:RefCell<Option<Value>>=const{RefCell::new(None)};}
/// Explicit capture is required before dispatching work to another thread.
pub struct DiagnosticScope(Option<Value>);
impl DiagnosticScope {
    pub fn capture()->Option<Value>{CURRENT.with(|v|v.borrow().clone())}
    pub fn resume(value:Option<Value>)->Self{Self(CURRENT.with(|v|v.replace(value)))}
}
impl Drop for DiagnosticScope{fn drop(&mut self){CURRENT.with(|v|v.replace(self.0.take()));}}
#[derive(Default)]pub(super) struct Contexts {pub active:HashMap<String,Value>}
pub(super) fn attach(event:&mut Value,context:&Value){for key in ["traceId","spanId","parentSpanId","actionId","initiator","stageKey"]{if let Some(v)=context.get(key){event[key]=v.clone();}}if let Some(targets)=context["targets"].as_array(){for target in targets{if !event["targets"].as_array().unwrap().contains(target){if event["targets"].as_array().unwrap().len()<8{event["targets"].as_array_mut().unwrap().push(target.clone());}else if !event["completeness"].as_array().unwrap().contains(&json!("targets-truncated")){event["completeness"].as_array_mut().unwrap().push(json!("targets-truncated"));}}}}if context["associationMissing"]==true{event["completeness"].as_array_mut().unwrap().push(json!("associationMissing"));}}
impl Core {
    /// This is a host entry point, deliberately absent from plugin RPC.
    pub fn host_diagnostic_action(&self,command:&str,user:bool,operation:impl FnOnce()->Result<Value>)->Result<Value>{
        if !descriptor::identifier(command){return operation();}
        let caller=Caller::shell();let definition:descriptor::Definition=serde_json::from_value(json!({"type":"core.command.execute","typeVersion":1,"kind":"action","fields":{"commandId":{"type":"string","classification":"safe-id","maxLength":128}}})).unwrap();
        let request=json!({"clientOperationId":crate::model::id(),"fields":{"commandId":command},"trigger":if user{"user"}else{"system"},"hostVerified":true});
        let Ok(context)=self.observability.begin_span(&caller,&request,definition,None)else{return operation()};
        let resolved=self.observability.resolve_context(&caller,context["contextRef"].as_str().unwrap(),true).ok();
        let scope=DiagnosticScope::resume(resolved);let result=operation();drop(scope);
        let end=json!({"contextRef":context["contextRef"],"outcome":if result.is_ok(){"succeeded"}else{"failed"},"fields":{}});let _=self.observability.trace_end(&caller,&end);result
    }
    pub fn diagnostic_request(&self,caller:&Caller,method:&str,params:&Value,metadata:Option<&Value>,operation:impl FnOnce()->Result<Value>)->Result<Value>{
        let called=std::cell::Cell::new(false);
        let result=self.diagnostic_metadata(caller,metadata,||{called.set(true);operation()});
        if called.get(){result}else{self.record(caller,method,params,||result)}
    }
    /// Host-only transfer after the destination plugin instance has been verified.
    pub fn transfer_diagnostic_context(&self,worker:&Caller)->Result<Option<Value>>{
        let Some(mut context)=DiagnosticScope::capture().filter(|v|v["spanId"].is_string())else{return Ok(None)};
        if !context["pluginId"].is_null()&&(context["instanceId"]!=worker.instance_id||context["pluginId"]!=json!(worker.plugin_id)){return Err(CoreError::new("INVALID_CONTEXT","诊断上下文不能跨插件转交"));}
        let mut decoded=model::unseal(&self.observability.signing_key,context["contextRef"].as_str().unwrap())?;
        decoded["instanceId"]=json!(worker.instance_id);decoded["pluginId"]=json!(worker.plugin_id);
        decoded["viewId"]=json!(worker.view_id);decoded["mountGeneration"]=json!(worker.mount_generation);
        context["contextRef"]=json!(model::seal(&self.observability.signing_key,&decoded));
        Ok(Some(json!({"diagnostics":{"version":1,"contextRef":context["contextRef"]}})))
    }
    pub fn diagnostic_metadata(&self,caller:&Caller,metadata:Option<&Value>,operation:impl FnOnce()->Result<Value>)->Result<Value>{
        let context=metadata.map(|m|{
            if m.to_string().len()>1024{return Err(CoreError::new("INVALID_REQUEST","诊断元数据超过1KiB"));}
            static SCHEMA:std::sync::OnceLock<Value>=std::sync::OnceLock::new();
            let schema=SCHEMA.get_or_init(||serde_json::from_str(include_str!("../../../../contracts/diagnostic-context-v1.schema.json")).unwrap());
            crate::contract::check(schema,schema,m,"metadata").map_err(|_|CoreError::new("INVALID_REQUEST","诊断元数据无效"))?;
            self.observability.resolve_context(caller,m["diagnostics"]["contextRef"].as_str().unwrap(),true)
        }).transpose()?;
        let _scope=DiagnosticScope::resume(context);operation()
    }
    pub fn current_diagnostic_metadata(&self)->Option<Value>{DiagnosticScope::capture().and_then(|v|v["contextRef"].as_str().map(|s|json!({"diagnostics":{"version":1,"contextRef":s}})))}
}
impl Observability {
    pub(super) fn resolve_context(&self,caller:&Caller,token:&str,allow_expired:bool)->Result<Value>{
        self.expire_contexts();
        let value=model::unseal(&self.signing_key,token).map_err(|_|CoreError::new("INVALID_CONTEXT","诊断上下文校验失败"))?;
        if value["instanceId"]!=caller.instance_id||value["pluginId"]!=json!(caller.plugin_id){return Err(CoreError::new("INVALID_CONTEXT","诊断上下文不属于当前实例"));}
        if value["viewId"]!=json!(caller.view_id){return Err(CoreError::new("INVALID_CONTEXT","诊断上下文不属于当前端点"));}
        if value["mountGeneration"]!=json!(caller.mount_generation){if allow_expired{return Ok(json!({"associationMissing":true}));}return Err(CoreError::new("STALE_CONTEXT","诊断上下文挂载已经结束"));}
        let now=crate::model::now_ms();let span=value["spanId"].as_str().unwrap_or_default();
        let mut registry=self.contexts.lock().unwrap();
        if value["bootId"]!=self.boot_id||registry.active.get(span).is_none_or(|v|(v["surfaceEnded"]==true&&value["viewId"].is_string())||v["expiresAt"].as_u64().unwrap_or(0)<=now||v["touchedAt"].as_u64().unwrap_or(0)+600000<=now){if allow_expired{return Ok(json!({"associationMissing":true}));}return Err(CoreError::new("STALE_CONTEXT","诊断上下文已过期"));}
        let stored=registry.active.get_mut(span).unwrap();stored["touchedAt"]=json!(now);let mut resolved=stored.clone();resolved["contextRef"]=json!(token);Ok(resolved)
    }
    pub(super) fn trace_begin(&self,caller:&Caller,p:&Value)->Result<Value>{
        for target in p["targets"].as_array().into_iter().flatten(){super::targets::validate(&self.signing_key,target)?;}
        let definition=self.definition(caller,"actions",p["actionType"].as_str().unwrap(),p["typeVersion"].as_u64().unwrap_or(1)as u32)?;
        descriptor::check_fields(&definition.fields,&p["fields"]).map_err(|e|CoreError::new("INVALID_EVENT",e))?;
        self.begin_span(caller,p,definition,DiagnosticScope::capture().filter(|v|v["spanId"].is_string()))
    }
    fn begin_span(&self,caller:&Caller,p:&Value,definition:descriptor::Definition,parent:Option<Value>)->Result<Value>{
        self.expire_contexts();
        let now=crate::model::now_ms();let client=p["clientOperationId"].as_str().unwrap_or_default().to_owned();let hash=model::hash(&json!([p,parent.as_ref().map(|v|&v["spanId"])]));
        let mut contexts=self.contexts.lock().unwrap();
        if let Some(existing)=contexts.active.values().find(|v|v["instanceId"]==caller.instance_id&&v["clientOperationId"]==client){
            if existing["requestHash"]!=hash{return Err(CoreError::new("CONFLICT","操作标识内容冲突"));}return Ok(public_context(existing));
        }
        if contexts.active.len()>=4096||contexts.active.values().filter(|v|v["instanceId"]==caller.instance_id).count()>=256||contexts.active.values().map(|v|v.to_string().len()*2+256).sum::<usize>()>2*1024*1024-8192{return Err(CoreError::new("CAPACITY","活动阶段已达上限"));}
        let span=crate::model::id();let trace=parent.as_ref().and_then(|v|v["traceId"].as_str()).map(str::to_owned).unwrap_or_else(crate::model::id);let action=parent.as_ref().and_then(|v|v["actionId"].as_str()).map(str::to_owned).unwrap_or_else(crate::model::id);
        let expires=parent.as_ref().and_then(|v|v["expiresAt"].as_u64()).unwrap_or(now+1800000).min(now+1800000);
        let mut context=json!({"traceId":trace,"spanId":span,"parentSpanId":parent.as_ref().map(|v|&v["spanId"]),"actionId":action,"instanceId":caller.instance_id,"pluginId":caller.plugin_id,"bootId":self.boot_id,"expiresAt":expires,"touchedAt":now,"clientOperationId":client,"requestHash":hash,"initiator":parent.as_ref().map(|v|v["initiator"].clone()).unwrap_or_else(||json!({"kind":p["trigger"].as_str().unwrap_or("plugin"),"assurance":if p["hostVerified"]==true{"verified"}else{"reported"}})),"definition":definition});
        context["targets"]=p.get("targets").cloned().or_else(||parent.as_ref().map(|v|v["targets"].clone())).unwrap_or_else(||json!([]));
        context["source"]=json!(caller);context["stageKey"]=p.get("stageKey").cloned().unwrap_or(Value::Null);
        let token=model::seal(&self.signing_key,&json!({"spanId":span,"instanceId":caller.instance_id,"pluginId":caller.plugin_id,"viewId":caller.view_id,"mountGeneration":caller.mount_generation,"bootId":self.boot_id,"expiresAt":expires}));context["contextRef"]=json!(token);
        let mut event=model::envelope(caller,&self.boot_id,&self.epoch,"action",&definition.name,p["fields"].clone());event["typeVersion"]=json!(definition.type_version);event["configEpoch"]=json!(self.config_epoch);event["outcome"]=json!("running");event["targets"]=p.get("targets").cloned().unwrap_or_else(||json!([]));attach(&mut event,&context);
        let save=context.clone();let key=self.signing_key.clone();let owner=caller.instance_id.clone();let plugin=caller.plugin_id.clone().unwrap_or_default();
        self.write(Partition::Facts,16*1024,move|db|{db.admit(Partition::Facts,16*1024)?;let tx=db.facts.transaction()?;
            let previous:Option<String>=tx.query_row("SELECT content_hash FROM observation_spans WHERE instance_id=? AND client_id=?",params![owner,client],|r|r.get(0)).optional()?;
            if let Some(previous)=previous{return Err(CoreError::new(if previous==hash{"STALE_CONTEXT"}else{"CONFLICT"},"操作已结束或内容冲突"));}
            super::catalog::save_type(&tx,&plugin,"actions",&definition)?;model::insert(&tx,&key,"f",&mut event,None)?;
            tx.execute("INSERT INTO observation_spans VALUES(?,?,?,?,?,?,?,?,?,?,?,?)",params![span,owner,client,hash,trace,save["parentSpanId"].as_str(),action,now as i64,now as i64,Option::<String>::None,save.to_string(),serde_json::to_string(&definition)?])?;tx.commit()?;Ok(Value::Null)
        })?;let result=public_context(&context);contexts.active.insert(context["spanId"].as_str().unwrap().into(),context);Ok(result)
    }
    pub(super) fn trace_stage(&self,caller:&Caller,p:&Value)->Result<Value>{
        if p["phase"]=="finish" {let result=self.trace_end(caller,p)?;return Ok(result["context"].clone());}
        let parent=self.resolve_context(caller,p["contextRef"].as_str().unwrap(),false)?;
        let definition:descriptor::Definition=serde_json::from_value(parent["definition"].clone())?;
        let fields=p.get("fields").cloned().unwrap_or_else(||json!({}));descriptor::check_fields(&definition.fields,&fields).map_err(|e|CoreError::new("INVALID_EVENT",e))?;
        let request=json!({"clientOperationId":format!("{}:{}",parent["spanId"].as_str().unwrap(),p["stageKey"].as_str().unwrap()),"fields":fields,"trigger":"plugin","stageKey":p["stageKey"]});self.begin_span(caller,&request,definition,Some(parent))
    }
    pub(super) fn trace_end(&self,caller:&Caller,p:&Value)->Result<Value>{
        let token=p["contextRef"].as_str().unwrap();let decoded=model::unseal(&self.signing_key,token)?;
        if decoded["instanceId"]!=caller.instance_id||decoded["bootId"]!=self.boot_id{return Err(CoreError::new("STALE_CONTEXT","诊断上下文无效"));}
        if decoded["pluginId"]!=json!(caller.plugin_id)||decoded["viewId"]!=json!(caller.view_id)||decoded["mountGeneration"]!=json!(caller.mount_generation){return Err(CoreError::new("INVALID_CONTEXT","诊断上下文来源无效"));}
        let span=decoded["spanId"].as_str().unwrap_or_default().to_owned();let owner=caller.instance_id.clone();let outcome=p["outcome"].as_str().unwrap_or("unknown").to_owned();
        if matches!(outcome.as_str(),"requested"|"running"){return Err(CoreError::new("INVALID_PARAMS","阶段需要终态"));}
        let fields=p.get("fields").cloned().unwrap_or_else(||json!({}));let error=p.get("errorCode").cloned().unwrap_or(Value::Null);let key=self.signing_key.clone();let caller=caller.clone();let boot=self.boot_id.clone();let epoch=self.epoch.clone();let config=self.config_epoch.clone();
        let end_hash=model::hash(&json!([outcome,fields,error]));let live=self.contexts.lock().unwrap().active.get(&span).is_some_and(|v|!(v["surfaceEnded"]==true&&decoded["viewId"].is_string())&&v["touchedAt"].as_u64().unwrap_or(0)+600000>crate::model::now_ms());
        let value=self.write(Partition::Facts,16*1024,move|db|{db.admit(Partition::Facts,16*1024)?;let tx=db.facts.transaction()?;
            let saved:Option<(String,Option<String>,String)>=tx.query_row("SELECT context,outcome,definition FROM observation_spans WHERE span_id=? AND instance_id=?",params![span,owner],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            let Some((context,previous,definition))=saved else{return Err(CoreError::new("STALE_CONTEXT","阶段不存在"));};let mut context:Value=serde_json::from_str(&context)?;let definition:descriptor::Definition=serde_json::from_str(&definition)?;
            descriptor::check_fields(&definition.fields,&fields).map_err(|e|CoreError::new("INVALID_EVENT",e))?;
            if let Some(previous)=previous{if previous!=outcome||context["endHash"]!=end_hash{return Err(CoreError::new("CONFLICT","阶段终态冲突"));}return Ok(json!({"context":public_context(&context),"outcome":outcome,"duplicate":true}));}
            if !live{return Err(CoreError::new("STALE_CONTEXT","阶段空闲已过期"));}
            if context["expiresAt"].as_u64().unwrap_or(0)<=crate::model::now_ms(){return Err(CoreError::new("STALE_CONTEXT","阶段已过期"));}
            let mut event=model::envelope(&caller,&boot,&epoch,"action",&definition.name,fields);event["typeVersion"]=json!(definition.type_version);event["configEpoch"]=json!(config);event["outcome"]=json!(outcome);event["errorCode"]=error;attach(&mut event,&context);model::insert(&tx,&key,"f",&mut event,None)?;
            context["endHash"]=json!(end_hash);tx.execute("UPDATE observation_spans SET outcome=?,touched=?,context=? WHERE span_id=?",params![outcome,crate::model::now_ms()as i64,context.to_string(),span])?;tx.commit()?;Ok(json!({"context":public_context(&context),"outcome":outcome,"duplicate":false}))
        })?;self.contexts.lock().unwrap().active.remove(decoded["spanId"].as_str().unwrap_or_default());Ok(value)
    }
    pub(super) fn expire_contexts(&self){
        let now=crate::model::now_ms();let ids={let contexts=self.contexts.lock().unwrap();contexts.active.iter().filter(|(_,v)|v["expiresAt"].as_u64().unwrap_or(0)<=now||v["touchedAt"].as_u64().unwrap_or(0)+600000<=now).take(32).map(|(id,_)|id.clone()).collect::<Vec<_>>()};
        if !ids.is_empty(){let _=self.interrupt_spans(ids,"CONTEXT_EXPIRED");}
    }
    pub(crate) fn recover_spans(&self)->Result<()>{
        loop{let boot=self.boot_id.clone();let ids=self.queries.query(Partition::Facts,move|db|{let mut statement=db.prepare("SELECT span_id FROM observation_spans WHERE outcome IS NULL AND json_extract(context,'$.bootId')!=? LIMIT 32")?;let rows=statement.query_map([boot],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;Ok(json!(rows))})?;
            let ids=ids.as_array().unwrap().iter().filter_map(Value::as_str).map(str::to_owned).collect::<Vec<_>>();if ids.is_empty(){return Ok(());}self.interrupt_spans(ids,"APP_INTERRUPTED")?;
        }
    }
    pub(super) fn interrupt_spans(&self,ids:Vec<String>,reason:&'static str)->Result<()>{
        let remove=ids.clone();let boot=self.boot_id.clone();let epoch=self.epoch.clone();let config=self.config_epoch.clone();let key=self.signing_key.clone();
        self.write(Partition::Facts,128*1024,move|db|{
            db.admit_audit(128*1024)?;let tx=db.facts.transaction()?;
            for id in ids{let saved:Option<(String,String)>=tx.query_row("SELECT context,definition FROM observation_spans WHERE span_id=? AND outcome IS NULL",[&id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
                if let Some((context,definition))=saved{let context:Value=serde_json::from_str(&context)?;let definition:descriptor::Definition=serde_json::from_str(&definition)?;let caller=serde_json::from_value::<Caller>(context["source"].clone()).unwrap_or_else(|_|Caller::shell());
                    let mut event=model::envelope(&caller,&boot,&epoch,"action",&definition.name,json!({}));event["typeVersion"]=json!(definition.type_version);event["configEpoch"]=json!(config);event["outcome"]=json!("interrupted");event["errorCode"]=json!(reason);event["provenance"]=json!("core-observed");attach(&mut event,&context);model::insert(&tx,&key,"f",&mut event,None)?;
                    tx.execute("UPDATE observation_spans SET outcome='interrupted',touched=? WHERE span_id=?",params![crate::model::now_ms()as i64,id])?;
                }
            }tx.commit()?;Ok(Value::Null)
        })?;let mut contexts=self.contexts.lock().unwrap();for id in remove{contexts.active.remove(&id);}Ok(())
    }
}
fn public_context(value:&Value)->Value{json!({"traceId":value["traceId"],"spanId":value["spanId"],"parentSpanId":value["parentSpanId"],"actionId":value["actionId"],"contextRef":value["contextRef"]})}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn context_deadlines_and_span_limits_detach_without_canceling_business(){
        let root=tempfile::tempdir().unwrap();let core=Core::open(root.path()).unwrap();let caller=Caller::shell();
        let definition:descriptor::Definition=serde_json::from_value(json!({"type":"core.command.execute","typeVersion":1,"kind":"action","fields":{}})).unwrap();
        for idle in [false,true]{
            let value=core.observability.begin_span(&caller,&json!({"clientOperationId":crate::model::id(),"fields":{}}),definition.clone(),None).unwrap();
            let id=value["spanId"].as_str().unwrap();{let mut contexts=core.observability.contexts.lock().unwrap();let active=contexts.active.get_mut(id).unwrap();if idle{active["touchedAt"]=json!(crate::model::now_ms()-600001);}else{active["expiresAt"]=json!(crate::model::now_ms()-1);}}
            assert_eq!(core.observability.resolve_context(&caller,value["contextRef"].as_str().unwrap(),false).unwrap_err().code,"STALE_CONTEXT");
            let invoked=std::cell::Cell::new(0);let output=core.diagnostic_metadata(&caller,Some(&json!({"diagnostics":{"version":1,"contextRef":value["contextRef"]}})),||{invoked.set(invoked.get()+1);Ok(json!(7))}).unwrap();assert_eq!(output,7);assert_eq!(invoked.get(),1);
        }
        for (count,per_instance) in [(256,true),(4096,false)]{
            {let mut contexts=core.observability.contexts.lock().unwrap();contexts.active.clear();for i in 0..count{contexts.active.insert(format!("limit-{i}"),json!({"instanceId":if per_instance{caller.instance_id.clone()}else{format!("other-{i}")},"expiresAt":crate::model::now_ms()+1800000,"touchedAt":crate::model::now_ms()}));}}
            assert_eq!(core.observability.begin_span(&caller,&json!({"clientOperationId":"beyond","fields":{}}),definition.clone(),None).unwrap_err().code,"CAPACITY");
        }
        core.observability.contexts.lock().unwrap().active.clear();
    }
}
