use super::{ingest,model,schema::Partition,Observability};
use crate::{Caller,CoreError,Result};
use rusqlite::{params,OptionalExtension};
use serde_json::{json,Value};
const RING_BUDGET:usize=8*1024*1024;
#[derive(Default)]pub(super) struct MetricRing {entries:std::collections::VecDeque<(String,u64,Value,usize,u64)>,pub bytes:usize,revision:u64,pub dropped:u64,pub pruned:u64}
impl MetricRing {
    fn prune(&mut self,now:u64){self.entries.retain(|(_,_,v,_,_)|v["toUtc"].as_u64().unwrap_or(0)>now.saturating_sub(900000));let bytes=self.entries.iter().map(|e|e.3).sum();if bytes!=self.bytes{self.pruned+=1;}self.bytes=bytes;}
}
impl Observability {
    pub(super) fn metric_query(&self,p:&Value)->Result<Value>{
        if p["resolutionMs"]!=10000{return self.data_query(p,true);}
        let now=crate::model::now_ms();let mut ring=self.metric_ring.lock().unwrap();ring.prune(now);
        let mut filter=p.clone();filter.as_object_mut().unwrap().remove("cursor");let hash=model::hash(&filter);
        let (high,after,prune)=if let Some(token)=p["cursor"].as_str(){let c=model::unseal(&self.cursor_key,token)?;
            if c["filter"]!=hash||c["expires"].as_u64().unwrap_or(0)<=now||c["prune"]!=ring.pruned{return Err(CoreError::new("CURSOR_EXPIRED","指标细节已淘汰或游标失效"));}(c["high"].as_u64().unwrap_or(0),c["after"].as_u64().unwrap_or(0),ring.pruned)
        }else{(ring.revision,0,ring.pruned)};
        let mut items=Vec::new();let mut bytes=2048;let limit=p["limit"].as_u64().unwrap_or(100).min(200)as usize;let mut last=after;let mut more=false;
        for(_,_,value,_,revision)in &ring.entries{
            if *revision<=after||*revision>high||["scope","type"].iter().any(|k|p.get(*k).is_some_and(|v|v!=&value[*k]))||p.get("pluginId").is_some_and(|v|v!=&value["source"]["pluginId"])||p["fromUtc"].as_u64().is_some_and(|v|value["fromUtc"].as_u64().unwrap()<v)||p["toUtc"].as_u64().is_some_and(|v|value["fromUtc"].as_u64().unwrap()>=v){continue;}
            let charge=value.to_string().len();if items.len()==limit||bytes+charge>256*1024{more=true;break;}bytes+=charge;items.push(value.clone());last=*revision;
        }
        let next=more.then(||model::seal(&self.cursor_key,&json!({"filter":hash,"expires":now+300000,"high":high,"after":last,"prune":prune})));
        Ok(json!({"items":items,"nextCursor":next,"coverage":model::coverage(true,vec!["reported-samples-only","memory-detail-current-boot"])}))
    }
    pub(super) fn metric_report(&self,caller:&Caller,p:&Value)->Result<Value>{
        let items=p["items"].as_array().unwrap();let _admission=ingest::admission(self,caller,items.len())?;let mut results=Vec::new();
        for item in items{let result=(||->Result<Value>{
            let d=self.definition(caller,"metrics",item["metricType"].as_str().unwrap(),item["typeVersion"].as_u64().unwrap()as u32)?;
            let labels=item["labels"].as_object().ok_or_else(||CoreError::new("INVALID_EVENT","指标标签无效"))?;
            if item["unit"]!=json!(d.unit)||labels.len()!=d.labels.len()||labels.iter().any(|(k,v)|!d.labels.get(k).is_some_and(|allowed|v.as_str().is_some_and(|s|allowed.iter().any(|a|a==s)))){return Err(CoreError::new("INVALID_EVENT","指标单位或标签无效"));}
            let from=item["fromUtc"].as_u64().unwrap();let to=item["toUtc"].as_u64().unwrap();let now=crate::model::now_ms();
            if from%10000!=0||to!=from+10000||to>now+1000||now.saturating_sub(to)>120000{return Err(CoreError::new("INVALID_EVENT","指标必须是最近2分钟内的完整10秒窗口"));}
            let kind=d.metric_kind.as_deref().unwrap();
            if (kind=="histogram"&&item.get("value").is_some())||(kind!="histogram"&&(item.get("buckets").is_some()||item.get("count").is_some())){return Err(CoreError::new("INVALID_EVENT","指标字段与种类不符"));}
            if kind=="histogram"{let buckets=item["buckets"].as_array().ok_or_else(||CoreError::new("INVALID_EVENT","缺少直方图桶"))?;if buckets.len()!=d.buckets.len()+1||buckets.iter().filter_map(Value::as_u64).sum::<u64>()!=item["count"].as_u64().unwrap_or(u64::MAX){return Err(CoreError::new("INVALID_EVENT","直方图样本数不一致"));}}
            else if item["value"].as_f64().is_none_or(|v|!v.is_finite()||(kind=="counter"&&v<0.)){return Err(CoreError::new("INVALID_EVENT","指标值无效"));}
            let series=model::hash(&json!([caller.instance_id,d.name,d.type_version,item["scope"],item["labels"]]));let hash=model::hash(item);let caller=caller.clone();let item=item.clone();let boot=self.boot_id.clone();
            let data=json!({"source":caller,"bootId":boot,"type":d.name,"typeVersion":d.type_version,"scope":item["scope"],"labels":item["labels"],"unit":d.unit,"kind":d.metric_kind,"fromUtc":from,"toUtc":to,"samples":1,"coveredMs":10000,"resolutionMs":10000,"value":item.get("value"),"buckets":item.get("buckets"),"count":item.get("count"),"definition":d});
            let ring_series=series.clone();let charge=data.to_string().len()*2+256;let mut ring=self.metric_ring.lock().unwrap();ring.prune(now);
            let full=ring.bytes+charge>RING_BUDGET;if full{ring.dropped+=1;}
            let receipt=self.write(Partition::Diagnostics,8192,move|db|{db.admit(Partition::Diagnostics,8192)?;let tx=db.diagnostics.transaction()?;
                let by_id:Option<String>=tx.query_row("SELECT content_hash FROM observation_metrics WHERE instance_id=? AND sample_id=?",params![caller.instance_id,item["sampleId"].as_str()],|r|r.get(0)).optional()?;
                if let Some(previous)=by_id{if previous!=hash{return Err(CoreError::new("CONFLICT","样本标识内容冲突"));}return Ok(json!({"sampleId":item["sampleId"],"status":"duplicate"}));}
                let closed:Option<u64>=tx.query_row("SELECT window FROM metric_closed WHERE series=?",[&series],|r|super::integer(r,0)).optional()?;if closed.is_some_and(|n|from<=n){return Err(CoreError::new("STALE_SAMPLE","指标窗口已封存"));}
                let previous:Option<String>=tx.query_row("SELECT content_hash FROM observation_metrics WHERE series=? AND window=?",params![series,from as i64],|r|r.get(0)).optional()?;
                if let Some(previous)=previous{if previous!=hash{return Err(CoreError::new("CONFLICT","指标窗口已经由不同样本结算"));}return Ok(json!({"sampleId":item["sampleId"],"status":"duplicate"}));}
                if full{return Err(CoreError::new("CAPACITY","指标内存环已满"));}
                let active:u32=tx.query_row("SELECT count(DISTINCT series) FROM metric_buckets WHERE json_extract(data,'$.source.instanceId')=? AND minute>=?",params![caller.instance_id,now.saturating_sub(900000)as i64],|r|r.get(0))?;
                let known:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM metric_buckets WHERE series=? AND minute>=?)",params![series,now.saturating_sub(900000)as i64],|r|r.get(0))?;
                if !known&&active>=128{return Err(CoreError::new("CAPACITY","来源指标序列超过128"));}
                let minute=from/60000*60000;let previous:Option<String>=tx.query_row("SELECT data FROM metric_buckets WHERE series=? AND minute=?",params![series,minute as i64],|r|r.get(0)).optional()?;
                let mut bucket=previous.map(|s|serde_json::from_str::<Value>(&s)).transpose()?.unwrap_or_else(||json!({"source":caller,"bootId":boot,"type":d.name,"typeVersion":d.type_version,"scope":item["scope"],"labels":item["labels"],"unit":d.unit,"kind":d.metric_kind,"fromUtc":minute,"toUtc":minute+60000,"resolutionMs":60000,"samples":0,"coveredMs":0,"lastWindow":0,"value":0.0,"buckets":vec![0u64;d.buckets.len()+1],"count":0,"definition":d}));
                let value=item["value"].as_f64().unwrap_or(0.);
                match d.metric_kind.as_deref().unwrap(){"counter"=>bucket["value"]=json!(bucket["value"].as_f64().unwrap_or(0.)+value),"gauge"=>{if from>=bucket["lastWindow"].as_u64().unwrap_or(0){bucket["value"]=json!(value);}},_=>{for(i,n)in item["buckets"].as_array().unwrap().iter().enumerate(){bucket["buckets"][i]=json!(bucket["buckets"][i].as_u64().unwrap_or(0)+n.as_u64().unwrap());}bucket["count"]=json!(bucket["count"].as_u64().unwrap_or(0)+item["count"].as_u64().unwrap());}}
                bucket["lastWindow"]=json!(from.max(bucket["lastWindow"].as_u64().unwrap_or(0)));bucket["samples"]=json!(bucket["samples"].as_u64().unwrap()+1);bucket["coveredMs"]=json!(bucket["coveredMs"].as_u64().unwrap()+10000);
                super::catalog::save_type(&tx,caller.plugin_id.as_deref().unwrap_or(""),"metrics",&d)?;
                tx.execute("INSERT INTO observation_metrics VALUES(?,?,?,?,?,?)",params![series,from as i64,item["sampleId"].as_str(),hash,item.to_string(),caller.instance_id])?;
                tx.execute("INSERT OR REPLACE INTO metric_buckets VALUES(?,?,?,?,?,?)",params![series,minute as i64,caller.plugin_id.unwrap_or_default(),item["scope"].as_str(),d.name,bucket.to_string()])?;
                let cutoff=now.saturating_sub(130000)as i64;tx.execute("INSERT INTO metric_closed SELECT series,MAX(window) FROM observation_metrics WHERE window<? GROUP BY series ON CONFLICT(series) DO UPDATE SET window=MAX(window,excluded.window)",[cutoff])?;tx.execute("DELETE FROM observation_metrics WHERE window<?",[cutoff])?;tx.commit()?;Ok(json!({"sampleId":item["sampleId"],"status":"persisted"}))
            })?;if receipt["status"]=="persisted"{ring.revision+=1;let revision=ring.revision;ring.entries.push_back((ring_series,from,data,charge,revision));ring.bytes+=charge;}Ok(receipt)
        })();results.push(match result{Ok(v)=>v,Err(e)=>json!({"sampleId":item["sampleId"],"status":"rejected","error":{"code":e.code,"message":e.message}})});}
        Ok(json!({"items":results}))
    }
}
