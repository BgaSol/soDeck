use super::{model,schema::Partition,Observability};
use crate::{Core,CoreError,Result};
use serde_json::{json,Value};
use rusqlite::{params,OptionalExtension};
use sha2::{Digest,Sha256};
use std::{collections::HashMap,sync::{Arc,atomic::{AtomicBool,Ordering}},time::{Duration,Instant}};
#[derive(Default)]pub(super) struct Jobs {pub active:HashMap<String,(String,Arc<AtomicBool>)>,terminal:HashMap<String,Value>,retry_at:Option<Instant>}
fn job(id:&str,kind:&str,manifest:Value)->Value{let now=crate::model::now_ms();json!({"jobId":id,"kind":kind,"state":"queued","phase":"queued","createdAt":now,"updatedAt":now,"manifest":manifest,"error":null})}
impl Core {
    /// Host-only export phase: destinations are never passed through plugin RPC.
    pub fn begin_evidence_export(&self,package:&str)->Result<Value>{self.observability.export_begin(package)}
    pub fn evidence_export_writing(&self,id:&str)->Result<()>{let mut v=self.observability.job_get(&json!({"jobId":id}))?["job"].clone();v["state"]=json!("running");v["phase"]=json!("writing");v["updatedAt"]=json!(crate::model::now_ms());self.observability.job_save(&v)}
    pub fn finish_evidence_export(&self,id:&str,state:&str,code:Option<&str>)->Result<Value>{self.observability.export_finish(id,state,code)}
    pub fn evidence_export_canceled(&self,id:&str)->bool{self.observability.jobs.lock().unwrap().active.get(id).is_none_or(|(_,v)|v.load(Ordering::Relaxed))}
    pub fn read_evidence_chunks(&self,id:&str,offset:u64)->Result<Value>{self.observability.job_get(&json!({"jobId":id,"offset":offset,"limit":4}))}
}
impl Observability {
    pub(super) fn new_job_id(&self)->String{let unsigned=format!("j2-{}-{}",self.epoch,crate::model::id());format!("{}-{}",unsigned,&model::hash(&json!([self.signing_key,unsigned]))[..32])}
    fn validate_job_id(&self,id:&str)->Result<()>{let Some((unsigned,tag))=id.rsplit_once('-')else{return Err(CoreError::new("INVALID_REFERENCE","诊断任务引用无效"));};
        if !unsigned.starts_with("j2-")||tag!=&model::hash(&json!([self.signing_key,unsigned]))[..32]{return Err(CoreError::new("INVALID_REFERENCE","诊断任务引用无效"));}Ok(())}

