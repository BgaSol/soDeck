use crate::{Fault, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::{HashMap, HashSet}, sync::Arc, time::{Duration, Instant}};
use uuid::Uuid;

const STATE_BYTES: usize = 32 * 1024;
const STORE_BYTES: usize = 8 * 1024 * 1024;
const INSTANCE_BYTES: usize = 512 * 1024;
const QUEUE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Key {
    pub contract: String,
    pub scope: String,
    pub object: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub scope: Value,
    pub object_id: String,
    pub revision: String,
    pub value: Value,
    #[serde(skip)]
    bytes: usize,
}
struct Object {
    snapshot: Arc<Snapshot>,
    subscribers: HashSet<String>,
}
struct Feed {
    owner: String,
    instance: String,
    key: Key,
    epoch: String,
    created: Instant,
    active: bool,
    sequence: u64,
    confirmed: u64,
    in_flight: Option<Arc<Snapshot>>,
    latest: Option<Arc<Snapshot>>,
    control: Option<&'static str>,
    ended: bool,
}
impl Feed {
    fn bytes(&self) -> usize {
        self.in_flight.as_ref().map_or(0, |s| s.bytes) + self.latest.as_ref().map_or(0, |s| s.bytes)
    }
}

#[derive(Clone, Debug)]
pub struct Delivery {
    pub owner: String,
    pub subscription_id: String,
    pub event: &'static str,
    pub payload: Value,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub objects: usize,
    pub subscriptions: usize,
    pub state_bytes: usize,
    pub queue_bytes: usize,
    pub contracts: HashMap<String,usize>,
}

#[derive(Default)]
pub struct Hub {
    objects: HashMap<Key, Object>,
    feeds: HashMap<String, Feed>,
    state_bytes: usize,
    reservations: HashMap<Key, usize>,
}
impl Hub {
    /// Reserve a bounded state slot before a domain's durable commit. Other
    /// objects cannot consume this capacity while persistence is in progress.
    pub fn reserve(&mut self, key: Key, bytes: usize) -> Result<()> {
        let used = self.state_bytes + self.reservations.iter().map(|(k,n)|n.saturating_sub(self.objects.get(k).map_or(0,|o|o.snapshot.bytes))).sum::<usize>();
        let current = self.reservations.get(&key).copied().unwrap_or(0);
        let stored=self.objects.get(&key).map_or(0,|o|o.snapshot.bytes);
        if bytes > STATE_BYTES || used-current.saturating_sub(stored)+bytes.saturating_sub(stored) > STORE_BYTES {return Err(Fault("CAPACITY"));}
        self.reservations.insert(key,bytes);Ok(())
    }
    pub fn get(&self, key: &Key) -> Result<Arc<Snapshot>> {
        self.objects.get(key).map(|o| o.snapshot.clone()).ok_or(Fault("STATE_NOT_FOUND"))
    }

    /// Call while the domain owner serializes its state commits. Callbacks are
    /// never executed here; drain deliveries after releasing all domain locks.
    pub fn put(&mut self, key: Key, scope: Value, value: Value) -> Result<Arc<Snapshot>> {
        let prior = self.objects.get(&key);
        if let Some(prior) = prior {
            if prior.snapshot.scope == scope && prior.snapshot.value == value {
                return Ok(prior.snapshot.clone());
            }
        }
        let revision = prior.map_or(Some(1), |o| o.snapshot.revision.parse::<u64>().unwrap().checked_add(1)).ok_or(Fault("CAPACITY"))?;
        let mut snapshot = Snapshot {scope, object_id:key.object.clone(), revision:revision.to_string(), value, bytes:0};
        snapshot.bytes = serde_json::to_vec(&snapshot).map_err(|_| Fault("INVALID_PARAMS"))?.len();
        let previous_bytes = prior.map_or(0, |o| o.snapshot.bytes);
        let reserved = self.reservations.iter().filter(|(k,_)|*k!=&key).map(|(k,n)|n.saturating_sub(self.objects.get(k).map_or(0,|o|o.snapshot.bytes))).sum::<usize>();
        if snapshot.bytes > STATE_BYTES || self.state_bytes - previous_bytes + snapshot.bytes.max(self.reservations.get(&key).copied().unwrap_or(0)) + reserved > STORE_BYTES {
            return Err(Fault("CAPACITY"));
        }
        let snapshot = Arc::new(snapshot);
        self.state_bytes = self.state_bytes - previous_bytes + snapshot.bytes;
        let object = self.objects.entry(key).or_insert_with(|| Object {snapshot:snapshot.clone(), subscribers:HashSet::new()});
        object.snapshot = snapshot.clone();
        let ids: Vec<_> = object.subscribers.iter().cloned().collect();
        for id in ids {
            if let Some(feed) = self.feeds.get_mut(&id) {
                if feed.control.is_none() && !feed.ended { feed.latest = Some(snapshot.clone()); }
            }
            self.enforce_budget(&id);
        }
        Ok(snapshot)
    }

