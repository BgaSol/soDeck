use super::{model,schema::Partition,writer::Databases,Observability};
use crate::{CoreError,Result};
use serde_json::{json,Value};
use rusqlite::OptionalExtension;
use std::sync::{Arc,atomic::{AtomicBool,Ordering}};

/// Bounded maintenance on the same owner as ingestion. Active calls and traces
/// cannot be removed while their final facts are still pending.
pub(super) fn maintain(db:&mut Databases)->Result<()> {
    let now=crate::model::now_ms();let policy=super::policy::read(&db.facts)?;
    for partition in [Partition::Facts,Partition::Logs]{
        let cutoff=now.saturating_sub(policy[if partition==Partition::Facts{"factRetentionDays"}else{"logRetentionDays"}].as_u64().unwrap_or(7)*86400000)as i64;
        let connection=if partition==Partition::Facts{&db.facts}else{&db.logs};
        if partition==Partition::Facts{
            connection.execute("DELETE FROM records WHERE event_id IN(SELECT event_id FROM records WHERE received<? AND call_id IS NULL AND NOT EXISTS(SELECT 1 FROM observation_spans s WHERE s.trace_id=records.trace_id AND s.outcome IS NULL) ORDER BY sequence LIMIT 32)",[cutoff])?;
            let ids={let mut stmt=connection.prepare("SELECT id FROM calls WHERE ended IS NOT NULL AND NOT EXISTS(SELECT 1 FROM records r JOIN observation_spans s ON s.trace_id=r.trace_id WHERE r.call_id=calls.id AND s.outcome IS NULL) AND ended<? ORDER BY ended LIMIT 16")?;let rows=stmt.query_map([cutoff],|r|r.get::<_,String>(0))?;rows.collect::<std::result::Result<Vec<_>,_>>()?};
            for id in ids{let tx=connection.unchecked_transaction()?;tx.execute("DELETE FROM records WHERE call_id=?",[&id])?;tx.execute("DELETE FROM calls WHERE id=?",[id])?;tx.commit()?;}
            connection.execute("DELETE FROM observation_spans WHERE span_id IN(SELECT span_id FROM observation_spans WHERE outcome IS NOT NULL AND touched<? LIMIT 32)",[cutoff])?;
        }else{connection.execute("DELETE FROM records WHERE event_id IN(SELECT event_id FROM records WHERE received<? OR (level IN('trace','debug') AND received<?) ORDER BY sequence LIMIT 32)",rusqlite::params![cutoff,now.saturating_sub(86400000)as i64])?;}
    }
    let cutoff=now.saturating_sub(policy["diagnosticRetentionDays"].as_u64().unwrap_or(7)*86400000)as i64;
    db.diagnostics.execute("DELETE FROM observation_snapshots WHERE id IN(SELECT id FROM observation_snapshots WHERE received<? LIMIT 32)",[cutoff])?;
    db.diagnostics.execute("DELETE FROM observation_jobs WHERE id IN(SELECT id FROM observation_jobs WHERE state NOT IN('queued','running') AND id NOT IN(SELECT json_extract(data,'$.manifest.packageId') FROM observation_jobs WHERE state IN('queued','running') AND json_extract(data,'$.kind')='export') AND updated<? LIMIT 16)",[cutoff])?;
    db.diagnostics.execute("DELETE FROM metric_buckets WHERE rowid IN(SELECT rowid FROM metric_buckets WHERE minute<? LIMIT 32)",[cutoff])?;
    db.diagnostics.execute("DELETE FROM metric_closed WHERE series NOT IN(SELECT DISTINCT series FROM metric_buckets) AND window<?",[now.saturating_sub(900000)as i64])?;
    db.facts.execute("DELETE FROM diagnostic_targets WHERE reference IN(SELECT reference FROM diagnostic_targets WHERE created<? AND reference NOT IN(SELECT reference FROM record_targets) LIMIT 32)",[now.saturating_sub(30*86400000)as i64])?;
    for connection in [&db.facts,&db.logs]{connection.execute("DELETE FROM type_catalog WHERE rowid IN(SELECT rowid FROM type_catalog t WHERE NOT EXISTS(SELECT 1 FROM records r INDEXED BY records_type WHERE r.event_type=t.name AND (t.plugin_id='' OR COALESCE(r.plugin_id,'')=t.plugin_id) AND json_extract(r.envelope,'$.typeVersion')=t.version) LIMIT 32)",[])?;}
    db.diagnostics.execute("DELETE FROM type_catalog WHERE rowid IN(SELECT rowid FROM type_catalog t WHERE NOT EXISTS(SELECT 1 FROM observation_snapshots s WHERE s.plugin_id=t.plugin_id AND s.type=t.name) AND NOT EXISTS(SELECT 1 FROM metric_buckets m WHERE m.plugin_id=t.plugin_id AND m.type=t.name) LIMIT 32)",[])?;
    for connection in [&db.facts,&db.logs,&db.diagnostics]{connection.execute_batch("PRAGMA incremental_vacuum(16)")?;}
    Ok(())
}
impl Observability {
    pub(super) fn cleanup_start(self:&Arc<Self>,p:&Value)->Result<Value>{
        if p["storeEpoch"]!=self.epoch{return Err(CoreError::new("STORE_EPOCH_CHANGED","观测存储已重建"));}
        let filter=p["filter"].clone();let high=self.watermarks()?["high"].as_u64().unwrap();let id=self.new_job_id();let cancel=Arc::new(AtomicBool::new(false));
        {let mut jobs=self.jobs.lock().unwrap();if jobs.active.values().any(|(kind,_)|kind=="cleanup"){return Err(CoreError::new("CAPACITY","清理任务正在运行"));}jobs.active.insert(id.clone(),("cleanup".into(),cancel.clone()));}
        let now=crate::model::now_ms();let initial=json!({"jobId":id,"kind":"cleanup","state":"queued","phase":"queued","createdAt":now,"updatedAt":now,"manifest":{"highWatermark":high.to_string(),"removed":0,"protected":0},"error":null});
        if let Err(e)=self.job_save(&initial){self.jobs.lock().unwrap().active.remove(&id);return Err(e);}
        let store=self.clone();let mut job=initial.clone();std::thread::spawn(move||{
            job["state"]=json!("running");job["phase"]=json!("deleting");let _=store.job_save(&job);let mut after=0;let began=std::time::Instant::now();
            let result=(||->Result<()>{loop{
                if cancel.load(Ordering::Relaxed){return Err(CoreError::new("CANCELED","清理已取消"));}if began.elapsed().as_secs()>=300{return Err(CoreError::new("CAPACITY","清理达到时间上限"));}
                let water=store.watermarks()?;let fingerprint=model::hash(&json!(["cleanup",filter,true]));
                let cursor=model::seal(&store.cursor_key,&json!({"epoch":store.epoch,"fingerprint":fingerprint,"expires":crate::model::now_ms()+300000,"high":high,"prune":water["prune"],"after":after}));
                let page=store.query_v2("cleanup",&json!({"filter":filter,"limit":32,"ascending":true,"cursor":cursor}))?;
                let records=page["items"].as_array().unwrap();if records.is_empty(){break;}after=records.last().unwrap()["sequence"].as_str().unwrap().parse().unwrap();let records=records.clone();
                let result=store.write(Partition::Facts,8192,move|db|{
                    let mut removed=0;let mut protected=0;
                    for record in records{
                        let trace=record["traceId"].as_str();
                        let pinned:bool=db.facts.query_row("SELECT EXISTS(SELECT 1 FROM observation_spans WHERE trace_id=? AND outcome IS NULL)",[trace],|r|r.get(0))?;
                        if pinned{protected+=1;continue;}
                        if let Some(call)=record["callId"].as_str(){
                            let ended:Option<Option<i64>>=db.facts.query_row("SELECT ended FROM calls WHERE id=?",[call],|r|r.get(0)).optional()?;
                            if ended==Some(None){protected+=1;continue;}
                            let tx=db.facts.transaction()?;removed+=tx.execute("DELETE FROM records WHERE call_id=?",[call])?;tx.execute("DELETE FROM calls WHERE id=?",[call])?;tx.commit()?;
                        }else{let conn=if record["kind"]=="log"{&db.logs}else{&db.facts};removed+=conn.execute("DELETE FROM records WHERE event_id=?",[record["eventId"].as_str()])?;}
                    }Ok(json!({"removed":removed,"protected":protected}))
                })?;
                for key in ["removed","protected"]{job["manifest"][key]=json!(job["manifest"][key].as_u64().unwrap()+result[key].as_u64().unwrap());}
                if page["nextCursor"].is_null(){break;}
            }Ok(())})();
            job["updatedAt"]=json!(crate::model::now_ms());job["phase"]=json!("finished");job["state"]=json!(match &result{Ok(())if job["manifest"]["protected"]==0=>"succeeded",Ok(())=>"partial",Err(e)if e.code=="CANCELED"=>"canceled",Err(_)=>"partial"});
            if let Err(e)=result{job["error"]=json!({"code":e.code,"message":"清理未完成，已提交的删除不会回滚"});}
            let _=store.complete_job(&mut job,true);if store.observe_internal(&crate::Caller::shell(),"core.observation.cleanup.completed",json!({"jobId":id,"removed":job["manifest"]["removed"],"protected":job["manifest"]["protected"],"state":job["state"]})).is_err(){store.management_tail_failed();}
        });Ok(initial)
    }
}


