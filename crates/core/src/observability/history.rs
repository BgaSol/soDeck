use super::{model,schema::Partition,Observability};
use crate::{CoreError,Result};
use rusqlite::{params_from_iter,types::Value as Sql,OptionalExtension};
use serde_json::{json,Value};
pub(super) fn number(db:&rusqlite::Connection,key:&str)->Result<u64>{let text:String=db.query_row("SELECT value FROM meta WHERE key=?",[key],|r|r.get(0))?;text.parse().map_err(|_|CoreError::new("STORE_UNAVAILABLE","观测元数据无效"))}
const DIAGNOSTIC_PREFIXES:[&str;8]=["events.","traces.","observability.","metrics.","diagnostics.","evidence.","communication.","quality."];
const DURATION_BOUNDS:[f64;10]=[1.,5.,10.,50.,100.,500.,1000.,5000.,10000.,60000.];
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_epochs_and_legacy_projection_remain_explicit() {
        let root=tempfile::tempdir().unwrap();Observability::initialize_fixture(root.path()).unwrap();
        let caller=crate::Caller::shell();
        let first=Observability::open(root.path(),"boot-a".into(),"config-a".into()).unwrap();
        let call=first.call_start(&caller,"servers.list",&json!({})).unwrap();first.call_finish(&call,&Ok(json!({}))).unwrap();first.shutdown();drop(first);
        let store=Observability::open(root.path(),"boot-b".into(),"config-b".into()).unwrap();
        let call=store.call_start(&caller,"servers.list",&json!({})).unwrap();store.call_finish(&call,&Ok(json!({}))).unwrap();
        let current=store.query_v2("fixture",&json!({"filter":{"kind":"call"}})).unwrap();assert_eq!(current["items"].as_array().unwrap().len(),2);assert!(current["items"].as_array().unwrap().iter().all(|e|e["configEpoch"]=="config-b"));
        let both=store.query_v2("fixture",&json!({"filter":{"kind":"call","configEpochs":["config-a","config-b"]}})).unwrap();assert_eq!(both["items"].as_array().unwrap().len(),4);
        let now=crate::model::now_ms();let old=store.events("fixture",&json!({"configEpoch":"config-a","fromUtc":now-60000,"toUtc":now+1,"limit":100})).unwrap();
        assert_eq!(old["items"].as_array().unwrap().len(),2);
        for event in old["items"].as_array().unwrap(){assert_eq!(event["configEpoch"],"config-a");assert!(event.get("schemaVersion").is_none());assert!(event.get("clientEventId").is_none());assert!(event.get("level").is_none());}
        store.shutdown();
    }
    #[test]
    fn v2_cursors_expire_and_reject_other_store_epochs() {
        let root = tempfile::tempdir().unwrap();
        let core = crate::Core::open(root.path()).unwrap();
        let owner = "cursor-fixture";
        let store = &core.observability;
        let water = store.watermarks().unwrap();
        let now = crate::model::now_ms();
        let cursor = json!({"epoch":store.epoch,"fingerprint":model::hash(&json!([owner,{},false])),"expires":now+300000,"high":water["high"],"prune":water["prune"],"after":water["high"].as_u64().unwrap()+1});
        let query = |value:&Value| store.query_v2(owner, &json!({"filter":{},"cursor":model::seal(&store.cursor_key,value)}));
        assert!(query(&cursor).is_ok());
        let mut expired = cursor.clone();
        expired["expires"] = json!(now.saturating_sub(1));
        assert_eq!(query(&expired).unwrap_err().code,"CURSOR_EXPIRED");
        let mut rebuilt = cursor.clone();
        rebuilt["epoch"] = json!("prior-observation-store");
        assert_eq!(query(&rebuilt).unwrap_err().code,"STORE_EPOCH_CHANGED");
        let mut pruned = cursor.clone();
        pruned["prune"][0] = json!(water["prune"][0].as_u64().unwrap()+1);
        assert_eq!(query(&pruned).unwrap_err().code,"CURSOR_EXPIRED");
        assert_eq!(store.query_v2(owner,&json!({"filter":{"kind":"domain"},"cursor":model::seal(&store.cursor_key,&cursor)})).unwrap_err().code,"CURSOR_EXPIRED");
        assert_eq!(store.query_v2(owner,&json!({"filter":{},"cursor":"tampered"})).unwrap_err().code,"CURSOR_EXPIRED");
    }
}
#[derive(Default)]
struct AggregateGroup{count:u64,samples:u64,mean:f64,min:Option<f64>,max:Option<f64>,buckets:[u64;11]}
impl AggregateGroup{
    fn add(&mut self,event:&Value){
        self.count+=1;
        let duration=event["attributes"]["durationMs"].as_f64()
            .or_else(||event["attributes"]["summary"]["durationMs"].as_f64())
            .or_else(||event["attributes"]["sample"]["durationMs"].as_f64());
        if let Some(value)=duration.filter(|v|v.is_finite()&&*v>=0.){
            self.samples+=1;self.mean+=(value-self.mean)/self.samples as f64;self.min=Some(self.min.map_or(value,|old|old.min(value)));self.max=Some(self.max.map_or(value,|old|old.max(value)));
            self.buckets[DURATION_BOUNDS.iter().position(|upper|value<=*upper).unwrap_or(10)]+=1;
        }
    }
    fn value(self,key:String)->Value{json!({"key":key,"count":self.count.to_string(),"duration":{"unit":"ms","sampleCount":self.samples.to_string(),"min":self.min,"max":self.max,"mean":(self.samples>0).then_some(self.mean),"buckets":self.buckets.into_iter().enumerate().map(|(index,count)|json!({"upperBound":DURATION_BOUNDS.get(index),"count":count.to_string()})).collect::<Vec<_>>()}})}
}
pub(super) fn exclude_diagnostic_sql()->String{let method="COALESCE(json_extract(envelope,'$.attributes.method'),'')";let mut predicates=DIAGNOSTIC_PREFIXES.iter().map(|prefix|format!("{method} LIKE '{prefix}%'" )).collect::<Vec<_>>();predicates.push(format!("{method}='calls.list'"));format!(" AND NOT(kind='call' AND ({}))",predicates.join(" OR "))}
impl Observability {
    pub(super) fn watermarks(&self)->Result<Value>{let mut values=Vec::new();for p in [Partition::Facts,Partition::Logs]{values.push(self.queries.query(p,|db|Ok(json!({"sequence":number(db,"sequence")?,"prune":number(db,"pruneRevision")?})))?);}Ok(json!({"high":values.iter().map(|v|v["sequence"].as_u64().unwrap()).max().unwrap_or(0),"prune":[values[0]["prune"],values[1]["prune"]]}))}
    pub(super) fn query_v2(&self,owner:&str,p:&Value)->Result<Value>{self.query_v2_at(owner,p,None)}
    pub(super) fn query_v2_at(&self,owner:&str,p:&Value,known_water:Option<&Value>)->Result<Value>{
        let _budget=super::query::ReadBudget::enter();
        let filter=p["filter"].clone();let now=crate::model::now_ms();let from=filter["fromUtc"].as_u64().unwrap_or(now.saturating_sub(30*86400000));let to=filter["toUtc"].as_u64().unwrap_or(now.saturating_add(1));
        if to<=from||to-from>30*86400000+1{return Err(CoreError::new("INVALID_FILTER","查询范围必须在30天以内"));}
        let ascending=p["ascending"].as_bool().unwrap_or(false);let fingerprint=model::hash(&json!([owner,filter,ascending]));let water=if let Some(water)=known_water{water.clone()}else{self.watermarks()?};
        let cursor=if let Some(token)=p["cursor"].as_str(){let c=model::unseal(&self.cursor_key,token).map_err(|_|CoreError::new("CURSOR_EXPIRED","分页游标已失效"))?;if c["epoch"]!=self.epoch{return Err(CoreError::new("STORE_EPOCH_CHANGED","观测存储已重建"));}if c["fingerprint"]!=fingerprint||c["expires"].as_u64().unwrap_or(0)<=now||c["prune"]!=water["prune"]{return Err(CoreError::new("CURSOR_EXPIRED","查询已过期或历史已清理"));}c}else{json!({"epoch":self.epoch,"fingerprint":fingerprint,"expires":now+300000,"high":water["high"],"prune":water["prune"],"after":if ascending{0}else{water["high"].as_u64().unwrap()+1}})};
        let workspace_refs=if let Some(id)=filter["workspaceId"].as_str(){let id=id.to_owned();self.queries.query(Partition::Facts,move|db|{let mut s=db.prepare("SELECT reference FROM diagnostic_targets WHERE kind='workspace' AND object_id=? LIMIT 257")?;let refs=s.query_map([id],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;if refs.len()>256{return Err(CoreError::new("CAPACITY","工作区历史代次过多，请按diagnosticRef缩小范围"));}Ok(json!(refs))})?}else{json!([])};
        let limit=p["limit"].as_u64().unwrap_or(100).min(200)as usize;let mut items=Vec::new();let mut frontier:Option<u64>=None;let mut partition_more=false;let mut retained_from:Option<u64>=None;let mut retained_to:Option<u64>=None;
        for part in [Partition::Facts,Partition::Logs]{let f=filter.clone();let config=self.config_epoch.clone();let c=cursor.clone();let signing_key=self.signing_key.clone();let workspace_refs=workspace_refs.clone();let data=self.queries.query(part,move|db|{
            let tx=db.unchecked_transaction()?;let mut sql=format!("received>=? AND received<? AND sequence<=? AND sequence {} ?",if ascending{">"}else{"<"});let mut args:Vec<Sql>=vec![(from as i64).into(),(to as i64).into(),(c["high"].as_u64().unwrap()as i64).into(),(c["after"].as_u64().unwrap()as i64).into()];
            if f["timeField"]=="occurredAt"{sql=sql.replacen("received>=? AND received<?","occurred>=? AND occurred<?",1);}
            let epochs=f["configEpochs"].as_array().cloned().unwrap_or_else(||vec![json!(config)]);if epochs.is_empty(){return Err(CoreError::new("INVALID_FILTER","配置代次不能为空"));}
            sql.push_str(&format!(" AND config_epoch IN({})",vec!["?";epochs.len()].join(",")));args.extend(epochs.iter().filter_map(Value::as_str).map(|s|Sql::Text(s.into())));
            for(field,column)in [("bootId","boot_id"),("pluginId","plugin_id"),("instanceId","instance_id"),("kind","kind"),("type","event_type"),("level","level"),("outcome","outcome"),("errorCode","error_code"),("traceId","trace_id"),("callId","call_id"),("operationId","operation_id"),("clientEventId","client_id")]{if let Some(v)=f[field].as_str(){sql.push_str(&format!(" AND {column}=?"));args.push(v.to_owned().into());}}
            for(field,kind)in [("serverId","server"),("diagnosticRef","")]{if let Some(v)=f[field].as_str(){sql.push_str(" AND EXISTS(SELECT 1 FROM record_targets t WHERE t.event_id=records.event_id AND t.reference=?");args.push(if field=="serverId"{super::targets::reference(&signing_key,&config,"server",v,Some(&config))["ref"].as_str().unwrap().to_owned().into()}else{v.to_owned().into()});if !kind.is_empty(){sql.push_str(" AND t.kind=?");args.push(Sql::Text(kind.into()));}sql.push(')');}}
            if f.get("workspaceId").is_some(){let refs=workspace_refs.as_array().unwrap();if refs.is_empty(){sql.push_str(" AND 0=1");}else{sql.push_str(&format!(" AND EXISTS(SELECT 1 FROM record_targets t WHERE t.event_id=records.event_id AND t.kind='workspace' AND t.reference IN({}))",vec!["?";refs.len()].join(",")));args.extend(refs.iter().filter_map(Value::as_str).map(|s|Sql::Text(s.into())));}}
            // Filtering call methods happens in SQL, so a page of excluded control calls cannot hide later facts.
            if f["excludeDiagnosticCalls"]==true{sql.push_str(&exclude_diagnostic_sql());}
            if let Some(source)=f["source"].as_str(){sql.push_str(" AND json_extract(envelope,'$.source.source')=?");args.push(Sql::Text(source.into()));}
            let mut statement=tx.prepare(&format!("SELECT envelope FROM records WHERE {sql} ORDER BY sequence {} LIMIT ?",if ascending{"ASC"}else{"DESC"}))?;args.push(((limit+1)as i64).into());
            let rows=statement.query_map(params_from_iter(args),|r|r.get::<_,String>(0))?;let mut values=Vec::new();let mut bytes=0;let mut more=false;for row in rows{let row=row?;if bytes+row.len()>192*1024{more=true;break;}bytes+=row.len();values.push(serde_json::from_str::<Value>(&row)?);}
            let retained=tx.query_row("SELECT (SELECT received FROM records ORDER BY received LIMIT 1),(SELECT received FROM records ORDER BY received DESC LIMIT 1)",[],|r|Ok((super::optional_integer(r,0)?,super::optional_integer(r,1)?)))?;
            Ok(json!({"items":values,"more":more,"retainedFrom":retained.0,"retainedTo":retained.1,"prune":number(&tx,"pruneRevision")?}))
        })?;
            partition_more|=data["more"]==true;if data["more"]==true{if let Some(last)=data["items"].as_array().and_then(|v|v.last()).and_then(|v|v["sequence"].as_str()).and_then(|v|v.parse::<u64>().ok()){frontier=Some(frontier.map_or(last,|prior|if ascending{prior.min(last)}else{prior.max(last)}));}}let idx=if part==Partition::Facts{0}else{1};if data["prune"]!=cursor["prune"][idx]{return Err(CoreError::new("CURSOR_EXPIRED","读取期间历史已清理"));}
            if let Some(t)=data["retainedFrom"].as_u64(){retained_from=Some(retained_from.map_or(t,|v|v.min(t)));}if let Some(t)=data["retainedTo"].as_u64(){retained_to=Some(retained_to.map_or(t,|v|v.max(t)));}items.extend(data["items"].as_array().unwrap().iter().cloned());
        }
        if let Some(frontier)=frontier{items.retain(|v|{let sequence=v["sequence"].as_str().unwrap().parse::<u64>().unwrap();if ascending{sequence<=frontier}else{sequence>=frontier}});}
        items.sort_by_key(|v|v["sequence"].as_str().unwrap().parse::<u64>().unwrap());if !ascending{items.reverse();}
        let mut bytes=2048;let count=items.iter().take(limit).take_while(|v|{bytes+=v.to_string().len()+64;bytes<=256*1024}).count();let more=partition_more||items.len()>count;items.truncate(count);
        let next=if more{items.last().map(|v|{let mut c=cursor.clone();c["after"]=json!(v["sequence"].as_str().unwrap().parse::<u64>().unwrap());model::seal(&self.cursor_key,&c)})}else{None};
        if known_water.is_none()&&self.watermarks()?["prune"]!=cursor["prune"]{return Err(CoreError::new("CURSOR_EXPIRED","读取期间历史已清理"));}
        let partial=cursor["prune"].as_array().unwrap().iter().any(|v|v.as_u64().unwrap()>0);
        Ok(json!({"storeEpoch":self.epoch,"highWatermark":cursor["high"].as_u64().unwrap().to_string(),"retainedFromUtc":retained_from,"retainedToUtc":retained_to,"coverage":model::coverage(partial,if partial{vec!["history-pruned"]}else{vec![]}),"items":items,"nextCursor":next}))
    }
    pub(super) fn get_v2(&self,p:&Value)->Result<Value>{
        if p["storeEpoch"]!=self.epoch{return Err(CoreError::new("STORE_EPOCH_CHANGED","观测存储已重建"));}
        let id=p["eventId"].as_str().unwrap();for partition in [Partition::Facts,Partition::Logs]{let id=id.to_owned();let value=self.queries.query(partition,move|db|{let data:Option<String>=db.query_row("SELECT envelope FROM records WHERE event_id=?",[id],|r|r.get(0)).optional()?;data.map(|s|serde_json::from_str(&s).map_err(Into::into)).unwrap_or(Ok(Value::Null))})?;if !value.is_null(){return Ok(value);}}
        Err(CoreError::new(if model::valid_id(&self.signing_key,&self.epoch,id){"GONE"}else{"NOT_FOUND"},"记录不存在或已被清理"))
    }
    pub(super) fn aggregate_v2(&self,owner:&str,p:&Value)->Result<Value>{
        let _budget=super::query::ReadBudget::enter();let started=std::time::Instant::now();let now=crate::model::now_ms();let mut filter=p["filter"].clone();
        let from=filter["fromUtc"].as_u64().unwrap_or(now.saturating_sub(30*86400000));let to=filter["toUtc"].as_u64().unwrap_or(now.saturating_add(1));filter["fromUtc"]=json!(from);filter["toUtc"]=json!(to);
        let mut query=json!({"filter":filter,"limit":200});let mut groups=std::collections::BTreeMap::<String,AggregateGroup>::new();let mut first=None;
        loop{if started.elapsed().as_secs()>=2{return Err(CoreError::new("CAPACITY","统计超过2秒，请缩小范围"));}let page=self.query_v2(owner,&query)?;if first.is_none(){first=Some(page.clone());}
            for e in page["items"].as_array().unwrap(){let key=match p["groupBy"].as_str().unwrap(){"minute"=>(e["receivedAt"].as_u64().unwrap()/60000).to_string(),"hour"=>(e["receivedAt"].as_u64().unwrap()/3600000).to_string(),"source"=>e["source"]["pluginId"].as_str().unwrap_or("core").into(),field=>e[field].as_str().unwrap_or("unknown").into()};groups.entry(key).or_default().add(e);if groups.len()>200{return Err(CoreError::new("CAPACITY","统计分组超限"));}}
            if page["nextCursor"].is_null(){break;}query["cursor"]=page["nextCursor"].clone();
        }let first=first.unwrap();Ok(json!({"storeEpoch":self.epoch,"highWatermark":first["highWatermark"],"coverage":first["coverage"],"fromUtc":from,"toUtc":to,"groups":groups.into_iter().map(|(key,group)|group.value(key)).collect::<Vec<_>>()}))
    }
}