    fn enforce_budget(&mut self, id: &str) {
        let Some(owner) = self.feeds.get(id).map(|f| f.instance.clone()) else { return; };
        let (mut total, mut instance) = (0, 0);
        for feed in self.feeds.values() {
            total += feed.bytes();
            if feed.instance == owner { instance += feed.bytes(); }
        }
        if total > QUEUE_BYTES || instance > INSTANCE_BYTES {
            let feed = self.feeds.get_mut(id).unwrap();
            feed.latest = None;
            feed.in_flight = None;
            feed.control = Some("capacity");
        }
    }

    /// Snapshot and registration are atomic under the caller's Hub lock.
    /// No event is deliverable until the client has installed snapshot + ack 0.
    pub fn open(&mut self, owner: &str, key: &Key, now: Instant) -> Result<Value> {
        self.open_for(owner, owner, key, now)
    }
    pub fn open_for(&mut self, owner: &str, instance: &str, key: &Key, now: Instant) -> Result<Value> {
        if self.feeds.len() >= 512 || self.feeds.values().filter(|f| f.instance == instance).count() >= 64 {
            return Err(Fault("CAPACITY"));
        }
        let snapshot = self.get(key)?;
        let id = Uuid::new_v4().to_string();
        let epoch = Uuid::new_v4().to_string();
        self.objects.get_mut(key).unwrap().subscribers.insert(id.clone());
        self.feeds.insert(id.clone(), Feed {owner:owner.into(), instance:instance.into(), key:key.clone(), epoch:epoch.clone(), created:now,
            active:false, sequence:0, confirmed:0, in_flight:None, latest:None, control:None, ended:false});
        Ok(json!({"subscriptionId":id,"feedEpoch":epoch,"sequence":"0","snapshot":snapshot.as_ref()}))
    }

    pub fn ack(&mut self, owner: &str, id: &str, epoch: &str, sequence: u64) -> Result<()> {
        let feed = self.feeds.get_mut(id).ok_or(Fault("SUBSCRIPTION_EXPIRED"))?;
        if feed.owner != owner || feed.epoch != epoch { return Err(Fault("SOURCE_INVALID")); }
        if sequence > feed.sequence { return Err(Fault("INVALID_CURSOR")); }
        if !feed.active && sequence == 0 { feed.active = true; }
        if sequence > feed.confirmed {
            feed.confirmed = sequence;
            feed.in_flight = None;
        }
        Ok(())
    }

    pub fn close(&mut self, owner: &str, id: &str, epoch: &str) -> Result<()> {
        if let Some(feed) = self.feeds.get(id) {
            if feed.owner != owner || feed.epoch != epoch { return Err(Fault("SOURCE_INVALID")); }
        }
        self.remove_feed(id);
        Ok(())
    }
    fn remove_feed(&mut self, id: &str) {
        if let Some(feed) = self.feeds.remove(id) {
            if let Some(object) = self.objects.get_mut(&feed.key) { object.subscribers.remove(id); }
        }
    }

