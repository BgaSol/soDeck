//! Bounded, in-memory execution facts. Authentication validity never depends on this cache.
use super::*;
use serde::Serialize;

#[derive(Clone, Copy, Debug)]
pub(crate) struct PhaseEvent<'a> { pub stage: &'a str, pub state: &'static str, pub error:Option<&'a CoreError> }
impl<'a> PhaseEvent<'a> {
    pub fn start(stage:&'a str)->Self {Self{stage,state:"running",error:None}}
    pub fn success(stage:&'a str)->Self {Self{stage,state:"succeeded",error:None}}
}
#[derive(Clone, Serialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct Phase {
    id:String, round:u32, step_index:Option<usize>, stage:String, state:String,
    started:u64, duration_ms:u64, error:Option<Value>,
    #[serde(skip)] clock:Option<Instant>,
}
pub(crate) struct Record {
    pub plan:Option<Arc<route::RoutePlan>>, pub reference:Value,
    workspace:String, workspace_generation:Option<String>, server:String,
    instance:String, source:String, operation:Option<String>, purpose:String, member:Option<String>,
    state:String, started:u64, clock:Instant, duration_ms:u64, updated:u64,
    revision:u64, round:u32, phases:Vec<Phase>, partial:bool,
    error:Option<Value>, previous:Option<Value>, continuation:bool,
    connection:Option<Value>, reused:bool,
    transport:Option<Arc<Mutex<Option<Value>>>>, lifecycle:Option<Value>,
}
impl Record {
    fn summary(&self)->Value {
        let route=self.plan.as_ref().map(|p|{
            let mut meta=crate::route_diagnostics::saved_meta(p);
            meta["reference"]=json!({"kind":"attempt","serverId":p.target_server_id,"workspaceId":self.workspace,"routeRevision":p.revision,"attemptId":self.reference["attemptId"]});meta
        });
        json!({"reference":self.reference,"workspaceId":self.workspace,"workspaceGeneration":self.workspace_generation,
            "serverId":self.server,"instanceId":self.instance,"source":self.source,"operationId":self.operation,"purpose":self.purpose,"memberId":self.member,
            "state":self.state,"started":self.started,"updated":self.updated,"durationMs":if self.state=="running" {self.clock.elapsed().as_millis() as u64}else{self.duration_ms},
            "revision":self.revision,"round":self.round,"route":route,"error":self.error,"previous":self.previous,
            "continuation":self.continuation,"connection":self.connection,"lifecycle":self.lifecycle,"reused":self.reused,"coverage":if self.partial{"partial"}else{"complete"}})
    }
    fn phase(&mut self,index:Option<usize>,event:PhaseEvent<'_>) {
        self.updated=model::now_ms();self.revision+=1;
        if let Some(error)=event.error {self.fail_phase(error);return;}
        if event.state=="round" {self.round+=1;return;}
        if event.state=="running" {
            self.phases.push(Phase{id:id(),round:self.round,step_index:index,stage:event.stage.into(),state:"running".into(),started:self.updated,duration_ms:0,error:None,clock:Some(Instant::now())});
        } else if let Some(p)=self.phases.iter_mut().rev().find(|p|p.round==self.round&&p.step_index==index&&p.stage==event.stage&&p.state=="running") {
            p.duration_ms=p.clock.take().map_or(0,|v|v.elapsed().as_millis() as u64);p.state=event.state.into();
        } else if event.state=="notApplicable" {
            self.phases.push(Phase{id:id(),round:self.round,step_index:index,stage:event.stage.into(),state:event.state.into(),started:self.updated,duration_ms:0,error:None,clock:None});
        }
    }
    fn fail_phase(&mut self,error:&CoreError) {
        let state=if is_connection_authentication_error(&error.code)&&!matches!(error.code.as_str(),"AUTH_FAILED"|"PROXY_AUTH_FAILED"){"waiting"}else if error.code=="CANCELED"{"canceled"}else{"failed"};
        if let Some(p)=self.phases.iter_mut().rev().find(|p|p.state=="running") {
            p.state=state.into();p.error=Some(safe_error(error));p.duration_ms=p.clock.take().map_or(0,|v|v.elapsed().as_millis() as u64);
        }
    }
    fn bytes(&self)->usize {self.phases.iter().map(|p|256+p.stage.len()+p.error.as_ref().map_or(0,|v|v.to_string().len()*4)).sum::<usize>()+8192}
    fn refresh_lifecycle(&mut self){
        if self.lifecycle.is_some(){return;}
        if let Some(value)=self.transport.as_ref().and_then(|p|p.lock().unwrap().clone()){self.lifecycle=Some(value);self.revision+=1;}
    }
}
#[derive(Default)]
pub(crate) struct Diagnostics { pub records:HashMap<String,Record> }
impl Diagnostics {
    fn bytes(&self)->usize {
        let mut plans=std::collections::HashSet::new();
        self.records.values().map(|r|r.bytes()+r.plan.as_ref().map_or(0,|p|if plans.insert(p.revision.clone()){p.cost.snapshot_bytes.saturating_mul(2)}else{0})).sum()
    }
    fn prune(&mut self) {
        let now=model::now_ms();
        self.records.retain(|_,r|matches!(r.state.as_str(),"running"|"waiting")||now.saturating_sub(r.updated)<1_800_000);
        for r in self.records.values_mut(){r.refresh_lifecycle();if r.phases.len()>2048{r.phases.drain(..r.phases.len()-2048);r.partial=true;r.revision+=1;}}
        let mut ended=self.records.iter().filter(|(_,r)|!matches!(r.state.as_str(),"running"|"waiting")).map(|(id,r)|(id.clone(),r.workspace.clone(),r.updated)).collect::<Vec<_>>();
        ended.sort_by_key(|r|std::cmp::Reverse(r.2));let mut counts=HashMap::<String,usize>::new();
        for (id,workspace,_) in &ended {let n=counts.entry(workspace.clone()).or_default();*n+=1;if *n>10{self.records.remove(id);}}
        let mut bytes=self.bytes();
        for (id,_,_) in ended.into_iter().rev() {if self.records.len()<=512&&bytes<=16*1024*1024{break;}self.records.remove(&id);bytes=self.bytes();}
        // Detail loss must never cancel a valid challenge or change transport admission.
        if bytes>16*1024*1024 {for r in self.records.values_mut(){if r.phases.len()>32 {let before=r.bytes();let n=r.phases.len()-32;r.phases.drain(..n);r.partial=true;r.revision+=1;bytes=bytes.saturating_sub(before-r.bytes());}}}
        while self.records.len()>512||bytes>16*1024*1024 {let oldest=self.records.iter().min_by_key(|(_,r)|(r.state=="running",r.updated)).map(|(id,_)|id.clone());if let Some(id)=oldest{self.records.remove(&id);bytes=self.bytes();}else{break;}}
    }
}

pub(crate) fn safe_error(error:&CoreError)->Value {
    let mut result=json!({"code":error.code.chars().filter(|c|c.is_ascii_uppercase()||c.is_ascii_digit()||*c=='_').take(128).collect::<String>()});
    if let Some(d)=&error.details {for key in ["stepIndex","reply","receivedBytes","expectedBytes","osErrorCode","sshErrorCode","candidateAttempts"] {if let Some(v)=d.get(key).filter(|v|v.is_i64()||v.is_u64()){result[key]=v.clone();}}
        if let Some(stage)=d["stage"].as_str().filter(|s|matches!(*s,"prepare"|"queue"|"admit"|"resolve"|"resolveCached"|"connect"|"forward"|"handshake"|"verify"|"credentials"|"authenticate"|"proxyCredentials"|"proxyNegotiate"|"proxyAuthenticate"|"proxyConnect"|"deliver")){result["stage"]=json!(stage);}
        if let Some(op)=d["operation"].as_str().filter(|s|matches!(*s,"read"|"write"|"connect")){result["operation"]=json!(op);}}
    result
}

impl Core {
    pub(crate) fn maintain_diagnostics(&self) {
        // Never hold the diagnostic cache while inspecting a transport owner.
        let waiting = {
            let cache = self.connection_diagnostics.lock().unwrap();
            cache.records.iter().filter(|(_, r)| r.state == "waiting")
                .map(|(id, r)| (id.clone(), r.revision, r.instance.clone(), r.operation.clone(),
                    r.workspace.clone(), r.workspace_generation.clone(), r.purpose.clone(), r.updated))
                .collect::<Vec<_>>()
        };
        for (token, revision, instance, operation, workspace, generation, purpose, updated) in waiting {
            let revoked = self.registry.lock().unwrap().revoked.contains(&instance);
            let workspace_gone = self.remote.lock().unwrap().scopes.get(&workspace).cloned() != generation;
            let owner_gone = revoked || workspace_gone;
            let reason = if owner_gone { Some("CANCELED") }
                else if model::now_ms().saturating_sub(updated) >= 300_000 { Some("HOST_CHALLENGE_EXPIRED") }
                else if let Some(operation) = operation {
                    if purpose == "network-data" { self.network_diagnostic_end(&instance, &operation) }
                    else { self.operations.lock().unwrap().get(&(instance.clone(), operation))
                        .filter(|op| op.canceled.load(Ordering::SeqCst)).map(|_| "CANCELED") }
                } else { None };
            let Some(code) = reason else { continue };
            let fact = {
                let mut cache = self.connection_diagnostics.lock().unwrap();
                let Some(r) = cache.records.get_mut(&token).filter(|r| r.state == "waiting" && r.revision == revision) else { continue };
                let state = if code == "CANCELED" { "canceled" } else { "failed" };
                let mut error = r.error.clone().unwrap_or_else(|| json!({}));
                error["code"] = json!(code);
                let phase = r.phases.iter_mut().rev().find(|p| p.state == "waiting");
                let details = phase.map(|p| {
                    p.state = state.into(); p.error = Some(error.clone());
                    p.duration_ms = model::now_ms().saturating_sub(p.started);
                    (p.step_index, p.stage.clone(), p.duration_ms)
                });
                r.state = state.into(); r.error = Some(error); r.updated = model::now_ms();
                r.duration_ms = r.clock.elapsed().as_millis() as u64; r.revision += 1;
                details.map(|(index, stage, duration)| (r.server.clone(), state, json!({
                    "originInstanceId":r.instance,"originPluginId":r.source,"operationId":r.operation,
                    "workspaceId":r.workspace,"attemptId":token,"round":r.round,"stepIndex":index,
                    "stage":stage,"state":state,"durationMs":duration,"errorCode":code
                })))
            };
            if let Some((server, state, payload)) = fact {
                if self.observability.route_phase(&server, state, payload).is_err() { self.fault("connections.route.phase", "phase-not-persisted"); }
            }
        }
        self.connection_diagnostics.lock().unwrap().prune();
    }
    pub(crate) fn start_diagnostic(&self,caller:&Caller,p:&Value,purpose:&str)->(String,Value) {
        self.maintain_diagnostics();
        let member=purpose.strip_prefix("network-data:").map(str::to_owned);let purpose=if member.is_some(){"network-data"}else{purpose};
        let token=id();let generation=self.next_generation.fetch_add(1,Ordering::SeqCst);
        let workspace=p["workspaceId"].as_str().unwrap_or("").to_owned();
        let workspace_generation=self.remote.lock().unwrap().scopes.get(&workspace).cloned();
        let mut cache=self.connection_diagnostics.lock().unwrap();cache.prune();
        let previous=cache.records.values_mut().filter(|r|r.workspace==workspace&&r.workspace_generation==workspace_generation&&r.server==p["serverId"].as_str().unwrap_or("")&&r.purpose==purpose&&r.instance==caller.instance_id).max_by_key(|r|r.reference["attemptGeneration"].as_u64().unwrap_or(0)).map(|r|{let continuing=r.state=="waiting"&&r.operation.is_some()&&r.operation.as_deref()==p["operationId"].as_str();if continuing{r.state="superseded".into();r.revision+=1;}(r.reference.clone(),continuing)});
        let now=model::now_ms();
        let record=Record{plan:None,reference:json!({"bootId":self.traffic.boot_id,"attemptId":token,"attemptGeneration":generation}),workspace,workspace_generation,server:p["serverId"].as_str().unwrap_or("").into(),instance:caller.instance_id.clone(),source:caller.plugin_id.clone().unwrap_or_else(||"shell".into()),operation:p["operationId"].as_str().map(str::to_owned),purpose:purpose.into(),member,state:"running".into(),started:now,clock:Instant::now(),duration_ms:0,updated:now,revision:1,round:0,phases:Vec::new(),partial:false,error:None,previous:previous.as_ref().map(|v|v.0.clone()),continuation:previous.is_some_and(|v|v.1),connection:None,reused:false,transport:None,lifecycle:None};
        let reference=record.reference.clone();cache.records.insert(token.clone(),record);cache.prune();drop(cache);self.diagnostic_phase(&token,None,PhaseEvent::start("prepare"));(token,reference)
    }
    pub(crate) fn diagnostic_plan(&self,token:&str,plan:&route::RoutePlan) {
        let mut cache=self.connection_diagnostics.lock().unwrap();
        let shared=cache.records.values().filter_map(|r|r.plan.as_ref()).find(|p|p.revision==plan.revision).cloned().unwrap_or_else(||Arc::new(plan.clone()));
        if let Some(r)=cache.records.get_mut(token){r.plan=Some(shared);}drop(cache);self.diagnostic_phase(token,None,PhaseEvent::success("prepare"));self.diagnostic_phase(token,None,PhaseEvent::start("queue"));
    }
    pub(crate) fn diagnostic_phase(&self,token:&str,index:Option<usize>,event:PhaseEvent<'_>) {
        let fact={let mut cache=self.connection_diagnostics.lock().unwrap();let fact=cache.records.get_mut(token).map(|r|{r.phase(index,event);let phase=r.phases.last();(r.server.clone(),json!({"originInstanceId":r.instance,"originPluginId":r.source,"operationId":r.operation,"workspaceId":r.workspace,"attemptId":token,"round":r.round,"stepIndex":index,"stage":event.stage,"state":event.state,"durationMs":phase.map(|p|p.duration_ms),"errorCode":event.error.map(|e|&e.code)}))});cache.prune();fact};
        if event.state!="round"{if let Some((server,payload))=fact{if self.observability.route_phase(&server,event.state,payload).is_err(){self.fault("connections.route.phase","phase-not-persisted");}}}
    }
    pub(crate) fn finish_diagnostic(&self,token:&str,result:&mut Result<Value>) {
        if let Err(e)=result{if e.details.as_ref().is_none_or(|d|d["stage"].is_null()){let cache=self.connection_diagnostics.lock().unwrap();if let Some(p)=cache.records.get(token).and_then(|r|r.phases.last()){let d=e.details.get_or_insert_with(||json!({}));d["stage"]=json!(p.stage);if let Some(index)=p.step_index{d["stepIndex"]=json!(index);}}}}
        if let Err(e)=result {let stage={let cache=self.connection_diagnostics.lock().unwrap();cache.records.get(token).and_then(|r|r.phases.iter().rev().find(|p|p.state=="running")).map(|p|(p.step_index,p.stage.clone()))};if let Some((index,stage))=stage{let state=if is_connection_authentication_error(&e.code){"waiting"}else if e.code=="CANCELED"{"canceled"}else{"failed"};self.diagnostic_phase(token,index,PhaseEvent{stage:&stage,state,error:Some(e)});}}
        else{self.diagnostic_phase(token,None,PhaseEvent::success("deliver"));}
        let mut cache=self.connection_diagnostics.lock().unwrap();
        if let Some(r)=cache.records.get_mut(token) {
            match result {
                Ok(value)=>{r.state="succeeded".into();r.connection=Some(json!({"connectionId":value["connectionId"],"generation":value["generation"]}));},
                Err(error)=>{r.fail_phase(error);r.state=if is_connection_authentication_error(&error.code){"waiting"}else if error.code=="CANCELED"{"canceled"}else{"failed"}.into();r.error=Some(safe_error(error));
                    error.details.get_or_insert_with(||json!({}))["diagnosticRef"]=r.reference.clone();}
            }
            r.duration_ms=r.clock.elapsed().as_millis() as u64;r.updated=model::now_ms();r.revision+=1;
        }
        cache.prune();
    }
    pub(crate) fn diagnostic_reused(&self,token:&str) {if let Some(r)=self.connection_diagnostics.lock().unwrap().records.get_mut(token){r.reused=true;r.revision+=1;}}
    pub(crate) fn diagnostic_connection(&self,token:&str,connection:&Arc<Connection>){if let Some(r)=self.connection_diagnostics.lock().unwrap().records.get_mut(token){r.transport=Some(connection.lifecycle.clone());}}

