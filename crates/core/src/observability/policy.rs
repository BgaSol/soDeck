use super::{schema::Partition,Observability};
use crate::{CoreError,Result};
use serde_json::{json,Value};
use rusqlite::OptionalExtension;
pub(super) fn default_policy()->Value{json!({"revision":"1","logLevel":"info","debugUntil":null,"factRetentionDays":30,"logRetentionDays":7,"diagnosticRetentionDays":7,"sourceOverrides":[]})}
pub(super) fn read(db:&rusqlite::Connection)->Result<Value>{let data:Option<String>=db.query_row("SELECT data FROM observation_policy WHERE id=1",[],|r|r.get(0)).optional()?;let mut p=data.map(|s|serde_json::from_str(&s)).transpose()?.unwrap_or_else(default_policy);if p["debugUntil"].as_u64().is_some_and(|t|t<=crate::model::now_ms()){p["logLevel"]=json!("info");p["debugUntil"]=Value::Null;}if let Some(values)=p["sourceOverrides"].as_array_mut(){values.retain(|v|v["debugUntil"].as_u64().is_some_and(|n|n>crate::model::now_ms()));}Ok(p)}
impl Observability {
    pub(super) fn policy(&self)->Result<Value>{self.queries.query(Partition::Facts,read)}
    pub(super) fn set_policy(&self,caller:&crate::Caller,p:&Value)->Result<Value>{
        let expected=p["expectedRevision"].as_str().unwrap().to_owned();let patch=p["patch"].clone();let caller=caller.clone();let boot=self.boot_id.clone();let epoch=self.epoch.clone();let config=self.config_epoch.clone();let key=self.signing_key.clone();let context=super::context::DiagnosticScope::capture();
        self.write(Partition::Facts,4096,move|db|{db.admit(Partition::Facts,4096)?;let tx=db.facts.transaction()?;let mut policy=read(&tx)?;
            if policy["revision"]!=expected{return Err(CoreError::new("CONFLICT","采集策略已经变化"));}
            for(k,v)in patch.as_object().unwrap(){policy[k]=v.clone();}
            if matches!(policy["logLevel"].as_str(),Some("trace"|"debug")){let now=crate::model::now_ms();if policy["debugUntil"].as_u64().is_none_or(|n|n<=now||n>now+86400000){return Err(CoreError::new("INVALID_PARAMS","调试采集到期必须在24小时内"));}}else{policy["debugUntil"]=Value::Null;}
            if let Some(overrides)=policy["sourceOverrides"].as_array(){let now=crate::model::now_ms();let mut ids=std::collections::HashSet::new();for entry in overrides{if !ids.insert(entry["sourceId"].as_str().unwrap())||entry["debugUntil"].as_u64().is_none_or(|t|t<=now||t>now+86400000){return Err(CoreError::new("INVALID_PARAMS","来源采集覆盖必须唯一且在24小时内到期"));}}}
            let revision=expected.parse::<u64>().ok().and_then(|v|v.checked_add(1)).ok_or_else(||CoreError::new("CAPACITY","策略修订已用尽"))?;policy["revision"]=json!(revision.to_string());tx.execute("INSERT OR REPLACE INTO observation_policy VALUES(1,?)",[policy.to_string()])?;let definition=super::builtin::definition("core.observation.policy.changed")?;super::catalog::save_type(&tx,"","events",&definition)?;
            let mut event=super::model::envelope(&caller,&boot,&epoch,"health","core.observation.policy.changed",json!({"revision":policy["revision"],"changedFields":patch.as_object().unwrap().keys().collect::<Vec<_>>()}));event["configEpoch"]=json!(config);event["provenance"]=json!("core-observed");if let Some(context)=context{super::context::attach(&mut event,&context);}super::model::insert(&tx,&key,"f",&mut event,None)?;tx.commit()?;Ok(policy)
        })
    }
    pub(super) fn describe(&self)->Result<Value>{let policy=self.policy()?;let catalog=self.catalog.lock().unwrap();let sources:Vec<_>=catalog.sources.iter().map(|(instance,(plugin,version,desc))|json!({"instanceId":instance,"pluginId":plugin,"pluginVersion":version,"coverage":if desc.is_some(){"full"}else{"audit-only"}})).collect();Ok(json!({"apiVersion":crate::capabilities::API_VERSION,"storeEpoch":self.epoch,"capabilities":["observability.events.v2","observability.context.v1","observability.snapshots.v1","observability.metrics.v1","observability.history-feed.v1","observability.evidence.v1","observability.management.v2"],"limits":{"eventBytes":8192,"batchItems":32,"batchBytes":131072,"instanceQueueBytes":262144,"writerQueueBytes":8388608,"traceMaxMs":1800000,"traceIdleMs":600000},"sources":sources,"policy":policy}))}
    pub(super) fn health_v2(&self)->Value{let mut health=self.health();let catalog=self.catalog.lock().unwrap();health["catalogBytes"]=json!(catalog.bytes);health["rejectedItems"]=json!(catalog.rejected.to_string());health["droppedItems"]=json!(catalog.dropped.to_string());health["activeSpans"]=json!(self.contexts.lock().unwrap().active.len());drop(catalog);
        health["sources"]=self.describe().ok().map(|v|v["sources"].clone()).unwrap_or_else(||json!([]));health["policy"]=self.policy().ok().unwrap_or(Value::Null);
        {let credits=self.credits.lock().unwrap();health["producerCredits"]=json!({"grantedBytes":credits.grants.values().map(|(_,n)|*n).sum::<usize>(),"budgetBytes":8*1024*1024,"endpoints":credits.grants.len()});}
        {let ring=self.metric_ring.lock().unwrap();health["metricRing"]=json!({"bytes":ring.bytes,"budgetBytes":8*1024*1024,"dropped":ring.dropped,"pruned":ring.pruned});}
        health["historyFeed"]=self.feed_health();health["automaticCapture"]=self.automatic_health();
        health["emergencyArchived"]=json!(self.emergency_archived().to_string());
        let ranges=[Partition::Facts,Partition::Logs].into_iter().filter_map(|partition|self.queries.query(partition,move|db|{
            let range=db.query_row("SELECT min(received),max(received),count(*) FROM records",[],|r|Ok((super::optional_integer(r,0)?,super::optional_integer(r,1)?,super::integer(r,2)?)))?;
            Ok(json!({"partition":partition.file(),"fromUtc":range.0,"toUtc":range.1,"count":range.2}))}).ok()).collect::<Vec<_>>();health["retained"]=json!(ranges);health}
}