#[cfg(test)]mod tests{
    use super::*;
    #[test]
    fn concurrent_cleanup_never_claims_incomplete_evidence_is_complete(){
        use std::time::{Duration,Instant};
        let root=tempfile::tempdir().unwrap();let core=crate::Core::open(root.path()).unwrap();let store=&core.observability;
        for start in (0..1024).step_by(32){
            let key=store.signing_key.clone();let epoch=store.epoch.clone();let config=store.config_epoch.clone();
            store.write(Partition::Facts,64*1024,move|db|{
                let tx=db.facts.transaction()?;super::super::catalog::save_type(&tx,"","events",&super::super::builtin::definition("core.observation.cleanup.completed")?)?;
                for n in start..start+32 {let mut event=model::envelope(&crate::Caller::shell(),"cleanup-boot",&epoch,"domain","core.observation.cleanup.completed",json!({"removed":n,"protected":0,"state":"succeeded","jobId":"fixture"}));event["configEpoch"]=json!(config);model::insert(&tx,&key,"f",&mut event,None)?;}
                tx.commit()?;Ok(Value::Null)
            }).unwrap();
        }
        let evidence=store.create_evidence(&json!({"filter":{"kind":"domain"},"allowPartial":true}),false).unwrap();
        assert!(matches!(store.job_get(&json!({"jobId":evidence["jobId"]})).unwrap()["job"]["state"].as_str(),Some("queued"|"running")));
        let cleanup=store.cleanup_start(&json!({"storeEpoch":store.epoch,"filter":{"kind":"domain"}})).unwrap();
        let wait=|id:&Value|{let start=Instant::now();loop{let value=store.job_get(&json!({"jobId":id})).unwrap();if !matches!(value["job"]["state"].as_str(),Some("queued"|"running")){break value;}assert!(start.elapsed()<Duration::from_secs(15));std::thread::sleep(Duration::from_millis(5));}};
        let collected=wait(&evidence["jobId"]);let cleaned=wait(&cleanup["jobId"]);assert_eq!(cleaned["job"]["state"],"succeeded");assert_eq!(cleaned["job"]["manifest"]["removed"],1024);
        let state=&collected["job"];if state["state"]=="succeeded"{assert_eq!(state["manifest"]["eventCount"],1024);}else{
            assert_eq!(state["state"],"partial","{collected}");assert_eq!(state["error"]["code"],"CURSOR_EXPIRED");assert!(state["manifest"]["gaps"].as_array().unwrap().contains(&json!("capture-incomplete")));
        }
        let export=core.begin_evidence_export(evidence["jobId"].as_str().unwrap()).unwrap();assert_eq!(export["state"],"queued");
        core.finish_evidence_export(export["jobId"].as_str().unwrap(),"canceled",Some("CANCELED")).unwrap();core.shutdown();
    }
    #[test]
    fn cancel_cleanup_after_committed_deletes_preserves_frozen_evidence_and_export(){
        use std::time::{Duration,Instant};
        let root=tempfile::tempdir().unwrap();let core=crate::Core::open(root.path()).unwrap();let store=&core.observability;
        for start in (0..1024).step_by(32){
        let key=store.signing_key.clone();let epoch=store.epoch.clone();let config=store.config_epoch.clone();
        store.write(Partition::Facts,64*1024,move|db|{
            let tx=db.facts.transaction()?;
            super::super::catalog::save_type(&tx,"","events",&super::super::builtin::definition("core.observation.cleanup.completed")?)?;
            for n in start..start+32 {let mut event=model::envelope(&crate::Caller::shell(),"cleanup-boot",&epoch,"domain","core.observation.cleanup.completed",json!({"removed":n,"protected":0,"state":"succeeded","jobId":"fixture"}));event["configEpoch"]=json!(config);model::insert(&tx,&key,"f",&mut event,None)?;}
            tx.commit()?;Ok(Value::Null)
        }).unwrap();}
        let wait=|id:&Value|{let start=Instant::now();loop{let value=store.job_get(&json!({"jobId":id})).unwrap();if !matches!(value["job"]["state"].as_str(),Some("queued"|"running")){break value;}assert!(start.elapsed()<Duration::from_secs(15));std::thread::sleep(Duration::from_millis(5));}};
        let evidence=store.create_evidence(&json!({"filter":{"kind":"domain"},"allowPartial":true}),false).unwrap();
        let frozen=wait(&evidence["jobId"]);assert_eq!(frozen["job"]["state"],"succeeded");
        let export=core.begin_evidence_export(evidence["jobId"].as_str().unwrap()).unwrap();
        let count=||store.queries.query(Partition::Facts,|db|Ok(json!(db.query_row("SELECT count(*) FROM records WHERE kind='domain'",[],|r|r.get::<_,i64>(0))?))).unwrap().as_u64().unwrap();
        let cleanup=store.cleanup_start(&json!({"storeEpoch":store.epoch,"filter":{"kind":"domain"}})).unwrap();
        let start=Instant::now();loop{if count()<1024{break;}assert!(start.elapsed()<Duration::from_secs(5));std::thread::yield_now();}
        store.job_cancel(&json!({"jobId":cleanup["jobId"]})).unwrap();
        let result=wait(&cleanup["jobId"]);assert_eq!(result["job"]["state"],"canceled");
        let removed=result["job"]["manifest"]["removed"].as_u64().unwrap();assert!(removed>0&&removed<1024,"{result}");assert!(count()>0);
        assert_eq!(store.job_get(&json!({"jobId":evidence["jobId"]})).unwrap(),frozen);
        assert_eq!(store.job_get(&json!({"jobId":export["jobId"]})).unwrap()["job"]["state"],"queued");
        core.finish_evidence_export(export["jobId"].as_str().unwrap(),"canceled",Some("CANCELED")).unwrap();core.shutdown();
    }
    #[test]fn retaining_core_facts_keeps_definitions_when_the_observed_caller_is_a_plugin(){
        let root=tempfile::tempdir().unwrap();Observability::initialize_fixture(root.path()).unwrap();let store=Observability::open(root.path(),"boot".into(),"config".into()).unwrap();let caller=crate::Caller{source:"plugin".into(),plugin_id:Some("fixture".into()),plugin_version:Some("1.0.0".into()),instance_id:"producer".into(),view_id:None,mount_generation:None};
        store.observe_internal(&caller,"core.plugin.ended",json!({"phase":"stop"})).unwrap();store.write(Partition::Facts,8192,|db|{maintain(db)?;Ok(Value::Null)}).unwrap();assert_eq!(store.types(&json!({"pluginId":"","type":"core.plugin.ended"}),true).unwrap()["type"],"core.plugin.ended");store.shutdown();
    }
}