    pub fn connection_diagnostics_snapshot(&self,p:&Value)->Result<Value> {
        self.diagnostic_list(p,"state-feed")
    }
    fn diagnostic_list(&self,p:&Value,instance:&str)->Result<Value> {
        self.maintain_diagnostics();
        let mut cache=self.connection_diagnostics.lock().unwrap();cache.prune();
        let mut records=cache.records.values().filter(|r|r.workspace==p["workspaceId"].as_str().unwrap_or("")
            &&p["workspaceGeneration"].as_str().is_none_or(|g|r.workspace_generation.as_deref()==Some(g))
            &&p["serverId"].as_str().is_none_or(|s|s==r.server)
            &&p["purpose"].as_str().is_none_or(|s|s==r.purpose)
            &&p["source"].as_str().is_none_or(|s|s==r.source)
            &&p["operationId"].as_str().is_none_or(|s|Some(s)==r.operation.as_deref())
            &&p["instanceId"].as_str().is_none_or(|s|s==r.instance)).collect::<Vec<_>>();
        records.sort_by_key(|r|std::cmp::Reverse(r.reference["attemptGeneration"].as_u64().unwrap_or(0)));
        let mut filter=p.clone();filter.as_object_mut().map(|o|o.remove("cursor"));
        let identity=route::digest(&json!([instance,filter,records.iter().map(|r|(&r.reference,r.revision)).collect::<Vec<_>>() ]));
        let offset=cursor_offset(p,&identity)?;
        let total=records.len();if offset>total{return Err(expired());}let mut rows=Vec::new();let mut bytes=0;
        for r in records.into_iter().skip(offset).take(10){let s=r.summary();bytes+=s.to_string().len();if bytes>24*1024{break;}rows.push(s);}
        let next=offset+rows.len();
        Ok(json!({"attempts":rows,"total":total,"nextCursor":if next<total{Some(STANDARD.encode(json!({"identity":identity,"offset":next}).to_string()))}else{None}}))
    }
    pub(crate) fn connection_diagnostics_dispatch(&self,caller:&Caller,method:&str,p:&Value)->Result<Value> {
        validate("x-methods",method,p)?;
        if method=="connections.diagnostics.list" {let result=self.diagnostic_list(p,&caller.instance_id)?;validate("x-results",method,&result)?;return Ok(result);}
        self.maintain_diagnostics();
        let mut cache=self.connection_diagnostics.lock().unwrap();cache.prune();
        let reference=&p["reference"];
        let r=cache.records.get(field(reference,"attemptId")?).filter(|r|r.reference==*reference).ok_or_else(expired)?;
        let result=if method=="connections.diagnostics.get" {r.summary()}
        else {
            if p["revision"].as_u64()!=Some(r.revision){return Err(CoreError::new("DIAGNOSTIC_CHANGED","连接记录已更新，请重新读取"));}
            let identity=route::digest(&json!([caller.instance_id,reference,r.revision]));
            let offset=cursor_offset(p,&identity)?;
            if offset>r.phases.len(){return Err(expired());}
            let phases=r.phases.iter().skip(offset).take(64).map(|p|{let mut p=p.clone();if let Some(clock)=p.clock{p.duration_ms=clock.elapsed().as_millis() as u64;}p}).collect::<Vec<_>>();
            let next=offset+phases.len();
            json!({"reference":reference,"revision":r.revision,"phases":phases,"nextCursor":if next<r.phases.len(){Some(STANDARD.encode(json!({"identity":identity,"offset":next}).to_string()))}else{None},"coverage":if r.partial{"partial"}else{"complete"}})
        };
        validate("x-results",method,&result)?;Ok(result)
    }
}
fn expired()->CoreError {CoreError::new("DIAGNOSTIC_EXPIRED","详细连接记录已过期")}
fn cursor_offset(p:&Value,identity:&str)->Result<usize>{
    if let Some(cursor)=p["cursor"].as_str(){let bytes=STANDARD.decode(cursor).map_err(|_|expired())?;let v:Value=serde_json::from_slice(&bytes).map_err(|_|expired())?;if v["identity"]!=identity{return Err(CoreError::new("DIAGNOSTIC_CHANGED","连接记录已更新，请重新读取"));}Ok(v["offset"].as_u64().ok_or_else(expired)? as usize)}else{Ok(0)}
}
/// Validate the public read/open contracts at both core and host boundaries.
pub fn validate(kind:&str,method:&str,value:&Value)->Result<()> {
    static SCHEMA:std::sync::OnceLock<Value>=std::sync::OnceLock::new();
    let schema=SCHEMA.get_or_init(||serde_json::from_str(include_str!("../../../contracts/connection-diagnostics.schema.json")).expect("diagnostic schema"));
    let definition=schema[kind][method].as_str().ok_or_else(||CoreError::new("METHOD_NOT_FOUND","未知连接诊断接口"))?;
    crate::contract::check(schema,&schema["$defs"][definition],value,method).map_err(|m|CoreError::new("INVALID_PARAMS",m))
}

pub(crate) struct AttemptGuard<'a>{core:&'a Core,pub token:String,reference:Value,finished:bool}
impl<'a> AttemptGuard<'a>{
    pub fn new(core:&'a Core,caller:&Caller,p:&Value,purpose:&str)->Self{let(token,reference)=core.start_diagnostic(caller,p,purpose);Self{core,token,reference,finished:false}}
    pub fn finish(&mut self,result:&mut Result<Value>){if let Err(e)=result{e.details.get_or_insert_with(||json!({}))["diagnosticRef"]=self.reference.clone();}self.core.finish_diagnostic(&self.token,result);self.finished=true;}
}
impl Drop for AttemptGuard<'_>{fn drop(&mut self){if !self.finished{let mut result=Err(CoreError::new("CONNECTION_INTERRUPTED","连接执行中断"));self.core.finish_diagnostic(&self.token,&mut result);}}}

#[cfg(test)]
mod tests;
