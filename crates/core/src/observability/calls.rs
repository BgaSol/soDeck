use super::{schema::Partition,writer::Observability};
use crate::{model::{id,now_ms},Caller,Result};
use rusqlite::params;
use serde_json::{json,Value};
use std::cell::RefCell;

thread_local! {static CURRENT:RefCell<Option<(String,String)>>=const{RefCell::new(None)};}
pub(crate) struct AuditScope(Option<(String,String)>);
impl AuditScope {
    pub fn capture()->Option<(String,String)>{CURRENT.with(|current|current.borrow().clone())}
    pub fn resume(context:Option<(String,String)>)->Self {Self(CURRENT.with(|current|current.replace(context)))}
    pub fn enter(call:Option<&str>,params:&Value)->Self {
        let next=call.map(|call|(call.to_owned(),params.get("operationId").and_then(Value::as_str).filter(|id|id.len()<=128).unwrap_or(call).to_owned()));
        Self(CURRENT.with(|current|current.replace(next)))
    }
}
impl Drop for AuditScope {fn drop(&mut self){CURRENT.with(|current|current.replace(self.0.take()));}}

pub(super) fn event(tx:&rusqlite::Connection,kind:&str,call:&str,time:u64,source:&str,boot:&str,epoch:&str,target:Option<&str>,outcome:Option<&str>,payload:&Value)->Result<()> {
    // Sequence allocation and its projection are committed together.
    let key:String=tx.query_row("SELECT value FROM meta WHERE key='signingKey'",[],|r|r.get(0))?;
    let store_epoch:String=tx.query_row("SELECT value FROM meta WHERE key='storeEpoch'",[],|r|r.get(0))?;
    let caller:Caller=serde_json::from_str(source).unwrap_or_else(|_|Caller::shell());
    let event_kind=if kind.starts_with("call."){"call"}else{"domain"};
    let mut record=super::model::envelope(&caller,boot,&store_epoch,event_kind,kind,payload.clone());
    record["provenance"]=json!("core-observed");record["operationId"]=payload.get("operationId").cloned().unwrap_or(Value::Null);
    record["configEpoch"]=json!(epoch);record["receivedAt"]=json!(time);record["occurredAt"]=json!(time);
    record["callId"]=json!((!call.is_empty()).then_some(call));record["traceId"]=json!(payload.get("traceId").and_then(Value::as_str).or((!call.is_empty()).then_some(call)));record["outcome"]=json!(outcome);
    if let Some(target)=target{record["targets"]=json!([super::targets::reference(&key,epoch,"server",target,Some(epoch))]);}
    if let Some(context)=payload.get("diagnosticContext").filter(|v|v.is_object()){record["operationId"]=context["operationId"].clone();super::context::attach(&mut record,context);record["attributes"].as_object_mut().map(|m|m.remove("diagnosticContext"));}
    fn omit_nulls(value:&mut Value){match value{Value::Object(o)=>{o.retain(|_,v|!v.is_null());for v in o.values_mut(){omit_nulls(v);}},Value::Array(a)=>for v in a{omit_nulls(v);},_=>{}}}
    omit_nulls(&mut record["attributes"]);
    if let Ok(definition)=super::builtin::definition(kind){super::catalog::save_type(tx,"","events",&definition)?;}
    super::model::insert(tx,&key,"f",&mut record,None)?;
    let sequence=record["sequence"].as_str().unwrap().parse::<i64>().unwrap();
    tx.execute("INSERT INTO events(event_id,sequence,time,kind,event_type,source,boot_id,config_epoch,target,call_id,trace_id,outcome,payload) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",
        params![record["eventId"].as_str(),sequence,time as i64,event_kind,kind,source,boot,epoch,target,(!call.is_empty()).then_some(call),record["traceId"].as_str(),outcome,record["attributes"].to_string()])?;
    Ok(())
}
impl Observability {
    pub fn route_phase(&self,target:&str,state:&str,mut summary:Value)->Result<()> {
        let (call,trace)=CURRENT.with(|current|current.borrow().clone()).unwrap_or_default();
        summary["traceId"]=json!(trace);summary["diagnosticContext"]=json!(super::context::DiagnosticScope::capture());
        let boot=self.boot_id.clone();let epoch=self.config_epoch.clone();let target=target.to_owned();
        let outcome=match state {"succeeded"=>Some("succeeded"),"failed"=>Some("failed"),"canceled"=>Some("canceled"),_=>None};
        let charge=summary.to_string().len()+2048;
        self.commit(charge,move|db|{db.admit(Partition::Facts,charge)?;let tx=db.facts.transaction()?;
            event(&tx,"connections.route.phase",&call,now_ms(),"{\"source\":\"core\",\"instanceId\":\"connection-broker\"}",&boot,&epoch,Some(&target),outcome,&summary)?;
            tx.commit()?;Ok(Value::Null)})?;Ok(())
    }
    pub fn calls(&self,filters:&Value)->Result<Value> {
        if serde_json::to_vec(filters)?.len()>8192{return Err(crate::CoreError::new("INVALID_PARAMS","调用记录过滤条件过大"));}
        let filters=filters.clone();self.queries.query(Partition::Facts,move|db|crate::store::query_calls(db,&filters,30))
    }
    pub fn recover_calls(&self)->Result<()> {
        loop {
            let boot=self.boot_id.clone();
            let count=self.commit(128*1024,move|db|{
                db.admit_audit(128*1024)?;let tx=db.facts.transaction()?;
                let calls={let mut query=tx.prepare("SELECT c.id,c.caller,c.server_id,e.config_epoch,e.trace_id FROM calls c JOIN events e ON e.call_id=c.id AND e.event_type='call.started' WHERE c.ended IS NULL ORDER BY c.started LIMIT 32")?;
                    let rows=query.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,String>(3)?,r.get::<_,Option<String>>(4)?)))?;rows.collect::<std::result::Result<Vec<_>,_>>()?};
                for (call,caller,target,epoch,trace) in &calls {let time=now_ms();
                    event(&tx,"call.interrupted",call,time,caller,&boot,epoch,target.as_deref(),Some("interrupted"),&json!({"errorCode":"APP_INTERRUPTED","reconciled":true,"traceId":trace}))?;
                    tx.execute("UPDATE calls SET ended=?,status='interrupted',error_code='APP_INTERRUPTED' WHERE id=?",params![time as i64,call])?;
                }
                tx.commit()?;Ok(json!(calls.len()))
            })?;
            if count==0{return Ok(());}
        }
    }
    pub fn call_start(&self,caller:&Caller,method:&str,p:&Value)->Result<String> {
        let call=id();let result=call.clone();let caller=serde_json::to_string(caller)?;let method=method.to_owned();let boot=self.boot_id.clone();let epoch=self.config_epoch.clone();
        let target=p.get("serverId").and_then(Value::as_str).map(str::to_owned);
        if caller.len()>2048||method.len()>200||target.as_ref().is_some_and(|s|s.len()>128){return Err(crate::CoreError::new("INVALID_PARAMS","调用来源或标识超过长度限制"));}
        let mut summary=json!({"fieldCount":p.as_object().map_or(0,|v|v.len()),"byteLength":p.get("bytes").and_then(Value::as_str).map(str::len),"inputSeq":p.get("inputSeq").and_then(Value::as_u64),"generation":p.get("generation").and_then(Value::as_u64)});
        settings_summary(&method,p,&mut summary);plugin_summary(&method,p,&mut summary);
        let mut context=super::context::DiagnosticScope::capture().unwrap_or_else(||json!({}));
        context["operationId"]=p.get("operationId").cloned().unwrap_or(Value::Null);
        let charge=caller.len()+method.len()+summary.to_string().len()+4096;
        self.audit(charge,move|tx|{let time=now_ms();
            event(&tx,"call.started",&call,time,&caller,&boot,&epoch,target.as_deref(),Some("running"),&json!({"method":method,"summary":summary,"diagnosticContext":context}))?;
            tx.execute("INSERT INTO calls(id,started,caller,method,server_id,summary,status) VALUES(?,?,?,?,?,?,'running')",params![call,time as i64,caller,method,target,summary.to_string()])?;
            tx.execute("INSERT INTO call_context VALUES(?,?)",params![call,context.to_string()])?;
            Ok(Value::Null)
        })?;Ok(result)
    }
    pub fn call_finish(&self,call:&str,result:&Result<Value>)->Result<()> {
        let summary=result.as_ref().ok().and_then(|value|{
            let revision=value["persistedRevision"].as_str().filter(|s|!s.is_empty()&&s.len()<=20&&s.bytes().all(|b|b.is_ascii_digit()))?;
            let persistence=value["persistence"].as_str().filter(|s|matches!(*s,"saved"|"unchanged"))?;
            let synchronization=value["synchronization"].as_str().filter(|s|matches!(*s,"current"|"pending"))?;
            Some(json!({"persistedRevision":revision,"persistence":persistence,"synchronization":synchronization}))
        });
        self.finish(call,if result.is_ok(){"succeeded"}else{"failed"},result.as_ref().err().map(|e|e.code.clone()),summary)
    }
    pub fn network_stream_finish(&self,call:&str,read:u64,written:u64,duration:u64,error:Option<&str>)->Result<()> {
        self.finish(call,if error.is_some(){"failed"}else{"succeeded"},error.map(str::to_owned),Some(json!({"bytesRead":read,"bytesWritten":written,"durationMs":duration})))
    }
    fn finish(&self,call:&str,status:&'static str,error:Option<String>,summary:Option<Value>)->Result<()> {
        let call=call.to_owned();let boot=self.boot_id.clone();let epoch=self.config_epoch.clone();
        self.audit(4096,move|tx|{let time=now_ms();
            let (caller,target):(String,Option<String>)=tx.query_row("SELECT caller,server_id FROM calls WHERE id=? AND ended IS NULL",[&call],|r|Ok((r.get(0)?,r.get(1)?)))?;
            use rusqlite::OptionalExtension;
            let context:Option<String>=tx.query_row("SELECT data FROM call_context WHERE call_id=?",[&call],|r|r.get(0)).optional()?;
            let method:String=tx.query_row("SELECT method FROM calls WHERE id=?",[&call],|r|r.get(0))?;
            let summary=if summary.as_ref().is_some_and(|s|s.get("persistedRevision").is_some()){
                if matches!(method.as_str(),"settings.commit"|"storage.preferences.commit"){
                    let previous:String=tx.query_row("SELECT summary FROM calls WHERE id=?",[&call],|r|r.get(0))?;let mut merged:Value=serde_json::from_str(&previous).unwrap_or(json!({}));
                    for (key,value) in summary.unwrap().as_object().unwrap(){merged[key]=value.clone();}Some(merged)
                }else{None}
            }else{summary};
            event(&tx,"call.finished",&call,time,&caller,&boot,&epoch,target.as_deref(),Some(status),&json!({"method":method,"errorCode":error,"summary":summary,"diagnosticContext":context.and_then(|s|serde_json::from_str::<Value>(&s).ok())}))?;
            tx.execute("UPDATE calls SET ended=?,status=?,error_code=?,summary=COALESCE(?,summary) WHERE id=?",params![time as i64,status,error,summary.map(|s|s.to_string()),call])?;Ok(Value::Null)
        })?;Ok(())
    }
    pub fn maintain(&self)->Result<()> {self.commit(128*1024,|db|{super::retention::maintain(db)?;Ok(Value::Null)})?;Ok(())}

}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn settings_audit_retains_identifiers_and_durable_result_without_contents(){
        let temp=tempfile::tempdir().unwrap();Observability::initialize_fixture(temp.path()).unwrap();let store=Observability::open(temp.path(),"boot".into(),"config".into()).unwrap();
        let call=store.call_start(&Caller::shell(),"settings.commit",&json!({"pageId":"plugin:sample:general","expectedRevision":"1","changes":[{"key":"name","value":"PRIVATE_TEXT"}],"ticket":"PRIVATE_TICKET","path":"PRIVATE_PATH"})).unwrap();
        store.call_finish(&call,&Ok(json!({"persistedRevision":"2","persistence":"saved","synchronization":"pending","values":"PRIVATE_RESULT"}))).unwrap();
        let records=store.calls(&json!({"method":"settings.commit"})).unwrap();assert!(!records.to_string().contains("PRIVATE"));let summary=&records["records"][0]["summary"];assert_eq!(summary["settingsPage"],"general");assert_eq!(summary["persistedRevision"],"2");assert_eq!(summary["synchronization"],"pending");assert!(store.shutdown());
    }
    #[test]
    fn audit_facts_and_projection_commit_once_without_request_secrets() {
        let temp=tempfile::tempdir().unwrap();Observability::initialize_fixture(temp.path()).unwrap();
        let store=Observability::open(temp.path(),"boot".into(),"configuration".into()).unwrap();
        let call=store.call_start(&Caller::shell(),"servers.create",&json!({"password":"not-stored-secret","host":"not-stored-address"})).unwrap();
        store.call_finish(&call,&Ok(json!({"private":"not-stored-result"}))).unwrap();
        let id=call.clone();store.commit(1024,move|db|{
            assert_eq!(db.facts.query_row("SELECT count(*) FROM events WHERE call_id=?",[&id],|r|r.get::<_,u32>(0))?,2);
            assert_eq!(db.facts.query_row("SELECT status FROM calls WHERE id=?",[&id],|r|r.get::<_,String>(0))?,"succeeded");
            let bodies:String=db.facts.query_row("SELECT group_concat(payload) FROM events",[],|r|r.get(0))?;
            assert!(!bodies.contains("not-stored"));Ok(Value::Null)
        }).unwrap();
        assert!(store.call_finish(&call,&Ok(Value::Null)).is_err());assert!(store.shutdown());
    }
    #[test]
    fn projection_failure_rolls_back_fact_and_sequence() {
        let temp=tempfile::tempdir().unwrap();Observability::initialize_fixture(temp.path()).unwrap();
        let store=Observability::open(temp.path(),"boot".into(),"configuration".into()).unwrap();
        store.commit(1024,|db|{db.facts.execute_batch("CREATE TRIGGER reject_start BEFORE INSERT ON calls BEGIN SELECT RAISE(ABORT,'injected'); END;")?;Ok(Value::Null)}).unwrap();
        assert!(store.call_start(&Caller::shell(),"test",&json!({})).is_err());
        store.commit(1024,|db|{assert_eq!(db.facts.query_row("SELECT count(*) FROM events",[],|r|r.get::<_,u32>(0))?,0);assert_eq!(db.facts.query_row("SELECT value FROM meta WHERE key='sequence'",[],|r|r.get::<_,String>(0))?,"0");Ok(Value::Null)}).unwrap();
        assert!(store.shutdown());
    }
    #[test]
    fn restart_reports_interrupted_once_and_legacy_calls_keep_their_cursor_shape() {
        let temp=tempfile::tempdir().unwrap();Observability::initialize_fixture(temp.path()).unwrap();
        let store=Observability::open(temp.path(),"boot-one".into(),"configuration".into()).unwrap();
        let id=store.call_start(&Caller::shell(),"pending.operation",&json!({})).unwrap();assert!(store.shutdown());drop(store);
        let store=Observability::open(temp.path(),"boot-two".into(),"new-configuration".into()).unwrap();store.recover_calls().unwrap();store.recover_calls().unwrap();
        let page=store.calls(&json!({"limit":1})).unwrap();assert_eq!(page["records"][0]["id"],id);assert_eq!(page["records"][0]["status"],"interrupted");assert_eq!(page["retentionDays"],30);
        store.commit(1024,move|db|{
            assert_eq!(db.facts.query_row("SELECT count(*) FROM events WHERE call_id=?",[&id],|r|r.get::<_,u32>(0))?,2);
            let recovered:(String,String,String)=db.facts.query_row("SELECT config_epoch,boot_id,trace_id FROM events WHERE call_id=? AND event_type='call.interrupted'",[&id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            assert_eq!(recovered,("configuration".into(),"boot-two".into(),id));Ok(Value::Null)
        }).unwrap();assert!(store.shutdown());
    }
    #[test]
    fn asynchronous_audit_context_is_explicit_and_restored_per_thread() {
        let _first=AuditScope::enter(Some("parent-a"),&json!({"operationId":"operation-a"}));
        let context=AuditScope::capture();
        let worker=std::thread::spawn(move||{
            assert!(AuditScope::capture().is_none());
            {let _scope=AuditScope::resume(context);assert_eq!(AuditScope::capture(),Some(("parent-a".into(),"operation-a".into())));}
            assert!(AuditScope::capture().is_none());
        });
        {let _second=AuditScope::enter(Some("parent-b"),&json!({}));worker.join().unwrap();assert_eq!(AuditScope::capture(),Some(("parent-b".into(),"parent-b".into())));}
        assert_eq!(AuditScope::capture(),Some(("parent-a".into(),"operation-a".into())));
    }
}

#[cfg(test)]mod audit_performance{
    use super::*;
    #[test]#[ignore="local durability throughput measurement"]fn audit_commit_measurement(){
        let root=tempfile::tempdir().unwrap();Observability::initialize_fixture(root.path()).unwrap();let store=Observability::open(root.path(),"boot".into(),"config".into()).unwrap();
        let begin=std::time::Instant::now();for _ in 0..200{let call=store.call_start(&crate::Caller::shell(),"events.v2.ack",&json!({})).unwrap();store.call_finish(&call,&Ok(json!({}))).unwrap();}println!("200 audited calls: {:?}",begin.elapsed());store.shutdown();
    }
}

