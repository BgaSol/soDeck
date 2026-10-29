use crate::model::{id, now_ms};
use serde::Serialize;
use ssh2::{ByteCounter, ByteMeter, ByteCounts};
use std::{collections::HashMap, sync::{Arc, Mutex, Weak, atomic::{AtomicBool, AtomicU64, Ordering}}, time::{Duration, Instant}};

const MAX_RESOURCES: usize = 4096;
const MAX_TARGETS: usize = 256;
const IDENTITY_BYTES:usize=4*1024*1024;
const MAX_SERIES:usize=8192;
const RING_BYTES: usize = 24 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct Owner {
    pub server_id: String,
    pub config_epoch: String,
    pub workspace_id: Option<String>,
    pub workspace_generation: Option<String>,
    pub plugin_id: Option<String>,
    pub instance_id: String,
    pub resource_id: String,
    pub generation: String,
    pub kind: String,
    pub route_run_id: String,
    pub layer: String,
    pub access_mode: String,
}
#[derive(Clone)]
pub(crate) struct Registration {
    pub counter: Option<ByteCounter>,
    pub id: String,
    life: Arc<()>,
}
struct Entry {
    owner: Owner,
    counter: ByteCounter,
    previous: ByteCounts,
    ended: bool,
    life: Weak<()>,
    latest: Sample,
}
#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct Sample {
    pub timestamp: u64,
    pub duration_ms: u32,
    pub sent: u64,
    pub received: u64,
    pub gap: bool,
    pub invalid: bool,
    pub valid_ms: u64,
    pub valid_sent: u64,
    pub valid_received: u64,
    pub peak_send: f64,
    pub peak_receive: f64,
    pub peak_resolution_ms: u32,
    pub samples: u32,
}
impl Sample {
    pub(crate) fn new(timestamp:u64,duration_ms:u32,sent:u64,received:u64,gap:bool,invalid:bool)->Self {
        let valid=!gap&&!invalid&&duration_ms>0;
        Self{timestamp,duration_ms,sent,received,gap,invalid,valid_ms:if valid{duration_ms as u64}else{0},valid_sent:if valid{sent}else{0},valid_received:if valid{received}else{0},
            peak_send:if valid{sent as f64*1000.0/duration_ms as f64}else{0.0},peak_receive:if valid{received as f64*1000.0/duration_ms as f64}else{0.0},peak_resolution_ms:duration_ms,samples:1}
    }
    pub(crate) fn merge(&mut self,previous:Sample) {
        increment(&mut self.sent,previous.sent,&mut self.invalid);increment(&mut self.received,previous.received,&mut self.invalid);
        increment(&mut self.valid_sent,previous.valid_sent,&mut self.invalid);increment(&mut self.valid_received,previous.valid_received,&mut self.invalid);
        self.valid_ms=self.valid_ms.saturating_add(previous.valid_ms);
        self.gap|=previous.gap||previous.timestamp.saturating_add(2)<self.timestamp.saturating_sub(self.duration_ms as u64);
        self.duration_ms=self.duration_ms.saturating_add(previous.duration_ms);self.invalid|=previous.invalid;
        self.peak_send=self.peak_send.max(previous.peak_send);self.peak_receive=self.peak_receive.max(previous.peak_receive);
        self.peak_resolution_ms=self.peak_resolution_ms.max(previous.peak_resolution_ms);self.samples=self.samples.saturating_add(previous.samples);
    }
}
#[derive(Clone, Debug, Default)]
struct Series {
    sent: u64,
    received: u64,
    active: usize,
    invalid: bool,
    ring: Option<u32>,
    latest: Option<Sample>,
    pending: Option<Sample>,
}
struct State {
    entries: HashMap<String, Entry>,
    targets: HashMap<String, u16>,
    series: HashMap<String, Series>,
    last_tick: Instant,
    last_wall: u64,
    revision: u64,
    limited: u64,
    rings: super::ring::Rings,
    unattributed: ByteCounts,
    first_wall: u64,
    identity_bytes: usize,
    last_flush: Instant,
}
pub(crate) struct Traffic {
    pub meter: ByteMeter,
    pub boot_id: String,
    pub config_epoch: String,
    state: Mutex<State>,
    stopped: AtomicBool,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    history: Mutex<Option<Arc<crate::observability::Observability>>>,
    history_dropped: AtomicU64,
    history_enabled: AtomicBool,
    history_skip_interval: AtomicBool,
}

