//! Existing-connection diagnostics; never owns a connection lease or reconnects.
pub(crate) mod probe;
mod services;
mod rules;
#[cfg(test)]mod tests;
use crate::{CoreError,Result,Connection};
use serde_json::{json,Value};
use std::{collections::{HashMap,VecDeque},sync::{Arc,Mutex,Weak,atomic::{AtomicBool,Ordering}},time::{Duration,Instant}};

const MAX_TARGETS:usize=256;
const MAX_RECEIPTS:usize=128;
const WINDOW_MS:u64=300_000;
const MAX_SAMPLES:usize=2408; // Eight completions/s for five minutes, plus in-flight boundary.
pub(crate) const METHOD:&str="ssh-channel-response-v1";
#[derive(Clone,serde::Serialize,serde::Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct Policy {pub revision:String,pub enabled:bool,pub slow_ms:u32,pub recovery_ms:u32,pub notifications:bool}
impl Default for Policy {fn default()->Self {Self{revision:"0".into(),enabled:false,slow_ms:500,recovery_ms:350,notifications:true}}}
struct Segment {
    serial:u64,server:String,route:String,connection:Weak<Connection>,connection_id:String,generation:u64,
    run:Option<Arc<probe::Request>>,next:Instant,ended:bool,seen:Instant,rules:rules::Rules,
}
struct Receipt {id:String,owner:String,operation:String,hash:String,server:String,route:String,connection:String,generation:u64,
    serial:u64,request:Arc<probe::Request>,canceled:Option<probe::Outcome>,created:Instant}