/// Identifiers and counts only: setting contents, tickets and file paths never enter audit summaries.
fn settings_summary(method:&str,p:&Value,summary:&mut Value){
    if !method.starts_with("settings."){return;}
    let safe=|s:&str|!s.is_empty()&&s.len()<=128&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b"._-".contains(&b));
    if let Some(page)=p["pageId"].as_str(){let parts=page.split(':').collect::<Vec<_>>();let identity=match parts.as_slice(){["core",page]=>Some(("core",*page)),["plugin",plugin,page]=>Some((*plugin,*page)),_=>None};if let Some((owner,page))=identity{if safe(owner)&&safe(page){summary["settingsOwner"]=json!(owner);summary["settingsPage"]=json!(page);}}}
    if let Some(revision)=p["expectedRevision"].as_str().filter(|s|s.len()<=20&&s.bytes().all(|b|b.is_ascii_digit())){summary["settingsRevision"]=json!(revision);}
    if let Some(changes)=p["changes"].as_array(){summary["changedKeyCount"]=json!(changes.len().min(64));summary["changedKeys"]=json!(changes.iter().filter_map(|e|e["key"].as_str().filter(|s|safe(s))).take(32).collect::<Vec<_>>());}
}
#[cfg(test)]mod settings_summary_tests{use super::*;#[test]fn never_records_values_or_paths(){let mut summary=json!({});settings_summary("settings.commit",&json!({"pageId":"plugin:sample:general","expectedRevision":"4","changes":[{"key":"name","value":"SECRET_CONTENT"}],"commitTicket":"SECRET_TICKET","path":"SECRET_PATH"}),&mut summary);assert_eq!(summary["settingsPage"],"general");assert_eq!(summary["changedKeys"],json!(["name"]));assert!(!summary.to_string().contains("SECRET"));}}

fn plugin_summary(method:&str,p:&Value,summary:&mut Value){
    if !method.starts_with("plugins."){return;}
    let safe=|s:&str|!s.is_empty()&&s.len()<=128&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b"._-".contains(&b));
    for field in ["pluginId","version","state","code"]{if let Some(v)=p[field].as_str().filter(|v|safe(v)){summary[field]=json!(v);}}
    if let Some(changes)=p["changes"].as_array(){summary["changedKeyCount"]=json!(changes.len());}
}
#[cfg(test)]mod plugin_summary_tests{use super::*;#[test]fn management_audit_contains_identity_and_counts_without_content(){let mut summary=json!({});plugin_summary("plugins.package.completed",&json!({"pluginId":"third.party","version":"1.0.0","state":"prepared","path":"SECRET_PATH","signature":"SECRET"}),&mut summary);assert_eq!(summary["pluginId"],"third.party");assert!(!summary.to_string().contains("SECRET"));}}