fn increment(total: &mut u64, delta: u64, invalid: &mut bool) {
    match total.checked_add(delta) { Some(n) => *total=n, None => { *total=u64::MAX; *invalid=true; } }
}
fn keys(owner: &Owner) -> [String; 3] {
    [format!("{}:all",owner.layer), format!("{}:server:{}",owner.layer,owner.server_id),
        format!("{}:source:{}:{}",owner.layer,owner.server_id,owner.plugin_id.as_deref().unwrap_or("@core"))]
}

impl Traffic {
    pub fn new(config_epoch: String) -> Arc<Self> {
        let now=now_ms();
        Arc::new(Self { meter:ByteMeter::new(8192), boot_id:id(), config_epoch,
            stopped:AtomicBool::new(false),worker:Mutex::new(None),history:Mutex::new(None),history_dropped:AtomicU64::new(0),history_enabled:AtomicBool::new(true),history_skip_interval:AtomicBool::new(false),
            state:Mutex::new(State { entries:HashMap::new(), targets:HashMap::new(),series:HashMap::new(),
                last_tick:Instant::now(), last_flush:Instant::now(),last_wall:now, revision:0, limited:0,rings:super::ring::Rings::new(RING_BYTES,MAX_SERIES),unattributed:ByteCounts::default(),first_wall:now,identity_bytes:0 }) })
    }
    pub fn attach_history(&self,store:Arc<crate::observability::Observability>){*self.history.lock().unwrap()=Some(store);}
    pub fn set_history_enabled(&self,enabled:bool) {
        let mut state=self.state.lock().unwrap();
        let previous=self.history_enabled.swap(enabled,Ordering::AcqRel);
        if enabled&&!previous{self.history_skip_interval.store(true,Ordering::Release);}
        if !enabled {
            // Only previously sampled data is drained. The following interval is never backfilled.
            if let Some(history)=self.history.lock().unwrap().clone() {
                let mut batch=Vec::with_capacity(128);
                for (key,series) in &mut state.series {
                    let Some(sample)=series.pending.take()else{continue};
                    let (layer,rest)=key.split_once(':').unwrap();let target=if rest=="all"{"@all"}else{rest.split(':').nth(1).unwrap_or("@other")};
                    batch.push(crate::observability::traffic::Bucket{owner:key.clone(),target:target.into(),layer:layer.into(),from:sample.timestamp.saturating_sub(sample.duration_ms as u64),sample});
                    if batch.len()==128&&history.save_traffic(std::mem::replace(&mut batch,Vec::with_capacity(128))).is_err(){self.history_dropped.fetch_add(1,Ordering::Relaxed);}
                }
                if !batch.is_empty()&&history.save_traffic(batch).is_err(){self.history_dropped.fetch_add(1,Ordering::Relaxed);}
            }
        }
    }
    pub fn start(self: &Arc<Self>) -> crate::Result<()> {
        let weak=Arc::downgrade(self);
        *self.worker.lock().unwrap()=Some(std::thread::Builder::new().name("sodeck-traffic".into()).spawn(move||loop {
            std::thread::park_timeout(Duration::from_secs(1));
            let Some(traffic)=weak.upgrade() else {break};
            traffic.sample();
            if traffic.stopped.load(Ordering::Acquire) {break;}
        })?);
        Ok(())
    }
    pub fn stop(&self) {
        if self.stopped.load(Ordering::Acquire){return;}
        let deadline=Instant::now()+Duration::from_secs(2);
        loop {
            self.sample();
            let pending=self.state.lock().unwrap().entries.values().any(|e|e.owner.kind=="transport"&&!e.counter.counts().closed);
            if !pending {break;}
            if Instant::now()>=deadline {
                let mut state=self.state.lock().unwrap();state.limited=state.limited.saturating_add(1);break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        self.stopped.store(true,Ordering::Release);
        if let Some(worker)=self.worker.lock().unwrap().take() {
            worker.thread().unpark();let _=worker.join();
        }
        std::thread::sleep(Duration::from_millis(1));self.sample();
    }
    pub fn register(&self, mut owner: Owner) -> Registration {
        let mut state=self.state.lock().unwrap();
        if !state.targets.contains_key(&owner.server_id) && state.targets.len()>=MAX_TARGETS {
            owner.server_id="@other".into(); state.limited=state.limited.saturating_add(1);
        }
        if owner.server_id!="@other" {state.targets.entry(owner.server_id.clone()).or_insert(0);}
        let original_key=format!("{}:{}:{}",owner.layer,owner.resource_id,owner.generation);
        if let Some(entry)=state.entries.get(&original_key).filter(|e|!e.counter.counts().closed) {
            if let Some(life)=entry.life.upgrade() {
                return Registration { counter:Some(entry.counter.clone()), id:original_key, life };
            }
        }
        // Transports have a separate 256-session core budget and must never
        // share a final-owner counter. Reserve metadata room for that boundary.
        let identity_limit=if owner.kind=="transport" {IDENTITY_BYTES}else{IDENTITY_BYTES-256*1024};
        if owner.kind!="transport" && (state.entries.len()>=MAX_RESOURCES || state.identity_bytes+owner_bytes(&owner)>identity_limit) {
            state.limited=state.limited.saturating_add(1);
            owner.resource_id=format!("@other:{}:{}",owner.server_id,owner.plugin_id.as_deref().unwrap_or("@core"));
            owner.generation=self.boot_id.clone(); owner.kind="other".into();
            let key=format!("{}:{}:{}",owner.layer,owner.resource_id,owner.generation);
            if let Some(entry)=state.entries.get(&key).filter(|e|!e.counter.counts().closed) {
                if let Some(life)=entry.life.upgrade() {
                    return Registration {counter:Some(entry.counter.clone()),id:key,life};
                }
            }
        }
        if state.identity_bytes+owner_bytes(&owner)>identity_limit {
            state.limited=state.limited.saturating_add(1);
            return Registration {counter:None,id:owner.resource_id,life:Arc::new(())};
        }
        let Some(counter)=self.meter.reserve() else {
            state.limited=state.limited.saturating_add(1);
            return Registration {counter:None,id:owner.resource_id,life:Arc::new(())};
        };
        let key=format!("{}:{}:{}",owner.layer,owner.resource_id,owner.generation);
        // Every primitive gets its own entry even when public resource identity is shared.
        let key=if state.entries.contains_key(&key) {format!("{key}:{}",counter.token())}else{key};
        let life=Arc::new(());
        state.identity_bytes+=owner_bytes(&owner);
        state.entries.insert(key.clone(),Entry {owner,counter:counter.clone(),previous:ByteCounts::default(),ended:false,life:Arc::downgrade(&life),latest:Sample::default()});
        Registration {counter:Some(counter),id:key,life}
    }
    pub fn ended(&self, registration: &Registration) {
        if let Some(entry)=self.state.lock().unwrap().entries.get_mut(&registration.id) {entry.ended=true;}
    }
    pub fn sample(&self) {
        self.sample_at(Instant::now(),now_ms());
    }
    fn sample_at(&self, mono:Instant, wall:u64) {
        let history=self.history.lock().unwrap().clone();
        let mut state=self.state.lock().unwrap();
        let elapsed=mono.saturating_duration_since(state.last_tick);
        if elapsed.as_millis()==0 {return;}
        let history=history.filter(|_|self.history_enabled.load(Ordering::Acquire)&&!self.history_skip_interval.swap(false,Ordering::AcqRel));
        let millis=elapsed.as_millis().clamp(1,u32::MAX as u128) as u32;
        let wall_elapsed=wall as i128-state.last_wall as i128;
        let gap=wall_elapsed.abs_diff(millis as i128)>2000 || elapsed>Duration::from_secs(3);
        let mut deltas:HashMap<String,(u64,u64,usize,bool)>=HashMap::new();
        for entry in state.entries.values_mut() {
            let counts=entry.counter.counts();
            let delta=(counts.sent.saturating_sub(entry.previous.sent),counts.received.saturating_sub(entry.previous.received));
            entry.previous=counts;
            entry.latest=Sample::new(wall,millis,delta.0,delta.1,gap,counts.invalid);
            if entry.owner.layer=="carrier" {continue;}
            let invalid=counts.invalid;
            let active=usize::from((counts.opened||entry.owner.kind=="transport")&&!counts.closed && !entry.ended);
            if entry.owner.kind=="transport"&&active>0 {
                deltas.entry("channel:all".into()).or_default();
                deltas.entry(format!("channel:server:{}",entry.owner.server_id)).or_default();
            }
            for key in keys(&entry.owner) {
                let item=deltas.entry(key).or_default();
                increment(&mut item.0,delta.0,&mut item.3); increment(&mut item.1,delta.1,&mut item.3);
                item.2+=active; item.3|=invalid;
            }
        }
        // Closed final counters are folded into aggregate totals before releasing slots.
        state.entries.retain(|_,e| !e.previous.closed && (e.owner.kind=="transport" || e.previous.opened || e.life.strong_count()>0));
        for series in state.series.values_mut() {series.active=0;}
        state.identity_bytes=state.entries.values().map(|e|owner_bytes(&e.owner)).sum();
        let missing=self.meter.unattributed();
        let lost=(missing.sent.saturating_sub(state.unattributed.sent),missing.received.saturating_sub(state.unattributed.received));
        if lost!=(0,0) {
            for key in ["channel:all","channel:server:@other","channel:source:@other:@other"] {
                let item=deltas.entry(key.into()).or_default();increment(&mut item.0,lost.0,&mut item.3);increment(&mut item.1,lost.1,&mut item.3);item.3|=missing.invalid;
            }
        }
        state.rings.expire(wall.saturating_sub(3600_000));
        let mut deltas:Vec<_>=deltas.into_iter().collect();
        deltas.sort_unstable_by_key(|(key,_)|if key.ends_with(":all"){0}else if key.contains(":server:"){1}else{2});
        let mut normalized:HashMap<String,(u64,u64,usize,bool)>=HashMap::new();
        for (mut key,(sent,received,active,invalid)) in deltas {
            if !state.series.contains_key(&key)&&state.series.len()>=MAX_SERIES-4*(MAX_TARGETS+1)&&key.contains(":source:") {
                let (prefix,rest)=key.split_once(":source:").unwrap();let server=rest.split_once(':').unwrap().0;
                key=format!("{prefix}:source:{server}:@other");state.limited=state.limited.saturating_add(1);
            }
            if !state.series.contains_key(&key) && state.series.len()>=MAX_SERIES {
                state.limited=state.limited.saturating_add(1);continue;
            }
            let State {series,rings,..}=&mut *state;
            series.entry(key.clone()).or_insert_with(||Series{ring:rings.create(),..Default::default()});
            let delta=normalized.entry(key).or_default();increment(&mut delta.0,sent,&mut delta.3);increment(&mut delta.1,received,&mut delta.3);delta.2+=active;delta.3|=invalid;
        }
        for (key,(sent,received,active,invalid)) in normalized {
            let State {series,rings,..}=&mut *state;let series=series.get_mut(&key).unwrap();
            increment(&mut series.sent,sent,&mut series.invalid);
            increment(&mut series.received,received,&mut series.invalid);
            series.invalid|=invalid;series.active=active;
            let sample=Sample::new(wall,millis,sent,received,gap,invalid);
            series.latest=Some(sample);rings.push(series.ring.unwrap(),sample);
            if history.is_some(){let mut combined=sample;if let Some(previous)=series.pending{combined.merge(previous);}series.pending=Some(combined);}
        }
        state.unattributed=missing;
        state.last_tick=mono;state.last_wall=wall;state.revision=state.revision.saturating_add(1);
        if let Some(history)=history.filter(|_|mono.saturating_duration_since(state.last_flush)>=Duration::from_secs(5)||self.stopped.load(Ordering::Acquire)) {
            state.last_flush=mono;
            let mut batch=Vec::with_capacity(128);
            for (key,series) in &mut state.series {
                let Some(sample)=series.pending.take()else{continue};
                let (layer,rest)=key.split_once(':').unwrap();
                let target=if rest=="all"{"@all"}else{rest.split(':').nth(1).unwrap_or("@other")};
                batch.push(crate::observability::traffic::Bucket{owner:key.clone(),target:target.into(),layer:layer.into(),from:sample.timestamp.saturating_sub(sample.duration_ms as u64),sample});
                // Admission only enqueues; the writer never takes the sampling lock.
                if batch.len()==128&&history.save_traffic(std::mem::replace(&mut batch,Vec::with_capacity(128))).is_err(){self.history_dropped.fetch_add(1,Ordering::Relaxed);}
            }
            if !batch.is_empty()&&history.save_traffic(batch).is_err(){self.history_dropped.fetch_add(1,Ordering::Relaxed);}
        }
    }

    pub fn summary(&self,server:Option<&str>,layer:&str)->serde_json::Value {
        let state=self.state.lock().unwrap();
        let key=server.map_or_else(||format!("{layer}:all"),|s|format!("{layer}:server:{s}"));
        let empty=Series::default();let series=state.series.get(&key).unwrap_or(&empty);
        let target=server.map_or_else(||serde_json::json!({"kind":"all","configEpoch":self.config_epoch}),|s|serde_json::json!({"kind":"server","serverId":s,"configEpoch":self.config_epoch}));
        self.summarize(&state,&target,layer,series)
    }
    pub fn revision(&self)->u64 {self.state.lock().unwrap().revision}
    pub fn live_history(&self,target:&serde_json::Value,layer:&str,minutes:u64)->crate::Result<serde_json::Value> {
        use serde_json::json;
        if target["kind"]=="resource" {return Err(crate::CoreError::new("NOT_RECORDED","此资源的逐秒明细尚不可用"));}
        let state=self.state.lock().unwrap();
        let key=target["serverId"].as_str().map_or_else(||format!("{layer}:all"),|s|format!("{layer}:server:{s}"));
        let empty=Series::default();let series=state.series.get(&key).unwrap_or(&empty);
        let until=state.last_wall;let from=until.saturating_sub(minutes*60_000);
        let finest=state.rings.iter(series.ring).filter(|s|s.timestamp<=until&&s.timestamp.saturating_sub(s.duration_ms as u64)>=from).map(|s|s.duration_ms as u64).max().unwrap_or(0);
        let resolution=(if minutes<=5 {1000}else{5000}).max(finest.div_ceil(1000)*1000).min(minutes*60_000);
        let mut buckets=vec![Sample::default();(minutes*60_000).div_ceil(resolution)as usize];
        let mut partial=false;
        for sample in state.rings.iter(series.ring) {
            if sample.timestamp<=from||sample.timestamp>until {continue;}
            let index=((sample.timestamp-from-1)/resolution)as usize;let bucket=&mut buckets[index];
            if sample.timestamp.saturating_sub(sample.duration_ms as u64)<from {bucket.gap=true;partial=true;continue;}
            increment(&mut bucket.sent,sample.sent,&mut bucket.invalid);increment(&mut bucket.received,sample.received,&mut bucket.invalid);
            bucket.duration_ms=bucket.duration_ms.saturating_add(sample.duration_ms);bucket.gap|=sample.gap;bucket.invalid|=sample.invalid;
            bucket.valid_ms=bucket.valid_ms.saturating_add(sample.valid_ms);
            increment(&mut bucket.valid_sent,sample.valid_sent,&mut bucket.invalid);increment(&mut bucket.valid_received,sample.valid_received,&mut bucket.invalid);
            bucket.peak_send=bucket.peak_send.max(sample.peak_send);bucket.peak_receive=bucket.peak_receive.max(sample.peak_receive);
            bucket.peak_resolution_ms=bucket.peak_resolution_ms.max(sample.peak_resolution_ms);bucket.samples=bucket.samples.saturating_add(sample.samples);
        }
        let points:Vec<_>=buckets.into_iter().enumerate().map(|(i,b)|{
            let gap=b.gap||b.invalid||b.duration_ms==0;partial|=gap;
            json!({"fromUtc":from+i as u64*resolution,"toUtc":(from+(i as u64+1)*resolution).min(until),
                "sentBytes":b.sent.to_string(),"receivedBytes":b.received.to_string(),"validMs":b.valid_ms,
                "sendRate":(!gap&&b.valid_ms>0).then(||b.valid_sent as f64*1000.0/b.valid_ms as f64),"receiveRate":(!gap&&b.valid_ms>0).then(||b.valid_received as f64*1000.0/b.valid_ms as f64),"gap":gap,
                "peakSendRate":(b.valid_ms>0&&!b.invalid).then_some(b.peak_send),"peakReceiveRate":(b.valid_ms>0&&!b.invalid).then_some(b.peak_receive),"peakResolutionMs":b.peak_resolution_ms.max(1),"sampleCount":b.samples,"invalid":b.invalid})
        }).collect();
        Ok(json!({"summary":self.summarize(&state,target,layer,series),"fromUtc":from,"toUtc":until,
            "retainedFromUtc":state.rings.iter(series.ring).next().map(|p|p.timestamp.saturating_sub(p.duration_ms as u64)),"resolutionMs":resolution,
            "points":points,"coverage":if partial||state.limited>0{"partial"}else{"complete"}}))
    }
    pub fn target_summary(&self,target:&serde_json::Value,layer:&str)->crate::Result<serde_json::Value> {
        if target["kind"]!="resource" {return Ok(self.summary(target["serverId"].as_str(),layer));}
        let state=self.state.lock().unwrap();
        let entry=state.entries.values().find(|e|e.owner.layer==layer&&target["resourceId"]==e.owner.resource_id&&target["generation"]==e.owner.generation)
            .ok_or_else(||crate::CoreError::new("TARGET_ENDED","资源已经结束或明细已合并"))?;
        let mut series=Series {sent:entry.previous.sent,received:entry.previous.received,invalid:entry.previous.invalid,
            active:usize::from(entry.previous.opened&&!entry.previous.closed),..Default::default()};
        series.latest=Some(entry.latest);
        Ok(self.summarize(&state,target,layer,&series))
    }
    fn summarize(&self,state:&State,target:&serde_json::Value,layer:&str,series:&Series)->serde_json::Value {
        let last=series.latest.as_ref();
        let transport_key=target["serverId"].as_str().map_or_else(||"transport:all".to_owned(),|s|format!("transport:server:{s}"));
        let connected=series.active>0||(target["kind"]!="resource"&&state.series.get(&transport_key).is_some_and(|s|s.active>0));
        let rate=|sent:bool|last.filter(|s|!s.gap&&!s.invalid&&s.duration_ms>0).map(|s|if s.timestamp!=state.last_wall {0.0}else{(if sent{s.sent}else{s.received}) as f64*1000.0/s.duration_ms as f64})
            .or_else(||(last.is_none()&&state.revision>0&&connected).then_some(0.0));
        let missing=state.unattributed;
        let sent=series.sent;let received=series.received;let invalid=series.invalid;
        serde_json::json!({"target":target,"bootId":self.boot_id,"configEpoch":self.config_epoch,"revision":state.revision.to_string(),
            "layer":layer,"serverId":target.get("serverId"),"observedAt":state.last_wall,"startedAt":state.first_wall,
            "sentBytes":sent.to_string(),"receivedBytes":received.to_string(),"sendRate":rate(true),"receiveRate":rate(false),
            "active":series.active,"invalid":invalid,"coverage":if state.limited>0||missing.sent>0||missing.received>0{"partial"}else{"complete"},
            "state":if invalid {"unavailable"}else if connected {"active"}else if state.revision==0 {"collecting"}else{"disconnected"},
            "limited":state.limited.to_string(),"unattributedSentBytes":missing.sent.to_string(),"unattributedReceivedBytes":missing.received.to_string()})
    }
    pub fn view(&self,target:&serde_json::Value,layer:&str,resources:bool,servers:bool)->crate::Result<(serde_json::Value,Vec<serde_json::Value>)> {
        let state=self.state.lock().unwrap();
        let server=target["serverId"].as_str();
        let selected=|e:&&Entry|e.owner.layer==layer && server.is_none_or(|s|e.owner.server_id==s)
            && (target["kind"]!="resource"||(target["resourceId"]==e.owner.resource_id&&target["generation"]==e.owner.generation));
        let key=server.map_or_else(||format!("{layer}:all"),|s|format!("{layer}:server:{s}"));
        let mut resource_series=Series::default();
        let series=if target["kind"]=="resource" {
            let entry=state.entries.values().find(selected).ok_or_else(||crate::CoreError::new("TARGET_ENDED","资源已经结束或明细已合并"))?;
            resource_series.sent=entry.previous.sent;resource_series.received=entry.previous.received;
            resource_series.invalid=entry.previous.invalid;resource_series.active=usize::from(!entry.previous.closed);
            resource_series.latest=Some(entry.latest);&resource_series
        } else {state.series.get(&key).unwrap_or(&resource_series)};
        let summary=self.summarize(&state,target,layer,series);
        let mut rows=Vec::new();
        if resources || target["kind"]=="resource" {
            for entry in state.entries.values().filter(selected) {
                rows.push(row(&entry.owner.resource_id,&entry.owner.server_id,entry.owner.plugin_id.as_deref(),Some(&entry.owner),
                    entry.previous.sent,entry.previous.received,usize::from(!entry.previous.closed),entry.previous.invalid,Some(&entry.latest),state.last_wall));
            }
        } else if servers {
            let prefix=format!("{layer}:server:");
            for (key,series) in &state.series {
                let Some(server_id)=key.strip_prefix(&prefix) else {continue};
                if server.is_some_and(|s|s!=server_id){continue;}
                rows.push(row(key,server_id,None,None,series.sent,series.received,series.active,series.invalid,series.latest.as_ref(),state.last_wall));
            }
        } else {
            let prefix=format!("{layer}:source:");
            for (key,series) in &state.series {
                let Some(rest)=key.strip_prefix(&prefix) else {continue};
                let Some((server_id,plugin_id))=rest.split_once(':') else {continue};
                if server.is_some_and(|s|s!=server_id) {continue;}
                rows.push(row(key,server_id,(plugin_id!="@core").then_some(plugin_id),None,
                    series.sent,series.received,series.active,series.invalid,series.latest.as_ref(),state.last_wall));
            }
        }
        Ok((summary,rows))
    }
}

fn row(id:&str,server:&str,plugin:Option<&str>,owner:Option<&Owner>,sent:u64,received:u64,active:usize,invalid:bool,last:Option<&Sample>,wall:u64)->serde_json::Value {
    let rate=|up|last.filter(|s|!s.gap&&!s.invalid&&s.duration_ms>0).map(|s|if s.timestamp!=wall {0.0}else{(if up{s.sent}else{s.received})as f64*1000.0/s.duration_ms as f64});
    serde_json::json!({"id":id,"serverId":server,"pluginId":plugin,"resource":owner,"sentBytes":sent.to_string(),"receivedBytes":received.to_string(),
        "sendRate":rate(true),"receiveRate":rate(false),"active":active,"invalid":invalid})
}

fn owner_bytes(owner:&Owner)->usize {
    // Charge both identity and map-key storage plus a counter/lease allocation.
    let strings=owner.server_id.len()+owner.config_epoch.len()+owner.workspace_id.as_ref().map_or(0,String::len)
        +owner.workspace_generation.as_ref().map_or(0,String::len)+owner.plugin_id.as_ref().map_or(0,String::len)
        +owner.instance_id.len()+owner.resource_id.len()+owner.generation.len()+owner.kind.len()+owner.route_run_id.len()+owner.layer.len()+owner.access_mode.len();
    std::mem::size_of::<Entry>()+strings+owner.resource_id.len()+owner.generation.len()+owner.layer.len()+128
}