    pub fn remove(&mut self, key: &Key) {
        self.reservations.remove(key);
        if let Some(object) = self.objects.remove(key) {
            self.state_bytes -= object.snapshot.bytes;
            for id in object.subscribers {
                if let Some(feed) = self.feeds.get_mut(&id) {
                    feed.latest = None; feed.in_flight = None;
                    feed.control = Some("object-ended"); feed.ended = true;
                }
            }
        }
    }
    pub fn remove_owner(&mut self, owner: &str) {
        let ids: Vec<_> = self.feeds.iter().filter(|(_, f)| f.owner == owner).map(|(id, _)| id.clone()).collect();
        for id in ids { self.remove_feed(&id); }
    }
    pub fn remove_instance(&mut self, instance: &str) {
        let ids: Vec<_> = self.feeds.iter().filter(|(_, f)| f.instance == instance).map(|(id, _)| id.clone()).collect();
        for id in ids { self.remove_feed(&id); }
    }
    pub fn retain_owners(&mut self, live: &HashSet<String>) {
        let ids:Vec<_>=self.feeds.iter().filter(|(_,f)|!live.contains(&f.owner)).map(|(id,_)|id.clone()).collect();
        for id in ids {self.remove_feed(&id);}
    }
    pub fn delivered(&mut self, id: &str, event: &str) {
        if event != "state.update" { self.remove_feed(id); }
    }
    pub fn remove_scope(&mut self, scope: &str) {
        let keys: Vec<_> = self.objects.keys().filter(|k| k.scope == scope).cloned().collect();
        for key in keys { self.remove(&key); }
    }
    pub fn keys(&self) -> impl Iterator<Item=&Key> { self.objects.keys() }