struct Sample {serial:u64,value:probe::Outcome}
#[derive(Default)]
struct State {policies:HashMap<String,Policy>,segments:Vec<Segment>,receipts:Vec<Receipt>,samples:VecDeque<Sample>,serial:u64,revision:u64,limited:bool}
#[derive(Default)]
pub(crate) struct Quality {state:Mutex<State>,pub budget:Arc<probe::Budget>,stopped:AtomicBool}
impl Quality {
    pub fn policy(&self,server:&str)->Policy {self.state.lock().unwrap().policies.get(server).cloned().unwrap_or_default()}
    pub fn load_policy(&self,server:&str,policy:Policy)->Result<()> {
        let mut state=self.state.lock().unwrap();
        if !state.policies.contains_key(server)&&state.policies.len()>=MAX_TARGETS{return Err(CoreError::new("CAPACITY","服务器探测设置已达到容量上限"));}
        state.policies.insert(server.into(),policy.clone());state.revision+=1;
        if !policy.enabled {
            for segment in &state.segments {
                if segment.server==server {
                    if let Some(run)=&segment.run {
                        if !state.receipts.iter().any(|r|r.canceled.is_none()&&Arc::ptr_eq(&r.request,run)){run.cancel();}
                    }
                }
            }
        }
        Ok(())
    }
    pub fn stop(&self) {
        self.stopped.store(true,Ordering::Release);let state=self.state.lock().unwrap();
        for s in &state.segments{if let Some(run)=&s.run{run.cancel();}}
        for r in &state.receipts{r.request.cancel();}
    }
    pub fn cancel_owner(&self,owner:&str) {
        let mut state=self.state.lock().unwrap();
        for receipt in &mut state.receipts {if receipt.owner==owner {cancel_receipt(receipt);}}
        cancel_unobserved(&state);
    }
    pub fn tick(&self,connections:&[Arc<Connection>],store:&crate::observability::Observability) {
        if self.stopped.load(Ordering::Acquire){return;}
        let mut state=self.state.lock().unwrap();let now=Instant::now();let wall=crate::model::now_ms();
        state.receipts.retain(|r|r.created.elapsed()<Duration::from_secs(600));
        while state.samples.front().is_some_and(|s|s.value.observed_at.saturating_add(WINDOW_MS)<wall){state.samples.pop_front();}
        // Consume a physical measurement once even when several views share its receipt.
        let mut completed=Vec::new();
        for segment in &mut state.segments {
            if let Some(run)=&segment.run {
                let result=run.result.lock().unwrap().clone();
                if let Some(result)=result {completed.push((segment.serial,segment.server.clone(),segment.route.clone(),segment.connection_id.clone(),segment.generation,result));segment.run=None;}
            }
        }
        for (serial,server,route,connection,generation,value) in completed {
            let policy=state.policies.get(&server).cloned().unwrap_or_default();
            if let Some(segment)=state.segments.iter_mut().find(|s|s.serial==serial) {segment.rules.observe(&value,&policy);}
            if store.quality_sample(&server,&route,&connection,generation,serial ,&value).is_err(){state.limited=true;}
            if state.samples.len()>=MAX_SAMPLES {state.samples.pop_front();state.limited=true;}
            state.samples.push_back(Sample{serial,value});state.revision+=1;
        }
        // Retain the original representative while it remains available.
        let mut changed=false;let mut ended=Vec::new();
        for segment in &mut state.segments {
            let alive=connections.iter().any(|c|c.id==segment.connection_id&&c.generation==segment.generation);
            if !alive&&!segment.ended {segment.ended=true;changed=true;if let Some(run)=&segment.run{run.cancel();}ended.push((segment.server.clone(),segment.connection_id.clone(),segment.generation));}
            if alive{segment.seen=now;}
        }
        for (server,connection,generation) in ended{if store.quality_ended(&server,&connection,generation).is_err(){state.limited=true;}}
        let enabled:Vec<_>=state.policies.iter().filter(|(_,p)|p.enabled).map(|(s,_)|s.clone()).collect();
        let mut candidates:Vec<_>=connections.iter().filter(|c|enabled.contains(&c.server.id)).collect();
        candidates.sort_by(|a,b|a.server.id.cmp(&b.server.id).then(a.route.revision.cmp(&b.route.revision)).then(a.id.cmp(&b.id)));
        for connection in candidates {let _=ensure_segment(&mut state,connection);}
        // Oldest due time first avoids a stable first-eight starvation pattern.
        let mut due:Vec<_>=state.segments.iter().enumerate().filter(|(_,s)|!s.ended&&s.run.is_none()&&s.next<=now&&state.policies.get(&s.server).is_some_and(|p|p.enabled)).map(|(i,s)|(i,s.next)).collect();
        due.sort_by_key(|(_,time)|*time);
        for (index,_) in due {
            changed=true;
            let segment=&mut state.segments[index];segment.next=now+Duration::from_secs(5);
            if let Some(connection)=segment.connection.upgrade() {
                match connection.check_response(&self.budget) {
                    Ok(request)=>segment.run=Some(request),
                    Err(error)=>{
                        let value=probe::Outcome{observed_at:wall,outcome:if error.code=="CAPACITY"{"CAPACITY"}else{"DISCONNECTED"},sent:false,duration_ms:None,queue_ms:0,open_ms:0};
                        let serial=segment.serial;
                        if state.samples.len()>=MAX_SAMPLES{state.samples.pop_front();state.limited=true;}
                        state.samples.push_back(Sample{serial,value});
                    }
                }
            }
        }
        if changed{state.revision+=1;}
    }
    pub fn start(&self,owner:&str,operation:&str,server:&str,route:Option<&str>,hash:&str,connections:&[Arc<Connection>])->Result<Value> {
        let mut state=self.state.lock().unwrap();
        if let Some(receipt)=state.receipts.iter().find(|r|r.owner==owner&&r.operation==operation) {
            if receipt.hash!=hash{return Err(CoreError::new("CONFLICT","该操作标识已用于另一项检查"));}return Ok(receipt_value(receipt));
        }
        if self.stopped.load(Ordering::Acquire){return Err(CoreError::new("CANCELED","应用正在退出"));}
        state.receipts.retain(|r|r.created.elapsed()<Duration::from_secs(600));
        if state.receipts.len()>=MAX_RECEIPTS{return Err(CoreError::new("CAPACITY","检查回执已满，请稍后重试"));}
        let existing=state.segments.iter().find(|s|s.server==server&&!s.ended&&route.is_none_or(|r|r==s.route)).map(|s|s.connection_id.as_str());
        let connection=connections.iter().filter(|c|c.server.id==server&&route.is_none_or(|r|r==c.route.revision)).min_by_key(|c|(Some(c.id.as_str())!=existing,c.id.as_str())).ok_or_else(||CoreError::new("DISCONNECTED","没有匹配路径的已认证连接；检查不会自动建立连接"))?;
        let index=ensure_segment(&mut state,connection)?;
        let segment=&mut state.segments[index];
        let request=match &segment.run {Some(run) if run.result.lock().unwrap().is_none()&&!run.canceled.load(Ordering::Acquire)=>run.clone(),_=>{
            let request=connection.check_response(&self.budget)?;segment.run=Some(request.clone());request
        }};
        let receipt=Receipt{id:crate::model::id(),owner:owner.into(),operation:operation.into(),hash:hash.into(),server:server.into(),route:segment.route.clone(),
            connection:segment.connection_id.clone(),generation:segment.generation,serial:segment.serial,request,canceled:None,created:Instant::now()};
        let result=receipt_value(&receipt);state.receipts.push(receipt);state.revision+=1;Ok(result)
    }
    pub fn receipt(&self,owner:&str,id:&str,cancel:bool)->Result<Value> {
        let mut state=self.state.lock().unwrap();
        let receipt=state.receipts.iter_mut().find(|r|r.id==id&&r.owner==owner).ok_or_else(||CoreError::new("TARGET_ENDED","检查回执不存在或不属于当前窗口"))?;
        if cancel{cancel_receipt(receipt);}
        let result=receipt_value(receipt);if cancel{cancel_unobserved(&state);}Ok(result)
    }
    pub fn summary(&self,server:Option<&str>)->Value {
        let state=self.state.lock().unwrap();let now=crate::model::now_ms();
        let segments:Vec<_>=state.segments.iter().filter(|s|server.is_none_or(|id|id==s.server)).map(|s|{
            let samples:Vec<_>=state.samples.iter().filter(|v|v.serial==s.serial&&v.value.observed_at.saturating_add(WINDOW_MS)>=now).map(|v|&v.value).collect();
            let enabled=state.policies.get(&s.server).is_some_and(|p|p.enabled);
            let latest=samples.last().copied();
            json!({"segmentId":s.serial.to_string(),"serverId":s.server,"routeRevision":s.route,"connectionId":s.connection_id,"generation":s.generation.to_string(),
                "state":if s.ended{"ended"}else if s.run.is_some(){"checking"}else if latest.is_some_and(|o|o.outcome=="UNSUPPORTED"){"unsupported"}else if latest.is_some_and(|o|o.outcome=="succeeded"){"ready"}else if latest.is_some(){"degraded"}else if enabled{"waiting"}else{"disabled"},
                "enabled":enabled,"latest":latest,"statistics":rules::statistics(&samples),"slow":s.rules.slow,"unresponsive":s.rules.unresponsive})
        }).collect();
        json!({"method":METHOD,"revision":state.revision.to_string(),"observedAt":now,"segments":segments,"activeProbes":self.budget.active(),"coverage":if state.limited{"partial"}else{"complete"}})
    }
    pub fn revision(&self)->u64 {self.state.lock().unwrap().revision}
}
fn ensure_segment(state:&mut State,connection:&Arc<Connection>)->Result<usize> {
    if let Some(index)=state.segments.iter().position(|s|!s.ended&&s.server==connection.server.id&&s.route==connection.route.revision){return Ok(index);}
    if state.segments.len()>=MAX_TARGETS {
        if let Some(index)=state.segments.iter().enumerate().filter(|(_,s)|s.ended&&s.run.is_none()).min_by_key(|(_,s)|s.seen).map(|(i,_)|i){state.segments.remove(index);state.limited=true;}
        else{return Err(CoreError::new("CAPACITY","探测目标已达到容量上限"));}
    }
    state.serial+=1;
    state.segments.push(Segment{serial:state.serial,server:connection.server.id.clone(),route:connection.route.revision.clone(),connection:Arc::downgrade(connection),connection_id:connection.id.clone(),generation:connection.generation,
        run:None,next:Instant::now(),ended:false,seen:Instant::now(),rules:rules::Rules::default()});
    state.revision+=1;Ok(state.segments.len()-1)
}
fn cancel_receipt(receipt:&mut Receipt) {
    if receipt.canceled.is_none()&&receipt.request.result.lock().unwrap().is_none(){receipt.canceled=Some(probe::Outcome{observed_at:crate::model::now_ms(),outcome:"CANCELED",sent:false,duration_ms:None,queue_ms:0,open_ms:0});}
}
fn cancel_unobserved(state:&State) {
    for receipt in &state.receipts {if receipt.canceled.is_some()&&!state.policies.get(&receipt.server).is_some_and(|p|p.enabled)&&!state.receipts.iter().any(|other|other.canceled.is_none()&&Arc::ptr_eq(&receipt.request,&other.request)){receipt.request.cancel();}}
}
fn receipt_value(r:&Receipt)->Value {
    let result=r.canceled.clone().or_else(||r.request.result.lock().unwrap().clone());
    json!({"taskId":r.id,"operationId":r.operation,"serverId":r.server,"routeRevision":r.route,"connectionId":r.connection,"generation":r.generation.to_string(),"segmentId":r.serial.to_string(),
        "method":METHOD,"state":if result.is_some(){"finished"}else{"checking"},"result":result,"cleanupPending":!r.request.retired.load(Ordering::Acquire)})
}