#[cfg(test)]mod boundaries{
    use super::*;
    #[test]fn day_week_month_retention_preserves_active_calls_and_exports(){
        let root=tempfile::tempdir().unwrap();Observability::initialize_fixture(root.path()).unwrap();let store=Observability::open(root.path(),"boot".into(),"config".into()).unwrap();let active=store.call_start(&crate::Caller::shell(),"active",&json!({})).unwrap();let completed=store.call_start(&crate::Caller::shell(),"done",&json!({})).unwrap();store.call_finish(&completed,&Ok(Value::Null)).unwrap();
        let key=store.signing_key.clone();let epoch=store.epoch.clone();let active_copy=active.clone();let completed_copy=completed.clone();
        store.write(Partition::Facts,8192,move|db|{
            let now=crate::model::now_ms();let day=86400000;
            let old=(now-31*day) as i64;
            let package_time=(now-8*day) as i64;
            for (name,kind,level,age)in [("debug-old","log","debug",day+1),("debug-fresh","log","debug",day-60000),("info-old","log","info",8*day),("info-fresh","log","info",6*day),("fact-old","domain","info",31*day),("fact-fresh","domain","info",29*day)]{
                let mut e=model::envelope(&crate::Caller::shell(),"boot",&epoch,kind,name,json!({}));e["configEpoch"]=json!("config");e["receivedAt"]=json!(now-age);e["level"]=json!(level);model::insert(if kind=="log"{&db.logs}else{&db.facts},&key,if kind=="log"{"l"}else{"f"},&mut e,None)?;
            }
            db.facts.execute("UPDATE calls SET started=?,ended=CASE WHEN id=? THEN NULL ELSE ? END WHERE id IN(?,?)",rusqlite::params![old,active_copy,old,active_copy,completed_copy])?;
            db.facts.execute("UPDATE records SET received=? WHERE call_id IN(?,?)",rusqlite::params![old,active_copy,completed_copy])?;
            for(id,state,data)in [("kept-package","succeeded",json!({"kind":"evidence"})),("old-package","succeeded",json!({"kind":"evidence"})),("export","running",json!({"kind":"export","manifest":{"packageId":"kept-package"}}))]{db.diagnostics.execute("INSERT INTO observation_jobs VALUES(?,?,?,?,?)",rusqlite::params![id,package_time,package_time,state,data.to_string()])?;}
            maintain(db)?;Ok(Value::Null)
        }).unwrap();
        let facts=store.queries.query(Partition::Facts,move|db|{let active:i64=db.query_row("SELECT count(*) FROM calls WHERE id=?",[active],|r|r.get(0))?;let done:i64=db.query_row("SELECT count(*) FROM calls WHERE id=?",[completed],|r|r.get(0))?;let names=db.prepare("SELECT event_type FROM records WHERE call_id IS NULL ORDER BY event_type")?.query_map([],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;Ok(json!({"active":active,"done":done,"names":names}))}).unwrap();assert_eq!(facts,json!({"active":1,"done":0,"names":["fact-fresh"]}));
        let logs=store.queries.query(Partition::Logs,|db|{let names=db.prepare("SELECT event_type FROM records ORDER BY event_type")?.query_map([],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;Ok(json!(names))}).unwrap();assert_eq!(logs,json!(["debug-fresh","info-fresh"]));
        let jobs=store.queries.query(Partition::Diagnostics,|db|{let ids=db.prepare("SELECT id FROM observation_jobs ORDER BY id")?.query_map([],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;Ok(json!(ids))}).unwrap();assert_eq!(jobs,json!(["export","kept-package"]));store.shutdown();
    }
}
