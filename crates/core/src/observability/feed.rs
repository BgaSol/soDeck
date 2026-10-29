use super::{model,Observability};
use crate::{Caller,Core,CoreError,Result};
use serde_json::{json,Value};
use std::{collections::BTreeMap,sync::{Arc,atomic::{AtomicUsize,Ordering}},time::{Duration,Instant}};
#[derive(Default)]pub(super) struct Feeds {entries:BTreeMap<String,Feed>,round:usize,delivery_round:usize,pumps:u64,pump_us:u64,pump_max_us:u64,generated:u64,delivered:u64,acked:u64,budget_waits:u64,pending_peak:usize,ack_ms:[u64;8],transit:Arc<AtomicUsize>,transit_peak:usize}
struct Feed {owner:String,instance:String,filter:Value,epoch:String,checkpoint:String,after:u64,prune:Value,active:bool,touched:Instant,pending:Option<Pending>,last_sent:Option<Instant>,last_scan:u64}
struct Pending{event:String,header:Arc<str>,items:Vec<Arc<str>>,checkpoint:Option<String>,batch_id:Option<String>,bytes:usize}
impl Pending{
    fn new(event:String,payload:Value)->Self{let checkpoint=payload["checkpoint"].as_str().map(str::to_owned);let batch_id=payload["batchId"].as_str().map(str::to_owned);let header:Arc<str>=payload.to_string().into();let bytes=header.len()+2048;Self{event,header,items:Vec::new(),checkpoint,batch_id,bytes}}
    fn render(&self)->Arc<str>{if self.event!="events.v2.batch"{return self.header.clone();}let mut payload=String::with_capacity(self.bytes);payload.push_str(self.header.strip_suffix('}').unwrap());payload.push_str(",\"items\":[");for(n,raw)in self.items.iter().enumerate(){if n>0{payload.push(',');}payload.push_str(raw);}payload.push_str("]}");payload.into()}
}
struct Transit{bytes:usize,total:Arc<AtomicUsize>}
impl Drop for Transit{fn drop(&mut self){self.total.fetch_sub(self.bytes,Ordering::SeqCst);}}
pub struct ObservationDelivery {pub owner:String,pub event:String,pub payload:Arc<str>,pub retention:Arc<dyn Send+Sync>}
fn unique_pending_bytes<'a>(items:impl Iterator<Item=&'a Pending>)->usize{
    let mut seen=std::collections::HashSet::new();let mut total=0;
    for p in items{total+=p.header.len()+2048+p.items.capacity()*std::mem::size_of::<Arc<str>>();for raw in &p.items{if seen.insert(Arc::as_ptr(raw)as *const u8){total+=raw.len()+32;}}}total
}
pub(super) fn owner(caller:&Caller)->String{format!("{}:{}:{}",caller.instance_id,caller.view_id.as_deref().unwrap_or("worker"),caller.mount_generation.map_or(String::new(),|n|n.to_string()))}
impl Core {
    pub fn observation_feed(&self,caller:&Caller,endpoint:&str,method:&str,p:&Value)->Result<Value>{
        self.record(caller,method,p,||{
            super::services::validate("x-methods",method,p)?;
            let value=match method{"events.v2.open"=>self.observability.feed_open(caller,p,Some(endpoint)),"events.v2.ack"=>self.observability.feed_ack(caller,p,Some(endpoint)),"events.v2.close"=>self.observability.feed_close(caller,p,Some(endpoint)),_=>Err(CoreError::new("UNSUPPORTED_API","未知历史订阅方法"))}?;
            super::services::validate("x-results",method,&value)?;Ok(value)
        })
    }
    pub fn observation_deliveries(&self)->Vec<ObservationDelivery>{self.observability.feed_deliveries()}
    pub fn close_observation_owner(&self,owner:&str){self.observability.feeds.lock().unwrap().entries.retain(|_,f|f.owner!=owner);self.observability.credits.lock().unwrap().grants.remove(owner);}
}
impl Observability {
    pub(super) fn start_feed_pump(store:&Arc<Self>){let weak=Arc::downgrade(store);std::thread::spawn(move||{let mut cache=super::feed_cache::Cache::default();loop{std::thread::sleep(Duration::from_millis(50));let Some(store)=weak.upgrade()else{break};if store.stopping(){break;}store.expire_contexts();store.pump_feeds(&mut cache);store.pump_automatic();store.retry_terminal_jobs();}});}
    pub(super) fn feed_open(&self,caller:&Caller,p:&Value,endpoint:Option<&str>)->Result<Value>{
        let mut filter=p["filter"].clone();filter["excludeDiagnosticCalls"]=json!(true);
        // A separate default exclusion cannot be represented by kind=domain, which would hide actions.
        let exclude_call=filter.get("kind").is_none();filter["excludeCall"]=json!(exclude_call);
        let water=self.watermarks()?;let high=water["high"].as_u64().unwrap();
        let mut after=if p["from"]=="retained"{0}else{high};let mut prune=water["prune"].clone();
        if let Some(token)=p["checkpoint"].as_str(){let v=model::unseal(&self.signing_key,token)?;if v["epoch"]!=self.epoch{return Err(CoreError::new("STORE_EPOCH_CHANGED","订阅存储已重建"));}if v["filter"]!=filter{return Err(CoreError::new("INVALID_FILTER","checkpoint过滤条件不同"));}after=v["after"].as_u64().ok_or_else(||CoreError::new("INVALID_REFERENCE","checkpoint无效"))?;prune=v["prune"].clone();}
        let mut feeds=self.feeds.lock().unwrap();if feeds.entries.len()>=128||feeds.entries.values().filter(|f|f.instance==caller.instance_id).count()>=16{return Err(CoreError::new("CAPACITY","历史订阅已满"));}
        let id=crate::model::id();let epoch=crate::model::id();let checkpoint=model::seal(&self.signing_key,&json!({"epoch":self.epoch,"filter":filter,"after":after,"prune":prune}));
        feeds.entries.insert(id.clone(),Feed{owner:endpoint.map(str::to_owned).unwrap_or_else(||owner(caller)),instance:caller.instance_id.clone(),filter:filter.clone(),epoch:epoch.clone(),checkpoint:checkpoint.clone(),after,prune,active:false,touched:Instant::now(),pending:None,last_sent:None,last_scan:0});
        Ok(json!({"subscriptionId":id,"feedEpoch":epoch,"highWatermark":high.to_string(),"checkpoint":checkpoint,"effectiveFilter":filter}))
    }
    pub(super) fn feed_ack(&self,caller:&Caller,p:&Value,endpoint:Option<&str>)->Result<Value>{
        let mut feeds=self.feeds.lock().unwrap();let id=p["subscriptionId"].as_str().unwrap();let ack_age=feeds.entries.get(id).filter(|f|f.pending.is_some()).map(|f|f.touched.elapsed().as_millis()as u64);if let Some(age)=ack_age{feeds.acked+=1;feeds.ack_ms[[50,100,200,500,1000,2000,5000].iter().position(|n|age<=*n).unwrap_or(7)]+=1;}let feed=feeds.entries.get_mut(id).ok_or_else(||CoreError::new("FEED_CLOSED","订阅已经结束"))?;
        if feed.owner!=endpoint.map(str::to_owned).unwrap_or_else(||owner(caller))||p["feedEpoch"]!=feed.epoch{return Err(CoreError::new("SOURCE_INVALID","订阅不属于当前来源"));}
        if let Some(pending)=&feed.pending{let event=&pending.event;
            if event=="events.v2.closed"{return Err(CoreError::new("FEED_CLOSED","订阅已经结束"));}
            if event=="events.v2.gap"{if p["acceptGap"]!=true{return Err(CoreError::new("GAP_REQUIRES_ACK","需要明确接受历史缺口"));}}
            else if p["batchId"].as_str()!=pending.batch_id.as_deref(){if p["batchId"].as_str().is_none(){return Err(CoreError::new("INVALID_ACK","缺少批次标识"));}return Ok(json!({"acknowledged":false,"checkpoint":feed.checkpoint}));}
            let checkpoint=pending.checkpoint.clone().unwrap();let value=model::unseal(&self.signing_key,&checkpoint)?;feed.after=value["after"].as_u64().unwrap();feed.prune=value["prune"].clone();feed.checkpoint=checkpoint;feed.pending=None;feed.last_sent=None;
        }
        feed.active=true;feed.touched=Instant::now();Ok(json!({"acknowledged":true,"checkpoint":feed.checkpoint}))
    }
    pub(super) fn feed_close(&self,caller:&Caller,p:&Value,endpoint:Option<&str>)->Result<Value>{let mut feeds=self.feeds.lock().unwrap();let id=p["subscriptionId"].as_str().unwrap();if let Some(f)=feeds.entries.get(id){if f.owner!=endpoint.map(str::to_owned).unwrap_or_else(||owner(caller))||p["feedEpoch"]!=f.epoch{return Err(CoreError::new("SOURCE_INVALID","订阅来源不符"));}}feeds.entries.remove(id);Ok(json!({"closed":true}))}
    pub(super) fn retain_feeds(&self,live:&std::collections::HashSet<String>){self.feeds.lock().unwrap().entries.retain(|_,f|live.contains(&f.owner));}
    fn pump_feeds(&self,cache:&mut super::feed_cache::Cache){
        let selected={let mut feeds=self.feeds.lock().unwrap();let ids:Vec<_>=feeds.entries.keys().cloned().collect();let count=ids.len();if count==0{return;}let selected=(0..count).map(|i|ids[(feeds.round+i)%count].clone()).collect::<Vec<_>>();feeds.round=(feeds.round+1)%count;selected};
        let began=Instant::now();let Ok(water)=self.watermarks()else{return};let high=water["high"].as_u64().unwrap();
        for id in selected {
            let input={let mut feeds=self.feeds.lock().unwrap();let Some(f)=feeds.entries.get_mut(&id)else{continue};
                if (!f.active||f.pending.is_some())&&f.touched.elapsed()>Duration::from_secs(if f.active{60}else{10}){if f.pending.as_ref().is_none_or(|p|p.event!="events.v2.closed"){f.pending=Some(Pending::new("events.v2.closed".into(),json!({"subscriptionId":id,"feedEpoch":f.epoch,"reason":"ack-timeout"})));}continue;}
                if !f.active||f.pending.is_some()||(f.last_scan==high&&f.prune==water["prune"]){continue;}
                (f.filter.clone(),f.after,f.prune.clone(),f.epoch.clone(),f.owner.clone())
            };
            let(filter,after,prune,epoch,owner)=input;
            let result=if prune!=water["prune"]{let checkpoint=model::seal(&self.signing_key,&json!({"epoch":self.epoch,"filter":filter,"after":after,"prune":water["prune"]}));Some(Pending::new("events.v2.gap".into(),json!({"subscriptionId":id,"feedEpoch":epoch,"reason":"history-pruned","checkpoint":checkpoint})))}else{
                match cache.page(self,&owner,&filter,after,high,&water){
                    Ok(page)=>{
                        let checkpoint=model::seal(&self.signing_key,&json!({"epoch":self.epoch,"filter":filter,"after":page.through,"prune":prune}));
                        if page.items.is_empty(){let mut feeds=self.feeds.lock().unwrap();if let Some(f)=feeds.entries.get_mut(&id){f.after=page.through;f.checkpoint=checkpoint;if page.through==high{f.last_scan=high;}}None}
                        else{let batch_id=crate::model::id();let header:Arc<str>=json!({"subscriptionId":id,"feedEpoch":epoch,"batchId":batch_id,"checkpoint":checkpoint}).to_string().into();let items:Vec<_>=page.items.into_iter().map(|(_,raw)|raw).collect();let bytes=header.len()+2048+items.iter().map(|raw|raw.len()+1).sum::<usize>();Some(Pending{event:"events.v2.batch".into(),header,items,checkpoint:Some(checkpoint),batch_id:Some(batch_id),bytes})}
                    },Err(_)=>None,
                }
            };
            if let Some(pending)=result{let bytes=pending.bytes;let mut feeds=self.feeds.lock().unwrap();let total=unique_pending_bytes(feeds.entries.values().filter_map(|f|f.pending.as_ref()).chain(std::iter::once(&pending)));let Some(instance)=feeds.entries.get(&id).map(|f|f.instance.clone())else{continue};let source_bytes:usize=feeds.entries.values().filter(|f|f.instance==instance).filter_map(|f|f.pending.as_ref().map(|p|p.bytes)).sum();if total>4*1024*1024||source_bytes+bytes>512*1024{feeds.budget_waits+=1;continue;}feeds.generated+=1;feeds.pending_peak=feeds.pending_peak.max(total);if let Some(f)=feeds.entries.get_mut(&id){if f.pending.is_none(){f.pending=Some(pending);f.last_sent=None;f.touched=Instant::now();}}}

        }
        let us=began.elapsed().as_micros()as u64;let mut feeds=self.feeds.lock().unwrap();feeds.pumps+=1;feeds.pump_us+=us;feeds.pump_max_us=feeds.pump_max_us.max(us);
    }
    pub(super) fn feed_health(&self)->Value{let f=self.feeds.lock().unwrap();json!({"active":f.entries.len(),"pumps":f.pumps,"pumpMicros":f.pump_us,"pumpMaxMicros":f.pump_max_us,"generated":f.generated,"delivered":f.delivered,"acked":f.acked,"budgetWaits":f.budget_waits,"pendingPeakBytes":f.pending_peak,"pendingBytes":unique_pending_bytes(f.entries.values().filter_map(|f|f.pending.as_ref())),"transitBytes":f.transit.load(Ordering::SeqCst),"transitPeakBytes":f.transit_peak,"ackLatencyBucketsMs":[50,100,200,500,1000,2000,5000],"ackLatency":f.ack_ms})}
    fn feed_deliveries(&self)->Vec<ObservationDelivery>{
        let mut feeds=self.feeds.lock().unwrap();let ids=feeds.entries.keys().cloned().collect::<Vec<_>>();let count=ids.len();if count==0{return vec![];}
        let transit=feeds.transit.clone();let mut peak=0;let mut result=Vec::new();let mut ended=Vec::new();let first=feeds.delivery_round%count;
        for offset in 0..count {let id=&ids[(first+offset)%count];feeds.delivery_round=(first+offset+1)%count;let feed=feeds.entries.get_mut(id).unwrap();
            if feed.last_sent.is_some_and(|t|t.elapsed()<Duration::from_millis(500)){continue;}
            if let Some(pending)=&feed.pending{let event=&pending.event;let charge=pending.bytes*if feed.owner.ends_with(":worker:"){2}else{12};
                if transit.fetch_update(Ordering::SeqCst,Ordering::SeqCst,|used|used.checked_add(charge).filter(|next|*next<=3*1024*1024)).is_err(){continue;}peak=peak.max(transit.load(Ordering::SeqCst));
                result.push(ObservationDelivery{owner:feed.owner.clone(),event:event.clone(),payload:pending.render(),retention:Arc::new(Transit{bytes:charge,total:transit.clone()})});feed.last_sent=Some(Instant::now());if event=="events.v2.closed"{ended.push(id.clone());}if result.len()==32{break;}}
        }
        feeds.transit_peak=feeds.transit_peak.max(peak);feeds.delivered+=result.len()as u64;for id in ended{feeds.entries.remove(&id);}result
    }
    pub(crate) fn close_source(&self,instance:&str){
        self.credits.lock().unwrap().grants.retain(|_,(owner,_)|owner!=instance);
        {let mut catalog=self.catalog.lock().unwrap();if let Some((_,_,descriptor))=catalog.sources.remove(instance){catalog.bytes=catalog.bytes.saturating_sub(descriptor.and_then(|v|serde_json::to_vec(&v).ok()).map_or(0,|v|v.len()));}}
        self.feeds.lock().unwrap().entries.retain(|_,f|f.instance!=instance);
        loop{let ids=self.contexts.lock().unwrap().active.iter().filter(|(_,v)|v["instanceId"]==instance).take(32).map(|(id,_)|id.clone()).collect::<Vec<_>>();if ids.is_empty()||self.interrupt_spans(ids,"INSTANCE_ENDED").is_err(){break;}}
        self.contexts.lock().unwrap().active.retain(|_,v|v["instanceId"]!=instance);
        let instance=instance.to_owned();let _=self.write(super::schema::Partition::Diagnostics,4096,move|db|{db.diagnostics.execute("UPDATE observation_snapshots SET data=json_set(data,'$.ended',json('true')) WHERE instance_id=?",[instance])?;Ok(Value::Null)});
    }
}