    pub(super) fn job_save(&self,v:&Value)->Result<()> {
        let v=v.clone();self.write(Partition::Diagnostics,8192,move|db|{
            db.admit(Partition::Diagnostics,8192)?;let tx=db.diagnostics.transaction()?;
            if matches!(v["kind"].as_str(),Some("capture"|"evidence"))&&matches!(v["state"].as_str(),Some("failed"|"canceled")){
                tx.execute("DELETE FROM evidence_chunks WHERE job_id=?",[v["jobId"].as_str()])?;
            }
            tx.execute("INSERT INTO observation_jobs VALUES(?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET updated=excluded.updated,state=excluded.state,data=excluded.data",params![v["jobId"].as_str(),v["createdAt"].as_u64().unwrap()as i64,v["updatedAt"].as_u64().unwrap()as i64,v["state"].as_str(),v.to_string()])?;
            tx.commit()?;Ok(Value::Null)
        })?;Ok(())
    }
    // Cancellation and terminal persistence have one order. Failed persistence
    // retains the active slot and a bounded terminal value for the next retry.
    pub(super) fn complete_job(&self,value:&mut Value,honor_cancel:bool)->Result<()> {
        let id=value["jobId"].as_str().unwrap().to_owned();let mut jobs=self.jobs.lock().unwrap();
        if honor_cancel&&jobs.active.get(&id).is_some_and(|(_,c)|c.load(Ordering::Relaxed)){
            value["state"]=json!("canceled");value["error"]=json!({"code":"CANCELED","message":"任务已取消"});
        }
        self.release_evidence_reservation(&id);
        let result=self.job_save(value);
        if result.is_ok(){jobs.active.remove(&id);jobs.terminal.remove(&id);}else{jobs.terminal.insert(id,value.clone());self.management_tail_failed();}
        result
    }
    pub(super) fn retry_terminal_jobs(&self){
        let mut jobs=self.jobs.lock().unwrap();if jobs.terminal.is_empty()||jobs.retry_at.is_some_and(|t|t.elapsed()<Duration::from_secs(1)){return;}
        jobs.retry_at=Some(Instant::now());let (id,value)=jobs.terminal.iter().next().map(|(k,v)|(k.clone(),v.clone())).unwrap();
        if self.job_save(&value).is_ok(){jobs.terminal.remove(&id);jobs.active.remove(&id);}
    }
    pub(super) fn job_get(&self,p:&Value)->Result<Value>{let id=p["jobId"].as_str().unwrap().to_owned();self.validate_job_id(&id)?;let offset=p["offset"].as_u64().unwrap_or(0);let limit=p["limit"].as_u64().unwrap_or(4).min(4);
        self.queries.query(Partition::Diagnostics,move|db|{let data:Option<String>=db.query_row("SELECT data FROM observation_jobs WHERE id=?",[&id],|r|r.get(0)).optional()?;let job:Value=serde_json::from_str(&data.ok_or_else(||CoreError::new("GONE","诊断任务已被清理"))?)?;
            let mut stmt=db.prepare("SELECT data FROM evidence_chunks WHERE job_id=? AND position>=? ORDER BY position LIMIT ?")?;let chunks=stmt.query_map(params![id,offset as i64,limit as i64+1],|r|r.get::<_,Vec<u8>>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
            let next=(chunks.len()>limit as usize).then_some(offset+limit);let chunks=chunks.into_iter().take(limit as usize).map(|v|String::from_utf8(v).map_err(|_|CoreError::new("STORE_UNAVAILABLE","证据块编码无效"))).collect::<Result<Vec<_>>>()?;Ok(json!({"job":job,"chunks":chunks,"nextOffset":next}))
        })
    }
    pub(super) fn job_cancel(&self,p:&Value)->Result<Value>{
        let id=p["jobId"].as_str().unwrap();let jobs=self.jobs.lock().unwrap();let mut value=self.job_get(p)?["job"].clone();
        if !jobs.terminal.contains_key(id){if let Some((_,cancel))=jobs.active.get(id){cancel.store(true,Ordering::Relaxed);value["phase"]=json!("cancelRequested");}}Ok(value)
    }
    pub(super) fn create_evidence(self:&Arc<Self>,p:&Value,capture:bool)->Result<Value>{
        let mut filter=p.get("filter").cloned().unwrap_or_else(||json!({}));let now=crate::model::now_ms();let mut anchor=now;
        if let Some(event)=p.get("eventId"){
            let event=self.get_v2(&json!({"storeEpoch":self.epoch,"eventId":event}))?;anchor=event["receivedAt"].as_u64().unwrap_or(now);
            if filter.get("traceId").is_none()&&p.get("traceId").is_none(){
                if let Some(trace)=event["traceId"].as_str(){filter["traceId"]=json!(trace);}
                else if filter.get("diagnosticRef").is_none(){if let Some(target)=event["targets"].as_array().and_then(|a|a.iter().find(|t|t["kind"]=="resource").or_else(||a.first())){filter["diagnosticRef"]=target["ref"].clone();}}
            }
        }
        if let Some(trace)=p.get("traceId"){filter["traceId"]=trace.clone();}
        if filter.get("fromUtc").is_none(){filter["fromUtc"]=json!(anchor.saturating_sub(300000));}if filter.get("toUtc").is_none(){filter["toUtc"]=json!(anchor.saturating_add(300000));}
        let water=self.watermarks()?;
        let id=self.new_job_id();let kind=if capture{"capture"}else{"evidence"};let cancel=Arc::new(AtomicBool::new(false));
        {let mut jobs=self.jobs.lock().unwrap();if jobs.active.values().filter(|(k,_)|k!="cleanup").count()>=2{return Err(CoreError::new("CAPACITY","诊断任务已满"));}jobs.active.insert(id.clone(),(kind.into(),cancel.clone()));}
        let reserve_id=id.clone();let reserved=match self.write(Partition::Diagnostics,1024,move|db|Ok(json!(db.reserve_evidence(&reserve_id)?))){Ok(v)=>v,Err(e)=>{self.jobs.lock().unwrap().active.remove(&id);return Err(e);}};
        let initial=job(&id,kind,json!({"storeEpoch":self.epoch,"filter":filter,"coverage":"pending","gaps":[],"highWatermark":water["high"].as_u64().unwrap().to_string(),"diagnosticsFrozenAt":now,"reservedBytes":reserved}));
        if let Err(e)=self.job_save(&initial){self.release_evidence_reservation(&id);self.jobs.lock().unwrap().active.remove(&id);return Err(e);}
        let frozen=match self.freeze_diagnostics(&filter){Ok(value)=>value,Err(error)=>{let mut failed=initial.clone();failed["state"]=json!("failed");failed["phase"]=json!("finished");failed["error"]=json!({"code":error.code,"message":error.message});let _=self.complete_job(&mut failed,true);return Ok(failed);}};
        let store=self.clone();let mut state=initial.clone();let partial=p["allowPartial"].as_bool().unwrap_or(false);
        std::thread::spawn(move||{let result=store.build_evidence(&mut state,filter,&cancel,capture,water,frozen);let canceled=cancel.load(Ordering::Relaxed);
            state["updatedAt"]=json!(crate::model::now_ms());state["phase"]=json!("finished");
            match result{Ok(())=>{state["state"]=json!(if state["manifest"]["gaps"].as_array().is_some_and(|g|!g.is_empty()){"partial"}else{"succeeded"});},Err(e)=>{state["state"]=json!(if canceled{"canceled"}else if partial&&state["manifest"]["chunks"].as_u64().unwrap_or(0)>0{"partial"}else{"failed"});state["error"]=json!({"code":e.code,"message":e.message});state["manifest"]["gaps"].as_array_mut().unwrap().push(json!("capture-incomplete"));}}
            if canceled{state["state"]=json!("canceled");state["error"]=json!({"code":"CANCELED","message":"采集已取消"});}
            if state["state"]=="partial"&&!partial{state["state"]=json!("failed");state["error"]=json!({"code":"INCOMPLETE_EVIDENCE","message":"请求要求完整证据，但保留范围存在缺口"});}
            if matches!(state["state"].as_str(),Some("succeeded"|"partial")) {if let Err(error)=store.finish_manifest(&mut state){state["state"]=json!("failed");state["error"]=json!({"code":error.code,"message":"证据清单未能提交"});}}
            let _=store.complete_job(&mut state,true);
        });Ok(initial)
    }
    fn release_evidence_reservation(&self,id:&str){let id=id.to_owned();if self.write(Partition::Diagnostics,512,move|db|{db.release_evidence(&id);Ok(Value::Null)}).is_err(){self.management_tail_failed();}}
    fn write_evidence_chunk(&self,id:&str,position:usize,bytes:Vec<u8>)->Result<()>{let id=id.to_owned();self.write(Partition::Diagnostics,bytes.len()+4096,move|db|{db.admit_evidence_body(&id,bytes.len()+4096)?;db.diagnostics.execute("INSERT INTO evidence_chunks VALUES(?,?,?)",params![id,position as i64,bytes])?;Ok(Value::Null)})?;Ok(())}
    fn build_evidence(&self,state:&mut Value,filter:Value,cancel:&AtomicBool,capture:bool,water:Value,frozen:Value)->Result<()> {
        state["state"]=json!("running");state["phase"]=json!("collecting");self.job_save(state)?;
        let started=Instant::now();let id=state["jobId"].as_str().unwrap().to_owned();let mut output=EvidenceOutput::new(self,&id);
        let check=||->Result<()>{if cancel.load(Ordering::Relaxed){Err(CoreError::new("CANCELED","采集已取消"))}else if started.elapsed()>Duration::from_secs(300){Err(CoreError::new("CAPACITY","采集超过5分钟"))}else{Ok(())}};
        let result=(||->Result<()>{
            output.push(json!({"recordType":"package","version":1,"jobId":id,"storeEpoch":self.epoch,"bootId":self.boot_id,"apiVersion":crate::capabilities::API_VERSION,"filter":filter}),false)?;
            let mut definitions=std::collections::BTreeSet::new();let mut records=0;
            if !capture{
                let cursor=model::seal(&self.cursor_key,&json!({"epoch":self.epoch,"fingerprint":model::hash(&json!(["evidence",filter,true])),"expires":crate::model::now_ms()+300000,"high":water["high"],"prune":water["prune"],"after":0}));
                let mut query=json!({"filter":filter,"limit":32,"ascending":true,"cursor":cursor});
                loop{check()?;let page=self.query_v2("evidence",&query)?;
                    if page["coverage"]["state"]=="partial"{add_gap(state,"history-pruned");}
                    for e in page["items"].as_array().unwrap(){check()?;if records>=10000{return Err(CoreError::new("CAPACITY","证据事件超过10000条"));}
                        let key=(if e["type"].as_str().unwrap_or("").starts_with("plugin."){e["source"]["pluginId"].as_str().unwrap_or("").to_owned()}else{String::new()},if e["kind"]=="action"{"actions"}else{"events"},e["type"].as_str().unwrap_or("").to_owned(),e["typeVersion"].as_u64().unwrap_or(1));
                        if definitions.insert(key.clone()){
                            match self.types(&json!({"pluginId":key.0,"category":key.1,"type":key.2,"typeVersion":key.3}),true){
                                Ok(definition)=>output.push(json!({"recordType":"type","value":definition}),false)?,
                                Err(_)=>add_gap(state,"type-definition-unavailable"),
                            }
                        }
                        output.push(json!({"recordType":"event","value":e}),true)?;records+=1;
                    }
                    if page["nextCursor"].is_null(){break;}query["cursor"]=page["nextCursor"].clone();
                }
            }
            if frozen["scopeUnresolved"]==true{add_gap(state,"diagnostic-scope-unresolved");}
            for item in frozen["items"].as_array().unwrap(){check()?;output.push(item.clone(),false)?;}
            let coverage=self.describe()?;if coverage["sources"].as_array().unwrap().iter().any(|s|s["coverage"]!="full"){add_gap(state,"audit-only-source");}
            output.push(json!({"recordType":"coverage","value":coverage}),false)?;Ok(())
        })();
        // Each committed chunk ends at a complete record. A failed flush cannot
        // expose half a UTF-8 string/JSON record or count an uncommitted event.
        let flushed=if cancel.load(Ordering::Relaxed){Err(CoreError::new("CANCELED","采集已取消"))}else{output.flush()};
        state["manifest"]["eventCount"]=json!(output.records);state["manifest"]["capturedAt"]=json!(crate::model::now_ms());
        state["manifest"]["bytes"]=json!(output.total);state["manifest"]["chunks"]=json!(output.position);
        if output.position==0{return Err(CoreError::new("EMPTY_EVIDENCE","无法保存证据包头"));}
        result.and(flushed)
    }
    fn freeze_diagnostics(&self,filter:&Value)->Result<Value>{
        let scopes=self.evidence_scopes(filter)?;let filter=filter.clone();self.queries.query(Partition::Diagnostics,move|db|{
            let tx=db.unchecked_transaction()?;let mut items=Vec::new();let mut bytes=0;
            for metrics in [false,true]{let mut sql="1=1".to_owned();let mut args:Vec<rusqlite::types::Value>=Vec::new();
                if let Some(plugin)=filter["pluginId"].as_str(){sql.push_str(" AND plugin_id=?");args.push(plugin.to_owned().into());}
                if let Some(instance)=filter["instanceId"].as_str(){sql.push_str(if metrics{" AND json_extract(data,'$.source.instanceId')=?"}else{" AND instance_id=?"});args.push(instance.to_owned().into());}
                if let Some(scopes)=&scopes{if scopes.is_empty(){sql.push_str(" AND 0=1");}else{sql.push_str(&format!(" AND scope IN({})",vec!["?";scopes.len()].join(",")));args.extend(scopes.iter().cloned().map(Into::into));}}
                if metrics{for(key,op)in [("fromUtc",">="),("toUtc","<")]{if let Some(time)=filter[key].as_u64(){sql.push_str(&format!(" AND minute{op}?"));args.push((time as i64).into());}}}
                let mut stmt=tx.prepare(&if metrics{format!("SELECT data FROM metric_buckets WHERE {sql} ORDER BY minute,series LIMIT 4097")}else{format!("SELECT data FROM observation_snapshots WHERE {sql} ORDER BY id LIMIT 4097")})?;
                let mut rows=stmt.query(rusqlite::params_from_iter(args))?;while let Some(row)=rows.next()?{
                    let data:Value=serde_json::from_str(&row.get::<_,String>(0)?)?;let item=json!({"recordType":if metrics{"metric"}else{"snapshot"},"value":data});bytes+=model::memory_bytes(&item);
                    if items.len()>=4096||bytes>512*1024{return Err(CoreError::new("CAPACITY","现场冻结副本超限，请缩小采集范围"));}items.push(item);
                }
            }Ok(json!({"items":items,"scopeUnresolved":scopes.as_ref().is_some_and(|v|v.is_empty())}))
        })
    }
    fn finish_manifest(&self,state:&mut Value)->Result<()> {
        let id=state["jobId"].as_str().unwrap().to_owned();let mut offset=0;let mut bytes=0usize;let mut count=0usize;let mut hash=Sha256::new();
        loop{let page=self.job_get(&json!({"jobId":id,"offset":offset,"limit":4}))?;for chunk in page["chunks"].as_array().unwrap(){let data=chunk.as_str().unwrap().as_bytes();hash.update(data);bytes+=data.len();count+=1;}match page["nextOffset"].as_u64(){Some(next)=>offset=next,None=>break}}
        state["manifest"]["contentSha256"]=json!(format!("{:x}",hash.finalize()));state["manifest"]["contentBytes"]=json!(bytes);state["manifest"]["contentChunks"]=json!(count);state["manifest"]["state"]=state["state"].clone();
        state["manifest"]["coverage"]=json!(if state["state"]=="partial"{"partial"}else{"recorded-range"});
        state["manifest"]["chunks"]=json!(count+1);state["manifest"]["bytes"]=json!(bytes);
        let data=loop{let data=(json!({"recordType":"manifest","value":state["manifest"]}).to_string()+"\n").into_bytes();let total=bytes+data.len();if state["manifest"]["bytes"]==total{break data;}state["manifest"]["bytes"]=json!(total);};let length=data.len();
        self.write(Partition::Diagnostics,length+4096,move|db|{db.admit_evidence(&id,length+4096)?;db.diagnostics.execute("INSERT INTO evidence_chunks VALUES(?,?,?)",params![id,count as i64,data])?;Ok(Value::Null)})?;
        state["manifest"]["bytes"]=json!(bytes+length);state["manifest"]["chunks"]=json!(count+1);Ok(())
    }
    fn export_begin(&self,package:&str)->Result<Value>{let value=self.job_get(&json!({"jobId":package}))?;if !matches!(value["job"]["state"].as_str(),Some("succeeded"|"partial"))||!matches!(value["job"]["kind"].as_str(),Some("capture"|"evidence")){return Err(CoreError::new("INVALID_STATE","证据尚未生成"));}
        let id=self.new_job_id();let mut jobs=self.jobs.lock().unwrap();if jobs.active.values().filter(|(k,_)|k!="cleanup").count()>=2{return Err(CoreError::new("CAPACITY","诊断任务已满"));}jobs.active.insert(id.clone(),("export".into(),Arc::new(AtomicBool::new(false))));drop(jobs);
        let mut state=job(&id,"export",json!({"packageId":package}));state["phase"]=json!("selectingDestination");if let Err(e)=self.job_save(&state){self.jobs.lock().unwrap().active.remove(&id);return Err(e);}Ok(state)
    }
    fn export_finish(&self,id:&str,state:&str,code:Option<&str>)->Result<Value>{let mut value=self.job_get(&json!({"jobId":id}))?["job"].clone();if !matches!(state,"succeeded"|"failed"|"canceled"){return Err(CoreError::new("INVALID_STATE","导出终态无效"));}value["state"]=json!(state);value["phase"]=json!("finished");value["updatedAt"]=json!(crate::model::now_ms());value["error"]=code.map(|c|json!({"code":c,"message":"证据导出未完成"})).unwrap_or(Value::Null);self.complete_job(&mut value,false)?;Ok(value)}
    pub(crate) fn recover_jobs(&self)->Result<()> {self.write(Partition::Diagnostics,8192,|db|{let tx=db.diagnostics.transaction()?;tx.execute("DELETE FROM evidence_chunks WHERE job_id IN(SELECT id FROM observation_jobs WHERE state IN('queued','running'))",[])?;tx.execute("UPDATE observation_jobs SET state='interrupted',data=json_set(data,'$.state','interrupted','$.phase','finished') WHERE state IN('queued','running')",[])?;tx.commit()?;Ok(Value::Null)})?;Ok(())}
}

fn add_gap(state:&mut Value,reason:&str){let gaps=state["manifest"]["gaps"].as_array_mut().unwrap();if !gaps.contains(&json!(reason)){gaps.push(json!(reason));}}
struct EvidenceOutput<'a>{store:&'a Observability,id:&'a str,buffer:String,pending_records:usize,records:usize,position:usize,total:usize}
impl<'a> EvidenceOutput<'a>{
    fn new(store:&'a Observability,id:&'a str)->Self{Self{store,id,buffer:String::with_capacity(60*1024),pending_records:0,records:0,position:0,total:0}}
    fn push(&mut self,value:Value,event:bool)->Result<()>{
        let text=value.to_string()+"\n";
        if text.len()>60*1024||self.total+self.buffer.len()+text.len()>50*1024*1024{return Err(CoreError::new("CAPACITY","证据输出超限"));}
        if self.buffer.len()+text.len()>60*1024{self.flush()?;}
        self.buffer.push_str(&text);if event{self.pending_records+=1;}Ok(())
    }
    fn flush(&mut self)->Result<()>{
        if self.buffer.is_empty(){return Ok(());}
        self.store.write_evidence_chunk(self.id,self.position,self.buffer.as_bytes().to_vec())?;
        self.total+=self.buffer.len();self.position+=1;self.records+=self.pending_records;self.pending_records=0;self.buffer.clear();Ok(())
    }
}

#[cfg(test)]mod tests{
    use super::*;
    fn fixture()->(tempfile::TempDir,Arc<Observability>){let root=tempfile::tempdir().unwrap();Observability::initialize_fixture(root.path()).unwrap();let store=Observability::open(root.path(),"boot".into(),"config".into()).unwrap();(root,store)}
    #[test]fn failed_chunk_never_exposes_a_partial_record_and_keeps_manifest_space(){
        let(_root,store)=fixture();let id=store.new_job_id();let initial=job(&id,"evidence",json!({"gaps":[]}));store.job_save(&initial).unwrap();let copy=id.clone();store.write(Partition::Diagnostics,1024,move|db|{db.reserve_evidence(&copy)?;Ok(Value::Null)}).unwrap();
        let mut out=EvidenceOutput::new(&store,&id);out.push(json!({"recordType":"package"}),false).unwrap();out.flush().unwrap();
        out.push(json!({"recordType":"event","text":"测".repeat(10000)}),true).unwrap();
        store.write(Partition::Diagnostics,1024,|db|{db.diagnostics.execute_batch("CREATE TRIGGER fail_chunk BEFORE INSERT ON evidence_chunks WHEN NEW.position>0 BEGIN SELECT RAISE(ABORT,'injected');END;")?;Ok(Value::Null)}).unwrap();
        assert!(out.flush().is_err());assert_eq!(out.records,0);assert_eq!(out.position,1);
        let read=store.job_get(&json!({"jobId":id})).unwrap();let text=read["chunks"][0].as_str().unwrap();assert_eq!(serde_json::from_str::<Value>(text).unwrap()["recordType"],"package");
        store.write(Partition::Diagnostics,1024,|db|{db.diagnostics.execute_batch("DROP TRIGGER fail_chunk")?;Ok(Value::Null)}).unwrap();
        let mut final_state=initial;final_state["state"]=json!("partial");store.finish_manifest(&mut final_state).unwrap();store.complete_job(&mut final_state,false).unwrap();assert_eq!(store.job_get(&json!({"jobId":id})).unwrap()["chunks"].as_array().unwrap().len(),2);store.shutdown();
    }
    #[test]fn cancellation_and_terminal_failure_are_atomic_and_retry_after_recovery(){
        let(_root,store)=fixture();let id=store.new_job_id();let mut state=job(&id,"evidence",json!({"gaps":[]}));store.job_save(&state).unwrap();let cancel=Arc::new(AtomicBool::new(false));store.jobs.lock().unwrap().active.insert(id.clone(),("evidence".into(),cancel));
        let copy=id.clone();store.write(Partition::Diagnostics,1024,move|db|{db.reserve_evidence(&copy)?;Ok(Value::Null)}).unwrap();store.write_evidence_chunk(&id,0,b"{}\n".to_vec()).unwrap();
        store.job_cancel(&json!({"jobId":id})).unwrap();state["state"]=json!("succeeded");state["phase"]=json!("finished");
        store.write(Partition::Diagnostics,1024,|db|{db.diagnostics.execute_batch("CREATE TRIGGER fail_terminal BEFORE UPDATE ON observation_jobs BEGIN SELECT RAISE(ABORT,'injected');END;")?;Ok(Value::Null)}).unwrap();
        assert!(store.complete_job(&mut state,true).is_err());assert_eq!(state["state"],"canceled");assert!(store.jobs.lock().unwrap().active.contains_key(&id));
        assert_eq!(store.job_get(&json!({"jobId":id})).unwrap()["chunks"].as_array().unwrap().len(),1);
        store.write(Partition::Diagnostics,1024,|db|{db.diagnostics.execute_batch("DROP TRIGGER fail_terminal")?;Ok(Value::Null)}).unwrap();store.jobs.lock().unwrap().retry_at=None;store.retry_terminal_jobs();
        let result=store.job_get(&json!({"jobId":id})).unwrap();assert_eq!(result["job"]["state"],"canceled");assert_eq!(result["chunks"],json!([]));assert!(!store.jobs.lock().unwrap().active.contains_key(&id));store.shutdown();
    }
}