    /// A transport failure never silently advances a live state view.
    pub fn delivery_failed(&mut self, id: &str) {
        if let Some(feed) = self.feeds.get_mut(id) {
            feed.latest = None; feed.in_flight = None; feed.control = Some("transport-gap");
        }
    }
    pub fn drain(&mut self, now: Instant) -> Vec<Delivery> {
        let expired: Vec<_> = self.feeds.iter().filter(|(_,f)| !f.active && now.duration_since(f.created) >= Duration::from_secs(10)).map(|(id,_)| id.clone()).collect();
        for id in expired { self.remove_feed(&id); }
        let mut deliveries = Vec::new();
        for (id, feed) in &mut self.feeds {
            if let Some(reason) = feed.control.take() {
                deliveries.push(Delivery {owner:feed.owner.clone(), subscription_id:id.clone(), event:if feed.ended {"state.closed"} else {"state.resyncRequired"},
                    payload:json!({"subscriptionId":id,"feedEpoch":feed.epoch,"reason":reason})});
                continue;
            }
            if !feed.active || feed.in_flight.is_some() { continue; }
            if let Some(snapshot) = feed.latest.take() {
                feed.sequence += 1;
                deliveries.push(Delivery {owner:feed.owner.clone(), subscription_id:id.clone(), event:"state.update",
                    payload:json!({"subscriptionId":id,"feedEpoch":feed.epoch,"sequence":feed.sequence.to_string(),"snapshot":snapshot.as_ref()})});
                feed.in_flight = Some(snapshot);
            }
        }
        deliveries
    }
    pub fn stats(&self) -> Stats {
        let mut contracts=HashMap::new();for feed in self.feeds.values(){*contracts.entry(feed.key.contract.clone()).or_default()+=1;}
        Stats {objects:self.objects.len(),subscriptions:self.feeds.len(),state_bytes:self.state_bytes,
            queue_bytes:self.feeds.values().map(Feed::bytes).sum(),contracts}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reserved_commit_survives_full_store_and_releases_on_remove(){
        let mut hub=Hub::default();let reserved=Key{contract:"app.theme.v1".into(),scope:"app".into(),object:"theme".into()};
        hub.reserve(reserved.clone(),STATE_BYTES).unwrap();hub.put(reserved.clone(),json!({}),json!(1)).unwrap();
        for n in 0..1000 {let key=Key{contract:"test.v1".into(),scope:"other".into(),object:n.to_string()};if hub.put(key,json!({}),json!("x".repeat(32000))).is_err(){break;}}
        assert!(hub.stats().state_bytes>STORE_BYTES-STATE_BYTES*3);
        for _ in 0..200{hub.put(reserved.clone(),json!({}),json!("y".repeat(32000))).unwrap();hub.put(reserved.clone(),json!({}),json!(1)).unwrap();}
        hub.remove_scope("other");assert!(hub.get(&reserved).is_ok());hub.remove(&reserved);assert!(hub.reservations.is_empty());
    }
    fn key() -> Key {Key {contract:"test.v1".into(),scope:"workspace-generation".into(),object:"a".into()}}
    #[test]
    fn multiple_surfaces_share_instance_budget_and_control_delivery_can_retry(){
        let mut hub=Hub::default();let now=Instant::now();let key=key();hub.put(key.clone(),json!({}),json!(1)).unwrap();
        for n in 0..64 {hub.open_for(&format!("instance:view-{n}"),"instance",&key,now).unwrap();}
        assert_eq!(hub.open_for("instance:worker","instance",&key,now).unwrap_err(),Fault("CAPACITY"));
        hub.remove(&key);let deliveries=hub.drain(now);assert_eq!(deliveries.len(),64);
        for d in &deliveries {hub.delivery_failed(&d.subscription_id);}
        let retried=hub.drain(now);assert_eq!(retried.len(),64);assert!(retried.iter().all(|d|d.event=="state.closed"));
        for d in retried {hub.delivered(&d.subscription_id,d.event);}
        assert_eq!(hub.stats().subscriptions,0);
    }
    #[test]
    fn atomic_open_coalesces_and_waits_for_ack_without_losing_latest() {
        let mut hub = Hub::default(); let now = Instant::now(); let key = key();
        hub.put(key.clone(),json!({}),json!(1)).unwrap();
        let open = hub.open("owner",&key,now).unwrap();
        let (id,epoch) = (open["subscriptionId"].as_str().unwrap(),open["feedEpoch"].as_str().unwrap());
        hub.put(key.clone(),json!({}),json!(2)).unwrap();
        assert!(hub.drain(now).is_empty());
        hub.ack("owner",id,epoch,0).unwrap();
        let first = hub.drain(now); assert_eq!(first[0].payload["snapshot"]["value"],2);
        hub.put(key.clone(),json!({}),json!(3)).unwrap(); hub.put(key,json!({}),json!(4)).unwrap();
        assert!(hub.drain(now).is_empty());
        hub.ack("owner",id,epoch,1).unwrap(); hub.ack("owner",id,epoch,1).unwrap();
        let last = hub.drain(now); assert_eq!(last[0].payload["snapshot"]["value"],4);
        assert_eq!(last[0].payload["sequence"],"2");
        assert_eq!(hub.ack("owner",id,epoch,3),Err(Fault("INVALID_CURSOR")));
    }
    #[test]
    fn ownership_timeout_and_scope_teardown() {
        let mut hub = Hub::default(); let now = Instant::now(); let key = key();
        hub.put(key.clone(),json!({}),json!(1)).unwrap();
        let open = hub.open("a",&key,now).unwrap();
        let (id,epoch) = (open["subscriptionId"].as_str().unwrap(),open["feedEpoch"].as_str().unwrap());
        assert_eq!(hub.close("b",id,epoch),Err(Fault("SOURCE_INVALID")));
        hub.drain(now+Duration::from_secs(10)); assert_eq!(hub.stats().subscriptions,0);
        hub.open("a",&key,now).unwrap(); hub.remove_scope(&key.scope);
        let deliveries=hub.drain(now);assert_eq!(deliveries[0].event,"state.closed");
        for d in deliveries {hub.delivered(&d.subscription_id,d.event);} assert_eq!(hub.stats().state_bytes,0);
        assert_eq!(hub.stats().queue_bytes,0); assert_eq!(hub.stats().subscriptions,0);
    }
    #[test]
    fn bounded_slow_consumer_does_not_starve_another_owner() {
        let mut hub = Hub::default(); let now = Instant::now(); let key = key();
        hub.put(key.clone(),json!({}),json!("x".repeat(30000))).unwrap();
        for _ in 0..64 {hub.open("slow",&key,now).unwrap();}
        assert_eq!(hub.open("slow",&key,now).unwrap_err(),Fault("CAPACITY"));
        let fast = hub.open("fast",&key,now).unwrap();
        hub.ack("fast",fast["subscriptionId"].as_str().unwrap(),fast["feedEpoch"].as_str().unwrap(),0).unwrap();
        hub.put(key.clone(),json!({}),json!("y".repeat(30000))).unwrap();
        assert!(hub.stats().queue_bytes <= INSTANCE_BYTES+STATE_BYTES);
        let frames=hub.drain(now);
        assert!(frames.iter().any(|d| d.owner=="fast"&&d.event=="state.update"));
        assert!(frames.iter().any(|d| d.owner=="slow"&&d.event=="state.resyncRequired"));
        hub.remove_owner("slow");hub.remove_owner("fast");hub.remove(&key);
        assert_eq!(hub.stats().queue_bytes,0);
    }
}
