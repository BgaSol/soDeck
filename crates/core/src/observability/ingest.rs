use super::{catalog,context,descriptor,model,schema::Partition,Observability};
use crate::{Caller,CoreError,Result};
use serde_json::{json,Value};
use rusqlite::{params,OptionalExtension};
use std::{collections::HashMap,sync::{Mutex,OnceLock},time::Instant};
struct Admission {key:String}
struct Rate {tokens:f64,updated:Instant,busy:bool}
fn rates()->&'static Mutex<HashMap<String,Rate>>{static RATES:OnceLock<Mutex<HashMap<String,Rate>>>=OnceLock::new();RATES.get_or_init(Default::default)}
impl Admission {
    fn enter(key:String,count:usize)->Result<Self>{
        let mut all=rates().lock().unwrap();all.retain(|_,r|r.busy||r.updated.elapsed().as_secs()<60);
        if !all.contains_key(&key)&&all.len()>=256{return Err(CoreError::new("CAPACITY","采集来源配额已满"));}
        let rate=all.entry(key.clone()).or_insert(Rate{tokens:400.,updated:Instant::now(),busy:false});
        rate.tokens=(rate.tokens+rate.updated.elapsed().as_secs_f64()*200.).min(400.);rate.updated=Instant::now();
        if rate.busy||rate.tokens<count as f64{return Err(CoreError::new("CAPACITY","采集请求正在执行或超过速率"));}
        rate.tokens-=count as f64;rate.busy=true;Ok(Self{key})
    }
}
impl Drop for Admission {fn drop(&mut self){if let Some(rate)=rates().lock().unwrap().get_mut(&self.key){rate.busy=false;}}}
pub(super) fn admission(store:&Observability,caller:&Caller,count:usize)->Result<impl Drop>{Admission::enter(format!("{}:{}",store.epoch,caller.instance_id),count)}
impl Observability {
    pub(super) fn append(&self,caller:&Caller,p:&Value)->Result<Value>{
        let items=p["items"].as_array().ok_or_else(||CoreError::new("INVALID_PARAMS","缺少事件数组"))?;
        if items.len()>32||serde_json::to_vec(p)?.len()>128*1024{return Err(CoreError::new("CAPACITY","批次超限"));}
        let _admission=admission(self,caller,items.len())?;let policy=self.policy()?;let level=policy["sourceOverrides"].as_array().and_then(|values|values.iter().find(|v|v["sourceId"]==caller.instance_id).or_else(||values.iter().find(|v|v["sourceId"].as_str()==caller.plugin_id.as_deref()))).unwrap_or(&policy);
        let threshold=descriptor::LEVELS.iter().position(|v|Some(*v)==level["logLevel"].as_str()).unwrap_or(2);
        let mut ready=Vec::new();let mut receipts=Vec::new();
        for(index,item)in items.iter().enumerate(){
            let validation=(||->Result<_>{
                super::services::validate_item(item)?;
                if serde_json::to_vec(item)?.len()>8192{return Err(CoreError::new("CAPACITY","事件超过8KiB"));}
                for target in item["targets"].as_array().into_iter().flatten(){super::targets::validate(&self.signing_key,target)?;}
                let definition=self.definition(caller,"events",item["type"].as_str().unwrap(),item["typeVersion"].as_u64().unwrap()as u32)?;
                descriptor::check_fields(&definition.fields,&item["fields"]).map_err(|e|CoreError::new("INVALID_EVENT",e))?;
                if item["errorCode"].as_str().is_some_and(|s|!descriptor::identifier(s)){return Err(CoreError::new("INVALID_EVENT","错误码必须是稳定标识"));}
                let kind=definition.kind.as_deref().unwrap_or("domain");let level=item["level"].as_str().or(definition.level.as_deref()).unwrap_or("info");
                let resolved=item["contextRef"].as_str().map(|s|self.resolve_context(caller,s,false)).transpose();
                let (context,stale)=match resolved{Ok(v)=>(v,false),Err(e)if e.code=="STALE_CONTEXT"=>(None,true),Err(e)=>return Err(e)};
                let mut event=model::envelope(caller,&self.boot_id,&self.epoch,kind,&definition.name,item["fields"].clone());event["configEpoch"]=json!(self.config_epoch);event["typeVersion"]=json!(definition.type_version);event["level"]=json!(level);event["occurredAt"]=item["occurredAt"].clone();
                for k in ["clientEventId","outcome","errorCode","operationId","causeEventId"]{if let Some(v)=item.get(k){event[k]=v.clone();}}
                event["targets"]=item.get("targets").cloned().unwrap_or_else(||json!([]));
                if let Some(c)=context{context::attach(&mut event,&c);}
                let filtered=kind=="log"&&descriptor::LEVELS.iter().position(|v|*v==level).unwrap_or(2)<threshold;
                Ok((definition,event,filtered,stale))
            })();
            let client=item["clientEventId"].as_str().filter(|s|s.len()<=128).unwrap_or("invalid");
            match validation {Ok((_,_,true,_))=>receipts.push((index,json!({"clientEventId":client,"status":"filtered"}))),Ok((d,event,false,stale))=>ready.push((index,d,event,model::hash(item),stale)),Err(e)=>{self.catalog.lock().unwrap().rejected+=1;receipts.push((index,json!({"clientEventId":client,"status":"rejected","error":{"code":e.code,"message":e.message}})));}}
        }
        let key=self.signing_key.clone();let plugin=caller.plugin_id.clone().unwrap_or_default();let owner=caller.instance_id.clone();
        if !ready.is_empty(){let data=self.write(Partition::Facts,128*1024,move|db|{
            let mut results=Vec::new();let mut ready=std::collections::VecDeque::from(ready);
            while !ready.is_empty(){
                let log=ready.front().unwrap().2["kind"]=="log";let partition=if log{Partition::Logs}else{Partition::Facts};let mut group=Vec::new();let mut charge=0;
                while let Some(item)=ready.front(){let bytes=item.2.to_string().len()+2048;if (item.2["kind"]=="log")!=log||(!group.is_empty()&&charge+bytes>192*1024){break;}charge+=bytes;group.push(ready.pop_front().unwrap());}
                let denied=|index:usize,id:Value,error:&CoreError|json!({"index":index,"receipt":{"clientEventId":id,"status":"rejected","error":{"code":error.code,"message":error.message}}});
                if let Err(error)=db.admit(partition,charge){for(index,_,e,_,_)in group{results.push(denied(index,e["clientEventId"].clone(),&error));}continue;}
                let database=if log{&db.logs}else{&db.facts};
                let tx=match database.unchecked_transaction(){Ok(tx)=>tx,Err(error)=>{let error=CoreError::from(error);for(index,_,e,_,_)in group{results.push(denied(index,e["clientEventId"].clone(),&error));}continue;}};
                let mut receipts=Vec::new();
                for(index,d,mut event,hash,stale)in group{
                    let id=event["clientEventId"].clone();let result=(||->Result<Value>{
                        for database in [&db.facts,&db.logs]{let existing:Option<(String,String,i64)>=database.query_row("SELECT content_hash,event_id,sequence FROM records WHERE instance_id=? AND client_id=?",params![owner,id.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
                            if let Some((previous,event_id,sequence))=existing{if previous!=hash{return Err(CoreError::new("CONFLICT","clientEventId内容冲突"));}return Ok(json!({"clientEventId":id,"status":"duplicate","eventId":event_id,"sequence":sequence.to_string()}));}
                        }
                        if stale{return Err(CoreError::new("STALE_CONTEXT","新事件的诊断上下文已经结束"));}
                        tx.execute_batch("SAVEPOINT report_item")?;
                        let inserted=catalog::save_type(&tx,&plugin,"events",&d).and_then(|_|model::insert(&tx,&key,if log{"l"}else{"f"},&mut event,Some(&hash)));
                        if let Err(error)=inserted{tx.execute_batch("ROLLBACK TO report_item; RELEASE report_item")?;return Err(error);}tx.execute_batch("RELEASE report_item")?;
                        Ok(json!({"clientEventId":id,"status":"persisted","eventId":event["eventId"],"sequence":event["sequence"]}))
                    })();
                    receipts.push(match result{Ok(v)=>json!({"index":index,"receipt":v}),Err(error)=>denied(index,id,&error)});
                }
                if let Err(error)=tx.commit(){let error=CoreError::from(error);for receipt in &mut receipts{if receipt["receipt"]["status"]!="rejected"{*receipt=denied(receipt["index"].as_u64().unwrap()as usize,receipt["receipt"]["clientEventId"].clone(),&error);}}}
                results.extend(receipts);
            }
            Ok(json!(results))
        })?;for v in data.as_array().unwrap(){receipts.push((v["index"].as_u64().unwrap()as usize,v["receipt"].clone()));}}

        for(index,receipt) in &receipts {let item=&items[*index];if receipt["status"]=="persisted"&&item["outcome"]=="failed" {self.request_automatic(caller,&item.get("targets").cloned().unwrap_or_else(||json!([])),Some(receipt["eventId"].clone()));}}
        receipts.sort_by_key(|r|r.0);Ok(json!({"storeEpoch":self.epoch,"items":receipts.into_iter().map(|(_,r)|r).collect::<Vec<_>>(),"deduplication":"retained-events"}))
    }
}