#[cfg(test)]mod fairness_tests{
    use super::*;
    #[test]fn inactive_and_slow_feeds_close_without_retaining_credit(){
        let root=tempfile::tempdir().unwrap();Observability::initialize_fixture(root.path()).unwrap();let store=Observability::open(root.path(),"boot".into(),"config".into()).unwrap();let caller=Caller::shell();
        for active in [false,true]{let open=store.feed_open(&caller,&json!({"filter":{},"from":"now"}),None).unwrap();let id=open["subscriptionId"].as_str().unwrap();
            assert_eq!(store.feed_ack(&caller,&json!({"subscriptionId":id,"feedEpoch":"old-epoch"}),None).unwrap_err().code,"SOURCE_INVALID");
            {let mut feeds=store.feeds.lock().unwrap();let f=feeds.entries.get_mut(id).unwrap();f.active=active;f.touched=Instant::now()-Duration::from_secs(61);if active{f.pending=Some(Pending::new("events.v2.batch".into(),json!({"subscriptionId":id})));}}
            store.pump_feeds(&mut super::super::feed_cache::Cache::default());
            assert_eq!(store.feed_ack(&caller,&json!({"subscriptionId":id,"feedEpoch":open["feedEpoch"]}),None).unwrap_err().code,"FEED_CLOSED");
            let deliveries=store.feed_deliveries();assert_eq!(deliveries.len(),1);assert_eq!(deliveries[0].event,"events.v2.closed");assert_eq!(store.feed_health()["active"],0);drop(deliveries);assert_eq!(store.feed_health()["transitBytes"],0);
            assert_eq!(store.feed_ack(&caller,&json!({"subscriptionId":id,"feedEpoch":open["feedEpoch"]}),None).unwrap_err().code,"FEED_CLOSED");assert_eq!(store.feed_close(&caller,&open,None).unwrap()["closed"],true);
        }store.shutdown();
    }
    #[test]fn fast_ack_refill_cannot_starve_later_feed_owners(){
        let root=tempfile::tempdir().unwrap();Observability::initialize_fixture(root.path()).unwrap();let store=Observability::open(root.path(),"boot".into(),"config".into()).unwrap();
        {let mut feeds=store.feeds.lock().unwrap();for n in 0..128{let id=format!("feed-{n:03}");feeds.entries.insert(id.clone(),Feed{owner:id.clone(),instance:id.clone(),filter:json!({}),epoch:"epoch".into(),checkpoint:String::new(),after:0,prune:json!([0,0]),active:false,touched:Instant::now(),pending:Some(Pending::new("events.v2.batch".into(),json!({"subscriptionId":id}))),last_sent:None,last_scan:0});}}
        let mut seen=std::collections::HashSet::new();for _ in 0..4{let batch=store.feed_deliveries();assert_eq!(batch.len(),32);let mut feeds=store.feeds.lock().unwrap();for delivery in batch{seen.insert(delivery.owner.clone());feeds.entries.get_mut(&delivery.owner).unwrap().last_sent=None;}}
        assert_eq!(seen.len(),128);store.shutdown();
    }
}
